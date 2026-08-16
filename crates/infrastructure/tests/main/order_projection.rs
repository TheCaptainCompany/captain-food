//! Integration test for the Order read-side slice: events in `domain_events` → projection worker
//! (Order-stream registry group) → materialized `ordertracking` row → read repository. Needs a real
//! Postgres: set `DATABASE_URL` (see restaurant_projection.rs for a throwaway docker one-liner).
//! Without it the test SKIPS (prints and returns) so `cargo test` stays green offline.

use std::sync::{Arc, Mutex};

use application::generated::services::{
    PaymentCaptureInput, PaymentRefundInput, PaymentReleaseInput, PaymentRequestInput,
    PaymentRequestOutput, PaymentService, ServiceCallMeta,
};
use application::queries::{OrderFilter, ReadScope, OrderReadRepository as _};
use async_trait::async_trait;
use domain::generated::scalars::{CustomerId, OrderId, OrderStatus, RestaurantId};
use domain::shared::errors::DomainError;
use infrastructure::{PgOrderRepository, ProcessManagerRunner, ProjectionWorker};
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

fn money(cents: i64) -> serde_json::Value {
    serde_json::json!({ "amountCents": cents, "currency": "EUR" })
}

fn address(line1: &str) -> serde_json::Value {
    serde_json::json!({
        "line1": line1, "postalCode": "37000", "city": "Tours", "country": "FR"
    })
}

