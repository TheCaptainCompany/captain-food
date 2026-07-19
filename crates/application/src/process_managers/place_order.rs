//! PlaceOrderProcess (`specs/processmanager.yaml#/PlaceOrderProcess`) — the EVENT legs of the
//! checkout saga, executed over its `payment_process_manager` state row (ADR-20260719-193500). The
//! COMMAND leg (`commands.yaml#/PlaceOrder` → PaymentIntentCreated + the AWAITING_PAYMENT_RESULT row)
//! is `commands::place_order`; this module reacts to the INBOUND Stripe outcomes recorded by the
//! Payment aggregate:
//!
//! - `events.yaml#/PaymentCaptured` → materialize the Order (`OrderPlaced` on `Order-<orderId>`) and
//!   close the cart (`CartCheckedOut` on `Cart-<cartId>`) from the checkout snapshot FROZEN on
//!   `PaymentIntentCreated` in the `Payment-<intentId>` stream — the log alone, no out-of-log store
//!   (retires the fail-closed `CheckoutSnapshotSource` seam).
//! - `events.yaml#/PaymentFailed` → resolve the run; no order, the cart stays OPEN
//!   (rules.yaml#/CheckoutAbortsOnPaymentFailure).
//!
//! Guard semantics: a payment outcome matching NO run (or a run whose Payment stream lost its birth)
//! is the `errors.yaml#/PaymentEventOrphaned` ERROR — the recorded fact stands but the run aborts and
//! surfaces (rules.yaml#/OrphanPaymentEventFlagged); an already-resolved run is the benign
//! `state.expect` skip (Stripe re-delivery).

use domain::generated::events::{
    CartCheckedOut, DomainEvent, OrderPlaced, PaymentCaptured, PaymentFailed,
};
use domain::generated::scalars::{
    CartStatus, ExternalReference, PaymentProcessStatus, PaymentStatus,
};
use domain::shared::errors::DomainError;
use serde_json::json;

use crate::pm_state::{PaymentProcessRow, PaymentProcessStateStore};
use crate::ports::EventStore;
use crate::process_managers::{cart_stream, order_stream, saga_actor, Outcome, TriggerEnvelope};
use crate::repository::Repository;

/// The typed error both payment-outcome legs throw when the intent matches no checkout run
/// (`errors.yaml#/PaymentEventOrphaned` — money may have moved with no order to materialize).
fn orphaned(payment_intent_id: &domain::generated::scalars::PaymentIntentId) -> DomainError {
    DomainError::rejected("PaymentEventOrphaned", json!({ "paymentIntentId": payment_intent_id }))
}

