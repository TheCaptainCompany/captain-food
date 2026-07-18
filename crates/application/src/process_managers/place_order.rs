//! PlaceOrderProcess (actors.yaml#/PlaceOrderProcess) — the event-driven legs of the checkout saga.
//! The command leg (`commands.yaml#/PlaceOrder` → `PaymentIntentCreated`) is `commands::place_order`;
//! this module reacts to the INBOUND Stripe payment outcomes:
//!
//! - `events.yaml#/PaymentCaptured` → emit `OrderPlaced` on `Order-<orderId>` + `CartCheckedOut` on
//!   `Cart-<cartId>` (the Order is born from the frozen checkout and the cart is closed).
//! - `events.yaml#/PaymentFailed` → nothing: no OrderPlaced, the cart stays OPEN
//!   (rules.yaml#/CheckoutAbortsOnPaymentFailure).
//!
//! The `OrderPlaced` payload (contact, priced items, breakdown) and the cart linkage are NOT in the
//! event log — `PaymentIntentCreated` (frozen DSL) does not carry them — so the reaction requires the
//! [`CheckoutSnapshot`] resolved by the `CheckoutSnapshotSource` port. No snapshot → [`Decision::Skip`]
//! (fail closed: an order fact is never guessed).

use domain::generated::events::{
    CartCheckedOut, DomainEvent, OrderPlaced, PaymentCaptured, PaymentFailed,
};
use domain::generated::scalars::CartStatus;

use crate::ports::CheckoutSnapshot;
use crate::process_managers::{cart_stream, order_stream, Decision, StreamAppend};

/// React to `PaymentCaptured`: materialize the Order and close the cart
/// (rules.yaml#/OrderMaterializedOnPaymentCapture).
///
/// - `snapshot` — the frozen checkout for this PaymentIntent (`None` → Skip, fail closed).
/// - `order_events`/`order_version` — the `Order-<orderId>` stream (holds `PaymentIntentCreated`;
///   holds `OrderPlaced` iff the saga already reacted → Nothing, idempotent).
/// - `cart_events`/`cart_version` — the `Cart-<cartId>` stream (skip the close when already
///   CHECKED_OUT, e.g. a replay after a partial reaction).
pub fn on_payment_captured(
    event: &PaymentCaptured,
    snapshot: Option<&CheckoutSnapshot>,
    order_events: &[DomainEvent],
    order_version: i64,
    cart_events: &[DomainEvent],
    cart_version: i64,
) -> Decision {
    let Some(snap) = snapshot else {
        // TODO(saga): the real CheckoutSnapshotSource is the Stripe integration workstream's
        // (PaymentIntent metadata frozen at create-intent time, or a pending-checkout store keyed by
        // payment_intent_id). Model gap escalated: events.yaml#/PaymentIntentCreated carries no
        // checkout snapshot (no cartId/contact/serviceType/items/breakdown), so the saga cannot
        // reconstruct OrderPlaced from the log alone.
        return Decision::Skip(format!(
            "no checkout snapshot for payment intent {} — cannot materialize OrderPlaced/CartCheckedOut \
             (TODO(saga): CheckoutSnapshotSource stand-in is fail-closed until the Stripe adapter lands)",
            event.payment_intent_id.0
        ));
    };
    // Cross-check: when Stripe metadata carried our orderId, it must be the snapshot's order.
    if let Some(order_id) = event.order_id {
        if order_id != snap.order_id {
            return Decision::Skip(format!(
                "payment intent {} reports order {} but the checkout snapshot is for order {} — not reacting",
                event.payment_intent_id.0, order_id.0, snap.order_id.0
            ));
        }
    }
    // Idempotent re-reaction: the order fold is Some(_) iff OrderPlaced is already on the stream
    // (a bare PaymentIntentCreated folds to None — domain::order).
    if domain::order::fold(order_events).is_some() {
        return Decision::Nothing;
    }

    let mut appends = vec![StreamAppend {
        stream_name: order_stream(&snap.order_id),
        expected_version: order_version,
        events: vec![DomainEvent::OrderPlaced(OrderPlaced {
            mode: snap.mode,
            order_id: snap.order_id,
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
        })],
    }];
    // Close the cart unless a previous (partial) reaction already did.
    let cart_open = matches!(
        domain::cart::fold(cart_events),
        Some(state) if state.status == CartStatus::OPEN
    );
    if cart_open {
        appends.push(StreamAppend {
            stream_name: cart_stream(&snap.cart_id),
            expected_version: cart_version,
            events: vec![DomainEvent::CartCheckedOut(CartCheckedOut {
                cart_id: snap.cart_id,
                order_id: snap.order_id,
            })],
        });
    }
    Decision::Act(appends)
}

