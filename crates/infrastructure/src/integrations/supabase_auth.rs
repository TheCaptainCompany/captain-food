//! Supabase Auth seam adapter (ADR-0015). The generated `identity` service port
//! (`application::generated::services::IdentityService`, services.yaml — issue #50) IS the ACL
//! boundary for the wrapped auth provider (passwordless phone-OTP + email magic-link). Two impls:
//!
//! - [`SupabaseIdentityService`] (#117) — the REAL adapter over the Supabase Auth REST API
//!   (`/auth/v1/otp|verify|token`), env-gated on `SUPABASE_URL` + `SUPABASE_PUBLISHABLE_KEY`;
//!   phone-OTP SMS DELIVERY still needs the OVHcloud Send-SMS hook (#118).
//! - [`FailClosedIdentityService`] — the deliberate stand-in when unconfigured: sends FAIL with a
//!   clear "not configured" error (never pretend a code was delivered), verifies FAIL CLOSED with
//!   the canonical typed rejections (`InvalidVerificationCode`/`InvalidVerificationToken`) — no
//!   identity is ever silently accepted.
//!
//! The composition root picks the real adapter when configured, else the stand-in (Stripe pattern).

use application::commands::canonical_phone;
use application::generated::services::{
    IdentitySendEmailMagicLinkInput, IdentitySendPhoneOtpInput, IdentityService,
    IdentityVerifyEmailTokenInput, IdentityVerifyEmailTokenOutput, IdentityVerifyPhoneOtpInput,
    IdentityRefreshSessionInput, IdentityRefreshSessionOutput, IdentityVerifyPhoneOtpOutput,
    ServiceCallMeta,
};
use async_trait::async_trait;
use domain::generated::scalars::{EmailAddress, ExternalReference};
use domain::shared::errors::DomainError;
use serde_json::{json, Value};

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

// ─── The real Supabase Auth adapter (#117, PROP-20260724-225804) ────────────────────────────────

/// The `IdentityService` implemented against the Supabase Auth REST API (`/auth/v1/*`) — the ACL
/// that keeps `domain`/`application` free of provider detail (ADR-0015). OTP verify/send are
/// anon-key operations (the `apikey` header = `SUPABASE_PUBLISHABLE_KEY`); the verify responses
/// carry the provider session (`access_token`/`refresh_token`/`expires_in`) the #112 handler parks.
///
/// Project-agnostic: reads `SUPABASE_URL`, so WHICH Supabase project auth resolves against is pure
/// config (the ADR-20260722-174500 repoint to `captain-identity` is an env change, no code).
pub struct SupabaseIdentityService {
    base_url: String,
    apikey: String,
    http: reqwest::Client,
}

impl SupabaseIdentityService {
    /// Build from env: `SUPABASE_URL` + `SUPABASE_PUBLISHABLE_KEY` (the anon key OTP flows use).
    /// `None` when either is unset — the composition root then falls back to the fail-closed stub
    /// (auth stays anonymous-only, never a half-configured surface — the Stripe env-gate pattern).
    ///
    /// Thin env-reading shell over [`Self::from_lookup`] — the `server/src/lib.rs`
    /// `env_flag`/`parse_flag` pattern (#388): the gating logic is tested against an injected
    /// lookup, so no test ever mutates process env (concurrent env mutation vs
    /// `getenv` under the parallel libtest harness is glibc UB — the intermittent SIGSEGV class).
    pub fn from_env() -> Option<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// The pure gating core of [`Self::from_env`]: both keys required (absent or empty ⇒ `None`),
    /// trailing slash trimmed, with the environment injected as a lookup function.
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let base_url = lookup("SUPABASE_URL").filter(|s| !s.is_empty())?;
        let apikey = lookup("SUPABASE_PUBLISHABLE_KEY").filter(|s| !s.is_empty())?;
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            apikey,
            http: reqwest::Client::new(),
        })
    }

    /// POST a JSON body to `/auth/v1/<path>` with the anon apikey; return the parsed JSON on 2xx,
    /// or a mapped [`DomainError`] (typed rejection on 4xx, `Repository` on transport/5xx).
    async fn post(&self, path: &str, body: Value, verify_ctx: Option<Value>) -> Result<Value, DomainError> {
        let url = format!("{}/auth/v1/{path}", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("apikey", &self.apikey)
            .header("Authorization", format!("Bearer {}", self.apikey))
            .json(&body)
            .send()
            .await
            .map_err(|e| DomainError::Repository(format!("supabase auth transport: {e}")))?;
        let status = resp.status();
        let json: Value = resp.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(json);
        }
        // A 4xx on a VERIFY is an anticipated rejection: expired vs invalid, from the error text.
        if matches!(status.as_u16(), 400 | 401 | 403 | 422) {
            if let Some(ctx) = verify_ctx {
                let code = classify_verify_error(&json, ctx.get("email").is_some());
                return Err(DomainError::rejected(code, ctx));
            }
        }
        Err(DomainError::Repository(format!("supabase auth {}: {}", status.as_u16(), json)))
    }
}

