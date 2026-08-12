//! The GENERIC deletion engine (#272 D2, ADR-20260731-214500 §4 / ADR-20260731-160000 §5-§6 /
//! ADR-20260801-010134) against a real Postgres:
//!
//! - a recorded `OrderExpired` fact drives the full journey: technical `$StreamTombstoned`
//!   instruction, stream deletion from `domain_events` + `domain_stream`, and the `OrderDeleted`
//!   receipt on `DeletionLedger-Order` — pseudonymous references + the window-policy key + the
//!   tombstone stamp, never the erased payloads;
//! - the scan is BOUNDED by the slowest projection checkpoint: a read model that has not folded
//!   the expiry yet DEFERS the journey (phase-1 verification), and catching the checkpoint up
//!   releases it;
//! - a re-run after completion is a no-op (the cursor advanced atomically with the deletion);
//! - `PgEventStore::load` skips `$`-prefixed technical rows while still counting their version.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

use application::ports::EventStore;
use infrastructure::{DeletionEngine, PgEventStore};
use sqlx::{PgPool, Row};

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

/// Seed one order's whole life up to its recorded expiry; returns the stream name.
async fn seed_expired_order(
    pool: &PgPool,
    order: uuid::Uuid,
    restaurant: uuid::Uuid,
    customer: Option<uuid::Uuid>,
) -> String {
    let stream = format!("Order-{order}");
    append_event(
        pool,
        &stream,
        1,
        "OrderPlaced",
        serde_json::json!({
            "orderId": order,
            "restaurantId": restaurant,
            "customerId": customer,
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
        pool,
        &stream,
        2,
        "OrderDelivered",
        serde_json::json!({ "orderId": order, "restaurantId": restaurant }),
    )
    .await;
    append_event(pool, &stream, 3, "OrderExpired", serde_json::json!({ "orderId": order })).await;
    // A per-stream retention config row — the journey must remove it with the stream.
    sqlx::query("INSERT INTO domain_stream (stream_name, max_count) VALUES ($1, 100) ON CONFLICT DO NOTHING")
        .bind(&stream)
        .execute(pool)
        .await
        .expect("domain_stream row");
    stream
}

async fn stream_rows(pool: &PgPool, stream: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM domain_events WHERE stream_name = $1")
        .bind(stream)
        .fetch_one(pool)
        .await
        .expect("count")
}

#[tokio::test]
async fn the_journey_erases_the_stream_and_records_the_receipt() {
    let Some(db) = crate::common::TestDb::acquire("deletion_engine").await else { return };
    let pool = db.pool();

    let order = uuid::Uuid::from_u128(0x0AD1);
    let restaurant = uuid::Uuid::from_u128(0x0E57);
    let customer = uuid::Uuid::from_u128(0xC057);
    let stream = seed_expired_order(&pool, order, restaurant, Some(customer)).await;

    let engine = DeletionEngine::new(pool.clone()).expect("every declared policy is servable");
    // No projection checkpoints at all: nothing to wait for — the journey runs.
    assert_eq!(engine.run_once().await.expect("tick"), 1, "one stream erased");

    // The streams are gone — events AND the per-stream retention config row.
    assert_eq!(stream_rows(&pool, &stream).await, 0, "domain_events rows erased");
    let cfg: i64 = sqlx::query_scalar("SELECT count(*) FROM domain_stream WHERE stream_name = $1")
        .bind(&stream)
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(cfg, 0, "domain_stream config row erased");

    // The receipt on the ledger: pseudonymous references, the window-policy key, the tombstone
    // stamp — and NOTHING of the erased payloads (address, phone, items).
    let receipt = sqlx::query(
        "SELECT event_type, version, payload FROM domain_events WHERE stream_name = 'DeletionLedger-Order'",
    )
    .fetch_one(&pool)
    .await
    .expect("the ledger receipt");
    assert_eq!(receipt.get::<String, _>("event_type"), "OrderDeleted");
    assert_eq!(receipt.get::<i32, _>("version"), 1);
    let payload = receipt.get::<serde_json::Value, _>("payload");
    assert_eq!(payload["orderId"], serde_json::json!(order.to_string()));
    assert_eq!(payload["restaurantId"], serde_json::json!(restaurant.to_string()));
    assert_eq!(payload["customerId"], serde_json::json!(customer.to_string()));
    assert_eq!(payload["policy"], serde_json::json!("ORDER_RETENTION_WINDOW_DAYS"));
    assert!(payload["tombstoneEventId"].is_string(), "the 'deleted thanks to tombstone <id>' stamp");
    assert!(payload.get("customerContact").is_none(), "erased payloads never reach the receipt");

    // The receipt deserializes as the typed business fact — the ledger stream stays loadable.
    let store = PgEventStore::new(pool.clone());
    let (events, version) = store.load("DeletionLedger-Order").await.expect("ledger load");
    assert_eq!(version, 1);
    assert!(
        matches!(events.as_slice(), [domain::generated::events::DomainEvent::OrderDeleted(_)]),
        "typed OrderDeleted on the ledger"
    );

    // Completion is durable: a re-run sees nothing (the cursor advanced with the deletion).
    assert_eq!(engine.run_once().await.expect("re-tick"), 0);
}

#[tokio::test]
async fn a_lagging_projection_defers_the_journey_until_it_folds_the_expiry() {
    let Some(db) = crate::common::TestDb::acquire("deletion_engine").await else { return };
    let pool = db.pool();

    let order = uuid::Uuid::from_u128(0x0AD2);
    let restaurant = uuid::Uuid::from_u128(0x0E57);
    // The order carries a customer like every post-#144 order — OrderPlaced.customerId is
    // REQUIRED now, so the "guest order" class is unrepresentable in a log born after #144 (and
    // this deployment's log was empty at the change; the narrowing is the recorded exception).
    let customer = uuid::Uuid::new_v4();
    let stream = seed_expired_order(&pool, order, restaurant, Some(customer)).await;
    let head: i64 = sqlx::query_scalar("SELECT MAX(position) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("head");

    // The Order read-model group has NOT folded the expiry yet: its checkpoint sits before it.
    sqlx::query("INSERT INTO projection_checkpoint (projector, position) VALUES ('Order', $1)")
        .bind(head - 1)
        .execute(&pool)
        .await
        .expect("lagging checkpoint");

    let engine = DeletionEngine::new(pool.clone()).expect("policies servable");
    assert_eq!(engine.run_once().await.expect("tick (lagging)"), 0, "journey deferred");
    assert_eq!(stream_rows(&pool, &stream).await, 3, "nothing erased while a read model lags");

    // The projection catches up — the SAME fact now clears the bound and the journey runs.
    sqlx::query("UPDATE projection_checkpoint SET position = $1 WHERE projector = 'Order'")
        .bind(head)
        .execute(&pool)
        .await
        .expect("caught-up checkpoint");
    assert_eq!(engine.run_once().await.expect("tick (caught up)"), 1);
    assert_eq!(stream_rows(&pool, &stream).await, 0, "erased once the fold is verified");

    // The receipt names the erased order's customer — the accountability reference the erasure
    // ADR designed the receipt around. (The wire field stays nullable for logs that predate
    // #144's required customerId; no such log exists for this deployment.)
    let payload: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM domain_events WHERE stream_name = 'DeletionLedger-Order'",
    )
    .fetch_one(&pool)
    .await
    .expect("receipt");
    assert_eq!(payload["customerId"], serde_json::json!(customer.to_string()));
}

#[tokio::test]
async fn load_skips_technical_rows_but_counts_their_version() {
    let Some(db) = crate::common::TestDb::acquire("deletion_engine").await else { return };
    let pool = db.pool();
    // Own schema namespace: reuse setup (serial within this file is unnecessary — each test
    // rebuilds the schema, and the three tests share no assertions across runs).

    let order = uuid::Uuid::from_u128(0x0AD3);
    let stream = format!("Order-{order}");
    append_event(&pool, &stream, 1, "OrderExpired", serde_json::json!({ "orderId": order })).await;
    append_event(
        &pool,
        &stream,
        2,
        "$StreamTombstoned",
        serde_json::json!({ "receipt": "OrderDeleted" }),
    )
    .await;

    let store = PgEventStore::new(pool.clone());
    let (events, version) = store.load(&stream).await.expect("load with a technical row");
    assert_eq!(events.len(), 1, "the `$` row is not domain vocabulary");
    assert_eq!(version, 2, "but its version still counts (optimistic concurrency)");
}
