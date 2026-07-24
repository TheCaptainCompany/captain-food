//! The Supabase Auth **Send-SMS hook** boundary (#118) — verify + parse, so the server route stays
//! thin. Supabase POSTs this hook when it needs an SMS delivered (phone OTP); we verify it really
//! came from Supabase (the **standard-webhooks** signature), extract the phone + OTP, and hand them
//! to the OVH client. Pure/testable; the HTTP + OVH call live in the server route.
//!
//! Signature (standard-webhooks, the scheme Supabase Auth hooks use):
//! `webhook-signature: v1,<base64(HMAC_SHA256(secret, "<id>.<timestamp>.<body>"))>` (space-separated
//! list; any match passes), with the secret shown as `v1,whsec_<base64>` — the MAC key is the
//! base64-decoded part after `whsec_`. `webhook-timestamp` is checked against a ±5-min replay window.

use base64::Engine;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Replay tolerance for `webhook-timestamp` (seconds).
const TIMESTAMP_TOLERANCE_SECS: i64 = 300;

/// The MAC key from a Supabase hook secret string (`v1,whsec_<base64>` or a bare `whsec_<base64>` or
/// raw base64) — base64-decode the part after `whsec_`.
pub fn decode_hook_secret(secret: &str) -> Option<Vec<u8>> {
    let s = secret.trim();
    // Take the last comma-separated token (Supabase shows `v1,whsec_…`).
    let token = s.rsplit(',').next().unwrap_or(s).trim();
    let b64 = token.strip_prefix("whsec_").unwrap_or(token);
    base64::engine::general_purpose::STANDARD.decode(b64).ok().filter(|b| !b.is_empty())
}

/// Verify a standard-webhooks signature. `now_unix` is injected for deterministic tests.
pub fn verify(
    secret_key: &[u8],
    webhook_id: &str,
    webhook_timestamp: &str,
    signature_header: &str,
    body: &str,
    now_unix: i64,
) -> bool {
    // Replay window on the timestamp.
    let Ok(ts) = webhook_timestamp.parse::<i64>() else { return false };
    if (now_unix - ts).abs() > TIMESTAMP_TOLERANCE_SECS {
        return false;
    }
    let signed = format!("{webhook_id}.{webhook_timestamp}.{body}");
    // Candidate sigs: space-separated `v1,<base64>` entries — decode each, constant-time compare.
    for entry in signature_header.split_whitespace() {
        let b64 = entry.rsplit(',').next().unwrap_or(entry);
        let Ok(expected) = base64::engine::general_purpose::STANDARD.decode(b64) else { continue };
        let mut mac = match HmacSha256::new_from_slice(secret_key) {
            Ok(m) => m,
            Err(_) => return false,
        };
        mac.update(signed.as_bytes());
        if mac.verify_slice(&expected).is_ok() {
            return true;
        }
    }
    false
}

/// The `(phone, otp)` from the Supabase Send-SMS hook payload
/// (`{ "user": { "phone": … }, "sms": { "otp": … } }`). `None` when either is absent.
pub fn parse_payload(body: &str) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(body).ok()?;
    let phone = v.get("user")?.get("phone")?.as_str()?.to_string();
    let otp = v.get("sms")?.get("otp")?.as_str()?.to_string();
    // Supabase may send the phone without a leading '+'; the OVH receiver wants E.164.
    let phone = if phone.starts_with('+') { phone } else { format!("+{phone}") };
    Some((phone, otp))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &[u8], id: &str, ts: &str, body: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret).unwrap();
        mac.update(format!("{id}.{ts}.{body}").as_bytes());
        let sig = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        format!("v1,{sig}")
    }

    #[test]
    fn secret_decodes_from_the_whsec_form() {
        let raw = base64::engine::general_purpose::STANDARD.encode(b"a-32-byte-secret-key-value-here!!");
        assert!(decode_hook_secret(&format!("v1,whsec_{raw}")).is_some());
        assert!(decode_hook_secret(&format!("whsec_{raw}")).is_some());
        assert!(decode_hook_secret("not base64 %%%").is_none());
    }

    #[test]
    fn verify_accepts_a_valid_signature_and_rejects_tamper_and_replay() {
        let key = b"super-secret-key-bytes".to_vec();
        let (id, ts, body) = ("msg_1", "1700000000", r#"{"user":{"phone":"33600000000"},"sms":{"otp":"123456"}}"#);
        let header = sign(&key, id, ts, body);
        // Valid, within the window.
        assert!(verify(&key, id, ts, &header, body, 1_700_000_100));
        // Tampered body → reject.
        assert!(!verify(&key, id, ts, &header, r#"{"user":{"phone":"33699999999"},"sms":{"otp":"123456"}}"#, 1_700_000_100));
        // Expired timestamp (> 5 min) → reject even with a correct MAC.
        assert!(!verify(&key, id, ts, &header, body, 1_700_000_000 + 400));
        // Wrong key → reject.
        assert!(!verify(b"other-key", id, ts, &header, body, 1_700_000_100));
    }

    #[test]
    fn parses_phone_and_otp_and_normalizes_e164() {
        let (phone, otp) =
            parse_payload(r#"{"user":{"phone":"33612345678"},"sms":{"otp":"654321"}}"#).unwrap();
        assert_eq!(phone, "+33612345678"); // '+' added
        assert_eq!(otp, "654321");
        // Already-E.164 phone kept; missing fields → None.
        assert_eq!(parse_payload(r#"{"user":{"phone":"+33600000000"},"sms":{"otp":"1"}}"#).unwrap().0, "+33600000000");
        assert!(parse_payload(r#"{"user":{"phone":"33600000000"}}"#).is_none());
    }
}
