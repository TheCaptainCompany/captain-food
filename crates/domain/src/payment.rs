//! Payment aggregate — the PURE write-side state fold (ADR-0035). Born by `PaymentIntentCreated`
//! (emitted by PlaceOrderProcess, carrying the frozen checkout snapshot — ADR-20260719-014434), then
//! driven to a terminal state by the inbound Stripe facts (`PaymentAuthorized`/`PaymentCaptured`/
//! `PaymentReleased`/`PaymentFailed`/`PaymentRefunded`; authorize-then-capture,
//! ADR-20260808-195315 §1.2), the capture failure PaymentSettlementProcess delivers
//! (`PaymentCaptureFailed` — records without moving the status) and the refund decisions
//! RefundProcess delivers (`RefundApproved`/`RefundDenied`).
//! Everything on this inbox is a FACT to record, never a command to reject
//! (`specs/actors.yaml#/Payment`): the only decision left to the caller is idempotency —
//! [`already_records`] answers "is this re-delivered fact already reflected?", so appending it again
//! becomes a no-op. No I/O, no serialization logic (dependency rule).
//!
//! NOT wired through `impl_aggregate!`: the identity is the Stripe [`PaymentIntentId`] — a String
//! provider reference, not a Copy uuid newtype — so the category/stream helpers live here as free
//! functions with the same `"<Category>-<id>"` shape.

use crate::generated::events::DomainEvent;
use crate::generated::entities::Money;
pub use crate::generated::lifecycles::payment as lifecycle;
use crate::generated::scalars::{OrderId, PaymentIntentId, PaymentStatus, RefundId, RestaurantId};

/// The stream-category prefix; the stream is `"Payment-<paymentIntentId>"`.
pub const CATEGORY: &str = "Payment";

/// This aggregate's event-stream name for `id` — same shape as the `Aggregate` trait streams.
pub fn stream(id: &PaymentIntentId) -> String {
    format!("{CATEGORY}-{}", id.0)
}

/// The refund decision RefundProcess recorded on this payment (restaurant or admin — ADR mapping in
/// `specs/actors.yaml`). Recorded BEFORE Stripe settles: `Approved` precedes `PaymentRefunded`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundDecision {
    Approved,
    Denied,
}

/// What the Payment fact-recording path needs to know about the aggregate — enough for
/// [`already_records`] and for RefundProcess to correlate back to the order. `None` (from [`fold`])
/// means no `PaymentIntentCreated` yet on this stream.
#[derive(Debug, Clone, PartialEq)]
pub struct PaymentState {
    /// The Stripe PaymentIntent id — the stream key.
    pub payment_intent_id: PaymentIntentId,
    /// The order this payment settles, frozen in the checkout snapshot at intent creation.
    pub order_id: OrderId,
    pub restaurant_id: RestaurantId,
    /// The intent amount (== checkout.totalAmount == checkout.breakdown.total).
    pub amount: Money,
    /// PENDING → AUTHORIZED | FAILED, AUTHORIZED → CAPTURED | RELEASED, CAPTURED → REFUNDED —
    /// folded from the Stripe facts (specs/payments/actors.yaml#/Payment/lifecycle).
    pub status: PaymentStatus,
    /// Whether PaymentSettlementProcess recorded a capture failure (`PaymentCaptureFailed`) — the
    /// status stays AUTHORIZED (funds still held); a retried capture or the hold's expiry decides.
    pub capture_failed: bool,
    /// Whether RefundProcess opened a refund for decision on this payment (`RefundOpened` recorded —
    /// the refund-queue fact View_PendingRefunds folds; rules.yaml#/PendingRefundVisibleUntilDecided).
    pub refund_opened: bool,
    /// The recorded refund decision, if RefundProcess delivered one.
    pub refund_decision: Option<RefundDecision>,
    /// The Stripe Refund id once `PaymentRefunded` settled.
    pub refund_id: Option<RefundId>,
}

/// Fold a Payment stream (events in version order) into its current state. `None` ⇔ the stream has
/// no `PaymentIntentCreated` yet, i.e. the payment does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<PaymentState> {
    events.iter().fold(None, apply)
}

