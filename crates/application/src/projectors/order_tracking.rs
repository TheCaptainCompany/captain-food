//! Hand-written `OrderTrackingCompute` (ADR-0040). Most columns fold straight from the events — the
//! OrderPlaced breakdown sub-fields, the Stripe payment status, the tip sums by recipient, and the mirrored
//! delivery state. Only the `uber_*` comparison needs the pricing policy tables (deferred to the runtime).
#![allow(unused_variables)]

use crate::projections::{Envelope, OrderTrackingCompute, OrderTrackingRow};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{
    ComparisonBasis, CurrencyCode, DeliveryStatus, MoneyCents, OrderStatus, TipRecipient,
};
use serde_json::{json, Value};

pub struct OrderTrackingProjector;

/// Σ of an OrderTipped event's tips for one recipient, ADDED to the running total (tips accumulate across
/// multiple tip events; ADR-012). Returns None only when nothing has ever been tipped to that recipient.
fn tip_sum(prev: Option<MoneyCents>, env: &Envelope, who: TipRecipient) -> Option<MoneyCents> {
    let base = prev.as_ref().map(|m| m.0).unwrap_or(0);
    if let DomainEvent::OrderTipped(e) = &env.event {
        let add: i64 = e.tips.iter().filter(|t| t.recipient == who).map(|t| t.amount.amount_cents.0).sum();
        if base + add > 0 {
            return Some(MoneyCents(base + add));
        }
    }
    prev
}

