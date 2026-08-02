//! THE CLUSTER FIXTURE (ADR-20260730-234918 D2.1, Proto.Actor's cluster-test shape): N workers
//! against one Postgres, membership churn (a late joiner, then a hard crash), invariant probes
//! running throughout. What it proves, beyond the two-worker test in `concurrency.rs`:
//!
//! - **Fair-share rebalancing is live** (the #270 review's deferred major): a first instance
//!   claims every lane and renews forever; a SECOND instance must converge to `floor(W/N)` lanes
//!   by stealing from the largest owner — while the victim is alive and heartbeating.
//! - **The single-concurrent-virtual-actor claim, made provable at COMMIT granularity** (their
//!   `ClusterMaintainsSingleConcurrentVirtualActorPerIdentity`, our stronger form): inside the
//!   steal window two workers MAY both enter the handler for one actor (dual belief — the probe
//!   observes and tolerates it, where Proto.Actor's harness comments duplicates are "by design"),
//!   but the fence guarantees at most one of those entries COMMITS — asserted as exactly-once
//!   committed deliveries and strict per-actor commit order, with steals demonstrably happening.
//! - **Handover completeness accounting** (their chunk-index reconciliation): after a rebalance
//!   AND a hard crash (abort, no lane release — expiry takeover), per-actor message counts
//!   processed exactly once — no loss, no double.
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use actor_runtime::{
    Delivery, HandlerVerdict, InboundMessage, MailboxWorker, MessageHandler, WorkerConfig,
};
use sqlx::{PgPool, Postgres, Row, Transaction};

const ACTOR_TYPE: &str = "Conversation";
const PARTITIONS: i16 = 8;
const ACTORS: u128 = 16;
const MESSAGES: u128 = 300;

/// In-memory concurrent-entry tracker shared by every worker in the "cluster" (one process
/// simulates N nodes, exactly like Proto.Actor's in-proc cluster fixtures). Entries are counted
/// per actor on handler entry/exit; an overlap is RECORDED, never asserted zero — the steal
/// window makes dual entry legitimate, and the real claim (dual COMMIT is impossible) is
/// asserted on the probe table.
#[derive(Default)]
struct EntryTracker {
    in_flight: std::sync::Mutex<std::collections::HashMap<uuid::Uuid, i64>>,
    overlaps_observed: AtomicU64,
}

