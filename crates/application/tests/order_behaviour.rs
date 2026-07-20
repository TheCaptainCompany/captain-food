//! BEHAVIOUR tests for the Order aggregate — the executable form of the `specs/tests.yaml`
//! Given/When/Then cases whose `when` is an Order-aggregate command (ADR-0032: each test cites the
//! `specs/rules.yaml` rule it asserts). Given = pre-seeded stream events (in-memory event store),
//! When = the command handler, Then = the emitted event(s) / the errors.yaml rejection code.
//!
//! Pure and offline: an in-memory [`EventStore`]. `OrderPlaced` in the GIVENs stands for the
//! PlaceOrderProcess outcome (the saga is exercised in `place_order_behaviour.rs`).

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use application::commands::{
    accept_order, cancel_order_by_customer, cancel_order_by_restaurant, mark_order_delivered,
    mark_order_ready, rate_order, rate_restaurant, reject_order, rejection_code, request_refund,
    start_preparation, tip_order,
};
use application::ports::{version_conflict, Actor, EventStore};
use domain::generated::commands::{
    AcceptOrder, CancelOrderByCustomer, CancelOrderByRestaurant, MarkOrderDelivered, MarkOrderReady,
    RateOrder, RateRestaurant, RejectOrder, RequestRefund, StartPreparation, TipOrder,
};
use domain::generated::entities::{
    CustomerContact, Money, OrderLineItem, PaymentBreakdown, Tip,
};
use domain::generated::events::{
    DomainEvent, OrderAcceptedByRestaurant, OrderCancelledByCustomer, OrderDelivered,
    OrderMarkedReady, OrderPlaced, OrderPreparationStarted, OrderRated, RestaurantRated,
};
use domain::generated::scalars::*;
use domain::shared::errors::DomainError;

// ------------------------------------------------------------------------------------------------
// Test doubles
// ------------------------------------------------------------------------------------------------

/// In-memory [`EventStore`]: version = number of events on the stream, same optimistic-concurrency
/// semantics as `PgEventStore` (a clash → the canonical `version_conflict`).
#[derive(Default)]
struct MemStore {
    streams: Mutex<HashMap<String, Vec<DomainEvent>>>,
}

impl MemStore {
    /// GIVEN: pre-seed a stream with already-recorded facts.
    fn seed(&self, stream: &str, events: Vec<DomainEvent>) {
        self.streams.lock().unwrap().insert(stream.to_string(), events);
    }

    /// THEN: the full stream after the command ran.
    fn stream(&self, stream: &str) -> Vec<DomainEvent> {
        self.streams.lock().unwrap().get(stream).cloned().unwrap_or_default()
    }
}

#[async_trait]
impl EventStore for MemStore {
    async fn append(
        &self,
        stream_name: &str,
        expected_version: i64,
        events: &[DomainEvent],
        _actor: &Actor,
    ) -> Result<i64, DomainError> {
        let mut streams = self.streams.lock().unwrap();
        let stream = streams.entry(stream_name.to_string()).or_default();
        if stream.len() as i64 != expected_version {
            return Err(version_conflict(stream_name, expected_version));
        }
        stream.extend(events.iter().cloned());
        Ok(stream.len() as i64)
    }

    async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
        let events = self.stream(stream_name);
        let version = events.len() as i64;
        Ok((events, version))
    }
}

// ------------------------------------------------------------------------------------------------
// Fixtures (tests.yaml `fixtures`, with UUIDs instead of the sample string ids)
// ------------------------------------------------------------------------------------------------

