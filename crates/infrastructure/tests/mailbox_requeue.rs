//! E2E of the poisoned-row recovery (#315, ADR-20260803-002712 Q1) on real Postgres: a
//! cap-poisoned `inbound_messages` row is listed by the supervision read
//! ([`application::queries::MailboxLaneRepository::poisoned`]), then a `RequeueMailboxMessage`
//! command delivered through a REAL worker fleet (the same standalone spawn the adapters use)
//! flips it back to RECEIVED — attempts reset, error and backoff schedule cleared — and records
//! the `MailboxMessageRequeued` audit fact on the row's `MailboxSupervision` stream. The port's
//! arbitration matrix (already-deliverable converges, handler verdicts refuse, unknown refuses)
//! is asserted directly on [`PgMailboxRequeue`].
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::Arc;

use application::queries::{MailboxLaneRepository, MailboxRequeue, RequeueOutcome};
use infrastructure::persistence::mailbox_lanes::{PgMailboxLaneRepository, PgMailboxRequeue};
use infrastructure::persistence::mailbox_store::MailboxNudges;
use infrastructure::FailClosedPaymentGateway;
use sqlx::PgPool;

async fn setup(pool: &PgPool) {
    sqlx::raw_sql(
        "DROP TABLE IF EXISTS inbound_messages, mailbox_partitions, domain_events CASCADE;\n\
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
    sqlx::raw_sql(
        "CREATE TABLE domain_events (\n\
           position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\n\
           id UUID NOT NULL UNIQUE,\n\
           stream_name TEXT NOT NULL,\n\
           version INTEGER NOT NULL,\n\
           user_id UUID NOT NULL,\n\
           user_type TEXT NOT NULL,\n\
           correlation_id UUID NOT NULL,\n\
           cause_id UUID NULL,\n\
           event_type TEXT NOT NULL,\n\
           payload JSONB NOT NULL,\n\
           metadata JSONB NULL,\n\
           occurred_at TIMESTAMPTZ NOT NULL,\n\
           expired_at TIMESTAMPTZ NULL,\n\
           UNIQUE (stream_name, version)\n\
         )",
    )
    .execute(pool)
    .await
    .expect("domain_events");
}

/// Insert one cap-poisoned row on the Cart lane and return its message id.
async fn seed_poisoned(pool: &PgPool, message_id: uuid::Uuid) {
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id, status, attempts, error, completed_at) \
         VALUES ($1, 'COMMAND', 'Cart', $2, 3, 'AddCartLine', '{}'::jsonb, 'h-poison', \
            'GRAPHQL', 'CUSTOMER', $1, 'FAILED', 5, \
            '{\"code\": \"DeliveryInfrastructureError\", \"context\": {\"error\": \"relation catalog does not exist\"}}'::jsonb, now())",
    )
    .bind(message_id)
    .bind(uuid::Uuid::from_u128(0xCA57))
    .execute(pool)
    .await
    .expect("seed poisoned row");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requeue_recovers_a_poisoned_row_end_to_end() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP mailbox_requeue: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let poisoned = uuid::Uuid::from_u128(0xB0150);
    seed_poisoned(&pool, poisoned).await;

    // (1) The supervision read surfaces it — id, lane, attempts, and the poison error code.
    let listed = PgMailboxLaneRepository::new(pool.clone())
        .poisoned(None, 200)
        .await
        .expect("poisoned listing");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].message_id, poisoned);
    assert_eq!(listed[0].actor_type, "Cart");
    assert_eq!(listed[0].attempts, 5);
    assert_eq!(listed[0].error_code.as_deref(), Some("DeliveryInfrastructureError"));
    // The lane filter matches and misses correctly.
    assert_eq!(
        PgMailboxLaneRepository::new(pool.clone())
            .poisoned(Some("Payment".into()), 200)
            .await
            .expect("filtered listing")
            .len(),
        0
    );

    // (2) Deliver RequeueMailboxMessage through a REAL fleet on the MailboxSupervision lane —
    // the adapters' spawn, the generated router, the fenced completion.
    let requeue_cmd = uuid::Uuid::from_u128(0xB0151);
    let partition = actor_client::stable_partition(&poisoned, 1);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id) \
         VALUES ($1, 'COMMAND', 'MailboxSupervision', $2, $3, 'RequeueMailboxMessage', $4, 'h-requeue', \
            'GRAPHQL', 'ADMIN', $1)",
    )
    .bind(requeue_cmd)
    .bind(poisoned)
    .bind(partition)
    .bind(serde_json::json!({ "targetMessageId": poisoned }))
    .execute(&pool)
    .await
    .expect("enqueue requeue command");

    let nudges = Arc::new(MailboxNudges::default());
    infrastructure::mailbox::spawn_standalone_workers(
        pool.clone(),
        "smoke",
        &["MailboxSupervision"],
        Arc::new(FailClosedPaymentGateway),
        nudges,
        Default::default(),
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
                .bind(requeue_cmd)
                .fetch_optional(&pool)
                .await
                .expect("requeue status");
        if status.as_deref() == Some("SUCCEEDED") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the fleet never delivered the requeue command (status {status:?})"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    // (3) The target row is deliverable again: RECEIVED, attempts reset, error + schedule gone.
    let row = sqlx::query_as::<_, (String, i16, Option<serde_json::Value>)>(
        "SELECT status, attempts, error FROM inbound_messages WHERE message_id = $1",
    )
    .bind(poisoned)
    .fetch_one(&pool)
    .await
    .expect("target row");
    assert_eq!(row.0, "RECEIVED", "the poisoned row is deliverable again");
    assert_eq!(row.1, 0, "attempts reset");
    assert!(row.2.is_none(), "error cleared");

    // (4) The audit fact is on the supervision stream, naming the lane.
    let (event_type, payload) = sqlx::query_as::<_, (String, serde_json::Value)>(
        "SELECT event_type, payload FROM domain_events WHERE stream_name = $1",
    )
    .bind(format!("MailboxSupervision-{poisoned}"))
    .fetch_one(&pool)
    .await
    .expect("audit fact");
    assert_eq!(event_type, "MailboxMessageRequeued");
    assert_eq!(payload["actorType"], "Cart");
    assert_eq!(payload["targetMessageId"], serde_json::json!(poisoned));

    // (5) The arbitration matrix, directly on the port: already-deliverable converges (the
    // retried-delivery case), a handler verdict refuses with its status, unknown refuses.
    let port = PgMailboxRequeue::new(pool.clone());
    assert_eq!(
        port.requeue_if_poisoned(poisoned).await.expect("re-requeue"),
        RequeueOutcome::AlreadyDeliverable { actor_type: "Cart".into() },
        "a duplicate/retried requeue converges instead of erroring"
    );
    assert_eq!(
        port.requeue_if_poisoned(requeue_cmd).await.expect("succeeded row"),
        RequeueOutcome::NotRequeueable { status: "SUCCEEDED".into() },
        "a row that already ran is never requeued"
    );
    assert_eq!(
        port.requeue_if_poisoned(uuid::Uuid::from_u128(0xDEAD)).await.expect("unknown row"),
        RequeueOutcome::NotFound
    );
    // And the listing is empty again — the screen's count and detail agree.
    assert_eq!(
        PgMailboxLaneRepository::new(pool.clone()).poisoned(None, 200).await.expect("relist").len(),
        0
    );
}
