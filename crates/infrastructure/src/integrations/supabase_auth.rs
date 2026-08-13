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
    IdentityRefreshSessionInput, IdentityRefreshSessionOutput, IdentityStampCustomerClaimInput,
    IdentityVerifyPhoneOtpOutput, ServiceCallMeta,
};
use async_trait::async_trait;
use domain::generated::scalars::{CustomerId, EmailAddress, ExternalReference};
use domain::shared::errors::DomainError;
use serde_json::{json, Value};
use tracing::Instrument as _;

/// In-lease HTTP timeout (#437, DBA concern): the claim stamp AND the session rotation run INSIDE
/// the VerifyPhone mailbox delivery, so their worst case (GET + PUT + refresh = 3 x 5s = 15s)
/// must stay WELL below the mailbox lease TTL (`MAILBOX_LEASE_SECONDS`, default 30s —
/// `actor_runtime::worker`), or a hung provider call would let the lane lease lapse mid-delivery
/// and stall the customer's whole lane (head-of-line). The user-facing OTP calls keep the client
/// default; only the in-lease calls (admin GET/PUT + the rotation POST) are bounded.
const ADMIN_HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

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

    async fn stamp_customer_claim(
        &self,
        _input: IdentityStampCustomerClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        // Fail CLOSED (services.yaml identity.stamp_customer_claim, #437): never pretend to have
        // stamped — the caller must then skip session rotation/parking entirely.
        Err(not_configured("customer claim stamp"))
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
    /// The Supabase SECRET (service-role) key — authorizes ONLY the admin `app_metadata` stamp
    /// (`stamp_customer_claim`, #437). `None` is a legal, boot-safe state (farley verdict): the
    /// anon OTP flows keep working and stamping fails CLOSED with `not_configured`.
    admin_key: Option<String>,
    http: reqwest::Client,
    /// The OTP send guards (#516) — used here for CHEAP SHEDDING only (a `peek`, never a claim).
    /// `None` leaves this adapter unguarded, which is safe ONLY because the authoritative guard is on
    /// the hook path where the euro is actually spent; see [`Self::with_send_guard`].
    send_guard: Option<std::sync::Arc<crate::sms_authorization::SmsSendAuthorizer>>,
}

/// The raw environment inputs of [`SupabaseIdentityService::from_env`], read verbatim
/// (empty-string filtering and slash-trimming are [`SupabaseIdentityService::from_parts`]'s job).
/// Exists so the gating logic is a pure function tests can drive without mutating process env
/// (#388).
#[derive(Default)]
struct SupabaseEnv {
    url: Option<String>,
    publishable_key: Option<String>,
    /// `SUPABASE_SECRET_KEY` — OPTIONAL: its absence must never fail construction (and so never
    /// boot); it only gates the admin claim stamp closed (#437).
    secret_key: Option<String>,
}

impl SupabaseIdentityService {
    /// Build from env: `SUPABASE_URL` + `SUPABASE_PUBLISHABLE_KEY` (the anon key OTP flows use).
    /// `None` when either is unset — the composition root then falls back to the fail-closed stub
    /// (auth stays anonymous-only, never a half-configured surface — the Stripe env-gate pattern).
    ///
    /// Thin env-reading shell over [`Self::from_parts`] — the `server/src/lib.rs`
    /// `env_flag`/`parse_flag` pattern (#388): the gating logic is tested against constructed
    /// parts, so no test ever mutates process env (concurrent env mutation vs `getenv` under the
    /// parallel libtest harness is glibc UB — the intermittent SIGSEGV class). The closure shape
    /// (`let var = |k: &str| …`) is deliberate: it is shape 3 of the env-inventory drift gate
    /// (`every_env_var_read_by_the_crates_is_declared`), which harvests the keys off `var("…")`.
    pub fn from_env() -> Option<Self> {
        let var = |k: &str| std::env::var(k).ok();
        Self::from_parts(SupabaseEnv {
            url: var("SUPABASE_URL"),
            publishable_key: var("SUPABASE_PUBLISHABLE_KEY"),
            secret_key: var("SUPABASE_SECRET_KEY"),
        })
    }

