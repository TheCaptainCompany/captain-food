//! Behaviour of the GENERATED in-memory PM state doubles (`application::pm_state::mem`, issue #27):
//! `upsert` replaces the whole row and stamps `last_update_utc = now()` server-side (the row's own
//! value is ignored), the pk lookup reads the stored row, and the UNIQUE-column lookups correlate a
//! fact back to its run — the same semantics the Postgres stores implement.

use application::pm_state::mem::*;
use application::pm_state::*;
use chrono::{DateTime, Utc};
use domain::generated::scalars::{
    CartId, DeliveryDispatchProcessStatus, DeliveryJobId, OrderId, PaymentIntentId,
    PaymentProcessStatus, PaymentStatus, RestaurantId,
};

fn payment_row(cart: uuid::Uuid, intent: &str) -> PaymentProcessRow {
    PaymentProcessRow {
        cart_id: CartId(cart),
        order_id: OrderId(uuid::Uuid::new_v4()),
        payment_intent_id: PaymentIntentId(intent.into()),
        process_status: PaymentProcessStatus::AWAITING_PAYMENT_RESULT,
        payment_status: PaymentStatus::PENDING,
        customer_id: None,
        session_id: None,
        client_secret: Some("pi_secret".into()),
        last_processed_stripe_event_id: None,
        last_update_utc: DateTime::<Utc>::MIN_UTC,
    }
}

#[tokio::test]
async fn mem_payment_store_upserts_and_correlates_by_intent() {
    let store = MemPaymentProcessState::default();
    let cart = uuid::Uuid::new_v4();
    let row = payment_row(cart, "pi_1");
    store.upsert(&row).await.unwrap();

    let by_cart = store.by_cart(CartId(cart)).await.unwrap().unwrap();
    assert_eq!(by_cart.payment_intent_id.0, "pi_1");
    // The envelope stamped last_update_utc server-side (the row's own value was ignored).
    assert!(by_cart.last_update_utc > DateTime::<Utc>::MIN_UTC);

    let by_intent = store
        .by_payment_intent(&PaymentIntentId("pi_1".into()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_intent.cart_id.0, cart);
    assert!(store.by_cart(CartId(uuid::Uuid::new_v4())).await.unwrap().is_none());

    // Upsert replaces the whole row (same pk).
    let mut second = payment_row(cart, "pi_1");
    second.process_status = PaymentProcessStatus::ORDER_PLACED;
    second.payment_status = PaymentStatus::CAPTURED;
    store.upsert(&second).await.unwrap();
    let replaced = store.by_cart(CartId(cart)).await.unwrap().unwrap();
    assert_eq!(replaced.process_status, PaymentProcessStatus::ORDER_PLACED);
}

#[tokio::test]
async fn mem_dispatch_store_finds_by_order_and_job() {
    let store = MemDeliveryDispatchState::default();
    let order = uuid::Uuid::new_v4();
    let job = uuid::Uuid::new_v4();
    let row = DeliveryDispatchRow {
        order_id: OrderId(order),
        restaurant_id: RestaurantId(uuid::Uuid::new_v4()),
        delivery_job_id: DeliveryJobId(job),
        process_status: DeliveryDispatchProcessStatus::OFFERED,
        offer_attempts: 1,
        current_rank: Some(1),
        current_channel: Some(domain::generated::scalars::DeliveryChannelKey("independent".into())),
        last_update_utc: DateTime::<Utc>::MIN_UTC,
    };
    store.upsert(&row).await.unwrap();
    assert!(store.by_order(OrderId(order)).await.unwrap().is_some());
    let by_job = store.by_delivery_job(DeliveryJobId(job)).await.unwrap().unwrap();
    assert_eq!(by_job.order_id.0, order);
    assert!(store
        .by_delivery_job(DeliveryJobId(uuid::Uuid::new_v4()))
        .await
        .unwrap()
        .is_none());
}