/// React to `PaymentFailed`: abort the saga — no OrderPlaced, the cart stays OPEN so the customer can
/// retry checkout (actors.yaml effect; rules.yaml#/CheckoutAbortsOnPaymentFailure). Deliberately emits
/// nothing (`emits: []`).
pub fn on_payment_failed(_event: &PaymentFailed) -> Decision {
    Decision::Nothing
}

// ================================================================================================
// Behaviour tests (specs/tests.yaml, PlaceOrderProcess saga) — each linked to its rules.yaml rule.
// ================================================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{Actor, EventStore};
    use domain::generated::entities::{CustomerContact, Money, PaymentBreakdown};
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
    /// Given: the saga's first leg (PaymentIntentCreated on the Order stream) + the open cart.
    fn given_intent() -> Vec<DomainEvent> {
        vec![DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            restaurant_id: RestaurantId(uid(3)),
            customer_id: None,
            amount: eur(1960),
        })]
    }
    fn given_open_cart() -> Vec<DomainEvent> {
        vec![
            DomainEvent::CartStarted(CartStarted {
                cart_id: CartId(uid(2)),
                restaurant_id: RestaurantId(uid(3)),
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

    /// tests.yaml#/TestPlaceOrderPaymentCapturedPlacesOrder —
    /// rules.yaml#/OrderMaterializedOnPaymentCapture: on payment capture the saga materializes the
    /// order (OrderPlaced) and closes the cart (CartCheckedOut).
    #[test]
    fn payment_captured_places_order_and_closes_cart() {
        let snap = snapshot();
        let decision =
            on_payment_captured(&captured(), Some(&snap), &given_intent(), 1, &given_open_cart(), 2);
        let appends = decision.appends();
        assert_eq!(appends.len(), 2, "expected order + cart appends, got {decision:?}");
        assert_eq!(appends[0].stream_name, format!("Order-{}", uid(1)));
        assert_eq!(appends[0].expected_version, 1);
        let DomainEvent::OrderPlaced(placed) = &appends[0].events[0] else {
            panic!("expected OrderPlaced, got {:?}", appends[0].events[0]);
        };
        assert_eq!(placed.order_id, OrderId(uid(1)));
        assert_eq!(placed.payment_intent_id, PaymentIntentId("pi_123".into()));
        assert_eq!(placed.total_amount, eur(1960));
        assert_eq!(appends[1].stream_name, format!("Cart-{}", uid(2)));
        assert_eq!(
            appends[1].events,
            vec![DomainEvent::CartCheckedOut(CartCheckedOut {
                cart_id: CartId(uid(2)),
                order_id: OrderId(uid(1)),
            })]
        );
    }

    /// rules.yaml#/OrderMaterializedOnPaymentCapture (idempotency corollary): re-reacting to the same
    /// PaymentCaptured after OrderPlaced is on the stream emits nothing — no duplicate order.
    #[test]
    fn payment_captured_re_reaction_is_a_no_op() {
        let snap = snapshot();
        let first =
            on_payment_captured(&captured(), Some(&snap), &given_intent(), 1, &given_open_cart(), 2);
        // Fold the first reaction's order event back into the stream, then react again.
        let mut order_events = given_intent();
        order_events.extend(first.appends()[0].events.iter().cloned());
        let mut cart_events = given_open_cart();
        cart_events.extend(first.appends()[1].events.iter().cloned());
        let second =
            on_payment_captured(&captured(), Some(&snap), &order_events, 2, &cart_events, 3);
        assert_eq!(second, Decision::Nothing);
    }

    /// Fail-closed seam: without a checkout snapshot the saga SKIPS (never guesses an order fact).
    #[test]
    fn payment_captured_without_snapshot_skips() {
        let decision = on_payment_captured(&captured(), None, &given_intent(), 1, &[], 0);
        assert!(matches!(decision, Decision::Skip(ref msg) if msg.contains("pi_123")), "{decision:?}");
    }

    /// tests.yaml#/TestPlaceOrderPaymentFailedPlacesNothing —
    /// rules.yaml#/CheckoutAbortsOnPaymentFailure: on payment failure the saga aborts, places no
    /// order, and the cart stays OPEN.
    #[test]
    fn payment_failed_places_nothing() {
        let failed = PaymentFailed {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            restaurant_id: RestaurantId(uid(3)),
            reason: "card_declined".into(),
        };
        assert_eq!(on_payment_failed(&failed), Decision::Nothing);
        // The cart fold over the untouched stream is still OPEN.
        let cart = domain::cart::fold(&given_open_cart()).expect("cart exists");
        assert_eq!(cart.status, CartStatus::OPEN);
    }

    // --- End-to-end over an in-memory EventStore: Given pre-seeded trigger streams, When the PM
    //     reacts and its appends are executed, Then the streams hold the emitted facts (and a replay
    //     of the same trigger is absorbed). Mirrors how the infrastructure runner executes decisions.
    struct InMemoryEventStore {
        streams: std::sync::Mutex<std::collections::HashMap<String, Vec<DomainEvent>>>,
    }

    #[async_trait::async_trait]
    impl EventStore for InMemoryEventStore {
        async fn append(
            &self,
            stream_name: &str,
            expected_version: i64,
            events: &[DomainEvent],
            _actor: &Actor,
        ) -> Result<i64, domain::shared::errors::DomainError> {
            let mut streams = self.streams.lock().unwrap();
            let stream = streams.entry(stream_name.to_string()).or_default();
            if stream.len() as i64 != expected_version {
                return Err(crate::ports::version_conflict(stream_name, expected_version));
            }
            stream.extend_from_slice(events);
            Ok(stream.len() as i64)
        }
        async fn load(
            &self,
            stream_name: &str,
        ) -> Result<(Vec<DomainEvent>, i64), domain::shared::errors::DomainError> {
            let streams = self.streams.lock().unwrap();
            let events = streams.get(stream_name).cloned().unwrap_or_default();
            let version = events.len() as i64;
            Ok((events, version))
        }
    }

    /// rules.yaml#/OrderMaterializedOnPaymentCapture, over the store: react → append → replay → no-op.
    #[tokio::test]
    async fn reaction_cycle_over_event_store_is_idempotent() {
        let store = InMemoryEventStore { streams: Default::default() };
        let actor = Actor {
            user_id: uuid::Uuid::nil(),
            user_type: 6,
            correlation_id: uuid::Uuid::nil(),
            cause_id: None,
        };
        let snap = snapshot();
        let order_stream_name = format!("Order-{}", uid(1));
        let cart_stream_name = format!("Cart-{}", uid(2));
        // Given: the saga's first leg + the open cart, pre-seeded.
        store.append(&order_stream_name, 0, &given_intent(), &actor).await.unwrap();
        store.append(&cart_stream_name, 0, &given_open_cart(), &actor).await.unwrap();

        // When: the PM reacts to PaymentCaptured (the runner's load → decide → append cycle).
        async fn react(
            store: &InMemoryEventStore,
            snap: &CheckoutSnapshot,
            order_stream_name: &str,
            cart_stream_name: &str,
        ) -> Decision {
            let (order_events, order_version) = store.load(order_stream_name).await.unwrap();
            let (cart_events, cart_version) = store.load(cart_stream_name).await.unwrap();
            on_payment_captured(
                &captured(),
                Some(snap),
                &order_events,
                order_version,
                &cart_events,
                cart_version,
            )
        }
        let decision = react(&store, &snap, &order_stream_name, &cart_stream_name).await;
        for a in decision.appends() {
            store.append(&a.stream_name, a.expected_version, &a.events, &actor).await.unwrap();
        }

        // Then: the order is born PLACED and the cart is CHECKED_OUT.
        let (order_events, _) = store.load(&order_stream_name).await.unwrap();
        assert_eq!(domain::order::fold(&order_events).unwrap().status, OrderStatus::PLACED);
        let (cart_events, _) = store.load(&cart_stream_name).await.unwrap();
        assert_eq!(domain::cart::fold(&cart_events).unwrap().status, CartStatus::CHECKED_OUT);

        // And: replaying the SAME trigger (webhook redelivery / runner re-scan) changes nothing.
        assert_eq!(
            react(&store, &snap, &order_stream_name, &cart_stream_name).await,
            Decision::Nothing
        );
    }
}
