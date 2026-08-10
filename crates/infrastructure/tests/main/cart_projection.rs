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
    //
    // `by_customer` is OPEN-ONLY as of #451 — a real SQL predicate, not a courtesy. Every consumer
    // prices what this port returns against TODAY's catalog, and this cart's money was frozen when
    // it checked out, so repricing it would render a number that never matched what was charged.
    // The cart just went CHECKED_OUT above, so the list is now EMPTY: that is the contract working,
    // not the projection losing a row.
    let repo = PgCartRepository::new(pool.clone());
    let carts = repo.by_customer(CustomerId(customer_id)).await.expect("by_customer");
    assert!(
        carts.is_empty(),
        "a CHECKED_OUT cart must not appear in the priced-list port (#451): {carts:?}"
    );

    // by-id is OPEN-only too (#451): ONE rule, both lookups. A CHECKED_OUT cart is `None` here —
    // post-checkout money is read from the Order, the aggregate that owns what was charged, never
    // re-derived from a cart against today's catalog.
    assert!(
        repo.by_id(CartId(cart_id)).await.expect("by_id").is_none(),
        "a CHECKED_OUT cart must not be readable through the priced by-id port (#451)"
    );

    // But the ROW is still there and still correctly folded — read beneath the port so the
    // emptiness above is provably the STATUS PREDICATE and not a lost projection. This assertion
    // is also what fails if the money-free upsert cannot write (the pre-#451 NOT NULL columns):
    // it is the difference between "filtered" and "the projector never wrote anything".
    let (raw_status, raw_lines, raw_restaurant): (String, serde_json::Value, uuid::Uuid) =
        sqlx::query_as("SELECT status, lines, restaurant_id FROM cart WHERE cart_id = $1")
            .bind(cart_id)
            .fetch_one(&pool)
            .await
            .expect("the checked-out row is still projected");
    assert_eq!(raw_status, "CHECKED_OUT");
    assert_eq!(raw_lines, serde_json::json!([]));
    assert_eq!(raw_restaurant, restaurant_id);

    // And while it was still OPEN the same ports DID return it — otherwise "empty" above would pass
    // even if they were broken outright. Fold a second, still-OPEN cart to prove the positive half
    // against the real SQL, through BOTH lookups.
    let open_cart = uuid::Uuid::new_v4();
    let open_stream = format!("Cart-{open_cart}");
    append_event(
        &pool,
        &open_stream,
        1,
        "CartStarted",
        serde_json::json!({
            "cartId": open_cart, "restaurantId": restaurant_id,
            "sessionId": uuid::Uuid::new_v4(), "customerId": customer_id,
        }),
    )
    .await;
    worker.run_once().await.expect("run_once (second cart)");
    let carts = repo.by_customer(CustomerId(customer_id)).await.expect("by_customer (open)");
    assert_eq!(carts.len(), 1, "the OPEN cart is returned: {carts:?}");
    assert_eq!(carts[0].cart_id.0, open_cart);
    assert_eq!(carts[0].status, CartStatus::OPEN);

    let open_by_id =
        repo.by_id(CartId(open_cart)).await.expect("by_id (open)").expect("the OPEN cart by id");
    assert_eq!(open_by_id.cart_id.0, open_cart);
    assert_eq!(open_by_id.status, CartStatus::OPEN);
    assert_eq!(open_by_id.restaurant_id.0, restaurant_id);
    assert_eq!(open_by_id.customer_id.map(|c| c.0), Some(customer_id));
    assert!(open_by_id.created_at <= open_by_id.updated_at);

    let absent = repo.by_id(CartId(uuid::Uuid::new_v4())).await.expect("by_id (absent)");
    assert!(absent.is_none());
}
