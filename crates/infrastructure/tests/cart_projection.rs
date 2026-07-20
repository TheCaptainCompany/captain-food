//! Integration test for the Cart read-side slice: events in `domain_events` → projection worker
//! (Cart-stream registry group) → materialized `cart` row → read repository. Needs a real Postgres:
//! set `DATABASE_URL` (see restaurant_projection.rs for a throwaway docker one-liner). Without it the
//! test SKIPS (prints and returns) so `cargo test` stays green offline.

use application::queries::CartReadRepository as _;
use domain::generated::scalars::{CartId, CartStatus, CustomerId};
use infrastructure::{PgCartRepository, ProjectionWorker};
use sqlx::PgPool;

/// Fresh copies of the three tables the slice touches (mirrors migrations/20260717120000 + …170000).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS domain_events, cart, projection_checkpoint CASCADE;
        CREATE TABLE domain_events (
          position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
          id UUID NOT NULL UNIQUE,
          stream_name TEXT NOT NULL,
          version INTEGER NOT NULL,
          user_id UUID NOT NULL,
          user_type INTEGER NOT NULL,
          correlation_id UUID NOT NULL,
          cause_id UUID NULL,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          metadata JSONB NULL,
          occurred_at TIMESTAMPTZ NOT NULL,
          expired_at TIMESTAMPTZ NULL,
          UNIQUE (stream_name, version)
        );
        CREATE TABLE cart (
          cart_id UUID PRIMARY KEY,
          restaurant_id UUID NOT NULL,
          session_id UUID NOT NULL,
          customer_id UUID,
          status INTEGER NOT NULL,
          lines JSONB NOT NULL,
          total_amount_cents BIGINT NOT NULL,
          currency TEXT NOT NULL,
          estimated_breakdown JSONB,
          uber_comparison JSONB,
          created_at TIMESTAMPTZ NOT NULL,
          updated_at TIMESTAMPTZ NOT NULL
        );
        CREATE TABLE projection_checkpoint (
          projector  TEXT        PRIMARY KEY,
          position   BIGINT      NOT NULL DEFAULT 0,
          updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        "#,
    )
    .execute(pool)
    .await
    .expect("reset schema");
}

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
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP cart_events_fold_into_the_read_model: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

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

    // The row materialized, OPEN, under the group's own 'Cart' checkpoint. The priced columns hold the
    // projector's documented defaults (lines [], total 0 EUR — pricing is TODO(runtime)).
    let (status, total, currency, projected_session): (i32, i64, String, uuid::Uuid) = sqlx::query_as(
        "SELECT status, total_amount_cents, currency, session_id FROM cart WHERE cart_id = $1",
    )
    .bind(cart_id)
    .fetch_one(&pool)
    .await
    .expect("projected cart row");
    assert_eq!(status, 0); // CartStatus::OPEN ordinal
    assert_eq!(total, 0);
    assert_eq!(currency, "EUR");
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
