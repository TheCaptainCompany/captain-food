//! DB-gated tests for the POISON BOUND (#313, PROP-20260802-223522 D4): a delivery whose
//! completion TRANSACTION fails records nothing on the row — the pre-#313 behaviour retried it
//! forever, silently wedging the lane's head-of-line. The cap turns that into bounded, visible
//! failure:
//!
//! 1. **Below the cap**: the attempt is counted, the row stays RECEIVED, and head-of-line holds —
//!    the row BEHIND the failing one is not delivered.
//! 2. **At the cap**: the row flips to terminal FAILED with the error recorded, and the SAME
//!    drain pass delivers the row behind it — the lane unblocks.
//! 3. **Cap 0 (the rollback lever)**: attempts keep counting, the row never flips — exactly the
//!    pre-#313 posture, kept executable so the lever cannot silently rot.
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::Arc;

use actor_runtime::{
    Delivery, HandlerVerdict, InboundMessage, MailboxWorker, MessageHandler, WorkerConfig,
};
use sqlx::{PgPool, Postgres, Row, Transaction};

const ACTOR_TYPE: &str = "Conversation";
const LANE: i16 = 2;

/// Fails the FIRST message's completion transaction with a real infrastructure error (a query
/// against a missing relation — the exact shape of the 2026-08-02 finding); delivers everything
/// else. The failure happens INSIDE the transaction, so the status flip aborts with it.
struct PoisonedHeadHandler {
    poison_id: uuid::Uuid,
}

#[async_trait::async_trait]
impl MessageHandler for PoisonedHeadHandler {
    async fn handle(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        message: &InboundMessage,
        _prepared: actor_runtime::Prepared,
    ) -> Result<Delivery, sqlx::Error> {
        if message.message_id == self.poison_id {
            sqlx::query("SELECT * FROM this_relation_does_not_exist")
                .execute(&mut **tx)
                .await?;
        }
        Ok(Delivery::of(HandlerVerdict::Succeeded))
    }
}

/// Serialize the suite: every test resets the same tables.
static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn database_url() -> Option<String> {
    match std::env::var("DATABASE_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            assert!(
                std::env::var("DB_TESTS_REQUIRED").is_err(),
                "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
            );
            eprintln!("SKIP poison: DATABASE_URL not set");
            None
        }
    }
}

async fn setup(pool: &PgPool) {
    sqlx::raw_sql(
        "DROP TABLE IF EXISTS inbound_messages, mailbox_partitions CASCADE;\n\
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
    sqlx::raw_sql(include_str!("../../../migrations/20260803004500_mailbox_backoff_next_attempt.sql"))
        .execute(pool)
        .await
        .expect("apply the mailbox backoff migration");
}

async fn enqueue(pool: &PgPool, n: u128) -> uuid::Uuid {
    let id = uuid::Uuid::from_u128(n);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id) \
         VALUES ($1, 'COMMAND', $2, $3, $4, 'PostMessage', '{}', $5, 'GRAPHQL', 'CUSTOMER', $1)",
    )
    .bind(id)
    .bind(ACTOR_TYPE)
    .bind(uuid::Uuid::from_u128(0xAC70))
    .bind(LANE)
    .bind(format!("h{n}"))
    .execute(pool)
    .await
    .expect("enqueue");
    id
}