    /// The pure gating core of [`Self::from_env`]: url + publishable key required (absent or
    /// empty ⇒ `None`), trailing slash trimmed, no environment access. The secret key is
    /// OPTIONAL — empty is unset, and unset only fails the claim stamp closed, never construction.
    fn from_parts(parts: SupabaseEnv) -> Option<Self> {
        let base_url = parts.url.filter(|s| !s.is_empty())?;
        let apikey = parts.publishable_key.filter(|s| !s.is_empty())?;
        Some(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            apikey,
            admin_key: parts.secret_key.filter(|s| !s.is_empty()),
            http: reqwest::Client::new(),
            send_guard: None,
        })
    }

    /// Attach the OTP send guards (#516) for edge shedding. Builder-shaped rather than a constructor
    /// parameter so `from_env`'s gating stays the pure function `from_parts` tests drive, and so a
    /// deployment without a database connection still constructs an identity service (it simply sheds
    /// nothing, and the hook-path wall still holds).
    pub fn with_send_guard(
        mut self,
        guard: std::sync::Arc<crate::sms_authorization::SmsSendAuthorizer>,
    ) -> Self {
        self.send_guard = Some(guard);
        self
    }

    /// The claim-stamp core (#437): GET the auth user's current `app_metadata` (the
    /// idempotence/conflict decision PRECEDES any write — the create_if_absent lesson), then PUT
    /// both claims via [`stamp_put_body`]. Redelivery-idempotent: already-equal → no-op Ok;
    /// DIFFERENT existing id → [`StampFailure::ClaimConflict`], never an overwrite.
    async fn stamp_inner(&self, input: &IdentityStampCustomerClaimInput) -> Result<(), StampFailure> {
        let Some(admin_key) = self.admin_key.as_deref() else {
            return Err(StampFailure::NotConfigured);
        };
        let url = format!("{}/auth/v1/admin/users/{}", self.base_url, input.auth_ref.0);
        let target = input.customer_id.0.to_string();

        let resp = self
            .http
            .get(&url)
            .timeout(ADMIN_HTTP_TIMEOUT)
            .header("apikey", admin_key)
            .header("Authorization", format!("Bearer {admin_key}"))
            .send()
            .await
            .map_err(|e| StampFailure::Provider(format!("admin get transport: {e}")))?;
        let status = resp.status();
        let user: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(StampFailure::Provider(format!("admin get {}: {user}", status.as_u16())));
        }

        let metadata = user.get("app_metadata").cloned().unwrap_or_else(|| json!({}));
        match stamp_decision(&metadata, &target) {
            StampDecision::Conflict { existing } => {
                return Err(StampFailure::ClaimConflict {
                    auth_ref: input.auth_ref.0.clone(),
                    existing,
                    target,
                });
            }
            StampDecision::Noop => return Ok(()),
            StampDecision::Put => {}
        }

        let resp = self
            .http
            .put(&url)
            .timeout(ADMIN_HTTP_TIMEOUT)
            .header("apikey", admin_key)
            .header("Authorization", format!("Bearer {admin_key}"))
            .json(&stamp_put_body(&input.customer_id))
            .send()
            .await
            .map_err(|e| StampFailure::Provider(format!("admin put transport: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body: Value = resp.json().await.unwrap_or(Value::Null);
            return Err(StampFailure::Provider(format!("admin put {}: {body}", status.as_u16())));
        }
        Ok(())
    }

    /// POST a JSON body to `/auth/v1/<path>` with the anon apikey; return the parsed JSON on 2xx,
    /// or a mapped [`DomainError`] (typed rejection on 4xx, `Repository` on transport/5xx).
    /// `timeout` bounds the one request when given — pass it for any call that runs INSIDE a
    /// mailbox delivery (the lease-vs-timeout math on [`ADMIN_HTTP_TIMEOUT`]); `None` keeps the
    /// client default for the user-facing OTP round-trips.
    async fn post(
        &self,
        path: &str,
        body: Value,
        verify_ctx: Option<Value>,
        timeout: Option<std::time::Duration>,
    ) -> Result<Value, DomainError> {
        let url = format!("{}/auth/v1/{path}", self.base_url);
        let mut req = self
            .http
            .post(&url)
            .header("apikey", &self.apikey)
            .header("Authorization", format!("Bearer {}", self.apikey))
            .json(&body);
        if let Some(t) = timeout {
            req = req.timeout(t);
        }
        let resp = req
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

// ─── The customer claim stamp (#437, services.yaml identity.stamp_customer_claim) ───────────────

/// Why a claim stamp failed — each variant is one bounded `reason` label of
/// `customer_claim_stamp_failed_total` (contract `customer-identification`).
#[derive(Debug)]
enum StampFailure {
    /// `SUPABASE_SECRET_KEY` absent: fail CLOSED, never pretend to have stamped (#437).
    NotConfigured,
    /// The auth user already carries a DIFFERENT `captain_customer_id` — a data defect to
    /// INVESTIGATE, never an overwrite and never retried (architect verdict on #437: retrying
    /// cannot fix a wrong binding, it can only hammer it).
    ClaimConflict { auth_ref: String, existing: String, target: String },
    /// Transport failure, non-2xx admin response, or a malformed provider payload.
    Provider(String),
}

impl StampFailure {
    /// The bounded metric label (`reason`).
    fn reason(&self) -> &'static str {
        match self {
            StampFailure::NotConfigured => "not_configured",
            StampFailure::ClaimConflict { .. } => "claim_conflict",
            StampFailure::Provider(_) => "provider_error",
        }
    }

    fn into_domain_error(self) -> DomainError {
        match self {
            StampFailure::NotConfigured => not_configured("customer claim stamp"),
            StampFailure::ClaimConflict { auth_ref, existing, target } => DomainError::Repository(
                format!(
                    "supabase claim stamp CONFLICT: auth user {auth_ref} already stamped with \
                     captain_customer_id {existing}, refusing to overwrite with {target} \
                     (investigate, never retry -- #437)"
                ),
            ),
            StampFailure::Provider(detail) => {
                DomainError::Repository(format!("supabase claim stamp: {detail}"))
            }
        }
    }
}

/// What [`SupabaseIdentityService::stamp_inner`] does after reading the auth user's current
/// `app_metadata` — the PURE redelivery/conflict decision, unit-tested with constructed metadata.
#[derive(Debug, PartialEq, Eq)]
enum StampDecision {
    /// Both claims already in place and correct → idempotent no-op (a redelivered VerifyPhone
    /// must not re-write).
    Noop,
    /// Write both claims via [`stamp_put_body`] — covers a fresh user, a half-stamped user
    /// (id present, role missing), and a present-but-WRONG role (the PUT repairs it: the body
    /// hardcodes CUSTOMER and the id is already equal, so the write is safe).
    Put,
    /// A DIFFERENT customer id is already stamped → refuse; never an overwrite.
    Conflict { existing: String },
}

/// The idempotence/conflict decision on the CURRENT provider metadata, before any write.
fn stamp_decision(metadata: &Value, target: &str) -> StampDecision {
    match metadata.get("captain_customer_id").and_then(Value::as_str) {
        Some(existing) if existing != target => StampDecision::Conflict { existing: existing.to_string() },
        // No-op ONLY when the role is exactly CUSTOMER — a present-but-wrong role (checkpoint-b
        // hole) must fall through to the repairing PUT, or the wrong-role token gets rotated
        // and parked.
        Some(_same) if metadata.get("captain_role").and_then(Value::as_str) == Some("CUSTOMER") => {
            StampDecision::Noop
        }
        _ => StampDecision::Put,
    }
}

/// The admin PUT body — PURE so a unit test pins the shallow-merge rule: GoTrue merges
/// `app_metadata` SHALLOWLY (top-level keys replace, siblings survive), so BOTH claims travel in
/// EVERY write — a single-key write would leave a token whose two claims came from different
/// writes, and on a fresh user would mint a customer id with no role. `captain_role` is HARDCODED
/// CUSTOMER: this operation exists for the paying customer's session, and a wrong-role stamp is
/// made unspellable rather than validated (architect verdict on #437).
fn stamp_put_body(customer_id: &CustomerId) -> Value {
    json!({
        "app_metadata": {
            "captain_role": "CUSTOMER",
            "captain_customer_id": customer_id.0.to_string(),
        }
    })
}

#[async_trait]
impl IdentityService for SupabaseIdentityService {
    async fn send_phone_otp(
        &self,
        input: IdentitySendPhoneOtpInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        // CHEAP SHEDDING (#516), NOT the wall. The euro is spent later and INBOUND: the provider
        // calls our `/auth/sms-hook` back, and that route makes the authoritative claim. What this
        // buys is still worth having, for two independent reasons — an obviously doomed request (an
        // unserved country, a number already at its cap) is refused with a TYPED, renderable reason
        // instead of a generic provider failure, and we skip a provider round-trip per attempt.
        //
        // It PEEKS rather than claims, so it never double-charges the budget the hook then claims
        // against. That makes it advisory by construction, which is the correct posture for a check
        // that is not the wall.
        if let Some(guard) = self.send_guard.as_ref() {
            guard
                .peek(&input.dialing_code, &input.national_number)
                .await
                .map_err(|refusal| refusal.into_domain_error())?;
        }
        let phone = canonical_phone(&input.dialing_code, &input.national_number);
        self.post("otp", json!({ "phone": phone }), None, None).await.map(|_| ())
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
                // PRE-EXISTING unbounded pattern (client default, no per-request bound): the
                // user-facing OTP verify round-trip. Recorded in ADR-20260809-212810's timeout
                // note; bounding it is outside #437's scope.
                None,
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
                // The rotation leg of stamp -> rotate -> park runs INSIDE the VerifyPhone mailbox
                // delivery, so it shares the in-lease bound (worst case GET + PUT + refresh = 15s
                // < 30s lease -- see ADMIN_HTTP_TIMEOUT).
                Some(ADMIN_HTTP_TIMEOUT),
            )
            .await?;
        Ok(IdentityRefreshSessionOutput {
            access_token: str_field(&v, "access_token")
                .ok_or_else(|| DomainError::Repository("supabase refresh: no access_token".into()))?,
            refresh_token: str_field(&v, "refresh_token"),
            expires_in: v.get("expires_in").and_then(Value::as_i64),
        })
    }

    async fn stamp_customer_claim(
        &self,
        input: IdentityStampCustomerClaimInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        // claims.stamp (CLIENT, customer-identification contract): emitted HERE at the ACL
        // boundary — it nests under the ambient verify-flow span, so it shares the run's
        // trace/correlation. A failed stamp records business.result=failed AND OTel ERROR status
        // (mob obligation: without ERROR a failed run is unclassifiable by the status rules),
        // and increments the defect counter with its bounded reason. The application layer sees
        // only the DomainError — it stays telemetry-SDK-free.
        let span = telemetry::spans::claims_stamp();
        let result = self.stamp_inner(&input).instrument(span.clone()).await;
        telemetry::spans::record_claims_stamp_result(&span, result.is_ok());
        result.map_err(|failure| {
            telemetry::meters::customer_identification::claim_stamp_failed(failure.reason());
            failure.into_domain_error()
        })
    }

    async fn send_email_magic_link(
        &self,
        input: IdentitySendEmailMagicLinkInput,
        _meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        self.post("otp", json!({ "email": input.email.0 }), None, None).await.map(|_| ())
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
                None,
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

    /// The pure core is exercised with constructed parts — NEVER by mutating process env (#388:
    /// mutating process env from parallel lib tests is the glibc-UB SIGSEGV class; clippy
    /// `disallowed-methods` now gates it).
    #[test]
    fn from_env_gates_on_both_url_and_key() {
        // Neither set → None; the composition root then falls back to fail-closed.
        assert!(SupabaseIdentityService::from_parts(SupabaseEnv::default()).is_none());
        let url_only = SupabaseEnv {
            url: Some("https://proj.supabase.co/".into()),
            ..Default::default()
        };
        assert!(SupabaseIdentityService::from_parts(url_only).is_none(), "URL alone is not enough");
        let both = SupabaseEnv {
            url: Some("https://proj.supabase.co/".into()),
            publishable_key: Some("anon-key".into()),
            ..Default::default()
        };
        let svc = SupabaseIdentityService::from_parts(both).expect("both set");
        // Trailing slash trimmed so `{base}/auth/v1/...` has no double slash.
        assert_eq!(svc.base_url, "https://proj.supabase.co");
        // The secret key is OPTIONAL (farley, #437): its absence never fails construction —
        // and so never boot — it only fails the claim stamp closed.
        assert!(svc.admin_key.is_none(), "no secret key -> constructed, stamp gated closed");
        // An empty value is UNSET (fail-closed), same as absent.
        let empty_key = SupabaseEnv {
            url: Some("https://proj.supabase.co/".into()),
            publishable_key: Some(String::new()),
            ..Default::default()
        };
        assert!(SupabaseIdentityService::from_parts(empty_key).is_none(), "empty key is unset");
        // Secret key present (and non-empty) -> the admin stamp is armed; empty = unset.
        let with_admin = SupabaseEnv {
            url: Some("https://proj.supabase.co/".into()),
            publishable_key: Some("anon-key".into()),
            secret_key: Some("sb_secret_x".into()),
        };
        let svc = SupabaseIdentityService::from_parts(with_admin).expect("constructed");
        assert_eq!(svc.admin_key.as_deref(), Some("sb_secret_x"));
        let empty_admin = SupabaseEnv {
            url: Some("https://proj.supabase.co/".into()),
            publishable_key: Some("anon-key".into()),
            secret_key: Some(String::new()),
        };
        let svc = SupabaseIdentityService::from_parts(empty_admin).expect("constructed");
        assert!(svc.admin_key.is_none(), "empty secret key is unset -> stamp fails closed");
    }

    /// The shallow-merge rule of services.yaml `identity.stamp_customer_claim`, pinned on the
    /// PURE body builder: BOTH claims in every write, exactly two keys, role hardcoded CUSTOMER
    /// (a wrong-role stamp is unspellable — architect verdict, #437).
    #[test]
    fn stamp_put_body_carries_both_claims_in_one_write() {
        let body = stamp_put_body(&CustomerId(uuid::Uuid::from_u128(0x437)));
        let md = body["app_metadata"].as_object().expect("app_metadata object");
        assert_eq!(md["captain_role"], json!("CUSTOMER"));
        assert_eq!(md["captain_customer_id"], json!("00000000-0000-0000-0000-000000000437"));
        assert_eq!(
            md.len(),
            2,
            "exactly the two claims -- GoTrue merges shallowly, so a missing sibling here means \
             a claim silently dropped on some provider states"
        );
    }

    /// The redelivery/conflict decision (#437, checkpoint-b hole): the no-op arm must demand the
    /// role be EXACTLY "CUSTOMER" — same id + any other role is a wrong-role stamp that the PUT
    /// must repair (body hardcodes CUSTOMER, id already equal, safe), NEVER a no-op that lets a
    /// wrong-role token get rotated and parked.
    #[test]
    fn stamp_decision_noop_only_on_exact_customer_role() {
        let target = "00000000-0000-0000-0000-000000000437";
        // Fully and correctly stamped -> idempotent no-op.
        let stamped = json!({ "captain_customer_id": target, "captain_role": "CUSTOMER" });
        assert_eq!(stamp_decision(&stamped, target), StampDecision::Noop);
        // Same id but WRONG role -> the PUT is issued (repair), not a no-op.
        let wrong_role = json!({ "captain_customer_id": target, "captain_role": "RESTAURANT" });
        assert_eq!(
            stamp_decision(&wrong_role, target),
            StampDecision::Put,
            "a present-but-wrong captain_role must fall through to the repairing PUT"
        );
        // Half-stamped (id, no role) -> PUT both keys.
        let half = json!({ "captain_customer_id": target });
        assert_eq!(stamp_decision(&half, target), StampDecision::Put);
        // Fresh user -> PUT.
        assert_eq!(stamp_decision(&json!({}), target), StampDecision::Put);
        // Different id -> conflict, never an overwrite (role irrelevant).
        let other = json!({ "captain_customer_id": "11111111-1111-1111-1111-111111111111", "captain_role": "CUSTOMER" });
        assert_eq!(
            stamp_decision(&other, target),
            StampDecision::Conflict { existing: "11111111-1111-1111-1111-111111111111".into() }
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