/// Apply one event — a pure transition, total over the whole event union. Duplicate facts are
/// harmless: every transition is an idempotent assignment and a re-delivered birth never resets an
/// existing state. The status moves ONLY through the generated lifecycle table
/// (`specs/actors.yaml#/Payment/lifecycle`, ADR-20260720-004419): [`lifecycle::initial`] births it
/// PENDING, [`lifecycle::target`] applies the recorded Stripe facts; the hand-written part is
/// payload extraction and the refund fields.
fn apply(state: Option<PaymentState>, event: &DomainEvent) -> Option<PaymentState> {
    if let Some(status) = lifecycle::initial(event) {
        if state.is_some() {
            return state; // duplicate birth — never reset an already-folded payment
        }
        if let DomainEvent::PaymentIntentCreated(e) = event {
            return Some(PaymentState {
                payment_intent_id: e.payment_intent_id.clone(),
                order_id: e.checkout.order_id,
                restaurant_id: e.restaurant_id,
                amount: e.amount.clone(),
                status,
                capture_failed: false,
                refund_opened: false,
                refund_decision: None,
                refund_id: None,
            });
        }
    }
    let mut s = state?;
    // The recorded fact wins at fold time: `target` maps a lifecycle event to its state regardless
    // of the current one (everything on this inbox is a fact to record, never to reject).
    if let Some(next) = lifecycle::target(event) {
        s.status = next;
    }
    match event {
        DomainEvent::PaymentCaptureFailed(_) => s.capture_failed = true,
        DomainEvent::PaymentRefunded(e) => s.refund_id = Some(e.refund_id.clone()),
        DomainEvent::RefundOpened(_) => s.refund_opened = true,
        DomainEvent::RefundApproved(_) => s.refund_decision = Some(RefundDecision::Approved),
        DomainEvent::RefundDenied(_) => s.refund_decision = Some(RefundDecision::Denied),
        _ => {}
    }
    Some(s)
}

