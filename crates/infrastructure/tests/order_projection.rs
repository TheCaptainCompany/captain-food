//! Integration test for the Order read-side slice: events in `domain_events` → projection worker
//! (Order-stream registry group) → materialized `ordertracking` row → read repository. Needs a real
//! Postgres: set `DATABASE_URL` (see restaurant_projection.rs for a throwaway docker one-liner).
//! Without it the test SKIPS (prints and returns) so `cargo test` stays green offline.

use application::queries::{OrderFilter, OrderReadRepository as _};
use domain::generated::scalars::{CustomerId, OrderId, OrderStatus, RestaurantId};
use infrastructure::{PgOrderRepository, ProjectionWorker};
use sqlx::PgPool;

/// Fresh copies of the three tables the slice touches (mirrors migrations/20260717120000 + …170000).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS domain_events, ordertracking, projection_checkpoint CASCADE;
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
        CREATE TABLE ordertracking (
          order_id UUID PRIMARY KEY,
          ref TEXT NOT NULL,
          restaurant_id UUID NOT NULL,
          customer_id UUID,
          status INTEGER NOT NULL,
          service_type INTEGER NOT NULL,
          items JSONB NOT NULL,
          total_amount_cents BIGINT NOT NULL,
          currency TEXT NOT NULL,
          articles_cents BIGINT NOT NULL,
          delivery_cents BIGINT NOT NULL,
          service_fee_cents BIGINT NOT NULL,
          restaurant_payout_cents BIGINT NOT NULL,
          rider_payout_cents BIGINT NOT NULL,
          captain_net_cents BIGINT NOT NULL,
          uber_total_cents BIGINT,
          uber_restaurant_cents BIGINT,
          uber_rider_cents BIGINT,
          uber_platform_cents BIGINT,
          uber_basis INTEGER,
          delivery_address JSONB,
          estimated_ready_at TIMESTAMPTZ,
          placed_at TIMESTAMPTZ NOT NULL,
          status_changed_at TIMESTAMPTZ NOT NULL,
          payment_intent_id TEXT,
          payment_status TEXT NOT NULL,
          restaurant_stars INTEGER,
          rating_comment TEXT,
          rider_thumb INTEGER,
          rider_tip_cents BIGINT,
          restaurant_tip_cents BIGINT,
          captain_tip_cents BIGINT,
          rated_at TIMESTAMPTZ,
          delivery_status INTEGER,
          courier JSONB,
          estimated_dropoff_at TIMESTAMPTZ,
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

fn money(cents: i64) -> serde_json::Value {
    serde_json::json!({ "amountCents": cents, "currency": "EUR" })
}

#[tokio::test]
async fn order_events_fold_into_the_read_model() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP order_events_fold_into_the_read_model: DATABASE_URL not set");
        return;
    };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    let order_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let customer_id = uuid::Uuid::new_v4();
    let stream = format!("Order-{order_id}");

    // 1) The creation fact, camelCase payload matching domain::generated::events::OrderPlaced.
    append_event(
        &pool,
        &stream,
        1,
        "OrderPlaced",
        serde_json::json!({
            "orderId": order_id,
            "ref": "CF-0001",
            "restaurantId": restaurant_id,
            "customerId": customer_id,
            "customerContact": { "displayName": "Léa", "phone": "+33612345678" },
            "serviceType": "DELIVERY",
            "items": [{
                "offerId": uuid::Uuid::new_v4(),
                "name": "Margherita",
                "quantity": 2,
                "unitPrice": money(980),
                "lineTotal": money(1960)
            }],
            "totalAmount": money(2560),
            "breakdown": {
                "articles": money(1960),
                "delivery": money(400),
                "serviceFee": money(200),
                "total": money(2560),
                "restaurantContribution": money(160),
                "restaurantPayout": money(1800),
                "riderPayout": money(400),
                "captainNet": money(360)
            },
            "paymentIntentId": "pi_test_123"
        }),
    )
    .await;

    let worker = ProjectionWorker::new(pool.clone());
    worker.run_once().await.expect("run_once (placed)");

    // The row materialized, enums stored as declaration-order ordinals, breakdown leaves extracted,
    // payment PENDING until a Stripe fact lands — under the group's own 'Order' checkpoint.
    let (status, service_type, total, articles, payment_status): (i32, i32, i64, i64, String) =
        sqlx::query_as(
            "SELECT status, service_type, total_amount_cents, articles_cents, payment_status \
             FROM ordertracking WHERE order_id = $1",
        )
        .bind(order_id)
        .fetch_one(&pool)
        .await
        .expect("projected order row");
    assert_eq!(status, 0); // OrderStatus::PLACED ordinal
    assert_eq!(service_type, 0); // ServiceType::DELIVERY ordinal
    assert_eq!(total, 2560);
    assert_eq!(articles, 1960);
    assert_eq!(payment_status, "PENDING");
    let checkpoint: i64 =
        sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = 'Order'")
            .fetch_one(&pool)
            .await
            .expect("Order checkpoint");
    assert_eq!(checkpoint, 1);

    // 2) A lifecycle fact folds over the existing row (and run_once is idempotent past it).
    append_event(
        &pool,
        &stream,
        2,
        "OrderAcceptedByRestaurant",
        serde_json::json!({
            "orderId": order_id,
            "restaurantId": restaurant_id,
            "estimatedReadyAt": "2026-07-18T12:30:00Z"
        }),
    )
    .await;
    worker.run_once().await.expect("run_once (accepted)");
    worker.run_once().await.expect("run_once (no-op)");

    // 3) The read repository sees the folded state — by id and through the list filters.
    let repo = PgOrderRepository::new(pool.clone());
    let row = repo.by_id(OrderId(order_id)).await.expect("by_id").expect("order exists by id");
    assert_eq!(row.status, OrderStatus::ACCEPTED);
    assert_eq!(row.r#ref.0, "CF-0001");
    assert_eq!(row.restaurant_payout_cents.0, 1800);
    assert!(row.estimated_ready_at.is_some());
    assert!(row.status_changed_at >= row.placed_at);

    // Customer history scope.
    let history = repo
        .list(OrderFilter { customer_id: Some(CustomerId(customer_id)), ..Default::default() })
        .await
        .expect("list by customer");
    assert_eq!(history.len(), 1);

    // Back-office queue scope: restaurant + status (bound as its INTEGER ordinal).
    let queue = repo
        .list(OrderFilter {
            restaurant_id: Some(RestaurantId(restaurant_id)),
            status: Some(OrderStatus::ACCEPTED),
            ..Default::default()
        })
        .await
        .expect("list by restaurant+status");
    assert_eq!(queue.len(), 1);

    let placed = repo
        .list(OrderFilter { status: Some(OrderStatus::PLACED), ..Default::default() })
        .await
        .expect("list PLACED");
    assert!(placed.is_empty());

    // 4) Cross-stream feed (docs/sagas.md open item): payment facts land on Payment-{intentId}
    //    streams, but the Order group also slices `Payment-%` and keys the row from the payload's
    //    orderId. A capture without an orderId is log-skipped (no row, checkpoint still advances).
    append_event(
        &pool,
        "Payment-pi_orphan",
        1,
        "PaymentCaptured",
        serde_json::json!({
            "paymentIntentId": "pi_orphan",
            "orderId": null,
            "restaurantId": restaurant_id,
            "amount": money(2560)
        }),
    )
    .await;
    append_event(
        &pool,
        "Payment-pi_test_123",
        1,
        "PaymentCaptured",
        serde_json::json!({
            "paymentIntentId": "pi_test_123",
            "orderId": order_id,
            "restaurantId": restaurant_id,
            "amount": money(2560)
        }),
    )
    .await;
    worker.run_once().await.expect("run_once (captured)");

    let (payment_status, payment_intent_id): (String, Option<String>) = sqlx::query_as(
        "SELECT payment_status, payment_intent_id FROM ordertracking WHERE order_id = $1",
    )
    .bind(order_id)
    .fetch_one(&pool)
    .await
    .expect("captured order row");
    assert_eq!(payment_status, "CAPTURED");
    assert_eq!(payment_intent_id.as_deref(), Some("pi_test_123"));

    append_event(
        &pool,
        "Payment-pi_test_123",
        2,
        "PaymentRefunded",
        serde_json::json!({
            "refundId": "re_test_1",
            "paymentIntentId": "pi_test_123",
            "orderId": order_id,
            "restaurantId": restaurant_id,
            "amount": money(2560),
            "reason": "restaurant rejected"
        }),
    )
    .await;
    worker.run_once().await.expect("run_once (refunded)");

    let payment_status: String =
        sqlx::query_scalar("SELECT payment_status FROM ordertracking WHERE order_id = $1")
            .bind(order_id)
            .fetch_one(&pool)
            .await
            .expect("refunded order row");
    assert_eq!(payment_status, "REFUNDED");

    // Both Payment-stream events advanced the shared 'Order' checkpoint (2 order + 3 payment facts).
    let checkpoint: i64 =
        sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = 'Order'")
            .fetch_one(&pool)
            .await
            .expect("Order checkpoint after payment facts");
    assert_eq!(checkpoint, 5);
}