async fn row_state(pool: &PgPool, id: uuid::Uuid) -> (String, i16, Option<serde_json::Value>) {
    let row = sqlx::query(
        "SELECT status, attempts, error FROM inbound_messages WHERE message_id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("row");
    (row.get("status"), row.get("attempts"), row.get("error"))
}

fn worker(pool: &PgPool, cap: i16, poison_id: uuid::Uuid) -> MailboxWorker {
    MailboxWorker::new(
        pool.clone(),
        "w-poison",
        ACTOR_TYPE,
        // Pacing off: these tests drive drain() back-to-back to count attempts deterministically;
        // the spacing window has its own test below.
        WorkerConfig {
            lease_seconds: 300,
            max_delivery_attempts: cap,
            retry_spacing_seconds: 0,
            ..WorkerConfig::default()
        },
        Arc::new(PoisonedHeadHandler { poison_id }),
    )
}

/// 1. Below the cap: attempts count, the row stays RECEIVED, head-of-line holds.
#[tokio::test]
async fn below_the_cap_the_attempt_is_counted_and_head_of_line_holds() {
    let Some(url) = database_url() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let poison = enqueue(&pool, 0x1).await;
    let behind = enqueue(&pool, 0x2).await;

    let w = worker(&pool, 5, poison);
    w.seed(5).await.expect("seed");
    w.claim().await.expect("claim");
    let delivered = w.drain().await.expect("drain pass survives the poisoned lane");
    assert_eq!(delivered, 0, "nothing may be delivered past the failing head");

    let (status, attempts, error) = row_state(&pool, poison).await;
    assert_eq!(status, "RECEIVED", "below the cap the row keeps retrying");
    assert_eq!(attempts, 1, "the failed transaction's only evidence is the counter");
    assert!(error.is_none(), "no error recorded while retrying");
    let (status, _, _) = row_state(&pool, behind).await;
    assert_eq!(status, "RECEIVED", "head-of-line: the row behind must NOT be delivered first");
}

/// 2. At the cap: FAILED + error recorded, and the SAME pass delivers the row behind.
#[tokio::test]
async fn at_the_cap_the_row_fails_terminally_and_the_lane_unblocks() {
    let Some(url) = database_url() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let poison = enqueue(&pool, 0x11).await;
    let behind = enqueue(&pool, 0x12).await;

    let w = worker(&pool, 2, poison);
    w.seed(5).await.expect("seed");
    w.claim().await.expect("claim");

    let first = w.drain().await.expect("pass 1");
    assert_eq!(first, 0, "attempt 1 of 2: still retrying, still blocking");
    let (status, attempts, _) = row_state(&pool, poison).await;
    assert_eq!((status.as_str(), attempts), ("RECEIVED", 1));

    let second = w.drain().await.expect("pass 2");
    let (status, attempts, error) = row_state(&pool, poison).await;
    assert_eq!(status, "FAILED", "attempt 2 of 2: terminal");
    assert_eq!(attempts, 2);
    let error = error.expect("the flip records the error — the row is the evidence");
    assert_eq!(error["code"], "DeliveryInfrastructureError");
    assert!(
        error["context"]["detail"].as_str().unwrap_or("").contains("this_relation_does_not_exist"),
        "the underlying database error is preserved: {error}"
    );
    assert_eq!(second, 1, "the SAME pass delivers the row behind — the lane unblocked");
    let (status, _, _) = row_state(&pool, behind).await;
    assert_eq!(status, "SUCCEEDED");
}

/// 3. Cap 0 is the rollback lever: infinite retry, attempts still visible.
#[tokio::test]
async fn cap_zero_keeps_the_pre_313_infinite_retry() {
    let Some(url) = database_url() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let poison = enqueue(&pool, 0x21).await;

    let w = worker(&pool, 0, poison);
    w.seed(5).await.expect("seed");
    w.claim().await.expect("claim");
    for pass in 1..=4i16 {
        let delivered = w.drain().await.expect("pass");
        assert_eq!(delivered, 0);
        let (status, attempts, error) = row_state(&pool, poison).await;
        assert_eq!(status, "RECEIVED", "cap 0 never flips");
        assert_eq!(attempts, pass, "attempts stay a visible diagnostic");
        assert!(error.is_none());
    }
}

/// 4. Exponential backoff (ADR-20260803-002712 Q2): attempt N schedules the next try
/// base x 2^(N-1) out — the schedule doubles, a storm of in-window drains consumes nothing, and
/// the lane still waits head-of-line. Time-to-poison is >= base x (2^cap - 1), not wake-bounded.
#[tokio::test]
async fn backoff_doubles_and_in_window_drains_consume_no_attempts() {
    let Some(url) = database_url() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let poison = enqueue(&pool, 0x31).await;

    let w = MailboxWorker::new(
        pool.clone(),
        "w-poison",
        ACTOR_TYPE,
        WorkerConfig {
            lease_seconds: 300,
            max_delivery_attempts: 3,
            retry_spacing_seconds: 100,
            ..WorkerConfig::default()
        },
        Arc::new(PoisonedHeadHandler { poison_id: poison }),
    );
    w.seed(5).await.expect("seed");
    w.claim().await.expect("claim");

    async fn schedule_secs(pool: &PgPool, id: uuid::Uuid) -> f64 {
        sqlx::query(
            "SELECT EXTRACT(EPOCH FROM (next_attempt_at - last_attempt_at))::float8 AS delay \
             FROM inbound_messages WHERE message_id = $1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("row")
        .get("delay")
    }

    let first = w.drain().await.expect("pass 1");
    assert_eq!(first, 0);
    let (status, attempts, _) = row_state(&pool, poison).await;
    assert_eq!((status.as_str(), attempts), ("RECEIVED", 1), "first attempt counted");
    assert_eq!(schedule_secs(&pool, poison).await, 100.0, "attempt 1 waits base x 1");

    // A storm of immediate re-drains (what a peak nudge burst looks like): all inside the
    // window — none may consume attempt 2, none may flip the row.
    for _ in 0..5 {
        let delivered = w.drain().await.expect("in-window drain");
        assert_eq!(delivered, 0);
    }
    let (status, attempts, error) = row_state(&pool, poison).await;
    assert_eq!(status, "RECEIVED", "still retrying -- the window protected the cap");
    assert_eq!(attempts, 1, "no attempt consumed inside the backoff window");
    assert!(error.is_none());

    // Fast-forward: expire the schedule and fail again — the NEXT window must double.
    sqlx::query("UPDATE inbound_messages SET next_attempt_at = now() - interval '1 second' WHERE message_id = $1")
        .bind(poison)
        .execute(&pool)
        .await
        .expect("expire window");
    let second = w.drain().await.expect("pass 2");
    assert_eq!(second, 0);
    let (status, attempts, _) = row_state(&pool, poison).await;
    assert_eq!((status.as_str(), attempts), ("RECEIVED", 2));
    assert_eq!(schedule_secs(&pool, poison).await, 200.0, "attempt 2 waits base x 2 -- doubling");
}
