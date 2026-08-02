//! DB-gated tests for the REMINDER write path (#272 Runtime D, ADR-20260731-150500 /
//! ADR-20260731-153000): the reference `schedule_reminder` over the real `PgMailbox` against the
//! real migration DDL — the deterministic `UUIDv5(actor_id, purpose)` identity, the SCHEDULED row
//! with `position` NULL (the sequence must NOT be consumed at declare time), reschedule IN PLACE
//! while SCHEDULED, Duplicate once the occurrence is spent, and `SCHEDULED → CANCELLED` beating
//! the promotion pass.
//!
//! Lived as unit tests inside `infrastructure::mailbox::enqueue` until #290 phase 1
//! (PROP-20260802-130500 D1) moved the constructors into the `actor_client` boundary crate; the
//! reference implementations (`schedule_reminder`/`cancel_reminder`) are reachable here through
//! the D5 `test-fixtures` feature — dev-dependencies only, CI-guarded.
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use actor_client::{cancel_reminder, reminder_message_id, schedule_reminder, ScheduleOutcome};
use domain::generated::scalars::InboundMessageStatus;
use infrastructure::persistence::mailbox_store::PgMailbox;
use sqlx::{PgPool, Row};

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
}

fn gated() -> Option<String> {
    match std::env::var("DATABASE_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            assert!(
                std::env::var("DB_TESTS_REQUIRED").is_err(),
                "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
            );
            eprintln!("SKIP mailbox schedule_pg test: DATABASE_URL not set");
            None
        }
    }
}

/// Serialize the suite: every test resets the same tables.
static DB_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn tagged(window: &str) -> serde_json::Value {
    serde_json::json!({
        "eventType": "OrderExpired",
        "payload": { "orderId": "9b2f8f3e-0000-0000-0000-000000000ad1", "window": window },
    })
}

async fn row(pool: &PgPool, message_id: uuid::Uuid) -> sqlx::postgres::PgRow {
    sqlx::query(
        "SELECT kind, message_type, channel, status, position, scheduled_at, completed_at, \
                payload, payload_hash \
         FROM inbound_messages WHERE message_id = $1",
    )
    .bind(message_id)
    .fetch_one(pool)
    .await
    .expect("row")
}

/// §1 (with ADR-153000 §1a): declaring the reminder writes ONE SCHEDULED row — kind MESSAGE,
/// message_type = the payload FACT, position NULL and the sequence untouched — and
/// re-declaring with a later time moves the SAME row in place.
#[tokio::test]
async fn schedule_then_redeclare_reschedules_the_same_row_in_place() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;
    let mailbox = PgMailbox::new(pool.clone());

    let order = uuid::Uuid::from_u128(0x0AD1);
    let t1 = chrono::Utc::now() + chrono::Duration::days(365);
    let out = schedule_reminder(&mailbox, "Order", order, "expire", tagged("P1Y"), t1, order)
        .await
        .expect("schedule");
    assert_eq!(out, ScheduleOutcome::Scheduled);

    let mid = reminder_message_id(order, "expire");
    let r = row(&pool, mid).await;
    assert_eq!(r.get::<String, _>("kind"), "MESSAGE");
    assert_eq!(r.get::<String, _>("message_type"), "OrderExpired");
    assert_eq!(r.get::<String, _>("channel"), "WORKER");
    assert_eq!(r.get::<String, _>("status"), "SCHEDULED");
    assert_eq!(r.get::<Option<i64>, _>("position"), None, "no position until promotion");
    assert_eq!(
        r.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_at").timestamp_micros(),
        t1.timestamp_micros()
    );
    // The declare must not have consumed the position sequence: the next immediate row takes 1.
    let next: i64 = sqlx::query("SELECT nextval('inbound_messages_position_seq') AS v")
        .fetch_one(&pool)
        .await
        .expect("nextval")
        .get("v");
    assert_eq!(next, 1, "declaring a reminder must not burn a position");

    // Re-declare with a LATER time and a newer payload: Rescheduled — same identity, one row,
    // scheduled_at and payload moved (ADR-150500 §1: the row "always carries the latest time").
    let t2 = t1 + chrono::Duration::days(365);
    let out = schedule_reminder(&mailbox, "Order", order, "expire", tagged("P2Y"), t2, order)
        .await
        .expect("reschedule");
    assert_eq!(out, ScheduleOutcome::Rescheduled);
    let count: i64 = sqlx::query("SELECT count(*) AS n FROM inbound_messages")
        .fetch_one(&pool)
        .await
        .expect("count")
        .get("n");
    assert_eq!(count, 1, "reschedule converges on ONE pending row");
    let r = row(&pool, mid).await;
    assert_eq!(
        r.get::<String, _>("status"),
        "SCHEDULED",
        "a reschedule never transitions status"
    );
    assert_eq!(r.get::<Option<i64>, _>("position"), None);
    assert_eq!(
        r.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_at").timestamp_micros(),
        t2.timestamp_micros()
    );
    assert_eq!(
        r.get::<serde_json::Value, _>("payload").pointer("/payload/window"),
        Some(&serde_json::json!("P2Y")),
        "the re-declaration's payload replaced the original"
    );
}

