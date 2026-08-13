//! OVHcloud SMS API client (#118, PROP-20260724-233605) — phone-OTP delivery for the wrapped auth
//! provider (ADR-20260722-174500: OVH over Twilio for FR price + EU residency). The Supabase
//! Send-SMS hook route (`crates/server/auth_routes.rs`) verifies Supabase's signature then calls
//! [`OvhSmsClient::send`] here; the domain never sees provider detail (ADR-0015).
//!
//! OVH API v1 auth: each request carries `X-Ovh-Application/-Consumer/-Timestamp` and a
//! `X-Ovh-Signature = "$1$" + sha1_hex(APP_SECRET + "+" + CONSUMER_KEY + "+" + METHOD + "+" + URL +
//! "+" + BODY + "+" + TIMESTAMP)`. Transactional OTP uses `noStopClause: true` (no STOP footer).

use domain::shared::errors::DomainError;
use serde_json::json;
use sha1::{Digest, Sha1};

/// The raw environment inputs of [`OvhSmsClient::from_env`], one field per key, read verbatim
/// (empty-string filtering and defaults are [`OvhSmsClient::from_parts`]'s job). Exists so the
/// gating logic is a pure function tests can drive without mutating process env (#388).
#[derive(Default)]
struct OvhSmsEnv {
    endpoint: Option<String>,
    application_key: Option<String>,
    application_secret: Option<String>,
    consumer_key: Option<String>,
    service_name: Option<String>,
    sender: Option<String>,
}

/// The OVH SMS sender client. `from_env` gates on the full credential set — a partial config is
/// treated as UNSET (fail-closed: never a half-configured send path).
pub struct OvhSmsClient {
    base_url: String,
    application_key: String,
    application_secret: String,
    consumer_key: String,
    service_name: String,
    sender: String,
    http: reqwest::Client,
}

impl OvhSmsClient {
    /// Build from env — ALL required, else `None`:
    /// `OVH_APPLICATION_KEY/SECRET`, `OVH_CONSUMER_KEY`, `OVH_SMS_SERVICE_NAME`. Optional:
    /// `OVH_ENDPOINT` (base URL, default the EU API) and `OVH_SMS_SENDER` (default `CaptainFood`).
    ///
    /// Thin env-reading shell over [`Self::from_parts`] — the `server/src/lib.rs`
    /// `env_flag`/`parse_flag` pattern (#388): the gating logic is tested against constructed
    /// parts, so no test ever mutates process env (concurrent env mutation vs `getenv` under the
    /// parallel libtest harness is glibc UB — the intermittent SIGSEGV class). The closure shape
    /// (`let var = |k: &str| …`) is deliberate: it is shape 3 of the env-inventory drift gate
    /// (`every_env_var_read_by_the_crates_is_declared`), which harvests the keys off `var("…")`.
    pub fn from_env() -> Option<Self> {
        let var = |k: &str| std::env::var(k).ok();
        Self::from_parts(OvhSmsEnv {
            endpoint: var("OVH_ENDPOINT"),
            application_key: var("OVH_APPLICATION_KEY"),
            application_secret: var("OVH_APPLICATION_SECRET"),
            consumer_key: var("OVH_CONSUMER_KEY"),
            service_name: var("OVH_SMS_SERVICE_NAME"),
            sender: var("OVH_SMS_SENDER"),
        })
    }

    /// A client for TESTS ONLY, pointed at `base_url` with dummy credentials — so a test can prove
    /// that a guarded path never REACHES the sender without either mutating process env or making a
    /// real OVH call. Point it at an unroutable address and any send that slips through is a loud
    /// transport failure rather than a silent pass.
    #[doc(hidden)]
    pub fn for_test(base_url: &str) -> Option<Self> {
        Self::from_parts(OvhSmsEnv {
            endpoint: Some(base_url.to_string()),
            application_key: Some("ak".into()),
            application_secret: Some("as".into()),
            consumer_key: Some("ck".into()),
            service_name: Some("sms-test-1".into()),
            sender: None,
        })
    }

    /// The pure gating core of [`Self::from_env`]: same fail-closed rule (any required part
    /// absent or empty ⇒ `None`), no environment access.
    fn from_parts(parts: OvhSmsEnv) -> Option<Self> {
        let set = |v: Option<String>| v.filter(|s| !s.is_empty());
        Some(Self {
            base_url: set(parts.endpoint)
                .unwrap_or_else(|| "https://eu.api.ovh.com/1.0".to_string())
                .trim_end_matches('/')
                .to_string(),
            application_key: set(parts.application_key)?,
            application_secret: set(parts.application_secret)?,
            consumer_key: set(parts.consumer_key)?,
            service_name: set(parts.service_name)?,
            sender: set(parts.sender).unwrap_or_else(|| "CaptainFood".to_string()),
            http: reqwest::Client::new(),
        })
    }

