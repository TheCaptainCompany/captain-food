//! LIVE two-worker concurrency over the mailbox (the behavior test the drain/lease design must
//! survive, PROP-20260728-152752 §3/§3.1): two [`MailboxWorker`]s running their REAL `run` loops
//! against one Postgres, competing for the same lanes while messages keep arriving — plus a
//! mid-run worker death (shutdown → lanes released) with the survivor absorbing the load.
//!
//! Invariants asserted at the end, over the delivery probe (written THROUGH each completion
//! transaction, so it cannot over- or under-count):
//!
//! - **Exactly-once**: every enqueued message has exactly ONE committed delivery — no loss, no
//!   duplicate, through claim races, rebalancing and the takeover.
//! - **Per-actor order**: each actor's deliveries commit in strictly increasing `position` order
//!   (head-of-line per lane holds under competition, because an actor maps to exactly one lane).
//! - **The work actually spread**: both workers committed deliveries (the lease split is real,
//!   not one worker starving the other) — and after A dies, B finishes the backlog.
//!
//! Needs `DATABASE_URL` (docker `postgres:16-alpine` locally, the CI service, any real Postgres);
//! skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::Arc;

use actor_runtime::{
    Delivery, HandlerVerdict, InboundMessage, MailboxWorker, MessageHandler, WorkerConfig,
};
use sqlx::{PgPool, Postgres, Row, Transaction};

const ACTOR_TYPE: &str = "Conversation";
const PARTITIONS: i16 = 8;
const ACTORS: u128 = 20;
const MESSAGES: u128 = 400;

/// Records each delivery (worker, actor, position) through the completion transaction.
struct ProbeHandler {
    worker: &'static str,
}

#[async_trait::async_trait]
impl MessageHandler for ProbeHandler {
    async fn handle(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        _prepared: actor_runtime::Prepared,
    ) -> Result<Delivery, sqlx::Error> {
        sqlx::query(
            "INSERT INTO delivered_probe (worker, message_id, actor_id, position) VALUES ($1, $2, $3, $4)",
        )
        .bind(self.worker)
        .bind(message.message_id)
        .bind(message.actor_id)
        .bind(message.position)
        .execute(&mut **tx)
        .await?;
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
    .bind(uuid::Uuid::from_u128(0x1000 + n))
    .bind(ACTOR_TYPE)
    .bind(actor_id)
    .bind(partition)
    .bind(format!("h{n}"))
    .execute(pool)
    .await
    .expect("enqueue");
}

fn worker(pool: &PgPool, id: &'static str) -> MailboxWorker {
    MailboxWorker::new(
        pool.clone(),
        id,
        ACTOR_TYPE,
        // Fast loop so the test converges in seconds; small claim bound so BOTH workers get lanes
        // even when one starts a beat earlier.
        WorkerConfig { lease_seconds: 30, heartbeat_seconds: 1, batch: 20, max_claims_per_pass: 4, ..WorkerConfig::default() },
        Arc::new(ProbeHandler { worker: id }),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_live_workers_deliver_exactly_once_in_actor_order_through_a_death() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP actor concurrency test: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;
    actor_runtime::seed_partitions(&pool, ACTOR_TYPE, PARTITIONS).await.expect("seed");

    // Half the load is already waiting when the workers start.
    for n in 0..MESSAGES / 2 {
        enqueue(&pool, n).await;
    }

    let a = Arc::new(worker(&pool, "w-A"));
    let b = Arc::new(worker(&pool, "w-B"));
    let (stop_a, rx_a) = tokio::sync::watch::channel(false);
    let (stop_b, rx_b) = tokio::sync::watch::channel(false);
    let run_a = tokio::spawn({
        let a = a.clone();
        async move { a.run(rx_a).await }
    });
    let run_b = tokio::spawn({
        let b = b.clone();
        async move { b.run(rx_b).await }
    });

    // The other half arrives WHILE both loops are competing.
    for n in MESSAGES / 2..MESSAGES {
        enqueue(&pool, n).await;
        if n % 50 == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    // Let both make progress, then KILL A mid-run (graceful shutdown releases its lanes — the
    // expiry-takeover path is covered separately in mailbox.rs; here the point is the handover
    // under live load).
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let _ = stop_a.send(true);
    run_a.await.expect("A join").expect("A run");

    // B alone drains the rest.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM inbound_messages WHERE status = 'RECEIVED'",
        )
        .fetch_one(&pool)
        .await
        .expect("pending");
        if pending == 0 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{pending} messages still pending at the deadline");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let _ = stop_b.send(true);
    run_b.await.expect("B join").expect("B run");

    // ── The invariants ──
    let rows = sqlx::query("SELECT worker, message_id, actor_id, position FROM delivered_probe ORDER BY delivery")
        .fetch_all(&pool)
        .await
        .expect("probe");

    // Exactly-once: every message delivered, none twice.
    assert_eq!(rows.len() as u128, MESSAGES, "every enqueued message delivered exactly once");
    let mut seen = std::collections::HashSet::new();
    for r in &rows {
        assert!(seen.insert(r.get::<uuid::Uuid, _>("message_id")), "duplicate committed delivery");
    }

    // Per-actor order: strictly increasing position per actor across BOTH workers and the handover.
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

    // The work spread: both workers committed deliveries, and B (the survivor) took over A's share.
    let a_count = rows.iter().filter(|r| r.get::<String, _>("worker") == "w-A").count();
    let b_count = rows.iter().filter(|r| r.get::<String, _>("worker") == "w-B").count();
    assert!(a_count > 0, "worker A never delivered — the lease split did not happen");
    assert!(b_count > 0, "worker B never delivered — the lease split did not happen");
    assert_eq!(a_count + b_count, MESSAGES as usize);

    // Nothing left non-terminal, and every lane's checkpoint reached its last delivered position.
    let non_terminal: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM inbound_messages WHERE status NOT IN ('SUCCEEDED')",
    )
    .fetch_one(&pool)
    .await
    .expect("non-terminal");
    assert_eq!(non_terminal, 0);
    let lagging: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mailbox_partitions p WHERE p.checkpoint < \
         (SELECT COALESCE(MAX(m.position), 0) FROM inbound_messages m \
          WHERE m.actor_type = p.actor_type AND m.partition = p.partition)",
    )
    .fetch_one(&pool)
    .await
    .expect("lagging lanes");
    assert_eq!(lagging, 0, "every lane's checkpoint reached its last delivered position");
}