/// §2: once the promotion pass has stamped a position the occurrence is SPENT — a
/// re-declaration is a Duplicate (same payload) or a PayloadConflict (different payload), and
/// the row is untouched either way.
#[tokio::test]
async fn redeclaring_a_promoted_reminder_is_a_duplicate_untouched() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;
    let mailbox = PgMailbox::new(pool.clone());

    let order = uuid::Uuid::from_u128(0x0AD2);
    let due = chrono::Utc::now() - chrono::Duration::seconds(1);
    schedule_reminder(&mailbox, "Order", order, "expire", tagged("P1Y"), due, order)
        .await
        .expect("schedule");
    assert_eq!(actor_runtime::promote_due(&pool, "Order").await.expect("promote"), 1);

    // Same payload → the idempotent redelivery case, carrying the row's live status.
    let later = chrono::Utc::now() + chrono::Duration::days(30);
    let out = schedule_reminder(&mailbox, "Order", order, "expire", tagged("P1Y"), later, order)
        .await
        .expect("re-declare");
    assert_eq!(out, ScheduleOutcome::Deduplicated(InboundMessageStatus::RECEIVED));
    // Different payload → the conflict case. Untouched all the same.
    let out = schedule_reminder(&mailbox, "Order", order, "expire", tagged("P2Y"), later, order)
        .await
        .expect("re-declare, conflicting");
    assert_eq!(out, ScheduleOutcome::PayloadConflict(InboundMessageStatus::RECEIVED));

    let r = row(&pool, reminder_message_id(order, "expire")).await;
    assert_eq!(r.get::<String, _>("status"), "RECEIVED");
    assert_eq!(
        r.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_at").timestamp_micros(),
        due.timestamp_micros(),
        "a spent occurrence's scheduled_at never moves"
    );
    assert_eq!(
        r.get::<serde_json::Value, _>("payload").pointer("/payload/window"),
        Some(&serde_json::json!("P1Y")),
        "a spent occurrence's payload never moves"
    );
}

/// §3: cancellation is the explicit withdrawal — `SCHEDULED → CANCELLED`, terminal, never
/// promoted, and idempotently `false` on a second attempt (or after promotion won the race).
#[tokio::test]
async fn cancelled_reminder_is_never_promoted() {
    let Some(url) = gated() else { return };
    let _guard = DB_LOCK.lock().await;
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;
    let mailbox = PgMailbox::new(pool.clone());

    let order = uuid::Uuid::from_u128(0x0AD3);
    // Already due — so a promotion pass WOULD take it if cancellation did not win first.
    let due = chrono::Utc::now() - chrono::Duration::seconds(1);
    schedule_reminder(&mailbox, "Order", order, "expire", tagged("P1Y"), due, order)
        .await
        .expect("schedule");

    assert!(cancel_reminder(&mailbox, order, "expire").await.expect("cancel"));
    assert!(
        !cancel_reminder(&mailbox, order, "expire").await.expect("re-cancel"),
        "already CANCELLED"
    );
    assert_eq!(actor_runtime::promote_due(&pool, "Order").await.expect("promote"), 0);

    let r = row(&pool, reminder_message_id(order, "expire")).await;
    assert_eq!(r.get::<String, _>("status"), "CANCELLED");
    assert_eq!(r.get::<Option<i64>, _>("position"), None, "never promoted, never deliverable");
    assert!(r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("completed_at").is_some());
}
