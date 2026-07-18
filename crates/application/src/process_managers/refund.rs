//! RefundProcess (actors.yaml#/RefundProcess) — coordinates refunds. Every inbox entry declares
//! `emits: []`: the reactions are an OUTBOUND Stripe refund request (an external call, not a domain
//! command) and the settled fact comes BACK as the inbound `PaymentRefunded` webhook (CLAUDE.md
//! "Commands vs inbound events" — the request/report split).
//!
//! The outbound refund call belongs to the Stripe adapter workstream (concurrent integration session)
//! and is deliberately NOT implemented here: each refund-triggering leg returns [`Decision::Skip`] with
//! a precise `TODO(saga)` so the pending effect is observable in the runner's log, never silently
//! dropped.

use domain::generated::events::{
    OrderCancelledByCustomer, OrderCancelledByRestaurant, OrderRejectedByRestaurant, PaymentRefunded,
    RefundRequested,
};

use crate::process_managers::Decision;

/// The one pending-effect message for a refund-triggering order fact
/// (rules.yaml#/RefundOnRejectionOrCancellation).
fn refund_todo(order_id: &domain::generated::scalars::OrderId, cause: &str) -> Decision {
    Decision::Skip(format!(
        "TODO(saga): request the Stripe refund for order {} ({cause}) — the outbound refund call is \
         the Stripe adapter's (integration workstream); the settled fact returns as the inbound \
         PaymentRefunded webhook",
        order_id.0
    ))
}

/// React to `OrderRejectedByRestaurant`: request the refund from Stripe (actors.yaml effect).
pub fn on_order_rejected(event: &OrderRejectedByRestaurant) -> Decision {
    refund_todo(&event.order_id, "rejected by restaurant")
}

/// React to `OrderCancelledByCustomer`: request the refund from Stripe (actors.yaml effect).
pub fn on_order_cancelled_by_customer(event: &OrderCancelledByCustomer) -> Decision {
    refund_todo(&event.order_id, "cancelled by customer")
}

/// React to `OrderCancelledByRestaurant`: request the refund from Stripe (actors.yaml effect).
pub fn on_order_cancelled_by_restaurant(event: &OrderCancelledByRestaurant) -> Decision {
    refund_todo(&event.order_id, "cancelled by restaurant")
}

/// React to `RefundRequested` (customer-initiated, already gated to DELIVERED orders by the
/// `request_refund` command handler): validate eligibility, then request the refund from Stripe
/// (actors.yaml effect). Eligibility policy beyond the command gate (deadlines, partial amounts) is
/// unmodelled in V0 — the Stripe leg carries the TODO.
pub fn on_refund_requested(event: &RefundRequested) -> Decision {
    refund_todo(&event.order_id, "customer refund request")
}

/// React to `PaymentRefunded` (INBOUND, reported by Stripe through the webhook ACL): the settled fact
/// is ALREADY recorded in `domain_events` by the ingestor — there is nothing further to append
/// (`emits: []`; read models fed by the event fold it, rules.yaml#/RefundSettledFactRecorded).
pub fn on_payment_refunded(_event: &PaymentRefunded) -> Decision {
    Decision::Nothing
}

// ================================================================================================
// Behaviour tests (specs/tests.yaml, RefundProcess saga) — each linked to its rules.yaml rule.
// Every actors.yaml leg emits [] → the decision must carry NO appends.
// ================================================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::entities::Money;
    use domain::generated::scalars::*;

    fn order_id() -> OrderId {
        OrderId(uuid::Uuid::from_u128(1))
    }
    fn restaurant_id() -> RestaurantId {
        RestaurantId(uuid::Uuid::from_u128(3))
    }

    /// tests.yaml#/TestRefundOnOrderRejected — rules.yaml#/RefundOnRejectionOrCancellation:
    /// a rejection triggers the Stripe refund request and emits no domain event.
    #[test]
    fn refund_on_order_rejected_emits_nothing_and_requests_stripe_refund() {
        let d = on_order_rejected(&OrderRejectedByRestaurant {
            order_id: order_id(),
            restaurant_id: restaurant_id(),
            reason: "Out of ingredients".into(),
        });
        assert!(d.appends().is_empty());
        assert!(matches!(d, Decision::Skip(ref m) if m.contains("Stripe refund")), "{d:?}");
    }

    /// tests.yaml#/TestRefundOnOrderCancelledByCustomer — rules.yaml#/RefundOnRejectionOrCancellation.
    #[test]
    fn refund_on_customer_cancellation_emits_nothing() {
        let d = on_order_cancelled_by_customer(&OrderCancelledByCustomer {
            order_id: order_id(),
            restaurant_id: restaurant_id(),
            reason: Some("Changed my mind".into()),
        });
        assert!(d.appends().is_empty());
        assert!(matches!(d, Decision::Skip(_)));
    }

    /// tests.yaml#/TestRefundOnOrderCancelledByRestaurant — rules.yaml#/RefundOnRejectionOrCancellation.
    #[test]
    fn refund_on_restaurant_cancellation_emits_nothing() {
        let d = on_order_cancelled_by_restaurant(&OrderCancelledByRestaurant {
            order_id: order_id(),
            restaurant_id: restaurant_id(),
            reason: "Kitchen closed".into(),
        });
        assert!(d.appends().is_empty());
        assert!(matches!(d, Decision::Skip(_)));
    }

    /// tests.yaml#/TestRefundOnRefundRequested — rules.yaml#/RefundOnRejectionOrCancellation.
    #[test]
    fn refund_on_refund_requested_emits_nothing() {
        let d = on_refund_requested(&RefundRequested {
            order_id: order_id(),
            restaurant_id: restaurant_id(),
            customer_id: None,
            reason: Some("Late delivery".into()),
        });
        assert!(d.appends().is_empty());
        assert!(matches!(d, Decision::Skip(_)));
    }

    /// tests.yaml#/TestRefundSettledFactRecorded — rules.yaml#/RefundSettledFactRecorded: the inbound
    /// settled fact is already in the log; the saga appends nothing further.
    #[test]
    fn settled_refund_fact_needs_no_further_reaction() {
        let d = on_payment_refunded(&PaymentRefunded {
            refund_id: RefundId("re_1".into()),
            payment_intent_id: PaymentIntentId("pi_123".into()),
            order_id: order_id(),
            restaurant_id: restaurant_id(),
            amount: Money {
                amount_cents: MoneyCents(1960),
                currency: CurrencyCode("EUR".into()),
            },
            reason: None,
        });
        assert_eq!(d, Decision::Nothing);
    }
}