impl EntryTracker {
    fn enter(&self, actor: uuid::Uuid) {
        let mut map = self.in_flight.lock().unwrap();
        let n = map.entry(actor).or_insert(0);
        *n += 1;
        if *n > 1 {
            self.overlaps_observed.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn exit(&self, actor: uuid::Uuid) {
        let mut map = self.in_flight.lock().unwrap();
        if let Some(n) = map.get_mut(&actor) {
            *n -= 1;
        }
    }
}

/// TEST THE FIXTURE (ADR-20260730-234918 port 5, Proto.Actor's gate-the-scanner lesson): a probe
/// that cannot detect a violation is indistinguishable from no violations. Prove the tracker
/// counts a genuine overlap and does NOT count sequential entries before trusting its readings.
#[test]
fn the_overlap_probe_detects_a_violation() {
    let t = EntryTracker::default();
    let actor = uuid::Uuid::from_u128(7);
    t.enter(actor);
    t.enter(actor); // second concurrent entry — the violation shape
    assert_eq!(t.overlaps_observed.load(Ordering::Relaxed), 1, "a real overlap must register");
    t.exit(actor);
    t.exit(actor);
    t.enter(actor); // sequential re-entry — NOT an overlap
    t.exit(actor);
    assert_eq!(t.overlaps_observed.load(Ordering::Relaxed), 1, "sequential entries must not");
    let other = uuid::Uuid::from_u128(8);
    t.enter(actor);
    t.enter(other); // different actor — NOT an overlap
    assert_eq!(t.overlaps_observed.load(Ordering::Relaxed), 1, "cross-actor entries must not");
}

/// Records each delivery (worker, actor, position) THROUGH the completion transaction — the
/// count that cannot lie about what committed — while tracking in-memory entry concurrency.
struct ProbeHandler {
    worker: &'static str,
    tracker: Arc<EntryTracker>,
    delivered: Arc<AtomicI64>,
}

#[async_trait::async_trait]
impl MessageHandler for ProbeHandler {
    async fn handle(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        _prepared: actor_runtime::Prepared,
    ) -> Result<Delivery, sqlx::Error> {
        self.tracker.enter(message.actor_id);
        // Widen the handler window so a steal has something to overlap with.
        tokio::time::sleep(std::time::Duration::from_millis(3)).await;
        let res = sqlx::query(
            "INSERT INTO delivered_probe (worker, message_id, actor_id, position) VALUES ($1, $2, $3, $4)",
        )
        .bind(self.worker)
        .bind(message.message_id)
        .bind(message.actor_id)
        .bind(message.position)
        .execute(&mut **tx)
        .await;
        self.tracker.exit(message.actor_id);
        res?;
        self.delivered.fetch_add(1, Ordering::Relaxed);
        Ok(Delivery::of(HandlerVerdict::Succeeded))
    }
}

async fn setup(pool: &PgPool) {
    sqlx::raw_sql(
        "DROP TABLE IF EXISTS inbound_messages, mailbox_partitions, delivered_probe CASCADE;\n\
         DROP SEQUENCE IF EXISTS inbound_messages_position_seq;",
    )
    .execute(pool)
    .await
    .expect("drop");
    sqlx::raw_sql(include_str!("../../../migrations/20260731063000_actor_mailbox_tables.sql"))
        .execute(pool)
        .await
        .expect("apply the actor-mailbox migration");
    sqlx::raw_sql(include_str!("../../../migrations/20260802230000_mailbox_attempts_column.sql"))
        .execute(pool)
        .await
        .expect("apply the mailbox attempts migration");
    sqlx::raw_sql(
        "CREATE TABLE delivered_probe (\n\
           delivery BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\n\
           worker TEXT NOT NULL,\n\
           message_id UUID NOT NULL,\n\
           actor_id UUID NOT NULL,\n\
           position BIGINT NOT NULL\n\
         )",
    )
    .execute(pool)
    .await
    .expect("probe table");
}

/// A deterministic in-range lane per actor for SEEDING rows. The runtime never computes
/// partitions (producers stamp the column; the drain trusts it), and the FROZEN product routing
/// hash lives in the actor_client boundary crate (#290 phase 1) — this test only needs "all of
/// one actor's rows land on one lane", not that specific hash.
fn test_partition(actor_id: &uuid::Uuid) -> i16 {
    (actor_id.as_u128() % PARTITIONS as u128) as i16
}

async fn enqueue(pool: &PgPool, n: u128) {
    let actor_id = uuid::Uuid::from_u128(1 + (n % ACTORS));
    let partition = test_partition(&actor_id);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id) \
         VALUES ($1, 'COMMAND', $2, $3, $4, 'PostMessage', '{}', $5, 'GRAPHQL', 'ADMIN', $1)",
    )
    .bind(uuid::Uuid::from_u128(0x2000 + n))
    .bind(ACTOR_TYPE)
    .bind(actor_id)
    .bind(partition)
    .bind(format!("h{n}"))
    .execute(pool)
    .await
    .expect("enqueue");
}

