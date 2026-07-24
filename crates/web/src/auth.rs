//! The OTP identity flow (#94, closing #17's identity item) — late identification at the
//! cart→checkout boundary (ADR-20260722-174500): an anonymous visitor verifies their phone in the
//! auth/OTP sheets and BECOMES a customer, carts binding to the session.
//!
//! The command payloads are the authority, not the sheet DSL's loose prop names: the phone travels
//! SPLIT (`dialingCode` + `nationalNumber` — what the country picker emits; the server composes
//! E.164), `VerifyPhone` additionally carries a client-minted `customerId` (used only when the
//! phone is new), the `sessionId` (business data here by spec: CartBindingProcess binds this
//! session's open carts onto the customer) and the OTP `code`. Both steps are ordinary two-step
//! writes through the persisted dispatcher — a rejected OTP is a normal [`ActionOutcome::Rejected`]
//! (`InvalidOtp`/`VerificationCodeExpired`), never an exception.
//!
//! On a SUCCEEDED verification the flow reads `me.profile` — the identity read is the proof, not
//! the acceptance.

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::actions::{ActionError, ActionOutcome, DispatchHandle};
use crate::generated::data_layer::{ActionKey, ResolverKey};
use crate::graphql::{execute_resolver, ResolverError, Transport};
use crate::pending::{dispatch_persisted, settle_with, PendingStore};
use crate::session::SessionId;

/// The split phone the country picker emits (`+33` / `612345678`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneParts {
    pub dialing_code: String,
    pub national_number: String,
}