/// EVENT leg `events.yaml#/PaymentCaptured` (rules.yaml#/OrderMaterializedOnPaymentCapture):
/// `state.by` the intent (missing run → throws PaymentEventOrphaned), `state.expect`
/// AWAITING_PAYMENT_RESULT (else benign skip), read the frozen checkout back from the
/// `Payment-<intentId>` stream, deliver `OrderPlaced` to the Order and `CartCheckedOut` to the Cart
/// (each idempotent against the target's own fold), then resolve the row
/// CAPTURED/ORDER_PLACED with the trigger id as the Stripe dedup key.
pub async fn on_payment_captured(
    store: &dyn EventStore,
    state: &dyn PaymentProcessStateStore,
    event: &PaymentCaptured,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    // state.by payment_intent_id — load the checkout run this capture belongs to.
    let Some(row) = state.by_payment_intent(&event.payment_intent_id).await? else {
        return Err(orphaned(&event.payment_intent_id));
    };
    // state.expect process_status = AWAITING_PAYMENT_RESULT — already-resolved run → benign skip.
    if row.process_status != PaymentProcessStatus::AWAITING_PAYMENT_RESULT {
        return Ok(Outcome::Skipped(format!(
            "checkout run for intent {} is already resolved ({:?}) — benign Stripe re-delivery",
            event.payment_intent_id.0, row.process_status
        )));
    }
    // The frozen checkout, read back from the Payment aggregate's own stream (ADR-20260719-193500):
    // the run exists, so its Payment (born by PaymentIntentCreated) must too — a missing birth is the
    // same orphan anomaly class, never a silent skip.
    let (payment_events, _) = store.load(&domain::payment::stream(&event.payment_intent_id)).await?;
    let Some(snap) = payment_events.iter().find_map(|e| match e {
        DomainEvent::PaymentIntentCreated(created) => Some(created.checkout.clone()),
        _ => None,
    }) else {
        return Err(orphaned(&event.payment_intent_id));
    };

    let actor = saga_actor(env);
    let repo = Repository::new(store);

    // deliver OrderPlaced → Order-<orderId> (from_state: order_id; payload from the frozen snapshot).
    // Idempotent: the order fold is Some(_) iff OrderPlaced is already on the stream (a bare
    // PaymentIntentCreated folds to None — domain::order).
    let (order_events, order_version) = store.load(&order_stream(&row.order_id)).await?;
    if domain::order::fold(&order_events).is_none() {
        let placed = DomainEvent::OrderPlaced(OrderPlaced {
            mode: snap.mode,
            order_id: row.order_id,
            r#ref: snap.r#ref.clone(),
            restaurant_id: snap.restaurant_id,
            customer_id: snap.customer_id,
            customer_contact: snap.customer_contact.clone(),
            service_type: snap.service_type,
            delivery_address: snap.delivery_address.clone(),
            items: snap.items.clone(),
            total_amount: snap.total_amount.clone(),
            breakdown: snap.breakdown.clone(),
            note: snap.note.clone(),
            payment_intent_id: event.payment_intent_id.clone(),
        });
        repo.save(&order_stream(&row.order_id), order_version, &[placed], &actor).await?;
    }

    // deliver CartCheckedOut → Cart-<cartId>, only while the cart is still OPEN (a replay after a
    // partial reaction finds it CHECKED_OUT and appends nothing).
    let (cart_events, cart_version) = store.load(&cart_stream(&row.cart_id)).await?;
    if matches!(domain::cart::fold(&cart_events), Some(c) if c.status == CartStatus::OPEN) {
        let checked_out = DomainEvent::CartCheckedOut(CartCheckedOut {
            cart_id: row.cart_id,
            order_id: row.order_id,
        });
        repo.save(&cart_stream(&row.cart_id), cart_version, &[checked_out], &actor).await?;
    }

    // state.set — resolve the run; the trigger's event id is the Stripe re-delivery dedup key.
    state
        .upsert(&PaymentProcessRow {
            payment_status: PaymentStatus::CAPTURED,
            process_status: PaymentProcessStatus::ORDER_PLACED,
            last_processed_stripe_event_id: Some(ExternalReference(env.event_id.to_string())),
            ..row
        })
        .await?;
    Ok(Outcome::Completed)
}

/// EVENT leg `events.yaml#/PaymentFailed` (rules.yaml#/CheckoutAbortsOnPaymentFailure): same
/// `state.by`/`state.expect` gate as the capture leg, then resolve the row FAILED/FAILED. NO domain
/// event — the cart stays OPEN for the customer to retry checkout.
pub async fn on_payment_failed(
    state: &dyn PaymentProcessStateStore,
    event: &PaymentFailed,
    env: &TriggerEnvelope,
) -> Result<Outcome, DomainError> {
    // state.by payment_intent_id — a failure matching no run is the same anomaly class as an orphan
    // capture: abort and surface.
    let Some(row) = state.by_payment_intent(&event.payment_intent_id).await? else {
        return Err(orphaned(&event.payment_intent_id));
    };
    // state.expect process_status = AWAITING_PAYMENT_RESULT — already-resolved run → benign skip.
    if row.process_status != PaymentProcessStatus::AWAITING_PAYMENT_RESULT {
        return Ok(Outcome::Skipped(format!(
            "checkout run for intent {} is already resolved ({:?}) — benign Stripe re-delivery",
            event.payment_intent_id.0, row.process_status
        )));
    }
    state
        .upsert(&PaymentProcessRow {
            payment_status: PaymentStatus::FAILED,
            process_status: PaymentProcessStatus::FAILED,
            last_processed_stripe_event_id: Some(ExternalReference(env.event_id.to_string())),
            ..row
        })
        .await?;
    Ok(Outcome::Completed)
}