/// The acting user for a given UserType ordinal (enums are declaration-order integers, ADR-0037):
/// 1 = CUSTOMER, 3 = RESTAURANT.
fn actor_as(user_type: i32) -> Actor {
    Actor {
        user_id: uuid::Uuid::new_v4(),
        user_type,
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

fn stream(id: OrderId) -> String {
    format!("Order-{}", id.0)
}

fn eur(cents: i64) -> Money {
    Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
}

/// Fixture `orderPlaced`: 2× Margherita @ 9.80 EUR, total 19.60, DELIVERY, paid via pi_123.
fn order_placed(order_id: OrderId, restaurant_id: RestaurantId, customer_id: Option<CustomerId>) -> DomainEvent {
    DomainEvent::OrderPlaced(OrderPlaced {
        mode: None,
        order_id,
        r#ref: None,
        restaurant_id,
        customer_id,
        customer_contact: CustomerContact {
            display_name: CustomerDisplayName("Johnny".into()),
            email: None,
            phone: PhoneNumber("+33612345678".into()),
        },
        service_type: ServiceType::DELIVERY,
        delivery_address: None,
        items: vec![OrderLineItem {
            offer_id: OfferId(uuid::Uuid::new_v4()),
            product_id: None,
            name: ProductName("Margherita".into()),
            offer_name: None,
            quantity: 2,
            unit_price: eur(980),
            selected_options: vec![],
            line_total: eur(1960),
        }],
        total_amount: eur(1960),
        breakdown: PaymentBreakdown {
            articles: eur(1960),
            delivery: eur(0),
            service_fee: eur(0),
            total: eur(1960),
            restaurant_contribution: eur(0),
            restaurant_payout: eur(1960),
            rider_payout: eur(0),
            captain_net: eur(0),
        },
        note: None,
        payment_intent_id: PaymentIntentId("pi_123".into()),
    })
}

fn accepted(order_id: OrderId, restaurant_id: RestaurantId) -> DomainEvent {
    DomainEvent::OrderAcceptedByRestaurant(OrderAcceptedByRestaurant {
        order_id,
        restaurant_id,
        estimated_ready_at: None,
    })
}

fn preparing(order_id: OrderId, restaurant_id: RestaurantId) -> DomainEvent {
    DomainEvent::OrderPreparationStarted(OrderPreparationStarted { order_id, restaurant_id })
}

fn ready(order_id: OrderId, restaurant_id: RestaurantId) -> DomainEvent {
    DomainEvent::OrderMarkedReady(OrderMarkedReady { order_id, restaurant_id })
}

fn delivered(order_id: OrderId, restaurant_id: RestaurantId) -> DomainEvent {
    DomainEvent::OrderDelivered(OrderDelivered { order_id, restaurant_id })
}

fn oid() -> OrderId {
    OrderId(uuid::Uuid::new_v4())
}
fn rid() -> RestaurantId {
    RestaurantId(uuid::Uuid::new_v4())
}

// ------------------------------------------------------------------------------------------------
// Lifecycle status machine (rules.yaml#/OrderLifecycleStatusMachine)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestOrderAcceptedByRestaurant — rules.yaml#/OrderLifecycleStatusMachine
#[tokio::test]
async fn restaurant_accepts_a_placed_order() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None)]);

    accept_order(
        &store,
        AcceptOrder { order_id: order, restaurant_id: resto, estimated_ready_at: None },
        &actor_as(3),
    )
    .await
    .expect("accept");

    let events = store.stream(&stream(order));
    assert!(matches!(&events[1], DomainEvent::OrderAcceptedByRestaurant(e) if e.order_id == order));
}

/// tests.yaml#/cases/TestOrderAcceptIsRejected (both arms + tenant scoping) —
/// rules.yaml#/OrderLifecycleStatusMachine
#[tokio::test]
async fn rejects_accepting_a_missing_or_already_accepted_order() {
    let store = MemStore::default();

    // Missing order → OrderNotFound.
    let err = accept_order(
        &store,
        AcceptOrder { order_id: oid(), restaurant_id: rid(), estimated_ready_at: None },
        &actor_as(3),
    )
    .await
    .expect_err("missing");
    assert_eq!(rejection_code(&err), Some("OrderNotFound"));

    // Already accepted → InvalidOrderStatus.
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None), accepted(order, resto)]);
    let err = accept_order(
        &store,
        AcceptOrder { order_id: order, restaurant_id: resto, estimated_ready_at: None },
        &actor_as(3),
    )
    .await
    .expect_err("already accepted");
    assert_eq!(rejection_code(&err), Some("InvalidOrderStatus"));

    // Another restaurant's order is scoped out as OrderNotFound.
    let err = accept_order(
        &store,
        AcceptOrder { order_id: order, restaurant_id: rid(), estimated_ready_at: None },
        &actor_as(3),
    )
    .await
    .expect_err("foreign restaurant");
    assert_eq!(rejection_code(&err), Some("OrderNotFound"));
}