fn worker(
    pool: &PgPool,
    id: &'static str,
    tracker: Arc<EntryTracker>,
    delivered: Arc<AtomicI64>,
) -> MailboxWorker {
    MailboxWorker::new(
        pool.clone(),
        id,
        ACTOR_TYPE,
        // Fast loop so the test converges in seconds. max_claims_per_pass is WIDE on purpose:
        // the first worker must grab everything so the rebalance (not a claim-bound accident)
        // is what gives the second worker its share. lease_seconds must survive slow-CI beats
        // yet expire quickly after the hard crash.
        WorkerConfig { lease_seconds: 6, heartbeat_seconds: 1, batch: 20, max_claims_per_pass: 100, ..WorkerConfig::default() },
        Arc::new(ProbeHandler { worker: id, tracker, delivered }),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn late_joiner_steals_to_fair_share_and_a_crash_hands_over_completely() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP rebalance cluster test: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;
    actor_runtime::seed_partitions(&pool, ACTOR_TYPE, PARTITIONS).await.expect("seed");

    let tracker = Arc::new(EntryTracker::default());
    let delivered = Arc::new(AtomicI64::new(0));

    // A third of the load is waiting before any worker exists.
    for n in 0..MESSAGES / 3 {
        enqueue(&pool, n).await;
    }

    // ── Phase 1: A alone claims EVERY lane. ──
    let a = Arc::new(worker(&pool, "w-A", tracker.clone(), delivered.clone()));
    let (_stop_a, rx_a) = tokio::sync::watch::channel(false);
    let run_a = tokio::spawn({
        let a = a.clone();
        async move { a.run(rx_a).await }
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if a.owned().await.len() == PARTITIONS as usize {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "A never claimed the full width");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // ── Phase 2: B joins while A is live and renewing — only a steal can feed it. ──
    let b = Arc::new(worker(&pool, "w-B", tracker.clone(), delivered.clone()));
    let (stop_b, rx_b) = tokio::sync::watch::channel(false);
    let run_b = tokio::spawn({
        let b = b.clone();
        async move { b.run(rx_b).await }
    });

    // Load keeps arriving during the rebalance.
    for n in MESSAGES / 3..(2 * MESSAGES / 3) {
        enqueue(&pool, n).await;
        if n % 25 == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    // Convergence: B reaches its floor share (8/2 = 4) while A is STILL ALIVE (the lease-liveness
    // filter in the census is what distinguishes a steal from an expiry takeover).
    let fair = (PARTITIONS / 2) as usize;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if b.owned().await.len() >= fair {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "B never converged to its fair share ({fair}) — owned {:?}",
            b.owned().await.len()
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(!run_a.is_finished(), "A must be alive during the steals — this was a rebalance, not a takeover");

    // A settles at the floor too (it drops stolen lanes at its next beat and must not steal back:
    // 4 >= floor(8/2), so A is no thief — the stability half of the convergence claim).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if a.owned().await.len() <= PARTITIONS as usize - fair {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "A never dropped its stolen lanes");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // ── Phase 3: HARD CRASH — abort A mid-run (no graceful release; lanes must EXPIRE). ──
    run_a.abort();
    let _ = run_a.await; // JoinError::Cancelled — the point.

    // The final third of the load arrives after the crash.
    for n in (2 * MESSAGES / 3)..MESSAGES {
        enqueue(&pool, n).await;
    }

    // B alone must finish EVERYTHING — the crashed owner's leases expire and B claims them.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(40);
    loop {
        let pending: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM inbound_messages WHERE status = 'RECEIVED'")
                .fetch_one(&pool)
                .await
                .expect("pending");
        if pending == 0 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{pending} messages still pending at the deadline"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let _ = stop_b.send(true);
    run_b.await.expect("B join").expect("B run");

    // ── The invariants (handover completeness accounting) ──
    let rows =
        sqlx::query("SELECT worker, message_id, actor_id, position FROM delivered_probe ORDER BY delivery")
            .fetch_all(&pool)
            .await
            .expect("probe");

    // Exactly-once across the rebalance AND the crash: no loss, no double.
    assert_eq!(rows.len() as u128, MESSAGES, "every enqueued message delivered exactly once");
    let mut seen = std::collections::HashSet::new();
    for r in &rows {
        assert!(seen.insert(r.get::<uuid::Uuid, _>("message_id")), "duplicate committed delivery");
    }

    // Per-actor commit order holds through both ownership changes.
    let mut last_per_actor: std::collections::HashMap<uuid::Uuid, i64> = Default::default();
    for r in &rows {
        let actor: uuid::Uuid = r.get("actor_id");
        let position: i64 = r.get("position");
        if let Some(prev) = last_per_actor.insert(actor, position) {
            assert!(
                position > prev,
                "actor {actor}: delivery at position {position} committed after {prev} — per-actor order broken"
            );
        }
    }

    // Per-actor completeness: every actor's full message count arrived (their reconciliation
    // idea — counts per identity, not just the grand total).
    let mut per_actor: std::collections::HashMap<uuid::Uuid, u128> = Default::default();
    for r in &rows {
        *per_actor.entry(r.get("actor_id")).or_default() += 1;
    }
    for actor_n in 0..ACTORS {
        let actor = uuid::Uuid::from_u128(1 + actor_n);
        let expected = (0..MESSAGES).filter(|n| n % ACTORS == actor_n).count() as u128;
        assert_eq!(
            per_actor.get(&actor).copied().unwrap_or(0),
            expected,
            "actor {actor}: committed count diverges from enqueued count"
        );
    }

    // Both instances really carried load, and the split was a steal (B delivered while A lived).
    let a_count = rows.iter().filter(|r| r.get::<String, _>("worker") == "w-A").count();
    let b_count = rows.iter().filter(|r| r.get::<String, _>("worker") == "w-B").count();
    assert!(a_count > 0, "worker A never delivered");
    assert!(b_count > 0, "worker B never delivered");

    // Overlapped entries during the steal window are legitimate (dual belief); what matters is
    // that every one of them resolved to a single COMMIT — which the exactly-once assertion
    // above just proved. Surface the observation so a run that exercised the window says so.
    eprintln!(
        "rebalance fixture: {} overlapping handler entries observed (all fenced to one commit)",
        tracker.overlaps_observed.load(Ordering::Relaxed)
    );
}
