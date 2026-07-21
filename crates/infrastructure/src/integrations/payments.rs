//! Payment seam stand-in (Stripe OUTBOUND side). The REAL adapter — creating PaymentIntents with our
//! `restaurantId`/`orderId` metadata and requesting refunds — belongs to the Stripe integration
//! workstream (`crates/adapters/stripe` owns the INBOUND webhook ACL already; the outbound client
//! lands there too). Until it does, the composition root injects this FAIL-CLOSED stand-in, mirroring
//! the Google/Supabase seams: a checkout is DECLINED (never silently "paid") and a refund approval is
//! DECLINED by the trait's provided fail-closed `request_refund` default (never silently "refunded").
//!
//! The former `UnavailableCheckoutSnapshotSource` stand-in is retired with its port: the capture leg
//! now reads the frozen checkout back from the `Payment-<intentId>` stream (ADR-20260719-193500).

use application::generated::services::{
    PaymentRefundInput, PaymentRequestInput, PaymentRequestOutput, PaymentService, ServiceCallMeta,
};
use async_trait::async_trait;
use domain::shared::errors::DomainError;

/// Fail-closed [`PaymentService`]: every create-intent is refused with the canonical
/// `errors.yaml#/PaymentDeclined` rejection (the port's contract for a synchronous decline), so
/// `placeOrder` is wired end-to-end but cannot fabricate a payment until the real Stripe adapter
/// lands, and `refund` DECLINES the same way so an approved refund can never be silently pretended.
pub struct FailClosedPaymentGateway;

#[async_trait]
impl PaymentService for FailClosedPaymentGateway {
    async fn request(
        &self,
        input: PaymentRequestInput,
        _meta: &ServiceCallMeta,
    ) -> Result<PaymentRequestOutput, DomainError> {
        Err(DomainError::Invariant(format!(
            "PaymentDeclined: payment gateway not configured (fail-closed stand-in; amount {} {}) — \
             set STRIPE_SECRET_KEY to enable the real Stripe adapter",
            input.amount.amount_cents.0, input.amount.currency.0
        )))
    }

    async fn refund(
        &self,
        input: PaymentRefundInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        Err(DomainError::Invariant(format!(
            "PaymentDeclined: refund gateway not configured (fail-closed stand-in; intent {}, \
             amount {} {}) — the real refund adapter is the Stripe integration workstream's",
            input.payment_intent_id.0, input.amount.amount_cents.0, input.amount.currency.0
        )))
    }
}