/// tests.yaml#/cases/TestOrderPreparationStarted — rules.yaml#/OrderLifecycleStatusMachine
#[tokio::test]
async fn restaurant_starts_preparing_an_accepted_order() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None), accepted(order, resto)]);

    start_preparation(&store, StartPreparation { order_id: order, restaurant_id: resto }, &actor_as(3))
        .await
        .expect("start preparation");
    assert!(matches!(&store.stream(&stream(order))[2], DomainEvent::OrderPreparationStarted(_)));

    // And starting preparation on a merely PLACED order is rejected (InvalidOrderStatus).
    let (order2, resto2) = (oid(), rid());
    store.seed(&stream(order2), vec![order_placed(order2, resto2, None)]);
    let err = start_preparation(&store, StartPreparation { order_id: order2, restaurant_id: resto2 }, &actor_as(3))
        .await
        .expect_err("not accepted yet");
    assert_eq!(rejection_code(&err), Some("InvalidOrderStatus"));
}

/// tests.yaml#/cases/TestOrderMarkedReady — rules.yaml#/OrderLifecycleStatusMachine
#[tokio::test]
async fn restaurant_marks_the_order_ready() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(
        &stream(order),
        vec![order_placed(order, resto, None), accepted(order, resto), preparing(order, resto)],
    );

    mark_order_ready(&store, MarkOrderReady { order_id: order, restaurant_id: resto }, &actor_as(3))
        .await
        .expect("mark ready");
    assert!(matches!(&store.stream(&stream(order))[3], DomainEvent::OrderMarkedReady(_)));
}

/// tests.yaml#/cases/TestOrderDelivered — rules.yaml#/OrderLifecycleStatusMachine
#[tokio::test]
async fn the_order_is_delivered_to_the_customer() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None), ready(order, resto)]);

    mark_order_delivered(&store, MarkOrderDelivered { order_id: order, restaurant_id: resto }, &actor_as(3))
        .await
        .expect("deliver");
    assert!(matches!(&store.stream(&stream(order))[2], DomainEvent::OrderDelivered(_)));

    // A not-yet-ready order cannot be delivered (InvalidOrderStatus).
    let (order2, resto2) = (oid(), rid());
    store.seed(&stream(order2), vec![order_placed(order2, resto2, None)]);
    let err = mark_order_delivered(&store, MarkOrderDelivered { order_id: order2, restaurant_id: resto2 }, &actor_as(3))
        .await
        .expect_err("not ready");
    assert_eq!(rejection_code(&err), Some("InvalidOrderStatus"));
}

/// tests.yaml#/cases/TestOrderRejected — rules.yaml#/OrderLifecycleStatusMachine
#[tokio::test]
async fn restaurant_rejects_a_placed_order() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None)]);

    reject_order(
        &store,
        RejectOrder { order_id: order, restaurant_id: resto, reason: "Out of ingredients".into() },
        &actor_as(3),
    )
    .await
    .expect("reject");
    assert!(matches!(
        &store.stream(&stream(order))[1],
        DomainEvent::OrderRejectedByRestaurant(e) if e.reason == "Out of ingredients"
    ));

    // An already-accepted order can no longer be rejected (InvalidOrderStatus).
    let (order2, resto2) = (oid(), rid());
    store.seed(&stream(order2), vec![order_placed(order2, resto2, None), accepted(order2, resto2)]);
    let err = reject_order(
        &store,
        RejectOrder { order_id: order2, restaurant_id: resto2, reason: "Too late".into() },
        &actor_as(3),
    )
    .await
    .expect_err("already accepted");
    assert_eq!(rejection_code(&err), Some("InvalidOrderStatus"));
}

/// tests.yaml#/cases/TestOrderCancelledByCustomer — rules.yaml#/OrderLifecycleStatusMachine
#[tokio::test]
async fn customer_cancels_before_acceptance() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None)]);

    cancel_order_by_customer(
        &store,
        CancelOrderByCustomer { order_id: order, restaurant_id: resto, reason: Some("Changed my mind".into()) },
        &actor_as(1),
    )
    .await
    .expect("cancel");
    assert!(matches!(&store.stream(&stream(order))[1], DomainEvent::OrderCancelledByCustomer(_)));

    // Once accepted, the customer can no longer cancel (InvalidOrderStatus).
    let (order2, resto2) = (oid(), rid());
    store.seed(&stream(order2), vec![order_placed(order2, resto2, None), accepted(order2, resto2)]);
    let err = cancel_order_by_customer(
        &store,
        CancelOrderByCustomer { order_id: order2, restaurant_id: resto2, reason: None },
        &actor_as(1),
    )
    .await
    .expect_err("already accepted");
    assert_eq!(rejection_code(&err), Some("InvalidOrderStatus"));
}