#[tokio::test]
async fn order_events_fold_into_the_read_model() {
    let Some(db) = crate::common::TestDb::acquire("order_projection").await else { return };
    let pool = db.pool();

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

    // The row materialized, enums stored as their TEXT values, breakdown leaves extracted,
    // payment AUTHORIZED (authorize-then-capture, ADR-20260808-195315 §1.2): PlaceOrderProcess
    // emits OrderPlaced only in reaction to PaymentAuthorized, and that authorization sits EARLIER
    // in the log than the row it would fold into — the creation arm carries the saga invariant
    // instead of losing it. Capture arrives later, on fulfilment, through the PaymentCaptured arm.
    let (status, service_type, total, articles, payment_status): (String, String, i64, i64, String) =
        sqlx::query_as(
            "SELECT status, service_type, total_amount_cents, articles_cents, payment_status \
             FROM ordertracking WHERE order_id = $1",
        )
        .bind(order_id)
        .fetch_one(&pool)
        .await
        .expect("projected order row");
    assert_eq!(status, "PLACED"); // OrderStatus::PLACED
    assert_eq!(service_type, "DELIVERY"); // ServiceType::DELIVERY
    assert_eq!(total, 2560);
    assert_eq!(articles, 1960);
    assert_eq!(payment_status, "AUTHORIZED");
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
    let row = repo.by_id(OrderId(order_id), &ReadScope::System).await.expect("by_id").expect("order exists by id");
    assert_eq!(row.status, OrderStatus::ACCEPTED);
    assert_eq!(row.r#ref.0, "CF-0001");
    assert_eq!(row.restaurant_payout_cents.0, 1800);
    assert!(row.estimated_ready_at.is_some());
    assert!(row.status_changed_at >= row.placed_at);

    // Customer history scope.
    let history = repo
        .list(OrderFilter { customer_id: Some(CustomerId(customer_id)), ..Default::default() }, &ReadScope::System)
        .await
        .expect("list by customer");
    assert_eq!(history.len(), 1);

    // Back-office queue scope: restaurant + status (bound as its INTEGER ordinal).
    let queue = repo
        .list(OrderFilter {
            restaurant_id: Some(RestaurantId(restaurant_id)),
            status: Some(OrderStatus::ACCEPTED),
            ..Default::default()
        }, &ReadScope::System)
        .await
        .expect("list by restaurant+status");
    assert_eq!(queue.len(), 1);

    let placed = repo
        .list(OrderFilter { status: Some(OrderStatus::PLACED), ..Default::default() }, &ReadScope::System)
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

    // 5) #167 (PR #586 Phase 3): a SECOND order times out — OrderAcceptanceTimedOut must fold to
    //    CANCELLED_BY_TIMEOUT (and move status_changed_at), or the customer's screen says PLACED
    //    forever after the money-adjacent cancellation recorded. This arm clears the banked
    //    +1 `event-not-projected` ratchet debt from Phases 0-1.
    let timed_out_order = uuid::Uuid::new_v4();
    let timeout_stream = format!("Order-{timed_out_order}");
    append_event(
        &pool,
        &timeout_stream,
        1,
        "OrderPlaced",
        serde_json::json!({
            "orderId": timed_out_order,
            "restaurantId": restaurant_id,
            "customerId": customer_id,
            "customerContact": { "displayName": "Léa", "phone": "+33612345678" },
            "serviceType": "COLLECTION",
            "items": [{
                "offerId": uuid::Uuid::new_v4(),
                "name": "Margherita",
                "quantity": 1,
                "unitPrice": money(980),
                "lineTotal": money(980)
            }],
            "totalAmount": money(980),
            "breakdown": {
                "articles": money(980),
                "delivery": money(0),
                "serviceFee": money(0),
                "total": money(980),
                "restaurantContribution": money(0),
                "restaurantPayout": money(980),
                "riderPayout": money(0),
                "captainNet": money(0)
            }
        }),
    )
    .await;
    append_event(
        &pool,
        &timeout_stream,
        2,
        "OrderAcceptanceTimedOut",
        serde_json::json!({ "orderId": timed_out_order }),
    )
    .await;
    worker.run_once().await.expect("run_once (timed out)");

    let (status, changed_after_placed): (String, bool) = sqlx::query_as(
        "SELECT status, status_changed_at >= placed_at FROM ordertracking WHERE order_id = $1",
    )
    .bind(timed_out_order)
    .fetch_one(&pool)
    .await
    .expect("timed-out order row");
    assert_eq!(status, "CANCELLED_BY_TIMEOUT", "the timeout folds — never PLACED forever");
    assert!(changed_after_placed, "status_changed_at moved with the terminal fact");
    let timed_out_list = repo
        .list(OrderFilter {
            restaurant_id: Some(RestaurantId(restaurant_id)),
            status: Some(OrderStatus::CANCELLED_BY_TIMEOUT),
            ..Default::default()
        }, &ReadScope::System)
        .await
        .expect("list CANCELLED_BY_TIMEOUT");
    assert_eq!(timed_out_list.len(), 1, "the back-office expired filter can find it");
}

/// The customer-anxiety quick win (#424, PROP-20260808-233000 §2): on the independent-rider path
/// the order view used to jump READY -> DELIVERED with a 15-25 minute silent hole, because NO
/// worker ever drained `DeliveryJob-%` into OrderTracking (docs/sagas.md: "mirror columns spec'd
/// but unfed"). This is the mirror's first end-to-end runtime proof: real delivery facts on a real
/// `DeliveryJob-{id}` stream, folded by the real worker, moving `ordertracking.delivery_status` on
/// the row of the ORDER they belong to.
///
/// It proves three things that were each independently broken, and any one of which reopens the
/// silent hole:
///   1. the Order projector group SLICES `DeliveryJob-%` at all (worker.rs stream_prefixes);
///   2. a delivery fact KEYS its OrderTracking row from the payload's `orderId` — the field
///      D-QW1 option (b) put on the payload (ADR-20260808-234907), with no view lookup;
///   3. `DeliveryPickedUp` is both dispatched (fedBy + column lineage -> generated arm) and MAPPED
///      (the hand-written `delivery_status` compute arm) — a fedBy entry with no mapping folds to
///      "still ASSIGNED forever", which looks identical to the bug being fixed.
#[tokio::test]
async fn delivery_facts_move_the_customers_delivery_mirror() {
    let Some(db) = crate::common::TestDb::acquire("order_projection").await else { return };
    let pool = db.pool();

    let order_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let customer_id = uuid::Uuid::new_v4();
    let job_id = uuid::Uuid::new_v4();
    let rider_id = uuid::Uuid::new_v4();
    let order_stream = format!("Order-{order_id}");
    let job_stream = format!("DeliveryJob-{job_id}");
    let worker = ProjectionWorker::new(pool.clone());

    // The customer's order exists and is on its way out of the kitchen.
    append_event(
        &pool,
        &order_stream,
        1,
        "OrderPlaced",
        serde_json::json!({
            "orderId": order_id,
            "ref": "CF-0424",
            "restaurantId": restaurant_id,
            "customerId": customer_id,
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
                "articles": money(980), "delivery": money(400), "serviceFee": money(200),
                "total": money(1580), "restaurantContribution": money(160),
                "restaurantPayout": money(820), "riderPayout": money(400), "captainNet": money(360)
            },
            "paymentIntentId": "pi_424"
        }),
    )
    .await;
    worker.run_once().await.expect("run_once (placed)");

    let delivery_status: Option<String> =
        sqlx::query_scalar("SELECT delivery_status FROM ordertracking WHERE order_id = $1")
            .bind(order_id)
            .fetch_one(&pool)
            .await
            .expect("placed order row");
    assert_eq!(delivery_status, None, "no dispatch yet — the mirror is null, not a stale value");

    // Dispatch: the job is born on its OWN stream, carrying the order it delivers.
    append_event(
        &pool,
        &job_stream,
        1,
        "DeliveryRequested",
        serde_json::json!({
            "deliveryJobId": job_id, "orderId": order_id, "restaurantId": restaurant_id,
            "pickup": address("1 rue de la Paix"), "dropoff": address("2 avenue Grammont"),
        }),
    )
    .await;
    // A Captain rider takes it (rider path — no partner, no DeliveryStatusUpdated in sight).
    append_event(
        &pool,
        &job_stream,
        2,
        "DeliveryAcceptedByRider",
        serde_json::json!({ "deliveryJobId": job_id, "orderId": order_id, "riderId": rider_id }),
    )
    .await;
    worker.run_once().await.expect("run_once (accepted by rider)");

    let (delivery_status, courier): (Option<String>, Option<serde_json::Value>) = sqlx::query_as(
        "SELECT delivery_status, courier FROM ordertracking WHERE order_id = $1",
    )
    .bind(order_id)
    .fetch_one(&pool)
    .await
    .expect("assigned order row");
    assert_eq!(delivery_status.as_deref(), Some("ASSIGNED"));
    assert_eq!(
        courier.as_ref().and_then(|c| c.get("rider_id")).and_then(|v| v.as_str()),
        Some(rider_id.to_string().as_str()),
        "the rider is mirrored onto the customer's row"
    );

    // THE FIX: the rider collects the food. Before #424 this event reached nothing at all.
    append_event(
        &pool,
        &job_stream,
        3,
        "DeliveryPickedUp",
        serde_json::json!({ "deliveryJobId": job_id, "orderId": order_id, "riderId": rider_id }),
    )
    .await;
    worker.run_once().await.expect("run_once (picked up)");

    let delivery_status: Option<String> =
        sqlx::query_scalar("SELECT delivery_status FROM ordertracking WHERE order_id = $1")
            .bind(order_id)
            .fetch_one(&pool)
            .await
            .expect("picked-up order row");
    assert_eq!(
        delivery_status.as_deref(),
        Some("PICKED_UP"),
        "the customer's screen moves when the rider collects the food (#424 quick win 1)"
    );

    // …through to hand-over, so the mirror is not a one-event fluke.
    append_event(
        &pool,
        &job_stream,
        4,
        "DeliveryCompleted",
        serde_json::json!({ "deliveryJobId": job_id, "orderId": order_id }),
    )
    .await;
    worker.run_once().await.expect("run_once (completed)");
    let delivery_status: Option<String> =
        sqlx::query_scalar("SELECT delivery_status FROM ordertracking WHERE order_id = $1")
            .bind(order_id)
            .fetch_one(&pool)
            .await
            .expect("delivered order row");
    assert_eq!(delivery_status.as_deref(), Some("DELIVERED"));

    // The orphan anomaly the nullable `orderId` exists for (PROP-20260808-233000 §2.2d): a partner
    // report on a birthless stream has no order to key, so the worker SKIPS it with a warn and
    // keeps draining — it is not poison, and it corrupts no row.
    append_event(
        &pool,
        &format!("DeliveryJob-{}", uuid::Uuid::new_v4()),
        1,
        "DeliveryStatusUpdated",
        serde_json::json!({
            "deliveryJobId": uuid::Uuid::new_v4(), "orderId": null, "status": "PICKED_UP"
        }),
    )
    .await;
    worker.run_once().await.expect("run_once (orphan skipped, not poison)");

    let delivery_status: Option<String> =
        sqlx::query_scalar("SELECT delivery_status FROM ordertracking WHERE order_id = $1")
            .bind(order_id)
            .fetch_one(&pool)
            .await
            .expect("order row after the orphan");
    assert_eq!(delivery_status.as_deref(), Some("DELIVERED"), "untouched by the orphan");
    // The checkpoint advanced past every fact — 1 order + 5 delivery.
    let checkpoint: i64 =
        sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = 'Order'")
            .fetch_one(&pool)
            .await
            .expect("Order checkpoint");
    assert_eq!(checkpoint, 6);
}

