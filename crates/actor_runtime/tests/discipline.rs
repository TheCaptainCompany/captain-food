//! The MAILBOX DISCIPLINE suite — ADR-20260730-234918 **test port 4** (Proto.Actor's
//! `defaultMailbox` ideas mapped onto our Postgres mailbox), the last of the five named ports
//! (1–3 + 5 live in `rebalance.rs`/`concurrency.rs`/`mailbox.rs`). Our mailbox has no in-memory
//! control queue, so each of their disciplines maps to the analogous guarantee here:
//!
//! - **Control-before-user ordering** → the pass bracket: promotion (the control plane's flip
//!   that makes reminders deliverable) runs BEFORE the user-message drain of the same pass, and
//!   a promoted row takes a FRESH position — control work is visible to the very next drain but
//!   never queue-jumps user messages already waiting on the lane.
//! - **Suspension semantics** → head-of-line blocking per lane: a failing head row suspends
//!   exactly its own lane (nothing behind the head slips through), sibling lanes keep flowing in
//!   the same pass, and recovery resumes the suspended lane in order.
//! - **Throughput budget honored** → the batch bound: a multi-batch backlog drains to empty in
//!   one call (liveness), but every batch boundary re-checks authority — a lane lost after a
//!   full batch stops the drain at that boundary instead of delivering fenced work, and the new
//!   owner finishes the remainder exactly once.
//! - **FIFO per sender** → per-lane position order under interleaved producers: concurrent
//!   senders on one lane each observe their own send order in the committed deliveries, with no
//!   kind-based precedence (COMMAND vs EVENT rows keep pure position order).
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use actor_runtime::{
    claim_due_lanes, complete_fenced, steal_lane, CompletionError, Delivery, HandlerVerdict,
    InboundMessage, MailboxWorker, MessageHandler, WorkerConfig,
};
use sqlx::{PgPool, Postgres, Row, Transaction};

const ACTOR_TYPE: &str = "Conversation";

/// Records each delivery through the completion transaction (the atomicity witness).
struct ProbeHandler;

#[async_trait::async_trait]
impl MessageHandler for ProbeHandler {
    async fn handle(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        _prepared: actor_runtime::Prepared,
    ) -> Result<Delivery, sqlx::Error> {
        probe(tx, message).await?;
        Ok(Delivery::of(HandlerVerdict::Succeeded))
    }
}

async fn probe(
    tx: &mut Transaction<'_, Postgres>,
    message: &InboundMessage,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO delivered_probe (message_id, actor_id, partition, position) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(message.message_id)
    .bind(message.actor_id)
    .bind(message.partition)
    .bind(message.position)
    .execute(&mut **tx)
    .await?;
    Ok(())
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
    sqlx::raw_sql(
        "CREATE TABLE delivered_probe (\n\
           delivery BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\n\
           message_id UUID NOT NULL,\n\
           actor_id UUID NOT NULL,\n\
           partition SMALLINT NOT NULL,\n\
           position BIGINT NOT NULL\n\
         )",
    )
    .execute(pool)
    .await
    .expect("probe table");
}

/// Enqueue one row on an explicit lane for an explicit actor; returns (message_id, position).
async fn enqueue(
    pool: &PgPool,
    partition: i16,
    actor: u128,
    n: u128,
    kind: &str,
) -> (uuid::Uuid, i64) {
    let id = uuid::Uuid::from_u128(n);
    let row = sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id) \
         VALUES ($1, $2, $3, $4, $5, 'PostMessage', '{}', $6, 'GRAPHQL', 'CUSTOMER', $1) \
         RETURNING position",
    )
    .bind(id)
    .bind(kind)
    .bind(ACTOR_TYPE)
    .bind(uuid::Uuid::from_u128(actor))
    .bind(partition)
    .bind(format!("h{n}"))
    .fetch_one(pool)
    .await
    .expect("enqueue");
    (id, row.get::<i64, _>("position"))
}

/// Schedule one MESSAGE row (a reminder): status SCHEDULED, position NULL until promotion.
async fn schedule(
    pool: &PgPool,
    partition: i16,
    n: u128,
    due: chrono::DateTime<chrono::Utc>,
) -> uuid::Uuid {
    let id = uuid::Uuid::from_u128(n);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, position, kind, actor_type, actor_id, partition, message_type, payload, \
            payload_hash, channel, user_type, correlation_id, scheduled_at, status) \
         VALUES ($1, NULL, 'MESSAGE', $2, $3, $4, 'OrderExpired', '{}', $5, 'WORKER', \
                 'EXTERNAL', $1, $6, 'SCHEDULED')",
    )
    .bind(id)
    .bind(ACTOR_TYPE)
    .bind(uuid::Uuid::from_u128(0xD15C))
    .bind(partition)
    .bind(format!("h{n}"))
    .bind(due)
    .execute(pool)
    .await
    .expect("schedule");
    id
}

