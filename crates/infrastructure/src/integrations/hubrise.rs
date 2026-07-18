//! HubRise callback (webhook) ingress ACL (ADR-20260718-145856).
//!
//! HubRise POSTs a callback whenever a resource changes. This module is the verified, self-contained
//! **ingress**: it authenticates the delivery and parses the envelope. It does NOT yet translate a
//! callback into a domain event — that is the next chapter and is deliberately out of scope here, because
//! HubRise **catalog/inventory** callbacks carry *no* state (`needs_pull()`), so producing
//! `OfferStockUpdated` / an `ImportCatalog` command requires an **OAuth2 API client to pull the resource**
//! plus an **external-ref → domain-id mapping** (HubRise ids → our `OfferId`/`CatalogId`/`RestaurantId`).
//!
//! Auth (per HubRise docs): `HMAC-SHA256(client_secret, raw_body)`, **hex**-encoded, in the
//! `X-HubRise-Hmac-SHA256` header (no timestamp element — simpler than Stripe). Verified constant-time.

use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Env var holding the HubRise app **client secret** — the HMAC key for callback verification.
pub const HUBRISE_WEBHOOK_SECRET_ENV: &str = "HUBRISE_WEBHOOK_SECRET";
/// The signature header HubRise sends.
pub const HUBRISE_SIGNATURE_HEADER: &str = "x-hubrise-hmac-sha256";

/// Why an `X-HubRise-Hmac-SHA256` header was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HubRiseSignatureError {
    /// The header value is not valid hex.
    MalformedSignature,
    /// The hex signature did not match the HMAC of the body under our secret.
    NoMatch,
}

impl std::fmt::Display for HubRiseSignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedSignature => write!(f, "X-HubRise-Hmac-SHA256 is not valid hex"),
            Self::NoMatch => write!(f, "signature does not match the request body"),
        }
    }
}

/// Verify the `X-HubRise-Hmac-SHA256` header: hex `HMAC-SHA256(client_secret, raw_body)`, constant-time
/// ([`Mac::verify_slice`]). Verify over the RAW body bytes — a re-serialized JSON would never match.
pub fn verify_hubrise_signature(
    secret: &str,
    header_hex: &str,
    body: &[u8],
) -> Result<(), HubRiseSignatureError> {
    let expected = hex::decode(header_hex.trim()).map_err(|_| HubRiseSignatureError::MalformedSignature)?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(body);
    mac.verify_slice(&expected).map_err(|_| HubRiseSignatureError::NoMatch)
}

/// A HubRise callback envelope (the subset we read; unknown fields are ignored by serde).
#[derive(Debug, Clone, Deserialize)]
pub struct HubRiseCallback {
    /// Callback id — the natural idempotency key once enrichment lands.
    pub id: String,
    /// `catalog` | `customer` | `customer_list` | `delivery` | `inventory` | `location` | `order`.
    pub resource_type: String,
    /// `create` | `update` | `delete` | `patch`.
    pub event_type: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub location_id: Option<String>,
    #[serde(default)]
    pub catalog_id: Option<String>,
}

impl HubRiseCallback {
    /// Catalog/inventory callbacks carry no state, so the domain enrichment must PULL the resource from
    /// the HubRise API (OAuth). Orders/customers carry `new_state` and could be mapped from the payload.
    pub fn needs_pull(&self) -> bool {
        matches!(self.resource_type.as_str(), "catalog" | "inventory")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hex signature HubRise would send for `body` under `secret`.
    fn sign(secret: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    #[test]
    fn valid_signature_passes() {
        let (secret, body) = ("cs_test_secret", br#"{"id":"evt_1","resource_type":"order"}"#);
        assert!(verify_hubrise_signature(secret, &sign(secret, body), body).is_ok());
    }

    #[test]
    fn tampered_body_or_secret_fails() {
        let (secret, body) = ("cs_test_secret", br#"{"id":"evt_1"}"#.as_slice());
        let good = sign(secret, body);
        assert_eq!(
            verify_hubrise_signature(secret, &good, br#"{"id":"evt_2"}"#),
            Err(HubRiseSignatureError::NoMatch),
        );
        assert_eq!(
            verify_hubrise_signature("wrong_secret", &good, body),
            Err(HubRiseSignatureError::NoMatch),
        );
    }

    #[test]
    fn non_hex_signature_is_malformed() {
        assert_eq!(
            verify_hubrise_signature("s", "not-hex!!", b"{}"),
            Err(HubRiseSignatureError::MalformedSignature),
        );
    }

    #[test]
    fn envelope_parses_and_flags_pull() {
        let cb: HubRiseCallback = serde_json::from_str(
            r#"{"id":"cb_1","resource_type":"inventory","event_type":"patch","created_at":"2026-07-18T10:00:00Z","extra":"ignored"}"#,
        )
        .expect("parse");
        assert_eq!(cb.id, "cb_1");
        assert_eq!(cb.resource_type, "inventory");
        assert!(cb.needs_pull(), "inventory needs an API pull");

        let order: HubRiseCallback =
            serde_json::from_str(r#"{"id":"cb_2","resource_type":"order","event_type":"create"}"#).unwrap();
        assert!(!order.needs_pull(), "order carries state, no pull");
    }
}
