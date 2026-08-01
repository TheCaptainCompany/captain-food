//! SMOKE of the standalone adapter worker fleet (#272 D3, `RUN_MAILBOX_WORKERS`): the
//! [`infrastructure::mailbox::spawn_standalone_workers`] helper — the adapter binaries' one call —
//! must deliver an enqueued command end to end with NO monolith composition root: seed, claim,
//! drain, fenced commit, terminal row. The delivery semantics themselves are proven by the
//! `mailbox_delivery`/`pm_prepare_delivery` suites (same handler, same worker); this guards the
//! standalone wiring — deps construction, supervision spawn, nudge hookup — from bit-rotting
//! unnoticed, which is exactly how the #270 review found the adapters ACKing into a void.
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::Arc;

use infrastructure::persistence::mailbox_store::MailboxNudges;
use infrastructure::FailClosedPaymentGateway;
use sqlx::PgPool;

async fn setup(pool: &PgPool) {
    sqlx::raw_sql(
        "DROP TABLE IF EXISTS inbound_messages, mailbox_partitions, domain_events, customer CASCADE;\n\
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
    sqlx::raw_sql(
        "CREATE TABLE customer (\n\
           customer_id UUID PRIMARY KEY,\n\
           phone TEXT NOT NULL UNIQUE,\n\
           auth_ref TEXT,\n\
           display_name TEXT,\n\
           email TEXT,\n\
           email_verified BOOLEAN NOT NULL,\n\
           locale TEXT,\n\
           timezone TEXT,\n\
           ratings JSONB NOT NULL,\n\
           favorite_restaurant_ids JSONB NOT NULL,\n\
           preferences JSONB,\n\
           addresses JSONB NOT NULL,\n\
           payment_method_id TEXT,\n\
           created_at TIMESTAMPTZ NOT NULL,\n\
           updated_at TIMESTAMPTZ NOT NULL\n\
         )",
    )
    .execute(pool)
    .await
    .expect("customer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_fleet_delivers_without_a_monolith() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP standalone_workers: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect");
    setup(&pool).await;

    let order = uuid::Uuid::from_u128(0x57A1);
    let partition = actor_runtime::stable_partition(&order, 100);
    let message_id = uuid::Uuid::from_u128(0x57A2);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash, \
            channel, user_type, correlation_id) \
         VALUES ($1, 'COMMAND', 'Conversation', $2, $3, 'OpenConversation', $4, 'h1', 'WORKER', 'ADMIN', $1)",
    )
    .bind(message_id)
    .bind(order)
    .bind(partition)
    .bind(serde_json::json!({
        "orderId": order,
        "restaurantId": uuid::Uuid::from_u128(0x57A3),
        "customerId": null,
        "customerChatEnabled": true,
    }))
    .execute(&pool)
    .await
    .expect("enqueue");

    let nudges = Arc::new(MailboxNudges::default());
    infrastructure::mailbox::spawn_standalone_workers(
        pool.clone(),
        "smoke",
        &["Conversation"],
        Arc::new(FailClosedPaymentGateway),
        nudges,
        Default::default(),
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
                .bind(message_id)
                .fetch_optional(&pool)
                .await
                .expect("status");
        if status.as_deref() == Some("SUCCEEDED") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "standalone fleet never delivered the row (status {status:?})"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let appended: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM domain_events WHERE stream_name = $1")
            .bind(format!("Conversation-{order}"))
            .fetch_one(&pool)
            .await
            .expect("events");
    assert_eq!(appended, 1, "the delivered command's fact reached the log");
}
