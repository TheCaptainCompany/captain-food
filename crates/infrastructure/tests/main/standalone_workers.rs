//! SMOKE of the standalone adapter worker fleet (#272 D3, `RUN_MAILBOX_WORKERS`): the
//! [`infrastructure::mailbox::spawn_standalone_workers`] helper — the adapter binaries' one call —
//! must deliver an enqueued command end to end with NO monolith composition root: seed, claim,
//! drain, fenced commit, terminal row. The delivery semantics themselves are proven by the
//! `mailbox_delivery`/`pm_prepare_delivery` suites (same handler, same worker); this guards the
//! standalone wiring — deps construction, supervision spawn, nudge hookup — from bit-rotting
//! unnoticed, which is exactly how the #270 review found the adapters ACKing into a void.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

use std::sync::Arc;

use infrastructure::persistence::mailbox_store::MailboxNudges;
use infrastructure::FailClosedPaymentGateway;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn standalone_fleet_delivers_without_a_monolith() {
    let Some(db) = crate::common::TestDb::acquire("standalone_workers").await else { return };
    let pool = db.pool();

    let order = uuid::Uuid::from_u128(0x57A1);
    let partition = actor_client::declared_lane("Conversation", &order)
        .expect("Conversation declares a mailbox");
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
