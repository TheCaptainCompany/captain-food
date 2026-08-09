//! END-TO-END retention pilot (#272 D2, ADR-20260731-153000/-150500/-214500): the Order actor's
//! `schedules:` declaration and the kind-MESSAGE delivery route, against a real Postgres.
//!
//! - delivering `MarkOrderDelivered` appends `OrderDelivered` AND upserts the SCHEDULED
//!   `OrderExpired` reminder in the SAME completion transaction (`apply_schedules_in_tx`), with
//!   the window taken from the wired configuration map — commit and clock start together;
//! - the promotion pass flips the due reminder to RECEIVED and the SAME worker's drain delivers
//!   it: the kind-MESSAGE route records `OrderExpired` on the order's stream with record
//!   semantics (cause-chained to the reminder row), verdict SUCCEEDED;
//! - a redelivered expiry (fresh mailbox identity, same fact) lands DUPLICATE and appends
//!   NOTHING — the fold-based dedupe stays authoritative;
//! - an expiry for an order whose stream does not exist lands IGNORED (nothing left to expire —
//!   never Rejected, ADR-20260731-153000 §1a).
//!
//! Needs `DATABASE_URL`; skips otherwise (DB_TESTS_REQUIRED makes the skip loud, #230).

use std::sync::Arc;

use actor_client::stable_partition;
use actor_runtime::{MailboxWorker, WorkerConfig};
use application::generated::services::{IdentityService, PaymentService};
use application::reminders::reminder_message_id;

use infrastructure::generated::command_router::CommandDeps;
use infrastructure::mailbox::MailboxCommandHandler;
use infrastructure::{
    FailClosedGoogleOwnershipVerifier, FailClosedIdentityService, FailClosedPaymentGateway,
    PgCatalogRepository, PgCustomerRepository, PgEventStore, PgProspectionRepository,
    PgRestaurantRepository, PgSlugReservationRepository, UnverifiedGbpOrderLinkProbe,
};
use sqlx::{PgPool, Row};

fn deps_over(pool: &PgPool) -> CommandDeps {
    CommandDeps {
        store: Arc::new(PgEventStore::new(pool.clone())),
        restaurants: Arc::new(PgRestaurantRepository::new(pool.clone())),
        slugs: Arc::new(PgSlugReservationRepository::new(pool.clone())),
        ownership: Arc::new(FailClosedGoogleOwnershipVerifier),
        probe: Arc::new(UnverifiedGbpOrderLinkProbe),
        prospection: Arc::new(PgProspectionRepository::new(pool.clone())),
        catalogs: Arc::new(PgCatalogRepository::new(pool.clone())),
        auth: Arc::new(FailClosedIdentityService) as Arc<dyn IdentityService>,
        customers: Arc::new(PgCustomerRepository::new(pool.clone())),
        sessions: Arc::new(application::auth_sessions::NoopAuthSessionStore),
        payments: Arc::new(FailClosedPaymentGateway) as Arc<dyn PaymentService>,
        pm_state: Arc::new(infrastructure::persistence::PgPaymentProcessState::new(pool.clone())),
        refund_state: Arc::new(infrastructure::persistence::PgRefundProcessState::new(
            pool.clone(),
        )),
        mailbox_requeue: Arc::new(infrastructure::persistence::mailbox_lanes::PgMailboxRequeue::new(pool.clone())),
    }
}

fn money(cents: i64) -> serde_json::Value {
    serde_json::json!({ "amountCents": cents, "currency": "EUR" })
}

async fn append_event(
    pool: &PgPool,
    stream: &str,
    version: i32,
    event_type: &str,
    payload: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO domain_events \
           (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, \
            payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 'ADMIN', $4, NULL, $5, $6, NULL, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream)
    .bind(version)
    .bind(uuid::Uuid::nil())
    .bind(event_type)
    .bind(payload)
    .execute(pool)
    .await
    .expect("seed event");
}

