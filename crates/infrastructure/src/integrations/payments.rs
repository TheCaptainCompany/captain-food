//! Payment seam stand-in (Stripe OUTBOUND side). The REAL adapter — creating PaymentIntents with our
//! `restaurantId`/`orderId` metadata and requesting refunds — belongs to the Stripe integration
//! workstream (`crates/adapters/stripe` owns the INBOUND webhook ACL already; the outbound client
//! lands there too). Until it does, the composition root injects this FAIL-CLOSED stand-in, mirroring
//! the Google/Supabase seams: a checkout is DECLINED (never silently "paid") and a refund approval is
//! DECLINED by the trait's provided fail-closed `request_refund` default (never silently "refunded").
//!
//! The former `UnavailableCheckoutSnapshotSource` stand-in is retired with its port: the capture leg
//! now reads the frozen checkout back from the `Payment-<intentId>` stream (ADR-20260719-193500).

use application::ports::{CreatedPaymentIntent, PaymentGateway};
use async_trait::async_trait;
use domain::generated::entities::Money;
use domain::shared::errors::DomainError;

/// Fail-closed [`PaymentGateway`]: every create-intent is refused with the canonical
/// `errors.yaml#/PaymentDeclined` rejection (the port's contract for a synchronous decline), so
/// `placeOrder` is wired end-to-end but cannot fabricate a payment until the real Stripe adapter
/// lands. `request_refund` deliberately stays on the trait's provided default — the SAME fail-closed
/// rejection style — so an approved refund can never be silently pretended either.
pub struct FailClosedPaymentGateway;

#[async_trait]
impl PaymentGateway for FailClosedPaymentGateway {
    async fn create_payment_intent(
        &self,
        amount: &Money,
        _payment_method_id: &str,
    ) -> Result<CreatedPaymentIntent, DomainError> {
        Err(DomainError::Invariant(format!(
            "PaymentDeclined: payment gateway not configured (fail-closed stand-in; amount {} {}) — \
             the real create-intent adapter is the Stripe integration workstream's",
            amount.amount_cents.0, amount.currency.0
        )))
    }
}
