//! Supabase Auth seam adapter (ADR-0015). The generated `identity` service port
//! (`application::generated::services::IdentityService`, services.yaml — issue #50) IS the ACL
//! boundary for the wrapped auth provider (passwordless phone-OTP + email magic-link); the REAL
//! `supabase-acl` adapter — Supabase HTTP/SDK calls, Twilio SMS delivery, token semantics — is
//! TODO(integration). Until it lands the composition root injects this deliberate stand-in, exactly
//! as the port contract documents (never silently accept):
//!
//! - send operations FAIL with a clear "not configured" error (never pretend an OTP/magic link was
//!   delivered — the caller would wait for a code that cannot arrive), and
//! - verify operations FAIL CLOSED with the canonical typed rejections (`InvalidVerificationCode` /
//!   `InvalidVerificationToken`), so no identity is ever silently accepted.

use application::commands::canonical_phone;
use application::generated::services::{
    IdentitySendEmailMagicLinkInput, IdentitySendPhoneOtpInput, IdentityService,
    IdentityVerifyEmailTokenInput, IdentityVerifyEmailTokenOutput, IdentityVerifyPhoneOtpInput,
    IdentityRefreshSessionInput, IdentityRefreshSessionOutput, IdentityVerifyPhoneOtpOutput,
    ServiceCallMeta,
};
use async_trait::async_trait;
use domain::shared::errors::DomainError;
use serde_json::json;

/// Fail-closed [`IdentityService`]: sends error ("not configured"), verifications reject with the
/// canonical typed rejections — so the identity flows reject cleanly until the real Supabase ACL
/// adapter lands.
pub struct FailClosedIdentityService;

/// The uniform "not configured" send failure.
fn not_configured(what: &str) -> DomainError {
    DomainError::Repository(format!(
        "auth provider not configured — cannot send {what} (supabase-acl adapter pending, ADR-0015)"
    ))
}

#[async_trait]
impl IdentityService for FailClosedIdentityService {
    async fn send_phone_otp(
        &self,
        _input: IdentitySendPhoneOtpInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        // TODO(integration): Supabase Auth -> Twilio SMS OTP delivery.
        Err(not_configured("phone OTP"))
    }

    async fn verify_phone_otp(
        &self,
        input: IdentityVerifyPhoneOtpInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityVerifyPhoneOtpOutput, DomainError> {
        // TODO(integration): verify the OTP with Supabase Auth and return the provider's authRef.
        Err(DomainError::rejected(
            "InvalidVerificationCode",
            json!({ "phone": canonical_phone(&input.dialing_code, &input.national_number) }),
        ))
    }

    async fn refresh_session(
        &self,
        _input: IdentityRefreshSessionInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityRefreshSessionOutput, DomainError> {
        // TODO(#117): rotate the session with Supabase Auth (grant_type=refresh_token).
        Err(not_configured("session refresh"))
    }

    async fn send_email_magic_link(
        &self,
        _input: IdentitySendEmailMagicLinkInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        // TODO(integration): Supabase Auth magic-link email delivery.
        Err(not_configured("email magic link"))
    }

    async fn verify_email_token(
        &self,
        _input: IdentityVerifyEmailTokenInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityVerifyEmailTokenOutput, DomainError> {
        // TODO(integration): verify the magic-link token server-side with Supabase Auth.
        Err(DomainError::rejected("InvalidVerificationToken", json!({})))
    }
}