/// Probe rows in commit order: (message_id, partition, position).
async fn probe_rows(pool: &PgPool) -> Vec<(uuid::Uuid, i16, i64)> {
    sqlx::query("SELECT message_id, partition, position FROM delivered_probe ORDER BY delivery")
        .fetch_all(pool)
        .await
        .expect("probe rows")
        .iter()
        .map(|r| (r.get("message_id"), r.get("partition"), r.get("position")))
        .collect()
}

fn gated() -> Option<String> {
    match std::env::var("DATABASE_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            assert!(
                std::env::var("DB_TESTS_REQUIRED").is_err(),
                "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
            );
            eprintln!("SKIP actor_runtime discipline test: DATABASE_URL not set");
            None
        }
    }
}

/// Serialize the suite: every test resets the same tables.
static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ================================================================================================
// Control-before-user ordering — the pass bracket + fresh-position discipline.
// ================================================================================================

#[tokio::test]
async fn promotion_is_visible_to_the_same_pass_but_never_queue_jumps() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let w = MailboxWorker::new(
        pool.clone(),
        "w-A",
        ACTOR_TYPE,
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(ProbeHandler),
    );
    w.seed(4).await.expect("seed");

    // Two user commands already waiting on the lane, then a reminder that fell due BEFORE they
    // were enqueued (worst case for queue-jumping: the control-plane row is older than the user
    // rows).
    let due = schedule(&pool, 1, 0xC1, chrono::Utc::now() - chrono::Duration::hours(1)).await;
    let (cmd1, p1) = enqueue(&pool, 1, 0xA1, 1, "COMMAND").await;
    let (cmd2, p2) = enqueue(&pool, 1, 0xA1, 2, "COMMAND").await;
    schedule(&pool, 1, 0xC2, chrono::Utc::now() + chrono::Duration::hours(1)).await;

    // ONE pass, in the production bracket order: promote → claim → drain.
    assert_eq!(w.promote().await.expect("promote"), 1, "exactly the due reminder promotes");
    w.claim().await.expect("claim");
    assert_eq!(w.drain().await.expect("drain"), 3, "the pass delivers commands AND the reminder");

    // The promoted reminder was stamped a FRESH position, so it delivers AFTER the user rows
    // that were already waiting — same-pass visibility without queue-jumping.
    let seen = probe_rows(&pool).await;
    assert_eq!(
        seen.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
        vec![cmd1, cmd2, due],
        "strict position order: waiting user rows first, the promoted control row last"
    );
    assert!(seen[2].2 > p2 && p1 < p2, "promotion allocated a position above the waiting rows");
}

// ================================================================================================
// Suspension semantics — head-of-line blocking per lane, isolation, ordered resume.
// ================================================================================================

/// Fails every delivery on `broken_partition` while `broken` holds — the poisoned-head stand-in.
struct SuspendingHandler {
    broken_partition: i16,
    broken: AtomicBool,
}

#[async_trait::async_trait]
impl MessageHandler for SuspendingHandler {
    async fn handle(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        _prepared: actor_runtime::Prepared,
    ) -> Result<Delivery, sqlx::Error> {
        if message.partition == self.broken_partition && self.broken.load(Ordering::SeqCst) {
            return Err(sqlx::Error::Protocol("suspended: head delivery failing".into()));
        }
        probe(tx, message).await?;
        Ok(Delivery::of(HandlerVerdict::Succeeded))
    }
}

