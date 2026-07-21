//! OUTBOUND Avelo37 client: the real adapter behind the generated [`DeliveryService`] port
//! (services.yaml `delivery`, issue #28 — replaces the composition root's logged
//! `NoopDeliveryService` stand-in when `AVELO37_API_KEY` is configured).
//!
//! - `delivery.offer_job` → `POST {base}/deliveries` (JSON), carrying OUR `job_reference`
//!   (= `deliveryJobId`) that the partner echoes back on every webhook — the exact Stripe
//!   `metadata` read-back pattern the inbound ACL (acl.rs) relies on — plus the pickup/dropoff
//!   addresses from the `DeliveryRequested` birth fact and the [`ServiceCallMeta`] business refs
//!   copied VERBATIM into `metadata`.
//!
//! A 2xx means the offer was RECEIVED by the partner — acceptance/decline always come back
//! asynchronously as the inbound webhook facts (`DeliveryAcceptedByPartner` /
//! `DeliveryRejectedByPartner`), never through this call's return value (services.yaml). The
//! delivery service catalog declares no typed rejections for `offer_job`, so every non-2xx maps to
//! `DomainError::Repository` — the saga leg surfaces it on /saga (fail-closed, no silent success).
//!
//! The base URL is injected (default [`DEFAULT_BASE_URL`]) so tests can point at a local mock; the
//! request encoding and response mapping are PURE functions, unit-tested without network.

use application::generated::services::{DeliveryOfferJobInput, DeliveryService, ServiceCallMeta};
use async_trait::async_trait;
use domain::generated::entities::Address;
use domain::shared::errors::DomainError;

/// Env var gating the outbound client (the composition root keeps the no-op stand-in when unset —
/// jobs then stay open to independent riders only).
pub const AVELO37_API_KEY_ENV: &str = "AVELO37_API_KEY";
/// Optional base-URL override (staging/mock environments).
pub const AVELO37_API_BASE_URL_ENV: &str = "AVELO37_API_BASE_URL";

pub const DEFAULT_BASE_URL: &str = "https://api.avelo37.fr";

/// The real outbound Avelo37 [`DeliveryService`] adapter.
pub struct Avelo37DeliveryGateway {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl Avelo37DeliveryGateway {
    /// Production constructor: [`DEFAULT_BASE_URL`] + the partner API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_base_url(DEFAULT_BASE_URL, api_key)
    }

    /// Test seam: point the client at a local Avelo37 mock.
    pub fn with_base_url(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
        }
    }

    /// Build from the environment: `Some` when `AVELO37_API_KEY` is set (base URL overridable via
    /// `AVELO37_API_BASE_URL`), `None` otherwise — the caller keeps the no-op stand-in.
    pub fn from_env() -> Option<Self> {
        let key = std::env::var(AVELO37_API_KEY_ENV).ok().filter(|k| !k.trim().is_empty())?;
        let base = std::env::var(AVELO37_API_BASE_URL_ENV)
            .ok()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Some(Self::with_base_url(base, key.trim().to_string()))
    }
}

#[async_trait]
impl DeliveryService for Avelo37DeliveryGateway {
    async fn offer_job(
        &self,
        input: DeliveryOfferJobInput,
        meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        let body = encode_offer_body(&input, meta);
        let response = self
            .http
            .post(format!("{}/deliveries", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                DomainError::Repository(format!("avelo37: transport error on /deliveries: {e}"))
            })?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        decode_offer_response(status, &text)
    }
}

// ------------------------------------------------------------------------------------------------
// Pure encoding/decoding (unit-tested without network)
// ------------------------------------------------------------------------------------------------

fn encode_address(address: &Address) -> serde_json::Value {
    serde_json::json!({
        "line1": address.line1.0,
        "line2": address.line2.as_ref().map(|l| l.0.clone()),
        "postal_code": address.postal_code.0,
        "city": address.city.0,
        "country": address.country.0,
    })
}

/// JSON body for `POST /deliveries`. `job_reference` is the read-back key: the partner MUST echo it
/// on every webhook (`data.delivery.job_reference`) so the inbound ACL can map facts onto the
/// `DeliveryJob` stream; a partner answer without it is unmappable (fail-closed downstream, acl.rs).
pub fn encode_offer_body(
    input: &DeliveryOfferJobInput,
    meta: &ServiceCallMeta,
) -> serde_json::Value {
    let job = &input.job;
    serde_json::json!({
        "job_reference": job.delivery_job_id.0,
        "order_reference": job.order_id.0,
        "restaurant_reference": job.restaurant_id.0,
        "pickup": encode_address(&job.pickup),
        "dropoff": encode_address(&job.dropoff),
        // The envelope's business refs, copied VERBATIM (the service-call analogue of Stripe's
        // intent metadata, ADR-20260721-043033).
        "metadata": meta.refs,
    })
}

/// Map a `POST /deliveries` response: 2xx = offer RECEIVED (the answer arrives as an inbound
/// webhook fact); everything else is an infrastructure error the saga leg surfaces.
pub fn decode_offer_response(status: u16, body: &str) -> Result<(), DomainError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    Err(DomainError::Repository(format!("avelo37: offer_job failed (HTTP {status}): {body}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::events::DeliveryRequested;
    use domain::generated::scalars::{
        AddressLine, CityName, CountryCode, DeliveryJobId, OrderId, PostalCode, RestaurantId,
    };

    fn uid(n: u128) -> uuid::Uuid {
        uuid::Uuid::from_u128(n)
    }
    fn address(line1: &str) -> Address {
        Address {
            line1: AddressLine(line1.into()),
            line2: None,
            postal_code: PostalCode("37000".into()),
            city: CityName("Tours".into()),
            country: CountryCode("FR".into()),
        }
    }
    fn input() -> DeliveryOfferJobInput {
        DeliveryOfferJobInput {
            job: DeliveryRequested {
                mode: None,
                delivery_job_id: DeliveryJobId(uid(1)),
                order_id: OrderId(uid(2)),
                restaurant_id: RestaurantId(uid(3)),
                pickup: address("1 Rue Nationale"),
                dropoff: address("9 Rue Colbert"),
                provider: None,
            },
        }
    }

    #[test]
    fn offer_body_carries_the_job_reference_addresses_and_verbatim_metadata() {
        let meta = ServiceCallMeta::new(uid(0xC0))
            .with_ref("orderId", uid(2).to_string())
            .with_ref("restaurantId", uid(3).to_string());
        let body = encode_offer_body(&input(), &meta);
        assert_eq!(body["job_reference"], serde_json::json!(uid(1)));
        assert_eq!(body["order_reference"], serde_json::json!(uid(2)));
        assert_eq!(body["restaurant_reference"], serde_json::json!(uid(3)));
        assert_eq!(body["pickup"]["line1"], "1 Rue Nationale");
        assert_eq!(body["pickup"]["postal_code"], "37000");
        assert_eq!(body["dropoff"]["line1"], "9 Rue Colbert");
        assert_eq!(body["metadata"]["orderId"], uid(2).to_string());
        assert_eq!(body["metadata"]["restaurantId"], uid(3).to_string());
    }

    #[test]
    fn twoxx_is_accepted_and_everything_else_is_a_repository_error() {
        assert!(decode_offer_response(201, r#"{"id":"dlv_77"}"#).is_ok());
        match decode_offer_response(503, "partner down") {
            Err(DomainError::Repository(msg)) => {
                assert!(msg.contains("HTTP 503"), "{msg}");
                assert!(msg.contains("partner down"), "{msg}");
            }
            other => panic!("expected Repository error, got {other:?}"),
        }
    }
}