impl OrderTrackingCompute for OrderTrackingProjector {
    /// Order lifecycle status, derived from the event type.
    fn status(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> OrderStatus {
        match &env.event {
            DomainEvent::OrderPlaced(_) => OrderStatus::PLACED,
            DomainEvent::OrderAcceptedByRestaurant(_) => OrderStatus::ACCEPTED,
            DomainEvent::OrderPreparationStarted(_) => OrderStatus::PREPARING,
            DomainEvent::OrderMarkedReady(_) => OrderStatus::READY,
            DomainEvent::OrderDelivered(_) => OrderStatus::DELIVERED,
            DomainEvent::OrderRejectedByRestaurant(_) => OrderStatus::REJECTED,
            DomainEvent::OrderCancelledByCustomer(_) => OrderStatus::CANCELLED_BY_CUSTOMER,
            DomainEvent::OrderCancelledByRestaurant(_) => OrderStatus::CANCELLED_BY_RESTAURANT,
            _ => prev.map(|r| r.status.clone()).unwrap_or(OrderStatus::PLACED),
        }
    }

    fn total_amount_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> MoneyCents {
        match &env.event {
            DomainEvent::OrderPlaced(e) => e.total_amount.amount_cents.clone(),
            _ => prev.map(|r| r.total_amount_cents.clone()).unwrap_or(MoneyCents(0)),
        }
    }
    fn currency(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> CurrencyCode {
        match &env.event {
            DomainEvent::OrderPlaced(e) => e.total_amount.currency.clone(),
            _ => prev.map(|r| r.currency.clone()).unwrap_or_else(|| CurrencyCode("EUR".into())),
        }
    }
    // The 3-way-split breakdown (ADR-0016/0018) — extracted once from OrderPlaced.breakdown, then held.
    fn articles_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> MoneyCents {
        match &env.event {
            DomainEvent::OrderPlaced(e) => e.breakdown.articles.amount_cents.clone(),
            _ => prev.map(|r| r.articles_cents.clone()).unwrap_or(MoneyCents(0)),
        }
    }
    fn delivery_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> MoneyCents {
        match &env.event {
            DomainEvent::OrderPlaced(e) => e.breakdown.delivery.amount_cents.clone(),
            _ => prev.map(|r| r.delivery_cents.clone()).unwrap_or(MoneyCents(0)),
        }
    }
    fn service_fee_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> MoneyCents {
        match &env.event {
            DomainEvent::OrderPlaced(e) => e.breakdown.service_fee.amount_cents.clone(),
            _ => prev.map(|r| r.service_fee_cents.clone()).unwrap_or(MoneyCents(0)),
        }
    }
    fn restaurant_payout_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> MoneyCents {
        match &env.event {
            DomainEvent::OrderPlaced(e) => e.breakdown.restaurant_payout.amount_cents.clone(),
            _ => prev.map(|r| r.restaurant_payout_cents.clone()).unwrap_or(MoneyCents(0)),
        }
    }
    fn rider_payout_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> MoneyCents {
        match &env.event {
            DomainEvent::OrderPlaced(e) => e.breakdown.rider_payout.amount_cents.clone(),
            _ => prev.map(|r| r.rider_payout_cents.clone()).unwrap_or(MoneyCents(0)),
        }
    }
    fn captain_net_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> MoneyCents {
        match &env.event {
            DomainEvent::OrderPlaced(e) => e.breakdown.captain_net.amount_cents.clone(),
            _ => prev.map(|r| r.captain_net_cents.clone()).unwrap_or(MoneyCents(0)),
        }
    }

    // Estimated Uber Eats comparison (ADR-0025) — coefficient × articles from the Uber policy tables.
    // TODO(runtime): compute via View_UberEstimationPolicy / View_UberSplitPolicy; preserved meanwhile.
    fn uber_total_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<MoneyCents> {
        prev.and_then(|r| r.uber_total_cents.clone())
    }
    fn uber_restaurant_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<MoneyCents> {
        prev.and_then(|r| r.uber_restaurant_cents.clone())
    }
    fn uber_rider_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<MoneyCents> {
        prev.and_then(|r| r.uber_rider_cents.clone())
    }
    fn uber_platform_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<MoneyCents> {
        prev.and_then(|r| r.uber_platform_cents.clone())
    }
    fn uber_basis(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<ComparisonBasis> {
        prev.and_then(|r| r.uber_basis.clone())
    }

    /// Restaurant's ETA, parsed from OrderAcceptedByRestaurant.estimatedReadyAt (RFC 3339).
    fn estimated_ready_at(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<chrono::DateTime<chrono::Utc>> {
        match &env.event {
            DomainEvent::OrderAcceptedByRestaurant(e) => e.estimated_ready_at.as_ref().and_then(|s| s.parse().ok()),
            _ => prev.and_then(|r| r.estimated_ready_at),
        }
    }

    /// Folded from the Stripe payment facts.
    fn payment_status(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> String {
        match &env.event {
            DomainEvent::PaymentCaptured(_) => "CAPTURED".to_string(),
            DomainEvent::PaymentRefunded(_) => "REFUNDED".to_string(),
            // PlaceOrderProcess emits OrderPlaced ONLY in reaction to PaymentCaptured (the V0 flow is
            // prepaid-online by construction), and that capture sits at an EARLIER log position than
            // the OrderPlaced that creates this row — folding it again here would find no row and be
            // lost. The saga invariant IS the value. Revisit when a non-prepaid service lands.
            DomainEvent::OrderPlaced(_) => "CAPTURED".to_string(),
            _ => prev.map(|r| r.payment_status.clone()).unwrap_or_else(|| "PENDING".to_string()),
        }
    }

    // Tip sums by recipient (ADR-012), accumulated across OrderTipped events.
    fn rider_tip_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<MoneyCents> {
        tip_sum(prev.and_then(|r| r.rider_tip_cents.clone()), env, TipRecipient::RIDER)
    }
    fn restaurant_tip_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<MoneyCents> {
        tip_sum(prev.and_then(|r| r.restaurant_tip_cents.clone()), env, TipRecipient::RESTAURANT)
    }
    fn captain_tip_cents(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<MoneyCents> {
        tip_sum(prev.and_then(|r| r.captain_tip_cents.clone()), env, TipRecipient::CAPTAIN)
    }

    /// Mirror of the order's DeliveryJob status (correlated by order_id; ADR-0031).
    fn delivery_status(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<DeliveryStatus> {
        match &env.event {
            DomainEvent::DeliveryStatusUpdated(e) => Some(e.status.clone()),
            DomainEvent::DeliveryAcceptedByPartner(_) | DomainEvent::DeliveryAcceptedByRider(_) => {
                Some(DeliveryStatus::ASSIGNED)
            }
            DomainEvent::DeliveryCompleted(_) => Some(DeliveryStatus::DELIVERED),
            // Terminal dispatch failure — the offer cap was exhausted (ADR-20260720-004556).
            DomainEvent::DeliveryDispatchFailed(_) => Some(DeliveryStatus::FAILED),
            _ => prev.and_then(|r| r.delivery_status.clone()),
        }
    }
    /// Assigned courier once a partner/rider accepts.
    fn courier(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<Value> {
        match &env.event {
            DomainEvent::DeliveryAcceptedByPartner(e) => serde_json::to_value(&e.courier).ok(),
            DomainEvent::DeliveryAcceptedByRider(e) => Some(json!({ "rider_id": e.rider_id })),
            _ => prev.and_then(|r| r.courier.clone()),
        }
    }
    /// Partner-reported drop-off ETA, parsed from DeliveryAcceptedByPartner.estimatedDropoffAt.
    fn estimated_dropoff_at(&self, prev: Option<&OrderTrackingRow>, env: &Envelope) -> Option<chrono::DateTime<chrono::Utc>> {
        match &env.event {
            DomainEvent::DeliveryAcceptedByPartner(e) => e.estimated_dropoff_at.as_ref().and_then(|s| s.parse().ok()),
            _ => prev.and_then(|r| r.estimated_dropoff_at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::entities::{Money, Tip};
    use domain::generated::events::OrderTipped;
    use domain::generated::scalars::{OrderId, RestaurantId, Tipper};

    const NIL: &str = "00000000-0000-0000-0000-000000000000";
    fn env_tipped(tips: Vec<Tip>) -> Envelope {
        Envelope {
            stream_name: "Order-1".into(),
            position: 1,
            occurred_at: chrono::DateTime::from_timestamp(1, 0).unwrap(),
            event: DomainEvent::OrderTipped(OrderTipped {
                order_id: OrderId(NIL.parse().unwrap()),
                restaurant_id: RestaurantId(NIL.parse().unwrap()),
                tipped_by: Tipper::CUSTOMER,
                customer_id: None,
                tips,
            }),
        }
    }
    fn tip(recipient: TipRecipient, cents: i64) -> Tip {
        Tip { recipient, amount: Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) } }
    }

    #[test]
    fn tips_sum_by_recipient_and_accumulate() {
        let e = env_tipped(vec![tip(TipRecipient::RIDER, 200), tip(TipRecipient::CAPTAIN, 50)]);
        assert_eq!(tip_sum(None, &e, TipRecipient::RIDER), Some(MoneyCents(200)));
        // a later tip event adds to the running total
        assert_eq!(tip_sum(Some(MoneyCents(100)), &e, TipRecipient::RIDER), Some(MoneyCents(300)));
        // nothing tipped to the restaurant → stays None
        assert_eq!(tip_sum(None, &e, TipRecipient::RESTAURANT), None);
    }
}