/// The canonical errors.yaml code for a Supabase verify 4xx: `expired` in the error text →
/// `VerificationCodeExpired`, else the invalid-token/code for the channel. Pure (unit-tested).
fn classify_verify_error(body: &Value, is_email: bool) -> &'static str {
    let msg = body
        .get("error_description")
        .or_else(|| body.get("msg"))
        .or_else(|| body.get("error"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if msg.contains("expired") {
        "VerificationCodeExpired"
    } else if is_email {
        "InvalidVerificationToken"
    } else {
        "InvalidVerificationCode"
    }
}

/// The Supabase `user.id` proving the identity — the domain `authRef`.
fn auth_ref_of(v: &Value) -> Result<ExternalReference, DomainError> {
    v.get("user")
        .and_then(|u| u.get("id"))
        .and_then(Value::as_str)
        .map(|s| ExternalReference(s.to_string()))
        .ok_or_else(|| DomainError::Repository("supabase verify: response has no user.id".into()))
}

fn str_field(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

#[async_trait]
impl IdentityService for SupabaseIdentityService {
    async fn send_phone_otp(
        &self,
        input: IdentitySendPhoneOtpInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        let phone = canonical_phone(&input.dialing_code, &input.national_number);
        self.post("otp", json!({ "phone": phone }), None).await.map(|_| ())
    }

    async fn verify_phone_otp(
        &self,
        input: IdentityVerifyPhoneOtpInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityVerifyPhoneOtpOutput, DomainError> {
        let phone = canonical_phone(&input.dialing_code, &input.national_number);
        let ctx = json!({ "phone": phone });
        let v = self
            .post(
                "verify",
                json!({ "type": "sms", "phone": phone, "token": input.code.0 }),
                Some(ctx),
            )
            .await?;
        Ok(IdentityVerifyPhoneOtpOutput {
            auth_ref: auth_ref_of(&v)?,
            access_token: str_field(&v, "access_token"),
            refresh_token: str_field(&v, "refresh_token"),
            expires_in: v.get("expires_in").and_then(Value::as_i64),
        })
    }

    async fn refresh_session(
        &self,
        input: IdentityRefreshSessionInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityRefreshSessionOutput, DomainError> {
        let v = self
            .post(
                "token?grant_type=refresh_token",
                json!({ "refresh_token": input.refresh_token }),
                None,
            )
            .await?;
        Ok(IdentityRefreshSessionOutput {
            access_token: str_field(&v, "access_token")
                .ok_or_else(|| DomainError::Repository("supabase refresh: no access_token".into()))?,
            refresh_token: str_field(&v, "refresh_token"),
            expires_in: v.get("expires_in").and_then(Value::as_i64),
        })
    }

    async fn send_email_magic_link(
        &self,
        input: IdentitySendEmailMagicLinkInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        self.post("otp", json!({ "email": input.email.0 }), None).await.map(|_| ())
    }

    async fn verify_email_token(
        &self,
        input: IdentityVerifyEmailTokenInput,
        _meta: &ServiceCallMeta,
    ) -> Result<IdentityVerifyEmailTokenOutput, DomainError> {
        // Magic-link tokens verify via the token_hash flow; the proven email is the response's.
        let ctx = json!({ "email": true });
        let v = self
            .post(
                "verify",
                json!({ "type": "email", "token_hash": input.token.0 }),
                Some(ctx),
            )
            .await?;
        let email = v
            .get("user")
            .and_then(|u| u.get("email"))
            .and_then(Value::as_str)
            .ok_or_else(|| DomainError::Repository("supabase verify email: no user.email".into()))?;
        Ok(IdentityVerifyEmailTokenOutput {
            auth_ref: auth_ref_of(&v)?,
            email: EmailAddress(email.to_string()),
            access_token: str_field(&v, "access_token"),
            refresh_token: str_field(&v, "refresh_token"),
            expires_in: v.get("expires_in").and_then(Value::as_i64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pure core is exercised with an injected lookup — NEVER by mutating process env (#388:
    /// mutating process env from parallel lib tests is the glibc-UB SIGSEGV class; clippy
    /// `disallowed-methods` now gates it).
    #[test]
    fn from_env_gates_on_both_url_and_key() {
        let from = |pairs: &[(&str, &str)]| {
            let map: std::collections::HashMap<String, String> =
                pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            SupabaseIdentityService::from_lookup(move |k| map.get(k).cloned())
        };
        // Neither set → None; the composition root then falls back to fail-closed.
        assert!(from(&[]).is_none());
        assert!(
            from(&[("SUPABASE_URL", "https://proj.supabase.co/")]).is_none(),
            "URL alone is not enough"
        );
        let svc = from(&[
            ("SUPABASE_URL", "https://proj.supabase.co/"),
            ("SUPABASE_PUBLISHABLE_KEY", "anon-key"),
        ])
        .expect("both set");
        // Trailing slash trimmed so `{base}/auth/v1/...` has no double slash.
        assert_eq!(svc.base_url, "https://proj.supabase.co");
        // An empty value is UNSET (fail-closed), same as absent.
        assert!(
            from(&[("SUPABASE_URL", "https://proj.supabase.co/"), ("SUPABASE_PUBLISHABLE_KEY", "")])
                .is_none(),
            "empty key is unset"
        );
    }

    #[test]
    fn verify_error_classification_is_expired_vs_invalid_by_channel() {
        let expired = json!({ "error_description": "Token has expired or is invalid" });
        assert_eq!(classify_verify_error(&expired, false), "VerificationCodeExpired");
        assert_eq!(classify_verify_error(&expired, true), "VerificationCodeExpired");
        let bad = json!({ "msg": "Invalid token" });
        assert_eq!(classify_verify_error(&bad, false), "InvalidVerificationCode");
        assert_eq!(classify_verify_error(&bad, true), "InvalidVerificationToken");
        // No error text → still a channel-appropriate invalid, never a panic.
        assert_eq!(classify_verify_error(&json!({}), false), "InvalidVerificationCode");
    }

    #[test]
    fn parses_auth_ref_and_session_from_a_verify_response() {
        let resp = json!({
            "access_token": "jwt.abc", "refresh_token": "refresh.xyz", "expires_in": 3600,
            "user": { "id": "user-uuid-1", "email": "jo@example.com" }
        });
        assert_eq!(auth_ref_of(&resp).unwrap().0, "user-uuid-1");
        assert_eq!(str_field(&resp, "access_token").as_deref(), Some("jwt.abc"));
        assert_eq!(resp.get("expires_in").and_then(Value::as_i64), Some(3600));
        // A response missing user.id is a technical error (not a silent empty authRef).
        assert!(auth_ref_of(&json!({ "access_token": "x" })).is_err());
    }
}
