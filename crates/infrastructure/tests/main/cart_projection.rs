//! Integration test for the Cart read-side slice: events in `domain_events` → projection worker
//! (Cart-stream registry group) → materialized `cart` row → read repository. Needs a real Postgres:
//! set `DATABASE_URL` (see restaurant_projection.rs for a throwaway docker one-liner). Without it the
//! test SKIPS (prints and returns) so `cargo test` stays green offline.

use application::queries::CartReadRepository as _;
use domain::generated::scalars::{CartId, CartStatus, CustomerId};
use infrastructure::{PgCartRepository, ProjectionWorker};
use sqlx::PgPool;

async fn append_event(
    pool: &PgPool,
    stream_name: &str,
    version: i32,
    event_type: &str,
    payload: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 5, $5, NULL, $6, $7, NULL, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream_name)
    .bind(version)
    .bind(uuid::Uuid::nil()) // acting user (ADMIN=5 above) — envelope metadata, ADR-0041
    .bind(uuid::Uuid::new_v4())
    .bind(event_type)
    .bind(payload)
    .execute(pool)
    .await
    .expect("append event");
}

#[tokio::test]
async fn cart_events_fold_into_the_read_model() {
    let Some(db) = crate::common::TestDb::acquire("cart_projection").await else { return };
    let pool = db.pool();

    let cart_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let customer_id = uuid::Uuid::new_v4();
    let session_id = uuid::Uuid::new_v4();
    let stream = format!("Cart-{cart_id}");

    // 1) The creation fact, camelCase payload matching domain::generated::events::CartStarted.
    append_event(
        &pool,
        &stream,
        1,
        "CartStarted",
        serde_json::json!({
            "cartId": cart_id,
            "restaurantId": restaurant_id,
            "sessionId": session_id,
            "customerId": customer_id
        }),
    )
    .await;

    let worker = ProjectionWorker::new(pool.clone());
    worker.run_once().await.expect("run_once (started)");

    // The row materialized, OPEN, under the group's own 'Cart' checkpoint. The fold is MONEY-FREE
    // (ADR-20260810-112836): no price columns exist — `lines` holds the repricing inputs (empty
    // until the Phase-2 line fold lands; the read side prices via price_cart).
    let (status, lines, projected_session): (String, serde_json::Value, uuid::Uuid) = sqlx::query_as(
        "SELECT status, lines, session_id FROM cart WHERE cart_id = $1",
    )
    .bind(cart_id)
    .fetch_one(&pool)
    .await
    .expect("projected cart row");
    assert_eq!(status, "OPEN"); // CartStatus::OPEN
    assert_eq!(lines, serde_json::json!([]));
    assert_eq!(projected_session, session_id);
    let checkpoint: i64 =
        sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = 'Cart'")
            .fetch_one(&pool)
            .await
            .expect("Cart checkpoint");
    assert_eq!(checkpoint, 1);

    // 2) Checkout folds over the existing row (and run_once is idempotent past it).
    append_event(
        &pool,
        &stream,
        2,
        "CartCheckedOut",
        serde_json::json!({ "cartId": cart_id, "orderId": uuid::Uuid::new_v4() }),
    )
    .await;
    worker.run_once().await.expect("run_once (checked out)");
    worker.run_once().await.expect("run_once (no-op)");

    // 3) The read repository sees the folded state through the typed row.
    let repo = PgCartRepository::new(pool.clone());
    let carts = repo.by_customer(CustomerId(customer_id)).await.expect("by_customer");
    assert_eq!(carts.len(), 1);
    assert_eq!(carts[0].cart_id.0, cart_id);
    assert_eq!(carts[0].status, CartStatus::CHECKED_OUT);
    assert_eq!(carts[0].restaurant_id.0, restaurant_id);
    assert_eq!(carts[0].lines, serde_json::json!([]));

    let by_id = repo.by_id(CartId(cart_id)).await.expect("by_id").expect("cart exists by id");
    assert_eq!(by_id.customer_id.map(|c| c.0), Some(customer_id));
    assert!(by_id.created_at <= by_id.updated_at);

    let absent = repo.by_id(CartId(uuid::Uuid::new_v4())).await.expect("by_id (absent)");
    assert!(absent.is_none());
}