/// Enqueue one kind-MESSAGE row (what a promoted reminder redelivery under a fresh identity, or
/// a crashed scheduler's re-enqueue, would look like) — RECEIVED, immediately deliverable.
async fn enqueue_message(
    pool: &PgPool,
    n: u128,
    actor_id: uuid::Uuid,
    partition: i16,
    payload: serde_json::Value,
) -> uuid::Uuid {
    let id = uuid::Uuid::from_u128(n);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
            payload_hash, channel, user_type, correlation_id) \
         VALUES ($1, 'MESSAGE', 'Order', $2, $3, 'OrderExpired', $4, $5, 'WORKER', 'EXTERNAL', $1)",
    )
    .bind(id)
    .bind(actor_id)
    .bind(partition)
    .bind(&payload)
    .bind(format!("h{n}"))
    .execute(pool)
    .await
    .expect("enqueue message");
    id
}

#[tokio::test]
async fn terminal_delivery_schedules_expiry_and_the_promoted_reminder_records_it() {
    let Some(db) = crate::common::TestDb::acquire("mailbox_retention").await else { return };
    let pool = db.pool();

    let order = uuid::Uuid::from_u128(0x0AD1);
    let restaurant = uuid::Uuid::from_u128(0x0E57);
    let partition = stable_partition(&order, 5);
    let stream = format!("Order-{order}");

    // The order's life up to READY (camelCase payloads matching domain::generated::events).
    append_event(
        &pool,
        &stream,
        1,
        "OrderPlaced",
        serde_json::json!({
            "orderId": order,
            "restaurantId": restaurant,
            "customerContact": { "displayName": "Léa", "phone": "+33612345678" },
            "serviceType": "DELIVERY",
            "items": [{
                "offerId": uuid::Uuid::new_v4(),
                "name": "Margherita",
                "quantity": 1,
                "unitPrice": money(980),
                "lineTotal": money(980)
            }],
            "totalAmount": money(1580),
            "breakdown": {
                "articles": money(980),
                "delivery": money(400),
                "serviceFee": money(200),
                "total": money(1580),
                "restaurantContribution": money(80),
                "restaurantPayout": money(900),
                "riderPayout": money(400),
                "captainNet": money(280)
            }
        }),
    )
    .await;
    append_event(
        &pool,
        &stream,
        2,
        "OrderAcceptedByRestaurant",
        serde_json::json!({ "orderId": order, "restaurantId": restaurant }),
    )
    .await;
    append_event(
        &pool,
        &stream,
        3,
        "OrderMarkedReady",
        serde_json::json!({ "orderId": order, "restaurantId": restaurant }),
    )
    .await;

    // The terminal command, addressed like the GraphQL edge would.
    let deliver_id = uuid::Uuid::from_u128(0x11);
    sqlx::query(
        "INSERT INTO inbound_messages \
           (message_id, kind, actor_type, actor_id, partition, message_type, payload, \
            payload_hash, channel, user_type, correlation_id) \
         VALUES ($1, 'COMMAND', 'Order', $2, $3, 'MarkOrderDelivered', $4, 'h11', 'GRAPHQL', 'ADMIN', $1)",
    )
    .bind(deliver_id)
    .bind(order)
    .bind(partition)
    .bind(serde_json::json!({ "orderId": order, "restaurantId": restaurant }))
    .execute(&pool)
    .await
    .expect("enqueue MarkOrderDelivered");

    // The wired window: 30 days (the map the composition root builds from Config).
    let windows = std::collections::HashMap::from([("ORDER_RETENTION_WINDOW_DAYS", 30i64)]);
    let worker = MailboxWorker::new(
        pool.clone(),
        "w-A",
        "Order",
        WorkerConfig { lease_seconds: 300, ..WorkerConfig::default() },
        Arc::new(MailboxCommandHandler::new(deps_over(&pool)).with_reminder_windows(windows)),
    );
    worker.seed(5).await.expect("seed");
    worker.claim().await.expect("claim");
    assert_eq!(worker.drain().await.expect("drain"), 1, "the terminal command delivered");

    // OrderDelivered committed…
    let delivered: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderDelivered'",
    )
    .bind(&stream)
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(delivered, 1);

    // …and the reminder rode the SAME transaction: SCHEDULED, deterministic identity, no
    // position yet, due at +30 days (the wired window, not the spec default).
    let reminder_id = reminder_message_id(order, "OrderExpired");
    let row = sqlx::query(
        "SELECT kind, message_type, status, position, scheduled_at, cause_id \
         FROM inbound_messages WHERE message_id = $1",
    )
    .bind(reminder_id)
    .fetch_one(&pool)
    .await
    .expect("the scheduled reminder row");
    assert_eq!(row.get::<String, _>("kind"), "MESSAGE");
    assert_eq!(row.get::<String, _>("message_type"), "OrderExpired");
    assert_eq!(row.get::<String, _>("status"), "SCHEDULED");
    assert_eq!(row.get::<Option<i64>, _>("position"), None, "position is stamped at promotion");
    assert_eq!(
        row.get::<Option<uuid::Uuid>, _>("cause_id"),
        Some(deliver_id),
        "the reminder is cause-chained to the delivery that declared it"
    );
    let due = row.get::<chrono::DateTime<chrono::Utc>, _>("scheduled_at");
    let days_out = (due - chrono::Utc::now()).num_days();
    assert!((29..=30).contains(&days_out), "due ~30 days out, got {days_out}");

    // Not due yet: promotion is a no-op and the drain sees nothing.
    assert_eq!(worker.promote().await.expect("promote (undue)"), 0);
    assert_eq!(worker.drain().await.expect("drain (undue)"), 0);

    // Time passes. The promotion pass stamps a position and the drain records the expiry.
    sqlx::query("UPDATE inbound_messages SET scheduled_at = now() - interval '1 minute' WHERE message_id = $1")
        .bind(reminder_id)
        .execute(&pool)
        .await
        .expect("age the reminder");
    assert_eq!(worker.promote().await.expect("promote (due)"), 1);
    assert_eq!(worker.drain().await.expect("drain (due)"), 1, "the promoted reminder delivered");

    let expired = sqlx::query(
        "SELECT version, cause_id FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderExpired'",
    )
    .bind(&stream)
    .fetch_one(&pool)
    .await
    .expect("OrderExpired recorded");
    assert_eq!(expired.get::<i32, _>("version"), 5, "recorded after OrderDelivered (v4)");
    assert_eq!(
        expired.get::<Option<uuid::Uuid>, _>("cause_id"),
        Some(reminder_id),
        "the recorded fact is cause-chained to the reminder row"
    );
    let verdict: String =
        sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(reminder_id)
            .fetch_one(&pool)
            .await
            .expect("verdict");
    assert_eq!(verdict, "SUCCEEDED");

    // A redelivered expiry under a fresh identity: DUPLICATE, nothing appended.
    let replay = enqueue_message(
        &pool,
        0x22,
        order,
        partition,
        serde_json::json!({ "eventType": "OrderExpired", "payload": { "orderId": order } }),
    )
    .await;
    assert_eq!(worker.drain().await.expect("drain (replay)"), 1);
    let verdict: String =
        sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(replay)
            .fetch_one(&pool)
            .await
            .expect("replay verdict");
    assert_eq!(verdict, "DUPLICATE", "the fold-based dedupe stays authoritative");
    let expired_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM domain_events WHERE stream_name = $1 AND event_type = 'OrderExpired'",
    )
    .bind(&stream)
    .fetch_one(&pool)
    .await
    .expect("count");
    assert_eq!(expired_count, 1, "the replay appended NOTHING");

    // An expiry for a stream that does not exist (already erased): IGNORED, never Rejected.
    let ghost = uuid::Uuid::from_u128(0xDEAD);
    let ghost_partition = stable_partition(&ghost, 5);
    let ghost_msg = enqueue_message(
        &pool,
        0x33,
        ghost,
        ghost_partition,
        serde_json::json!({ "eventType": "OrderExpired", "payload": { "orderId": ghost } }),
    )
    .await;
    worker.claim().await.expect("claim (ghost lane)");
    assert!(worker.drain().await.expect("drain (ghost)") >= 1);
    let verdict: String =
        sqlx::query_scalar("SELECT status FROM inbound_messages WHERE message_id = $1")
            .bind(ghost_msg)
            .fetch_one(&pool)
            .await
            .expect("ghost verdict");
    assert_eq!(verdict, "IGNORED", "nothing left to expire is a no-op, never a rejection");
}