#[tokio::test]
async fn failing_head_suspends_exactly_its_lane_and_resumes_in_order() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let handler = Arc::new(SuspendingHandler { broken_partition: 1, broken: AtomicBool::new(true) });
    let w = MailboxWorker::new(
        pool.clone(),
        "w-A",
        ACTOR_TYPE,
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        handler.clone(),
    );
    w.seed(4).await.expect("seed");

    let lane1: Vec<(uuid::Uuid, i64)> = {
        let mut v = Vec::new();
        for n in 1..=3u128 {
            v.push(enqueue(&pool, 1, 0xB1, n, "COMMAND").await);
        }
        v
    };
    let (g1, _) = enqueue(&pool, 2, 0xB2, 11, "COMMAND").await;
    let (g2, _) = enqueue(&pool, 2, 0xB2, 12, "COMMAND").await;

    // Pass 1: lane 1's head fails — the lane suspends; lane 2 flows to empty IN THE SAME PASS.
    w.claim().await.expect("claim");
    assert_eq!(w.drain().await.expect("pass 1"), 2, "only the healthy lane delivered");
    let seen = probe_rows(&pool).await;
    assert_eq!(seen.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(), vec![g1, g2]);
    let suspended: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM inbound_messages WHERE partition = 1 AND status = 'RECEIVED'",
    )
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(suspended, 3, "nothing behind the failing head slipped through");

    // Recovery: the suspended lane resumes IN ORDER on the next pass — no loss, no reorder.
    handler.broken.store(false, Ordering::SeqCst);
    assert_eq!(w.drain().await.expect("pass 2"), 3, "the suspended lane drained on resume");
    let seen = probe_rows(&pool).await;
    assert_eq!(
        seen.iter().skip(2).map(|(id, _, _)| *id).collect::<Vec<_>>(),
        lane1.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        "resume delivers the suspended lane head-of-line, in position order"
    );
}

// ================================================================================================
// Throughput budget honored — multi-batch liveness + authority recheck at every batch boundary.
// ================================================================================================

/// Delivers normally, but the post-commit hook of `steal_on` hands the lane to `thief` — landing
/// exactly BETWEEN the first full batch and the between-batch heartbeat, the boundary under test.
struct BatchBoundaryThief {
    pool: PgPool,
    steal_on: uuid::Uuid,
    thief: &'static str,
}

