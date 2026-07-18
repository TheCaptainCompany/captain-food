//! Payment seam stand-ins (Stripe OUTBOUND side). The REAL adapter — creating PaymentIntents with our
//! `restaurantId`/`orderId` metadata and freezing the checkout snapshot — belongs to the Stripe
//! integration workstream (`crates/adapters/stripe` owns the INBOUND webhook ACL already; the outbound
//! client lands there too). Until it does, the composition root injects these FAIL-CLOSED stand-ins,
//! mirroring the Google/Supabase seams: a checkout is DECLINED (never silently "paid") and a captured
//! payment without a resolvable checkout is SKIPPED by the saga (never guessed).

use application::ports::{
    CheckoutSnapshot, CheckoutSnapshotSource, CreatedPaymentIntent, PaymentGateway,
};
use async_trait::async_trait;
use domain::generated::entities::Money;
use domain::generated::scalars::PaymentIntentId;
use domain::shared::errors::DomainError;

/// Fail-closed [`PaymentGateway`]: every create-intent is refused with the canonical
/// `errors.yaml#/PaymentDeclined` rejection (the port's contract for a synchronous decline), so
/// `placeOrder` is wired end-to-end but cannot fabricate a payment until the real Stripe adapter lands.
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

/// Fail-closed [`CheckoutSnapshotSource`]: no checkout is resolvable, so PlaceOrderProcess SKIPS a
/// `PaymentCaptured` (logged) instead of guessing the `OrderPlaced` payload. The real source is the
/// Stripe adapter's PaymentIntent metadata / pending-checkout store, written at create-intent time.
pub struct UnavailableCheckoutSnapshotSource;

#[async_trait]
impl CheckoutSnapshotSource for UnavailableCheckoutSnapshotSource {
    async fn by_payment_intent(
        &self,
        _payment_intent_id: &PaymentIntentId,
    ) -> Result<Option<CheckoutSnapshot>, DomainError> {
        Ok(None)
    }
}