/// tests.yaml#/cases/TestOrderCancelledByRestaurant — rules.yaml#/OrderLifecycleStatusMachine
#[tokio::test]
async fn restaurant_cancels_an_order_it_had_accepted() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None), accepted(order, resto)]);

    cancel_order_by_restaurant(
        &store,
        CancelOrderByRestaurant { order_id: order, restaurant_id: resto, reason: "Kitchen closed".into() },
        &actor_as(3),
    )
    .await
    .expect("cancel");
    assert!(matches!(
        &store.stream(&stream(order))[2],
        DomainEvent::OrderCancelledByRestaurant(e) if e.reason == "Kitchen closed"
    ));
}

// ------------------------------------------------------------------------------------------------
// Post-delivery feedback (rules.yaml#/OrderRatedOnceWhenDelivered, #/RestaurantRatedOncePerOrder)
// ------------------------------------------------------------------------------------------------

/// tests.yaml#/cases/TestOrderRated — rules.yaml#/OrderRatedOnceWhenDelivered
#[tokio::test]
async fn customer_rates_the_delivery_after_delivery() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    let customer = CustomerId(uuid::Uuid::new_v4());
    store.seed(
        &stream(order),
        vec![order_placed(order, resto, Some(customer)), delivered(order, resto)],
    );

    rate_order(
        &store,
        RateOrder { order_id: order, restaurant_id: resto, rider_thumb: ThumbRating::UP },
        &actor_as(1),
    )
    .await
    .expect("rate");
    assert!(matches!(
        &store.stream(&stream(order))[2],
        DomainEvent::OrderRated(e) if e.rider_thumb == ThumbRating::UP && e.customer_id == Some(customer)
    ));
}

/// tests.yaml#/cases/TestOrderRateOrderIsRejected (all three arms) —
/// rules.yaml#/OrderRatedOnceWhenDelivered
#[tokio::test]
async fn rejects_rating_a_missing_undelivered_or_already_rated_delivery() {
    let store = MemStore::default();

    // Missing order → OrderNotFound.
    let err = rate_order(
        &store,
        RateOrder { order_id: oid(), restaurant_id: rid(), rider_thumb: ThumbRating::UP },
        &actor_as(1),
    )
    .await
    .expect_err("missing");
    assert_eq!(rejection_code(&err), Some("OrderNotFound"));

    // Not delivered yet → InvalidOrderStatus.
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None)]);
    let err = rate_order(
        &store,
        RateOrder { order_id: order, restaurant_id: resto, rider_thumb: ThumbRating::UP },
        &actor_as(1),
    )
    .await
    .expect_err("not delivered");
    assert_eq!(rejection_code(&err), Some("InvalidOrderStatus"));

    // Already rated → OrderAlreadyRated (rate-once, final).
    let (order, resto) = (oid(), rid());
    store.seed(
        &stream(order),
        vec![
            order_placed(order, resto, None),
            delivered(order, resto),
            DomainEvent::OrderRated(OrderRated {
                order_id: order,
                restaurant_id: resto,
                customer_id: None,
                rider_thumb: ThumbRating::UP,
            }),
        ],
    );
    let err = rate_order(
        &store,
        RateOrder { order_id: order, restaurant_id: resto, rider_thumb: ThumbRating::DOWN },
        &actor_as(1),
    )
    .await
    .expect_err("already rated");
    assert_eq!(rejection_code(&err), Some("OrderAlreadyRated"));
}

/// tests.yaml#/cases/TestOrderRestaurantRated — rules.yaml#/RestaurantRatedOncePerOrder
#[tokio::test]
async fn customer_rates_the_restaurant_after_delivery() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None), delivered(order, resto)]);

    rate_restaurant(
        &store,
        RateRestaurant {
            order_id: order,
            restaurant_id: resto,
            stars: StarRating(5),
            comment: Some(RatingComment("Excellent!".into())),
        },
        &actor_as(1),
    )
    .await
    .expect("rate restaurant");
    assert!(matches!(
        &store.stream(&stream(order))[2],
        DomainEvent::RestaurantRated(e) if e.stars == StarRating(5)
    ));
}

