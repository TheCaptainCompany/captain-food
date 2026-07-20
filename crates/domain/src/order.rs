//! Order aggregate — the PURE write-side state fold (ADR-0035/0046). Command handlers rehydrate an
//! [`OrderState`] by folding the stream's events (loaded through the `EventStore` port) and then enforce
//! the invariants declared in `specs/actors.yaml`/`specs/errors.yaml` against it — chiefly the lifecycle
//! status machine (`rules.yaml#/OrderLifecycleStatusMachine`) and the rate-once flags. Deliberately
//! MINIMAL: the full read model lives in the `OrderTracking` projection (ADR-0040), not here. No I/O, no
//! serialization logic (dependency rule).
//!
//! The status machine is the DECLARED lifecycle (`specs/actors.yaml#/Order/lifecycle`,
//! ADR-20260720-004419): the fold moves `status` exclusively through the GENERATED tables
//! ([`lifecycle::initial`] births it, [`lifecycle::target`] applies recorded facts) and the command
//! handlers guard with [`lifecycle::transition`], so the write side can never disagree with the
//! spec — and it mirrors the read-side `OrderTrackingProjector`. The stream is born
//! by `OrderPlaced` (emitted by PlaceOrderProcess on payment capture); a `PaymentIntentCreated`
//! appended before it (the saga's first leg lives on the same `Order-<id>` stream) folds as a no-op,
//! so the order "does not exist" until it is actually placed.

use crate::generated::events::DomainEvent;
pub use crate::generated::lifecycles::order as lifecycle;
use crate::generated::scalars::{CustomerId, OrderStatus, RestaurantId};

/// What the Order command handlers need to know about the aggregate to accept or reject a command.
/// `None` (from [`fold`]) means the order does not exist → `OrderNotFound`.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderState {
    /// Lifecycle position (PLACED → ACCEPTED → PREPARING → READY → … or a terminal reject/cancel) —
    /// `InvalidOrderStatus`.
    pub status: OrderStatus,
    /// The restaurant the order was placed against; commands carrying another restaurantId are scoped
    /// out as `OrderNotFound`.
    pub restaurant_id: RestaurantId,
    /// The owning customer when the order was placed authenticated — stamped onto the feedback events
    /// (OrderRated / RestaurantRated / OrderTipped / RefundRequested payloads).
    pub customer_id: Option<CustomerId>,
    /// Whether the delivery (rider thumb) was already rated — `OrderAlreadyRated` (rate-once).
    pub delivery_rated: bool,
    /// Whether the restaurant was already rated for this order — `RestaurantAlreadyRated` (rate-once).
    pub restaurant_rated: bool,
}

impl OrderState {
    /// Whether the order sits in a terminal reject/cancel state (no further transitions, no tips).
    /// Narrower than [`lifecycle::is_terminal`]: DELIVERED is lifecycle-terminal too, but a
    /// delivered order still accepts feedback (ratings, tips, refund requests) — a rejected or
    /// cancelled one does not.
    pub fn is_terminated(&self) -> bool {
        lifecycle::is_terminal(self.status) && self.status != OrderStatus::DELIVERED
    }
}

/// Fold an Order stream (events in version order) into its current state. `None` ⇔ the stream has no
/// `OrderPlaced` yet, i.e. the order does not exist (a bare `PaymentIntentCreated` is not an order).
pub fn fold(events: &[DomainEvent]) -> Option<OrderState> {
    events.iter().fold(None, apply)
}