#[async_trait::async_trait]
impl MessageHandler for BatchBoundaryThief {
    async fn handle(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        _prepared: actor_runtime::Prepared,
    ) -> Result<Delivery, sqlx::Error> {
        probe(tx, message).await?;
        if message.message_id == self.steal_on {
            let pool = self.pool.clone();
            let partition = message.partition;
            let thief = self.thief;
            Ok(Delivery::then(HandlerVerdict::Succeeded, move || {
                // post_commit is sync by contract; the steal is a test-side DB call, so hop
                // through block_in_place (the test runtime is multi-threaded).
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        steal_lane(&pool, ACTOR_TYPE, partition, thief, 300)
                            .await
                            .expect("steal")
                            .expect("lane exists");
                    })
                });
            }))
        } else {
            Ok(Delivery::of(HandlerVerdict::Succeeded))
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn batch_boundary_rechecks_authority_and_backlog_survives_exactly_once() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    actor_runtime::seed_partitions(&pool, ACTOR_TYPE, 4).await.expect("seed");

    // Liveness first: a 5-row backlog against batch = 2 drains to EMPTY in one drain_lane call
    // (the loop renews between batches instead of stopping at the bound).
    let mut lane1_rows = Vec::new();
    for n in 1..=5u128 {
        lane1_rows.push(enqueue(&pool, 1, 0xE1, n, "COMMAND").await);
    }
    let plain = MailboxWorker::new(
        pool.clone(),
        "w-A",
        ACTOR_TYPE,
        WorkerConfig { batch: 2, lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(ProbeHandler),
    );
    plain.claim().await.expect("claim");
    let lane = plain.owned().await.into_iter().find(|l| l.partition == 1).expect("lane 1");
    assert_eq!(plain.drain_lane(&lane).await.expect("multi-batch drain"), 5);
    assert_eq!(
        probe_rows(&pool).await.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
        lane1_rows.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        "the whole backlog drained, in order, in one call"
    );

    // The boundary: 5 rows on lane 2, and the lane is stolen right after the first FULL batch
    // commits. The between-batch heartbeat must see the bumped fence and STOP the drain — the
    // stale owner never touches rows 3–5; the thief delivers exactly the remainder, in order.
    let mut lane2_rows = Vec::new();
    for n in 11..=15u128 {
        lane2_rows.push(enqueue(&pool, 2, 0xE2, n, "COMMAND").await);
    }
    let victim = MailboxWorker::new(
        pool.clone(),
        "w-victim",
        ACTOR_TYPE,
        WorkerConfig { batch: 2, lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(BatchBoundaryThief {
            pool: pool.clone(),
            steal_on: lane2_rows[1].0,
            thief: "w-thief",
        }),
    );
    // Phase A's worker holds every lane live — the victim takes lane 2 the deterministic way.
    let lane2 = steal_lane(&pool, ACTOR_TYPE, 2, "w-victim", 300)
        .await
        .expect("steal")
        .expect("lane 2");
    let out = victim.drain_lane(&lane2).await;
    assert!(
        matches!(out, Err(CompletionError::FencedOut)),
        "the batch boundary must stop a drain whose authority is gone: {out:?}"
    );

    let thief_lane = claim_due_lanes(&pool, ACTOR_TYPE, "w-thief", 300, 100)
        .await
        .expect("thief claims nothing new")
        .into_iter()
        .find(|l| l.partition == 2);
    assert!(thief_lane.is_none(), "the steal already owns lane 2 — nothing claimable");
    // The thief drains with ITS authority (fresh worker; the stolen Lane was returned to the
    // post-commit hook, so re-read the registry row the way a real thief's pass would).
    let thief = MailboxWorker::new(
        pool.clone(),
        "w-thief",
        ACTOR_TYPE,
        WorkerConfig { batch: 2, lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(ProbeHandler),
    );
    let stolen = sqlx::query(
        "SELECT actor_type, partition, ownership_version, checkpoint FROM mailbox_partitions \
         WHERE actor_type = $1 AND partition = 2",
    )
    .bind(ACTOR_TYPE)
    .fetch_one(&pool)
    .await
    .expect("registry row");
    let stolen_lane = actor_runtime::Lane {
        actor_type: stolen.get("actor_type"),
        partition: stolen.get("partition"),
        ownership_version: stolen.get("ownership_version"),
        checkpoint: stolen.get("checkpoint"),
    };
    assert_eq!(thief.drain_lane(&stolen_lane).await.expect("thief drains"), 3);

    // Exactly once, in order, across the boundary: rows 1–2 by the victim, 3–5 by the thief.
    let lane2_seen: Vec<uuid::Uuid> = probe_rows(&pool)
        .await
        .iter()
        .filter(|(_, p, _)| *p == 2)
        .map(|(id, _, _)| *id)
        .collect();
    assert_eq!(
        lane2_seen,
        lane2_rows.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        "no loss, no duplicate, order preserved across the authority change"
    );
}

// ================================================================================================
// FIFO per sender — interleaved concurrent producers on one lane, no kind precedence.
// ================================================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifo_per_sender_holds_under_interleaved_producers() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    actor_runtime::seed_partitions(&pool, ACTOR_TYPE, 4).await.expect("seed");

    // Three concurrent senders interleave on the SAME lane — distinct actors, mixed kinds
    // (COMMAND vs EVENT must get no precedence, pure position order).
    const PER_SENDER: u128 = 10;
    let mut tasks = Vec::new();
    for (k, kind) in [(1u128, "COMMAND"), (2, "EVENT"), (3, "COMMAND")] {
        let pool = pool.clone();
        tasks.push(tokio::spawn(async move {
            let mut sent = Vec::new();
            for i in 0..PER_SENDER {
                sent.push(enqueue(&pool, 1, 0xF0 + k, (k << 32) | i, kind).await);
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
            (k, sent)
        }));
    }
    let mut sent_per_sender = std::collections::HashMap::new();
    for t in tasks {
        let (k, sent) = t.await.expect("producer");
        sent_per_sender.insert(k, sent);
    }

    let w = MailboxWorker::new(
        pool.clone(),
        "w-A",
        ACTOR_TYPE,
        WorkerConfig { batch: 7, lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(ProbeHandler),
    );
    w.claim().await.expect("claim");
    assert_eq!(w.drain().await.expect("drain"), 3 * PER_SENDER as u64);

    let seen = probe_rows(&pool).await;
    // Global discipline: strict position order on the lane, every message exactly once.
    let mut last = 0i64;
    let mut ids = std::collections::HashSet::new();
    for (id, _, pos) in &seen {
        assert!(*pos > last, "deliveries must commit in strict position order");
        last = *pos;
        assert!(ids.insert(*id), "duplicate committed delivery");
    }
    // FIFO per sender: each producer's messages surface in ITS send order.
    for (k, sent) in &sent_per_sender {
        let delivered: Vec<uuid::Uuid> = seen
            .iter()
            .map(|(id, _, _)| *id)
            .filter(|id| sent.iter().any(|(sid, _)| sid == id))
            .collect();
        assert_eq!(
            delivered,
            sent.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            "sender {k}: send order must be delivery order"
        );
    }
}