/// tests.yaml#/cases/TestOrderRateRestaurantTwiceIsRejected — rules.yaml#/RestaurantRatedOncePerOrder
#[tokio::test]
async fn rejects_rating_the_restaurant_a_second_time() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(
        &stream(order),
        vec![
            order_placed(order, resto, None),
            delivered(order, resto),
            DomainEvent::RestaurantRated(RestaurantRated {
                order_id: order,
                restaurant_id: resto,
                customer_id: None,
                stars: StarRating(5),
                comment: Some(RatingComment("Excellent!".into())),
            }),
        ],
    );

    let err = rate_restaurant(
        &store,
        RateRestaurant { order_id: order, restaurant_id: resto, stars: StarRating(4), comment: None },
        &actor_as(1),
    )
    .await
    .expect_err("second rating");
    assert_eq!(rejection_code(&err), Some("RestaurantAlreadyRated"));
}

// ------------------------------------------------------------------------------------------------
// Tips (rules.yaml#/TipsAdditiveMultiRecipientSeparate) & refunds (rules.yaml#/RefundRequestByCustomer)
// ------------------------------------------------------------------------------------------------

fn tip(recipient: TipRecipient, cents: i64) -> Tip {
    Tip { recipient, amount: eur(cents) }
}

/// tests.yaml#/cases/TestOrderTipped — rules.yaml#/TipsAdditiveMultiRecipientSeparate
#[tokio::test]
async fn customer_tips_rider_restaurant_and_captain_after_delivery() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    let customer = CustomerId(uuid::Uuid::new_v4());
    store.seed(&stream(order), vec![order_placed(order, resto, Some(customer)), delivered(order, resto)]);

    tip_order(
        &store,
        TipOrder {
            order_id: order,
            restaurant_id: resto,
            tips: vec![
                tip(TipRecipient::RIDER, 200),
                tip(TipRecipient::RESTAURANT, 100),
                tip(TipRecipient::CAPTAIN, 50),
            ],
        },
        &actor_as(1), // CUSTOMER
    )
    .await
    .expect("tip");

    let events = store.stream(&stream(order));
    assert!(matches!(
        &events[2],
        DomainEvent::OrderTipped(e)
            if e.tipped_by == Tipper::CUSTOMER && e.customer_id == Some(customer) && e.tips.len() == 3
    ));
}

/// tests.yaml#/cases/TestOrderTippedByRestaurant — rules.yaml#/TipsAdditiveMultiRecipientSeparate
#[tokio::test]
async fn restaurant_tips_the_rider() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None), delivered(order, resto)]);

    tip_order(
        &store,
        TipOrder { order_id: order, restaurant_id: resto, tips: vec![tip(TipRecipient::RIDER, 300)] },
        &actor_as(3), // RESTAURANT — tippedBy is derived from the caller's role, never client-supplied
    )
    .await
    .expect("tip");

    let events = store.stream(&stream(order));
    assert!(matches!(
        &events[2],
        DomainEvent::OrderTipped(e)
            if e.tipped_by == Tipper::RESTAURANT && e.customer_id.is_none() && e.tips.len() == 1
    ));
}

/// tests.yaml#/cases/TestOrderTipIsRejected (all three arms) —
/// rules.yaml#/TipsAdditiveMultiRecipientSeparate
#[tokio::test]
async fn rejects_tipping_a_missing_or_cancelled_order_or_a_restaurant_tipping_itself() {
    let store = MemStore::default();

    // Missing order → OrderNotFound.
    let err = tip_order(
        &store,
        TipOrder { order_id: oid(), restaurant_id: rid(), tips: vec![tip(TipRecipient::RESTAURANT, 100)] },
        &actor_as(3),
    )
    .await
    .expect_err("missing");
    assert_eq!(rejection_code(&err), Some("OrderNotFound"));

    // A restaurant tipping itself → InvalidTipRecipient.
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None), delivered(order, resto)]);
    let err = tip_order(
        &store,
        TipOrder { order_id: order, restaurant_id: resto, tips: vec![tip(TipRecipient::RESTAURANT, 100)] },
        &actor_as(3),
    )
    .await
    .expect_err("self-tip");
    assert_eq!(rejection_code(&err), Some("InvalidTipRecipient"));

    // A cancelled order cannot be tipped → InvalidOrderStatus.
    let (order, resto) = (oid(), rid());
    store.seed(
        &stream(order),
        vec![
            order_placed(order, resto, None),
            DomainEvent::OrderCancelledByCustomer(OrderCancelledByCustomer {
                order_id: order,
                restaurant_id: resto,
                reason: None,
            }),
        ],
    );
    let err = tip_order(
        &store,
        TipOrder { order_id: order, restaurant_id: resto, tips: vec![tip(TipRecipient::RIDER, 100)] },
        &actor_as(1),
    )
    .await
    .expect_err("cancelled order");
    assert_eq!(rejection_code(&err), Some("InvalidOrderStatus"));
}