    /// Send one transactional SMS (the OTP) to an **authorised** recipient.
    ///
    /// The recipient is an [`AuthorizedSmsRecipient`](crate::sms_authorization::AuthorizedSmsRecipient)
    /// rather than a `&str` ON PURPOSE (#516, compiler-first per ADR-20260803-234035): that type's
    /// field is private to `crate::sms_authorization` and it has no public constructor, so the ONLY
    /// way to obtain one is `SmsSendAuthorizer::authorize`, which checks the country allowlist and
    /// claims the send against the shared per-number and global-daily counters first.
    ///
    /// A caller holding a phone number and a message therefore has **no path to this method**.
    /// "Someone added a second call site and forgot the guard" becomes a type error rather than a
    /// review finding — which matters because this is a money path that already has more than one
    /// door. Emits `sms_send_total{result}` at the seam where a message becomes a euro.
    ///
    /// **The witness is taken BY VALUE, so one claim buys exactly one send.** It was `&`-borrowed
    /// first, which quietly allowed `for _ in 0..1000 { sms.send(&w, m) }` — a thousand messages
    /// against a single claim, i.e. the budget bypassed by a loop. Consuming it makes "one claim, one
    /// send" a property of the type rather than a sentence in a doc comment. Do not add a `&` here.
    pub async fn send(
        &self,
        recipient: crate::sms_authorization::AuthorizedSmsRecipient,
        message: &str,
    ) -> Result<(), DomainError> {
        let result = self.send_authorized(recipient.as_str(), message).await;
        telemetry::meters::otp_send::sms_send(if result.is_ok() { "sent" } else { "failed" });
        result
    }

    /// The transport itself, split out so [`Self::send`] is only the witness + telemetry wrapper.
    /// **Private on purpose**: it takes a bare `&str`, so it must stay unreachable from outside this
    /// type or the witness would be decorative.
    async fn send_authorized(&self, phone: &str, message: &str) -> Result<(), DomainError> {
        let url = format!("{}/sms/{}/jobs", self.base_url, self.service_name);
        let body = json!({
            "message": message,
            "sender": self.sender,
            "receivers": [phone],
            "noStopClause": true,        // transactional OTP — no STOP footer
            "senderForResponse": false,
        })
        .to_string();
        let ts = now_unix();
        let sig = self.sign("POST", &url, &body, ts);
        let resp = self
            .http
            .post(&url)
            .header("X-Ovh-Application", &self.application_key)
            .header("X-Ovh-Consumer", &self.consumer_key)
            .header("X-Ovh-Timestamp", ts.to_string())
            .header("X-Ovh-Signature", sig)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| DomainError::Repository(format!("ovh sms transport: {e}")))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            Err(DomainError::Repository(format!("ovh sms {status}: {text}")))
        }
    }

    /// The `X-Ovh-Signature` for a request (see module docs). Public-in-crate for the unit test.
    fn sign(&self, method: &str, url: &str, body: &str, timestamp: u64) -> String {
        ovh_signature(&self.application_secret, &self.consumer_key, method, url, body, timestamp)
    }
}

/// OVH request signature: `"$1$" + sha1_hex(AS + "+" + CK + "+" + METHOD + "+" + URL + "+" + BODY +
/// "+" + TS)`. Pure — unit-tested against a fixed vector.
fn ovh_signature(
    app_secret: &str,
    consumer_key: &str,
    method: &str,
    url: &str,
    body: &str,
    timestamp: u64,
) -> String {
    let to_sign =
        format!("{app_secret}+{consumer_key}+{method}+{url}+{body}+{timestamp}");
    let mut hasher = Sha1::new();
    hasher.update(to_sign.as_bytes());
    format!("$1${}", hex::encode(hasher.finalize()))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pure core is exercised with constructed parts — NEVER by mutating process env (#388:
    /// mutating process env from parallel lib tests is the glibc-UB SIGSEGV class; clippy
    /// `disallowed-methods` now gates it).
    #[test]
    fn from_env_requires_the_full_credential_set() {
        assert!(
            OvhSmsClient::from_parts(OvhSmsEnv::default()).is_none(),
            "no config → None (fail-closed)"
        );
        let partial = OvhSmsEnv {
            application_key: Some("app".into()),
            application_secret: Some("secret".into()),
            consumer_key: Some("consumer".into()),
            ..Default::default()
        };
        assert!(
            OvhSmsClient::from_parts(partial).is_none(),
            "service name still missing → None"
        );
        let full = OvhSmsEnv {
            application_key: Some("app".into()),
            application_secret: Some("secret".into()),
            consumer_key: Some("consumer".into()),
            service_name: Some("sms-test-1".into()),
            ..Default::default()
        };
        let c = OvhSmsClient::from_parts(full).expect("full set → Some");
        assert_eq!(c.sender, "CaptainFood", "default sender");
        assert_eq!(c.base_url, "https://eu.api.ovh.com/1.0", "default EU endpoint");
        // An empty required value is UNSET, not a half-configured send path (fail-closed).
        let empty_value = OvhSmsEnv {
            application_key: Some("app".into()),
            application_secret: Some("secret".into()),
            consumer_key: Some("consumer".into()),
            service_name: Some(String::new()),
            ..Default::default()
        };
        assert!(OvhSmsClient::from_parts(empty_value).is_none(), "empty required value → None");
    }

    #[test]
    fn signature_matches_the_ovh_scheme_fixed_vector() {
        // Deterministic vector: sha1("AS+CK+POST+https://eu.api.ovh.com/1.0/sms/x/jobs++1700000000").
        let sig = ovh_signature("AS", "CK", "POST", "https://eu.api.ovh.com/1.0/sms/x/jobs", "", 1_700_000_000);
        assert!(sig.starts_with("$1$"), "OVH signature prefix");
        assert_eq!(sig.len(), 3 + 40, "$1$ + 40 hex chars of SHA1");
        // Recomputed independently below to pin the exact bytes.
        let expect = {
            let mut h = Sha1::new();
            h.update(b"AS+CK+POST+https://eu.api.ovh.com/1.0/sms/x/jobs++1700000000");
            format!("$1${}", hex::encode(h.finalize()))
        };
        assert_eq!(sig, expect);
    }
}
