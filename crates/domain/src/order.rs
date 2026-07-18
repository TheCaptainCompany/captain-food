//! Order aggregate — the PURE write-side state fold (ADR-0035/0046). Command handlers rehydrate an
//! [`OrderState`] by folding the stream's events (loaded through the `EventStore` port) and then enforce
//! the invariants declared in `specs/actors.yaml`/`specs/errors.yaml` against it — chiefly the lifecycle
//! status machine (`rules.yaml#/OrderLifecycleStatusMachine`) and the rate-once flags. Deliberately
//! MINIMAL: the full read model lives in the `OrderTracking` projection (ADR-0040), not here. No I/O, no
//! serialization logic (dependency rule).
//!
//! The status mapping mirrors the read-side `OrderTrackingProjector` so write-side decisions and the
//! projected `status` column can never disagree. The stream is born by `OrderPlaced` (emitted by
//! PlaceOrderProcess on payment capture); a `PaymentIntentCreated` appended before it (the saga's first
//! leg lives on the same `Order-<id>` stream) folds as a no-op, so the order "does not exist" until it
//! is actually placed.

use crate::generated::events::DomainEvent;
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
    pub fn is_terminated(&self) -> bool {
        matches!(
            self.status,
            OrderStatus::REJECTED
                | OrderStatus::CANCELLED_BY_CUSTOMER
                | OrderStatus::CANCELLED_BY_RESTAURANT
        )
    }
}

/// Fold an Order stream (events in version order) into its current state. `None` ⇔ the stream has no
/// `OrderPlaced` yet, i.e. the order does not exist (a bare `PaymentIntentCreated` is not an order).
pub fn fold(events: &[DomainEvent]) -> Option<OrderState> {
    events.iter().fold(None, apply)
}

/// Apply one event to the state — a pure transition, total over the whole event union (events not
/// touching the folded fields are no-ops, so a fatter stream never breaks rehydration).
fn apply(state: Option<OrderState>, event: &DomainEvent) -> Option<OrderState> {
    if let DomainEvent::OrderPlaced(e) = event {
        return Some(OrderState {
            status: OrderStatus::PLACED,
            restaurant_id: e.restaurant_id,
            customer_id: e.customer_id,
            delivery_rated: false,
            restaurant_rated: false,
        });
    }
    let mut s = state?;
    match event {
        DomainEvent::OrderAcceptedByRestaurant(_) => s.status = OrderStatus::ACCEPTED,
        DomainEvent::OrderPreparationStarted(_) => s.status = OrderStatus::PREPARING,
        DomainEvent::OrderMarkedReady(_) => s.status = OrderStatus::READY,
        DomainEvent::OrderDelivered(_) => s.status = OrderStatus::DELIVERED,
        DomainEvent::OrderRejectedByRestaurant(_) => s.status = OrderStatus::REJECTED,
        DomainEvent::OrderCancelledByCustomer(_) => s.status = OrderStatus::CANCELLED_BY_CUSTOMER,
        DomainEvent::OrderCancelledByRestaurant(_) => s.status = OrderStatus::CANCELLED_BY_RESTAURANT,
        DomainEvent::OrderRated(_) => s.delivery_rated = true,
        DomainEvent::RestaurantRated(_) => s.restaurant_rated = true,
        _ => {}
    }
    Some(s)
}