// ================================================================================================
// Behaviour tests (specs/tests.yaml, PlaceOrderProcess saga) — each linked to its rules.yaml rule.
// ================================================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::pm_state::mem::MemPaymentProcessState;
    use crate::process_managers::test_support::{envelope, MemStore};
    use domain::generated::entities::{
        CheckoutSnapshot, CustomerContact, Money, PaymentBreakdown,
    };
    use domain::generated::events::{CartLineAdded, CartStarted, PaymentIntentCreated};
    use domain::generated::scalars::*;

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }
    fn eur(cents: i64) -> Money {
        Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
    }
    fn breakdown(total: i64) -> PaymentBreakdown {
        PaymentBreakdown {
            articles: eur(total),
            delivery: eur(0),
            service_fee: eur(0),
            total: eur(total),
            restaurant_contribution: eur(0),
            restaurant_payout: eur(total),
            rider_payout: eur(0),
            captain_net: eur(0),
        }
    }
    fn snapshot() -> CheckoutSnapshot {
        CheckoutSnapshot {
            order_id: OrderId(uid(1)),
            cart_id: CartId(uid(2)),
            restaurant_id: RestaurantId(uid(3)),
            customer_id: None,
            mode: None,
            r#ref: None,
            customer_contact: CustomerContact {
                display_name: CustomerDisplayName("Johnny".into()),
                email: None,
                phone: PhoneNumber("+33612345678".into()),
            },
            service_type: ServiceType::DELIVERY,
            delivery_address: None,
            items: Vec::new(),
            total_amount: eur(1960),
            breakdown: breakdown(1960),
            note: None,
        }
    }
    fn captured() -> PaymentCaptured {
        PaymentCaptured {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            order_id: Some(OrderId(uid(1))),
            restaurant_id: RestaurantId(uid(3)),
            amount: eur(1960),
        }
    }
    fn failed() -> PaymentFailed {
        PaymentFailed {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            restaurant_id: RestaurantId(uid(3)),
            reason: "card_declined".into(),
        }
    }
    /// GIVEN: the AWAITING_PAYMENT_RESULT run row the command leg opened.
    fn awaiting_row() -> PaymentProcessRow {
        PaymentProcessRow {
            cart_id: CartId(uid(2)),
            order_id: OrderId(uid(1)),
            payment_intent_id: PaymentIntentId("pi_123".into()),
            process_status: PaymentProcessStatus::AWAITING_PAYMENT_RESULT,
            payment_status: PaymentStatus::PENDING,
            last_processed_stripe_event_id: None,
            last_update_utc: chrono::DateTime::<chrono::Utc>::MIN_UTC,
        }
    }
    /// GIVEN: the Payment stream born with the frozen checkout (delivered by the command leg).
    fn payment_stream_events() -> Vec<DomainEvent> {
        vec![DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            restaurant_id: RestaurantId(uid(3)),
            customer_id: None,
            amount: eur(1960),
            checkout: snapshot(),
        })]
    }
    fn open_cart_events() -> Vec<DomainEvent> {
        vec![
            DomainEvent::CartStarted(CartStarted {
                cart_id: CartId(uid(2)),
                restaurant_id: RestaurantId(uid(3)),
                session_id: SessionId(uid(7)),
                customer_id: None,
            }),
            DomainEvent::CartLineAdded(CartLineAdded {
                cart_id: CartId(uid(2)),
                line: domain::generated::entities::CartLineItem {
                    cart_line_id: CartLineId(uid(9)),
                    offer_id: OfferId(uid(8)),
                    quantity: 2,
                    selected_option_ids: Vec::new(),
                },
            }),
        ]
    }
    /// GIVEN wiring: run row + Payment stream + open cart, as the command leg leaves them.
    async fn given(store: &MemStore, state: &MemPaymentProcessState) {
        use crate::pm_state::PaymentProcessStateStore as _;
        state.upsert(&awaiting_row()).await.unwrap();
        store.seed(&domain::payment::stream(&PaymentIntentId("pi_123".into())), payment_stream_events());
        store.seed(&format!("Cart-{}", uid(2)), open_cart_events());
    }

    /// tests.yaml#/TestPlaceOrderPaymentCapturedPlacesOrder —
    /// rules.yaml#/OrderMaterializedOnPaymentCapture: on payment capture the saga materializes the
    /// order from the frozen snapshot, closes the cart, and resolves the run row.
    #[tokio::test]
    async fn payment_captured_places_order_and_closes_cart() {
        let store = MemStore::default();
        let state = MemPaymentProcessState::default();
        given(&store, &state).await;

        let outcome = on_payment_captured(&store, &state, &captured(), &envelope()).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);

        // THEN: the order is born PLACED from the frozen checkout…
        let order_events = store.stream(&format!("Order-{}", uid(1)));
        let placed = order_events
            .iter()
            .find_map(|e| match e {
                DomainEvent::OrderPlaced(p) => Some(p.clone()),
                _ => None,
            })
            .expect("OrderPlaced on the order stream");
        assert_eq!(placed.order_id, OrderId(uid(1)));
        assert_eq!(placed.payment_intent_id, PaymentIntentId("pi_123".into()));
        assert_eq!(placed.total_amount, eur(1960));
        assert_eq!(domain::order::fold(&order_events).unwrap().status, OrderStatus::PLACED);
        // …the cart is CHECKED_OUT…
        let cart_events = store.stream(&format!("Cart-{}", uid(2)));
        assert_eq!(domain::cart::fold(&cart_events).unwrap().status, CartStatus::CHECKED_OUT);
        // …and the row is resolved with the Stripe dedup key.
        let row = state.by_cart(CartId(uid(2))).await.unwrap().unwrap();
        assert_eq!(row.process_status, PaymentProcessStatus::ORDER_PLACED);
        assert_eq!(row.payment_status, PaymentStatus::CAPTURED);
        assert_eq!(
            row.last_processed_stripe_event_id,
            Some(ExternalReference(envelope().event_id.to_string()))
        );
    }

    /// rules.yaml#/OrderMaterializedOnPaymentCapture (idempotency corollary): a re-delivered capture
    /// finds the run resolved (`state.expect` fails) and skips — no duplicate order.
    #[tokio::test]
    async fn payment_captured_re_delivery_is_a_benign_skip() {
        let store = MemStore::default();
        let state = MemPaymentProcessState::default();
        given(&store, &state).await;

        on_payment_captured(&store, &state, &captured(), &envelope()).await.unwrap();
        let first_order = store.stream(&format!("Order-{}", uid(1)));
        let second = on_payment_captured(&store, &state, &captured(), &envelope()).await.unwrap();
        assert!(matches!(second, Outcome::Skipped(ref m) if m.contains("already resolved")), "{second:?}");
        assert_eq!(store.stream(&format!("Order-{}", uid(1))), first_order);
    }

    /// tests.yaml#/TestPaymentCaptureOrphanIsFlagged — rules.yaml#/OrphanPaymentEventFlagged: a
    /// capture matching no checkout run aborts the saga with the typed error (never a silent skip).
    #[tokio::test]
    async fn orphan_capture_is_flagged_with_the_typed_error() {
        let store = MemStore::default();
        let state = MemPaymentProcessState::default();
        let err = on_payment_captured(&store, &state, &captured(), &envelope()).await.unwrap_err();
        assert_eq!(err.code(), Some("PaymentEventOrphaned"), "{err:?}");
    }

    /// rules.yaml#/OrphanPaymentEventFlagged (Payment-stream corollary): a run whose Payment stream
    /// has no `PaymentIntentCreated` birth is the same orphan anomaly class.
    #[tokio::test]
    async fn capture_with_a_run_but_no_payment_stream_is_flagged() {
        let store = MemStore::default();
        let state = MemPaymentProcessState::default();
        use crate::pm_state::PaymentProcessStateStore as _;
        state.upsert(&awaiting_row()).await.unwrap(); // row, but NO Payment stream seeded
        let err = on_payment_captured(&store, &state, &captured(), &envelope()).await.unwrap_err();
        assert_eq!(err.code(), Some("PaymentEventOrphaned"), "{err:?}");
    }

    /// tests.yaml#/TestPlaceOrderPaymentFailedPlacesNothing —
    /// rules.yaml#/CheckoutAbortsOnPaymentFailure: on payment failure the saga resolves the run,
    /// places no order, and the cart stays OPEN.
    #[tokio::test]
    async fn payment_failed_places_nothing_and_keeps_the_cart_open() {
        let store = MemStore::default();
        let state = MemPaymentProcessState::default();
        given(&store, &state).await;

        let outcome = on_payment_failed(&state, &failed(), &envelope()).await.unwrap();
        assert_eq!(outcome, Outcome::Completed);
        assert!(store.stream(&format!("Order-{}", uid(1))).is_empty()); // no OrderPlaced
        let cart_events = store.stream(&format!("Cart-{}", uid(2)));
        assert_eq!(domain::cart::fold(&cart_events).unwrap().status, CartStatus::OPEN);
        let row = state.by_cart(CartId(uid(2))).await.unwrap().unwrap();
        assert_eq!(row.process_status, PaymentProcessStatus::FAILED);
        assert_eq!(row.payment_status, PaymentStatus::FAILED);

        // A re-delivered failure (or a late capture on the failed run) is a benign skip.
        let again = on_payment_failed(&state, &failed(), &envelope()).await.unwrap();
        assert!(matches!(again, Outcome::Skipped(_)), "{again:?}");
        // An orphan failure (no run) is the same typed error as an orphan capture.
        let mut other = failed();
        other.payment_intent_id = PaymentIntentId("pi_unknown".into());
        let err = on_payment_failed(&state, &other, &envelope()).await.unwrap_err();
        assert_eq!(err.code(), Some("PaymentEventOrphaned"), "{err:?}");
    }
}