/// Everything the verify step can end in.
#[derive(Debug)]
pub enum VerifyResult {
    /// Verified: the customer exists (created or resolved server-side) and `me` answered.
    SignedIn { profile: Value },
    /// The anticipated business rejection (wrong/expired code…) — normal UX flow: show the
    /// message, offer resend.
    Rejected { error_code: String, message: Option<String> },
    /// Technical failure after acceptance.
    Failed { message: Option<String> },
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error(transparent)]
    Action(#[from] ActionError),
    #[error(transparent)]
    Resolver(#[from] ResolverError),
}

/// Step 1 — `send_otp` (`RequestPhoneVerification`): ask for the SMS. Returns the acceptance
/// handle (the sheet flips to the OTP entry on acceptance; a rejection surfaces via settle).
pub async fn request_otp(
    transport: &dyn Transport,
    store: &dyn PendingStore,
    phone: &PhoneParts,
    locale: Option<&str>,
) -> Result<DispatchHandle, ActionError> {
    let mut input = Map::new();
    input.insert("dialingCode".into(), json!(phone.dialing_code));
    input.insert("nationalNumber".into(), json!(phone.national_number));
    if let Some(locale) = locale {
        input.insert("locale".into(), json!(locale));
    }
    dispatch_persisted(transport, store, ActionKey::SendOtp, input).await
}

/// Step 2 — `verify_otp` (`VerifyPhone`) then, on success, the `me` read. Mints the `customerId`
/// (spec: "used only when the phone is new; ignored for a returning phone") and carries the
/// session id so CartBindingProcess binds the anonymous cart (#12's payoff moment).
pub async fn verify_otp(
    transport: &dyn Transport,
    store: &dyn PendingStore,
    phone: &PhoneParts,
    code: &str,
    session: SessionId,
    max_attempts: u32,
    interval: std::time::Duration,
) -> Result<VerifyResult, AuthError> {
    let mut input = Map::new();
    input.insert("customerId".into(), json!(Uuid::now_v7()));
    input.insert("dialingCode".into(), json!(phone.dialing_code));
    input.insert("nationalNumber".into(), json!(phone.national_number));
    input.insert("code".into(), json!(code));
    input.insert("sessionId".into(), json!(session.as_uuid()));

    let handle = dispatch_persisted(transport, store, ActionKey::VerifyOtp, input).await?;
    match settle_with(transport, store, &handle, max_attempts, interval).await? {
        ActionOutcome::Succeeded { .. } => {
            // The proof is the read: `me` resolves the (new or returning) customer.
            let profile = execute_resolver(transport, ResolverKey::MeProfile, Map::new()).await?;
            Ok(VerifyResult::SignedIn { profile })
        }
        ActionOutcome::Rejected { error_code, message, .. } => {
            Ok(VerifyResult::Rejected { error_code, message })
        }
        ActionOutcome::Failed { message, .. } => Ok(VerifyResult::Failed { message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::test_support::FakeTransport;
    use crate::pending::MemoryPendingStore;
    use std::time::Duration;

    fn phone() -> PhoneParts {
        PhoneParts { dialing_code: "+33".into(), national_number: "612345678".into() }
    }

    fn acceptance(mutation: &str, status: &str) -> Value {
        json!({ mutation: {
            "messageId": "00000000-0000-7000-8000-000000000000",
            "correlationId": "00000000-0000-7000-8000-000000000000",
            "causeId": null, "sessionId": null, "traceId": null,
            "operationStatus": status, "duplicate": false,
        }})
    }

    fn operation(status: &str, error_code: Option<&str>) -> Value {
        json!({ "operationStatus": {
            "messageId": "00000000-0000-7000-8000-000000000000",
            "correlationId": "00000000-0000-7000-8000-000000000000",
            "status": status, "errorCode": error_code, "message": null,
            "occurredAt": "2026-07-24T12:00:00Z",
        }})
    }

    #[tokio::test]
    async fn the_full_otp_flow_signs_in_and_reads_me() {
        let store = MemoryPendingStore::default();
        let session = SessionId::mint();
        // request -> accepted + settled; verify -> accepted, SUCCEEDED, then the me read answers.
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("requestPhoneVerification", "PENDING")),
            Ok(operation("SUCCEEDED", None)),
            Ok(acceptance("verifyPhone", "PENDING")),
            Ok(operation("SUCCEEDED", None)),
            Ok(json!({ "me": { "customerId": "c-1", "phone": "+33612345678", "displayName": null,
                        "email": null, "emailVerified": false, "locale": null, "timezone": null } })),
        ]);

        let request = request_otp(&fake, &store, &phone(), Some("fr")).await.unwrap();
        crate::pending::settle_with(&fake, &store, &request, 5, Duration::ZERO).await.unwrap();
        // The RequestPhoneVerification payload is the COMMAND's shape (split phone), not the
        // sheet's loose `phone` prop.
        let sent = fake.call(0).1;
        assert_eq!(sent["input"]["dialingCode"], "+33");
        assert_eq!(sent["input"]["nationalNumber"], "612345678");
        assert_eq!(sent["input"]["locale"], "fr");

        let result =
            verify_otp(&fake, &store, &phone(), "123456", session, 5, Duration::ZERO).await.unwrap();
        let VerifyResult::SignedIn { profile } = result else { panic!("expected sign-in") };
        assert_eq!(profile["customerId"], "c-1");

        // VerifyPhone carried the minted customerId + THE session (the cart-binding contract).
        let verify = fake.call(2).1;
        assert_eq!(verify["input"]["code"], "123456");
        assert_eq!(verify["input"]["sessionId"], json!(session.as_uuid()));
        assert!(verify["input"]["customerId"].is_string());
        // Both intents settled: nothing left pending.
        assert!(store.load().is_empty());
    }

    #[tokio::test]
    async fn a_wrong_code_is_the_anticipated_rejection_not_an_error() {
        let store = MemoryPendingStore::default();
        let fake = FakeTransport::scripted(vec![
            Ok(acceptance("verifyPhone", "PENDING")),
            Ok(operation("REJECTED", Some("InvalidOtp"))),
        ]);
        let result = verify_otp(&fake, &store, &phone(), "000000", SessionId::mint(), 5, Duration::ZERO)
            .await
            .unwrap();
        match result {
            VerifyResult::Rejected { error_code, .. } => assert_eq!(error_code, "InvalidOtp"),
            other => panic!("expected the rejection, got {other:?}"),
        }
        // A rejection is TERMINAL — the record cleared; resend/re-enter is a fresh intent.
        assert!(store.load().is_empty());
        assert_eq!(fake.call_count(), 2, "no me read on a rejected verification");
    }
}