/// Whether `event` is already reflected in `state` — the "record idempotently" decision: a webhook or
/// saga re-delivery whose fact is already folded should append nothing. Events outside the Payment
/// inbox are never "already recorded" (they should not be routed here at all).
pub fn already_records(state: &PaymentState, event: &DomainEvent) -> bool {
    match event {
        // The stream exists at all ⇔ the birth is recorded.
        DomainEvent::PaymentIntentCreated(_) => true,
        // "Already reflected" for the authorization is `status != PENDING`: appending a (late or
        // re-delivered) authorization onto a CAPTURED/RELEASED/FAILED payment would fold the status
        // BACKWARDS (the recorded fact wins at fold time), so any post-PENDING state absorbs it.
        DomainEvent::PaymentAuthorized(_) => state.status != PaymentStatus::PENDING,
        DomainEvent::PaymentCaptured(_) => state.status == PaymentStatus::CAPTURED,
        DomainEvent::PaymentCaptureFailed(_) => state.capture_failed,
        DomainEvent::PaymentReleased(_) => state.status == PaymentStatus::RELEASED,
        DomainEvent::PaymentFailed(_) => state.status == PaymentStatus::FAILED,
        // Keyed by the Stripe Refund id, not just the status — a different refund is a new fact.
        DomainEvent::PaymentRefunded(e) => state.refund_id.as_ref() == Some(&e.refund_id),
        DomainEvent::RefundOpened(_) => state.refund_opened,
        DomainEvent::RefundApproved(_) => state.refund_decision == Some(RefundDecision::Approved),
        DomainEvent::RefundDenied(_) => state.refund_decision == Some(RefundDecision::Denied),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::entities::{
        CheckoutSnapshot, CustomerContact, PaymentBreakdown,
    };
    use crate::generated::events::{
        PaymentAuthorized, PaymentCaptureFailed, PaymentCaptured, PaymentFailed,
        PaymentIntentCreated, PaymentRefunded, PaymentReleased, RefundApproved, RefundDenied,
        RefundOpened,
    };
    use crate::generated::scalars::{
        CartId, CurrencyCode, CustomerDisplayName, CustomerId, MoneyCents, PhoneNumber, ServiceType,
    };

    fn pi() -> PaymentIntentId {
        PaymentIntentId("pi_test_1".into())
    }
    fn money(cents: i64) -> Money {
        Money { amount_cents: MoneyCents(cents), currency: CurrencyCode("EUR".into()) }
    }
    fn order_id() -> OrderId {
        OrderId(uuid::Uuid::nil())
    }
    fn restaurant_id() -> RestaurantId {
        RestaurantId(uuid::Uuid::nil())
    }

    fn intent_created() -> DomainEvent {
        let z = money(0);
        DomainEvent::PaymentIntentCreated(PaymentIntentCreated {
            payment_intent_id: pi(),
            restaurant_id: restaurant_id(),
            customer_id: CustomerId(uuid::Uuid::nil()),
            amount: money(1000),
            checkout: CheckoutSnapshot {
                order_id: order_id(),
                cart_id: CartId(uuid::Uuid::nil()),
                restaurant_id: restaurant_id(),
                customer_id: CustomerId(uuid::Uuid::nil()),
                mode: None,
                r#ref: None,
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
            },
        })
    }
    fn authorized() -> DomainEvent {
        DomainEvent::PaymentAuthorized(PaymentAuthorized {
            payment_intent_id: pi(),
            order_id: Some(order_id()),
            restaurant_id: restaurant_id(),
            amount: money(1000),
        })
    }
    fn captured() -> DomainEvent {
        DomainEvent::PaymentCaptured(PaymentCaptured {
            payment_intent_id: pi(),
            order_id: Some(order_id()),
            restaurant_id: restaurant_id(),
            amount: money(1000),
        })
    }
    fn capture_failed() -> DomainEvent {
        DomainEvent::PaymentCaptureFailed(PaymentCaptureFailed {
            payment_intent_id: pi(),
            order_id: order_id(),
            restaurant_id: restaurant_id(),
            reason: crate::generated::scalars::CaptureFailureReason::CARD_DECLINED,
            detail: Some("Your card was declined.".into()),
        })
    }
    fn released() -> DomainEvent {
        DomainEvent::PaymentReleased(PaymentReleased {
            payment_intent_id: pi(),
            order_id: Some(order_id()),
            restaurant_id: restaurant_id(),
            reason: Some("requested_by_customer".into()),
        })
    }
    fn failed() -> DomainEvent {
        DomainEvent::PaymentFailed(PaymentFailed {
            payment_intent_id: pi(),
            restaurant_id: restaurant_id(),
            reason: "card_declined".into(),
        })
    }
    fn refunded(refund_id: &str) -> DomainEvent {
        DomainEvent::PaymentRefunded(PaymentRefunded {
            refund_id: RefundId(refund_id.into()),
            payment_intent_id: pi(),
            order_id: order_id(),
            restaurant_id: restaurant_id(),
            amount: money(1000),
            reason: None,
        })
    }
    fn refund_opened() -> DomainEvent {
        DomainEvent::RefundOpened(RefundOpened {
            payment_intent_id: pi(),
            order_id: order_id(),
            restaurant_id: restaurant_id(),
            amount: money(1000),
            reason: Some("Out of ingredients".into()),
        })
    }
    fn refund_approved() -> DomainEvent {
        DomainEvent::RefundApproved(RefundApproved {
            payment_intent_id: pi(),
            order_id: order_id(),
            amount: money(1000),
            reason: None,
        })
    }
    fn refund_denied() -> DomainEvent {
        DomainEvent::RefundDenied(RefundDenied {
            payment_intent_id: pi(),
            order_id: order_id(),
            reason: "too late".into(),
        })
    }

    #[test]
    fn no_intent_created_means_no_payment() {
        assert_eq!(fold(&[]), None);
        assert_eq!(fold(&[captured()]), None); // a fact without a birth folds to nothing
    }

    #[test]
    fn intent_created_births_a_pending_payment_with_the_snapshot_order() {
        let s = fold(&[intent_created()]).unwrap();
        assert_eq!(s.payment_intent_id, pi());
        assert_eq!(s.order_id, order_id()); // from checkout.orderId, not a top-level field
        assert_eq!(s.status, PaymentStatus::PENDING);
        assert_eq!(s.amount, money(1000));
        assert_eq!(s.refund_decision, None);
        assert_eq!(s.refund_id, None);
    }

    /// tests.yaml#/TestPaymentAuthorizedRecorded (fold half) — the authorize-then-capture
    /// lifecycle: PENDING → AUTHORIZED → CAPTURED, with PENDING → CAPTURED kept legal for the
    /// recorded at-table advance-capture arm (ADR-20260808-195315 §1.2).
    #[test]
    fn authorization_capture_release_and_failure_fold_through_the_lifecycle() {
        assert_eq!(fold(&[intent_created(), authorized()]).unwrap().status, PaymentStatus::AUTHORIZED);
        assert_eq!(
            fold(&[intent_created(), authorized(), captured()]).unwrap().status,
            PaymentStatus::CAPTURED
        );
        assert_eq!(fold(&[intent_created(), captured()]).unwrap().status, PaymentStatus::CAPTURED);
        assert_eq!(
            fold(&[intent_created(), authorized(), released()]).unwrap().status,
            PaymentStatus::RELEASED
        );
        assert_eq!(fold(&[intent_created(), failed()]).unwrap().status, PaymentStatus::FAILED);
    }

    /// rules.yaml#/CaptureFailureIsRecordedAndPaged (fold half): the failure is recorded WITHOUT
    /// moving the status — the money is still only held; a retried capture still settles.
    #[test]
    fn capture_failure_is_recorded_without_moving_the_status() {
        let s = fold(&[intent_created(), authorized(), capture_failed()]).unwrap();
        assert_eq!(s.status, PaymentStatus::AUTHORIZED);
        assert!(s.capture_failed);
        assert!(already_records(&s, &capture_failed())); // re-delivered failure appends nothing
        // A retried capture still reaches CAPTURED after a recorded failure.
        let s = fold(&[intent_created(), authorized(), capture_failed(), captured()]).unwrap();
        assert_eq!(s.status, PaymentStatus::CAPTURED);
    }

    /// The already-records guard that keeps a late authorization from folding a settled payment
    /// BACKWARDS: any post-PENDING state absorbs a (re-)delivered PaymentAuthorized.
    #[test]
    fn late_authorization_never_regresses_a_settled_payment() {
        let s = fold(&[intent_created(), authorized(), captured()]).unwrap();
        assert!(already_records(&s, &authorized()));
        let s = fold(&[intent_created(), authorized(), released()]).unwrap();
        assert!(already_records(&s, &authorized()));
        assert!(already_records(&s, &released()));
        let fresh = fold(&[intent_created()]).unwrap();
        assert!(!already_records(&fresh, &authorized()));
        assert!(!already_records(&fresh, &released()));
    }

    #[test]
    fn refund_lifecycle_records_the_decision_then_the_settled_refund() {
        // Approval alone is a decision, not the settlement: status stays CAPTURED.
        let s = fold(&[intent_created(), captured(), refund_approved()]).unwrap();
        assert_eq!(s.status, PaymentStatus::CAPTURED);
        assert_eq!(s.refund_decision, Some(RefundDecision::Approved));
        // Stripe settles: REFUNDED + the refund id.
        let s = fold(&[intent_created(), captured(), refund_approved(), refunded("re_1")]).unwrap();
        assert_eq!(s.status, PaymentStatus::REFUNDED);
        assert_eq!(s.refund_id, Some(RefundId("re_1".into())));
    }

    /// tests.yaml#/TestPendingRefundVisibleUntilDecided — rules.yaml#/PendingRefundVisibleUntilDecided:
    /// the opened refund is recorded on the Payment (idempotent), so the refund queue
    /// (View_PendingRefunds) folds it as REQUESTED until a decision resolves it.
    #[test]
    fn opened_refund_is_recorded_idempotently_until_decided() {
        let s = fold(&[intent_created(), captured(), refund_opened()]).unwrap();
        assert!(s.refund_opened);
        assert_eq!(s.status, PaymentStatus::CAPTURED); // opening decides nothing
        assert_eq!(s.refund_decision, None);
        assert!(already_records(&s, &refund_opened())); // re-delivered opening appends nothing
        // Undecided before the fact, so a fresh capture does not "already record" an opening.
        let fresh = fold(&[intent_created(), captured()]).unwrap();
        assert!(!already_records(&fresh, &refund_opened()));
        // The decision resolves the request without erasing the opened fact.
        let s = fold(&[intent_created(), captured(), refund_opened(), refund_denied()]).unwrap();
        assert!(s.refund_opened);
        assert_eq!(s.refund_decision, Some(RefundDecision::Denied));
    }

    #[test]
    fn refund_denial_is_recorded_without_touching_the_status() {
        let s = fold(&[intent_created(), captured(), refund_denied()]).unwrap();
        assert_eq!(s.status, PaymentStatus::CAPTURED);
        assert_eq!(s.refund_decision, Some(RefundDecision::Denied));
    }

    #[test]
    fn duplicate_facts_refold_harmlessly() {
        let once = fold(&[intent_created(), captured(), refunded("re_1")]);
        let twice = fold(&[
            intent_created(),
            captured(),
            captured(),
            intent_created(), // re-delivered birth must not reset the state
            refunded("re_1"),
            refunded("re_1"),
        ]);
        assert_eq!(once, twice);
    }

    #[test]
    fn already_records_detects_re_delivered_facts() {
        let s = fold(&[intent_created(), captured()]).unwrap();
        assert!(already_records(&s, &intent_created()));
        assert!(already_records(&s, &captured()));
        assert!(!already_records(&s, &failed()));
        assert!(!already_records(&s, &refunded("re_1")));
        let s = fold(&[intent_created(), captured(), refund_approved(), refunded("re_1")]).unwrap();
        assert!(already_records(&s, &refund_approved()));
        assert!(!already_records(&s, &refund_denied()));
        assert!(already_records(&s, &refunded("re_1")));
        assert!(!already_records(&s, &refunded("re_2"))); // a DIFFERENT refund is a new fact
    }

    #[test]
    fn stream_name_matches_the_aggregate_format() {
        assert_eq!(stream(&pi()), "Payment-pi_test_1");
    }
}