/// Apply one event to the state — a pure transition, total over the whole event union (events not
/// touching the folded fields are no-ops, so a fatter stream never breaks rehydration). The status
/// moves ONLY through the generated lifecycle table; the hand-written part is payload extraction and
/// the non-status flags.
fn apply(state: Option<OrderState>, event: &DomainEvent) -> Option<OrderState> {
    if let Some(status) = lifecycle::initial(event) {
        if let DomainEvent::OrderPlaced(e) = event {
            return Some(OrderState {
                status,
                restaurant_id: e.restaurant_id,
                customer_id: e.customer_id,
                delivery_rated: false,
                restaurant_rated: false,
            });
        }
    }
    let mut s = state?;
    // The recorded fact wins at fold time: `target` maps a lifecycle event to its state regardless
    // of the current one (legality is `transition`'s job, enforced by the handlers at append time).
    if let Some(next) = lifecycle::target(event) {
        s.status = next;
    }
    match event {
        DomainEvent::OrderRated(_) => s.delivery_rated = true,
        DomainEvent::RestaurantRated(_) => s.restaurant_rated = true,
        _ => {}
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::entities::{CustomerContact, Money, PaymentBreakdown};
    use crate::generated::events::{
        OrderAcceptedByRestaurant, OrderDelivered, OrderMarkedReady, OrderPlaced,
        OrderPreparationStarted, OrderRejectedByRestaurant,
    };
    use crate::generated::scalars::*;

    fn oid() -> OrderId {
        OrderId(uuid::Uuid::nil())
    }
    fn rid() -> RestaurantId {
        RestaurantId(uuid::Uuid::nil())
    }
    fn money(cents: i64) -> Money {
        Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
    }
    fn placed() -> DomainEvent {
        let z = money(0);
        DomainEvent::OrderPlaced(OrderPlaced {
            mode: None,
            order_id: oid(),
            r#ref: None,
            restaurant_id: rid(),
            customer_id: None,
            customer_contact: CustomerContact {
                display_name: CustomerDisplayName("Jo".into()),
                email: None,
                phone: PhoneNumber("+33600000000".into()),
            },
            service_type: ServiceType::DELIVERY,
            delivery_address: None,
            items: vec![],
            total_amount: money(1000),
            breakdown: PaymentBreakdown {
                articles: z.clone(),
                delivery: z.clone(),
                service_fee: z.clone(),
                total: money(1000),
                restaurant_contribution: z.clone(),
                restaurant_payout: z.clone(),
                rider_payout: z.clone(),
                captain_net: z,
            },
            note: None,
            payment_intent_id: PaymentIntentId("pi_test_1".into()),
        })
    }
    fn accepted() -> DomainEvent {
        DomainEvent::OrderAcceptedByRestaurant(OrderAcceptedByRestaurant {
            order_id: oid(),
            restaurant_id: rid(),
            estimated_ready_at: None,
        })
    }
    fn preparing() -> DomainEvent {
        DomainEvent::OrderPreparationStarted(OrderPreparationStarted { order_id: oid(), restaurant_id: rid() })
    }
    fn ready() -> DomainEvent {
        DomainEvent::OrderMarkedReady(OrderMarkedReady { order_id: oid(), restaurant_id: rid() })
    }
    fn delivered() -> DomainEvent {
        DomainEvent::OrderDelivered(OrderDelivered { order_id: oid(), restaurant_id: rid() })
    }
    fn rejected() -> DomainEvent {
        DomainEvent::OrderRejectedByRestaurant(OrderRejectedByRestaurant {
            order_id: oid(),
            restaurant_id: rid(),
            reason: "Out of ingredients".into(),
        })
    }

    /// The generated table IS the declared machine (actors.yaml#/Order/lifecycle,
    /// rules.yaml#/OrderLifecycleIsExplicit): spot-check the legal moves, the notable illegal
    /// jumps, and that every terminal state has no outgoing transition for any lifecycle event.
    #[test]
    fn generated_transition_table_matches_the_declared_machine() {
        use OrderStatus::*;
        // Birth.
        assert_eq!(lifecycle::initial(&placed()), Some(PLACED));
        assert_eq!(lifecycle::initial(&accepted()), None);
        // The happy path (with the ACCEPTED → READY preparation shortcut).
        assert_eq!(lifecycle::transition(PLACED, &accepted()), Some(ACCEPTED));
        assert_eq!(lifecycle::transition(ACCEPTED, &preparing()), Some(PREPARING));
        assert_eq!(lifecycle::transition(ACCEPTED, &ready()), Some(READY));
        assert_eq!(lifecycle::transition(PREPARING, &ready()), Some(READY));
        assert_eq!(lifecycle::transition(READY, &delivered()), Some(DELIVERED));
        assert_eq!(lifecycle::transition(PLACED, &rejected()), Some(REJECTED));
        // The notable illegal jumps.
        assert_eq!(lifecycle::transition(PLACED, &ready()), None);
        assert_eq!(lifecycle::transition(PLACED, &delivered()), None);
        assert_eq!(lifecycle::transition(ACCEPTED, &accepted()), None);
        assert_eq!(lifecycle::transition(PREPARING, &rejected()), None);
        // OUT_FOR_DELIVERY is read-side only — no write-side event leaves (or enters) it.
        assert_eq!(lifecycle::transition(OUT_FOR_DELIVERY, &delivered()), None);
        // Terminal states admit no lifecycle transition at all.
        for &terminal in lifecycle::TERMINAL {
            assert!(lifecycle::is_terminal(terminal));
            for ev in [&accepted(), &preparing(), &ready(), &delivered(), &rejected()] {
                assert_eq!(lifecycle::transition(terminal, ev), None, "{:?}", terminal);
            }
        }
        assert!(!lifecycle::is_terminal(PLACED));
    }

    /// The fold births via `initial` and applies recorded facts via `target` — the appended fact
    /// wins at fold time (legality is the handlers' append-time concern).
    #[test]
    fn fold_follows_the_recorded_facts_through_the_generated_table() {
        assert_eq!(fold(&[]), None);
        assert_eq!(fold(&[accepted()]), None); // no birth, no order
        let s = fold(&[placed(), accepted(), preparing(), ready(), delivered()]).unwrap();
        assert_eq!(s.status, OrderStatus::DELIVERED);
        assert!(!s.is_terminated()); // DELIVERED still accepts feedback
        let s = fold(&[placed(), rejected()]).unwrap();
        assert_eq!(s.status, OrderStatus::REJECTED);
        assert!(s.is_terminated());
        // An abbreviated (test-style) stream still folds to the recorded fact's state.
        assert_eq!(fold(&[placed(), ready()]).unwrap().status, OrderStatus::READY);
    }
}
