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
    pub fn from_env() -> Option<Self> {
        let var = |k: &str| std::env::var(k).ok().filter(|s| !s.is_empty());
        Some(Self {
            base_url: var("OVH_ENDPOINT")
                .unwrap_or_else(|| "https://eu.api.ovh.com/1.0".to_string())
                .trim_end_matches('/')
                .to_string(),
            application_key: var("OVH_APPLICATION_KEY")?,
            application_secret: var("OVH_APPLICATION_SECRET")?,
            consumer_key: var("OVH_CONSUMER_KEY")?,
            service_name: var("OVH_SMS_SERVICE_NAME")?,
            sender: var("OVH_SMS_SENDER").unwrap_or_else(|| "CaptainFood".to_string()),
            http: reqwest::Client::new(),
        })
    }

    /// Send one transactional SMS (the OTP) to an E.164 recipient.
    pub async fn send(&self, phone: &str, message: &str) -> Result<(), DomainError> {
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

    #[test]
    fn from_env_requires_the_full_credential_set() {
        for k in ["OVH_APPLICATION_KEY", "OVH_APPLICATION_SECRET", "OVH_CONSUMER_KEY", "OVH_SMS_SERVICE_NAME", "OVH_SMS_SENDER", "OVH_ENDPOINT"] {
            std::env::remove_var(k);
        }
        assert!(OvhSmsClient::from_env().is_none(), "no config → None (fail-closed)");
        std::env::set_var("OVH_APPLICATION_KEY", "app");
        std::env::set_var("OVH_APPLICATION_SECRET", "secret");
        std::env::set_var("OVH_CONSUMER_KEY", "consumer");
        assert!(OvhSmsClient::from_env().is_none(), "service name still missing → None");
        std::env::set_var("OVH_SMS_SERVICE_NAME", "sms-test-1");
        let c = OvhSmsClient::from_env().expect("full set → Some");
        assert_eq!(c.sender, "CaptainFood", "default sender");
        assert_eq!(c.base_url, "https://eu.api.ovh.com/1.0", "default EU endpoint");
        for k in ["OVH_APPLICATION_KEY", "OVH_APPLICATION_SECRET", "OVH_CONSUMER_KEY", "OVH_SMS_SERVICE_NAME"] {
            std::env::remove_var(k);
        }
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