/// A payment gateway SPY: records every capture/release call so the settlement path can be
/// asserted end-to-end. Settlement legs never open an intent or refund, so those stay
/// `unreachable!()` — but capture/release are real recording seams (HIGH-2: no DB test drove
/// `OrderDelivered -> PaymentSettlementProcess -> capture` before this one).
#[derive(Default)]
struct SettlementSpy {
    captures: Mutex<Vec<String>>,
    releases: Mutex<Vec<String>>,
}

#[async_trait]
impl PaymentService for SettlementSpy {
    async fn request(
        &self,
        _input: PaymentRequestInput,
        _meta: &ServiceCallMeta,
    ) -> Result<PaymentRequestOutput, DomainError> {
        unreachable!("settlement never opens an intent")
    }
    async fn capture(
        &self,
        input: PaymentCaptureInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        self.captures.lock().unwrap().push(input.payment_intent_id.0);
        Ok(())
    }
    async fn release(
        &self,
        input: PaymentReleaseInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        self.releases.lock().unwrap().push(input.payment_intent_id.0);
        Ok(())
    }
    async fn refund(
        &self,
        _input: PaymentRefundInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        unreachable!("settlement never refunds — that is RefundProcess")
    }
}

/// HIGH-2 / CRITICAL-1 regression, end-to-end through the REAL projector AND the REAL saga runner
/// (rules.yaml#/PaymentCapturedOnFulfilment). A charging order is folded `OrderPlaced(intent
/// present) -> OrderAcceptedByRestaurant -> OrderDelivered` by the ProjectionWorker into the
/// `ordertracking` read model, then the ProcessManagerRunner drains `OrderDelivered` into
/// PaymentSettlementProcess, whose guard reads THAT row's `payment_intent_id`.
///
/// This is the test that masks nothing: it never hand-injects the read-model row. Before the
/// CRITICAL-1 wiring fix (OrderPlaced added to `payment_intent_id.from`), the projector's OrderPlaced
/// arm hardcodes `payment_intent_id: None`, so the guard reads NULL, SKIPS, and NO capture is ever
/// requested — the whole feature is inert and this assertion goes RED. With the seed wired, the row
/// is born AUTHORIZED carrying its intent and delivery captures the full authorized hold.
#[tokio::test]
async fn delivered_order_captures_through_the_real_projected_intent() {
    let Some(db) = crate::common::TestDb::acquire("order_projection").await else { return };
    let pool = db.pool();

    let order_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let customer_id = uuid::Uuid::new_v4();
    let stream = format!("Order-{order_id}");

    // 1) Birth: a paid order carrying its AUTHORIZED PaymentIntent (authorize-then-capture).
    append_event(
        &pool,
        &stream,
        1,
        "OrderPlaced",
        serde_json::json!({
            "orderId": order_id,
            "ref": "CF-0544",
            "restaurantId": restaurant_id,
            "customerId": customer_id,
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
                "articles": money(980), "delivery": money(400), "serviceFee": money(200),
                "total": money(1580), "restaurantContribution": money(160),
                "restaurantPayout": money(820), "riderPayout": money(400), "captainNet": money(360)
            },
            "paymentIntentId": "pi_capture_544"
        }),
    )
    .await;
    // 2) …through acceptance to hand-over — the lifecycle the ellipsis covers (payment_status stays
    //    AUTHORIZED across it; only fulfilment settles).
    append_event(
        &pool,
        &stream,
        2,
        "OrderAcceptedByRestaurant",
        serde_json::json!({ "orderId": order_id, "restaurantId": restaurant_id, "estimatedReadyAt": "2026-08-14T19:30:00Z" }),
    )
    .await;
    append_event(
        &pool,
        &stream,
        3,
        "OrderDelivered",
        serde_json::json!({ "orderId": order_id, "restaurantId": restaurant_id }),
    )
    .await;

    // The REAL read-model fold — NOT a hand-injected fixture.
    ProjectionWorker::new(pool.clone()).run_once().await.expect("project the order lifecycle");
    let (payment_status, payment_intent_id): (String, Option<String>) = sqlx::query_as(
        "SELECT payment_status, payment_intent_id FROM ordertracking WHERE order_id = $1",
    )
    .bind(order_id)
    .fetch_one(&pool)
    .await
    .expect("delivered order row");
    // The seed CRITICAL-1 restores: born AUTHORIZED, carrying its intent — pre-fix this id is NULL.
    assert_eq!(payment_status, "AUTHORIZED", "capture has not run yet — the hold is still authorized");
    assert_eq!(
        payment_intent_id.as_deref(),
        Some("pi_capture_544"),
        "the OrderPlaced seed carries the intent id into the read model (CRITICAL-1)",
    );

    // 3) The saga runner drains OrderDelivered into PaymentSettlementProcess, whose guard reads THAT
    //    projected row and requests the capture of the FULL authorized intent.
    let spy = Arc::new(SettlementSpy::default());
    let runner = ProcessManagerRunner::new(pool.clone())
        .with_only("PaymentSettlementProcess")
        .with_payments(spy.clone());
    runner.run_once().await.expect("settlement drains clean");

    assert_eq!(
        *spy.captures.lock().unwrap(),
        vec!["pi_capture_544".to_string()],
        "OrderDelivered captured the authorized hold (INERT before CRITICAL-1's fix)",
    );
    assert!(spy.releases.lock().unwrap().is_empty(), "a delivered order captures, never releases");
}