/// tests.yaml#/cases/TestOrderRefundRequested — rules.yaml#/RefundRequestByCustomer
#[tokio::test]
async fn customer_requests_a_refund_for_a_delivered_order() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    let customer = CustomerId(uuid::Uuid::new_v4());
    store.seed(&stream(order), vec![order_placed(order, resto, Some(customer)), delivered(order, resto)]);

    request_refund(
        &store,
        RequestRefund { order_id: order, restaurant_id: resto, reason: Some("Late delivery".into()) },
        &actor_as(1),
    )
    .await
    .expect("request refund");
    assert!(matches!(
        &store.stream(&stream(order))[2],
        DomainEvent::RefundRequested(e)
            if e.reason.as_deref() == Some("Late delivery") && e.customer_id == Some(customer)
    ));

    // An undelivered order cannot be refund-requested (InvalidOrderStatus).
    let (order2, resto2) = (oid(), rid());
    store.seed(&stream(order2), vec![order_placed(order2, resto2, None)]);
    let err = request_refund(
        &store,
        RequestRefund { order_id: order2, restaurant_id: resto2, reason: None },
        &actor_as(1),
    )
    .await
    .expect_err("not delivered");
    assert_eq!(rejection_code(&err), Some("InvalidOrderStatus"));
}

/// tests.yaml#/cases/TestOrderLifecycleRejectsSkippedTransition —
/// rules.yaml#/OrderLifecycleIsExplicit (+ #/OrderLifecycleStatusMachine): the handlers guard with
/// the GENERATED transition table (actors.yaml#/Order/lifecycle, ADR-20260720-004419), so a move the
/// declared machine does not contain — here re-accepting an already-accepted order — rejects with
/// InvalidOrderStatus.
#[tokio::test]
async fn rejects_a_move_the_declared_machine_does_not_contain() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(&stream(order), vec![order_placed(order, resto, None), accepted(order, resto)]);

    let err = accept_order(
        &store,
        AcceptOrder { order_id: order, restaurant_id: resto, estimated_ready_at: None },
        &actor_as(3),
    )
    .await
    .expect_err("re-accept");
    assert_eq!(rejection_code(&err), Some("InvalidOrderStatus"));
    // Nothing was appended — the stream still ends at the acceptance.
    assert_eq!(store.stream(&stream(order)).len(), 2);
}

/// tests.yaml#/cases/TestOrderLifecycleTerminalStateRefusesTransitions —
/// rules.yaml#/OrderLifecycleIsExplicit: DELIVERED is a declared terminal state
/// (`domain::order::lifecycle::TERMINAL`), so every lifecycle transition out of it — here a
/// restaurant cancellation — rejects with InvalidOrderStatus.
#[tokio::test]
async fn terminal_state_refuses_every_lifecycle_transition() {
    let store = MemStore::default();
    let (order, resto) = (oid(), rid());
    store.seed(
        &stream(order),
        vec![order_placed(order, resto, None), ready(order, resto), delivered(order, resto)],
    );

    let err = cancel_order_by_restaurant(
        &store,
        CancelOrderByRestaurant { order_id: order, restaurant_id: resto, reason: "Too late".into() },
        &actor_as(3),
    )
    .await
    .expect_err("cancel after delivery");
    assert_eq!(rejection_code(&err), Some("InvalidOrderStatus"));
    assert!(domain::order::lifecycle::is_terminal(domain::generated::scalars::OrderStatus::DELIVERED));
}
