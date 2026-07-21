//! OUTBOUND Uber Direct client: the real adapter behind the generated [`DeliveryService`] port
//! (services.yaml `delivery`, issue #57 — a PARTNER implementation alongside Avelo37/CoopCycle,
//! replacing the composition root's `NoopDeliveryService` stand-in when `UBER_DIRECT_*` is configured).
//!
//! Uber Direct is ONE central API (no CoopCycle-style federation), so `offer_job`:
//!   1. fetches/refreshes the **OAuth2 client-credentials** token (a single cached token — Uber Direct
//!      is the token-manager pattern the issue calls out; unlike Avelo37's static bearer key),
//!   2. POSTs `POST {base}/v1/customers/{customer_id}/deliveries` carrying OUR `deliveryJobId` as the
//!      `external_id` — the read-back key Uber echoes on every webhook so the inbound ACL (acl.rs)
//!      maps facts onto the `DeliveryJob` stream (the exact Avelo37/CoopCycle/Stripe read-back pattern).
//!
//! A 2xx means the delivery was CREATED; courier assignment/progress always come back asynchronously
//! as inbound webhook facts, never through this call's return value (services.yaml). The request
//! encoding is a PURE function, unit-tested without network.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use application::generated::services::{DeliveryOfferJobInput, DeliveryService, ServiceCallMeta};
use async_trait::async_trait;
use domain::generated::entities::Address;
use domain::shared::errors::DomainError;
use serde::Deserialize;

use crate::config::UberDirectConfig;

/// A cached OAuth2 token with its expiry (refreshed shortly before it lapses).
struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

/// Refresh a token this long BEFORE its stated expiry, to avoid racing the boundary.
const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(60);

/// The real outbound Uber Direct [`DeliveryService`] adapter.
pub struct UberDirectDeliveryGateway {
    http: reqwest::Client,
    config: UberDirectConfig,
    /// OAuth2 token cache (single token — one central API, unlike CoopCycle's per-instance map).
    token: Mutex<Option<CachedToken>>,
}

impl UberDirectDeliveryGateway {
    /// Construct from a resolved config.
    pub fn new(config: UberDirectConfig) -> Self {
        Self { http: reqwest::Client::new(), config, token: Mutex::new(None) }
    }

    /// Build from the environment: `Some` when `UBER_DIRECT_*` is fully configured, `None` when unset
    /// (the caller keeps the no-op stand-in). A PARTIALLY-set config is a misconfiguration surfaced as
    /// `Err` — the operator must see it, not get a silent no-op.
    pub fn from_env() -> Result<Option<Self>, String> {
        Ok(UberDirectConfig::from_env()?.map(Self::new))
    }

    /// A valid cached token, fetching a fresh one via the OAuth2 client-credentials grant on
    /// miss/expiry. The network fetch happens OUTSIDE the cache lock (never held across await).
    async fn token(&self) -> Result<String, DomainError> {
        if let Some(tok) = self.token.lock().expect("token cache poisoned").as_ref() {
            if tok.expires_at > Instant::now() {
                return Ok(tok.access_token.clone());
            }
        }
        let (access_token, expires_in) = self.fetch_token().await?;
        let expires_at =
            Instant::now() + expires_in.saturating_sub(TOKEN_REFRESH_SKEW).max(Duration::from_secs(1));
        *self.token.lock().expect("token cache poisoned") =
            Some(CachedToken { access_token: access_token.clone(), expires_at });
        Ok(access_token)
    }

    /// OAuth2 client-credentials token fetch against the configured token endpoint.
    async fn fetch_token(&self) -> Result<(String, Duration), DomainError> {
        let form = [
            ("grant_type", "client_credentials"),
            ("client_id", self.config.client_id.as_str()),
            ("client_secret", self.config.client_secret.as_str()),
            ("scope", self.config.scope.as_str()),
        ];
        let response = self
            .http
            .post(&self.config.token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| {
                DomainError::Repository(format!("uber_direct: OAuth2 token transport error: {e}"))
            })?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(DomainError::Repository(format!(
                "uber_direct: OAuth2 token request failed (HTTP {status}): {text}"
            )));
        }
        let token: OAuthTokenResponse = serde_json::from_str(&text).map_err(|e| {
            DomainError::Repository(format!("uber_direct: unparsable OAuth2 token response: {e}"))
        })?;
        // `expires_in` is optional in the wild; default to a conservative 5 minutes when absent.
        Ok((token.access_token, Duration::from_secs(token.expires_in.unwrap_or(300))))
    }
}

#[async_trait]
impl DeliveryService for UberDirectDeliveryGateway {
    async fn offer_job(
        &self,
        input: DeliveryOfferJobInput,
        meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        let token = self.token().await?;
        let body = encode_offer_body(&input, meta);
        let response = self
            .http
            .post(self.config.create_delivery_url())
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                DomainError::Repository(format!(
                    "uber_direct: transport error on create-delivery: {e}"
                ))
            })?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        decode_offer_response(status, &text)
    }
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    expires_in: Option<u64>,
}

// ------------------------------------------------------------------------------------------------
// Pure encoding/decoding (unit-tested without network)
// ------------------------------------------------------------------------------------------------

/// A single-line Uber Direct address string (`structured_address` accepts a JSON string in the real
/// API; we send the formatted line, keeping the strong `Address` typing on our side).
fn format_address(address: &Address) -> String {
    let mut parts = vec![address.line1.0.clone()];
    if let Some(line2) = &address.line2 {
        if !line2.0.trim().is_empty() {
            parts.push(line2.0.clone());
        }
    }
    parts.push(format!("{} {}", address.postal_code.0, address.city.0));
    parts.push(address.country.0.clone());
    parts.join(", ")
}

/// JSON body for `POST /v1/customers/{customer_id}/deliveries`. `external_id` is the read-back key:
/// Uber MUST echo it on every webhook (`data.external_id`) so the inbound ACL can map facts onto the
/// `DeliveryJob` stream (the exact Avelo37/CoopCycle/Stripe-`metadata` read-back pattern).
pub fn encode_offer_body(input: &DeliveryOfferJobInput, meta: &ServiceCallMeta) -> serde_json::Value {
    let job = &input.job;
    serde_json::json!({
        // Our DeliveryJobId, echoed back on every webhook — the idempotent correlation key.
        "external_id": job.delivery_job_id.0,
        "manifest_reference": job.order_id.0,
        "pickup_address": format_address(&job.pickup),
        "dropoff_address": format_address(&job.dropoff),
        // The envelope's business refs, copied VERBATIM (the service-call analogue of Stripe's
        // intent metadata, ADR-20260721-043033).
        "metadata": meta.refs,
    })
}

/// Map a create-delivery response: 2xx = delivery CREATED (courier assignment/progress arrive as
/// inbound webhook facts); everything else is an infrastructure error the saga leg surfaces.
pub fn decode_offer_response(status: u16, body: &str) -> Result<(), DomainError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    Err(DomainError::Repository(format!(
        "uber_direct: offer_job failed (HTTP {status}): {body}"
    )))
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
            channel: domain::generated::scalars::DeliveryChannelKey("uber_direct".into()),
        }
    }

    #[test]
    fn offer_body_carries_external_id_addresses_and_verbatim_metadata() {
        let meta = ServiceCallMeta::new(uid(0xC0))
            .with_ref("orderId", uid(2).to_string())
            .with_ref("restaurantId", uid(3).to_string());
        let body = encode_offer_body(&input(), &meta);
        assert_eq!(body["external_id"], serde_json::json!(uid(1)));
        assert_eq!(body["manifest_reference"], serde_json::json!(uid(2)));
        assert_eq!(body["pickup_address"], "1 Rue Nationale, 37000 Tours, FR");
        assert_eq!(body["dropoff_address"], "9 Rue Colbert, 37000 Tours, FR");
        assert_eq!(body["metadata"]["orderId"], uid(2).to_string());
    }

    #[test]
    fn twoxx_is_accepted_and_everything_else_is_a_repository_error() {
        assert!(decode_offer_response(201, r#"{"id":"del_77"}"#).is_ok());
        match decode_offer_response(422, "no courier available") {
            Err(DomainError::Repository(msg)) => {
                assert!(msg.contains("HTTP 422"), "{msg}");
                assert!(msg.contains("uber_direct"), "{msg}");
            }
            other => panic!("expected Repository error, got {other:?}"),
        }
    }

    #[test]
    fn create_delivery_url_embeds_the_customer_id() {
        let cfg = UberDirectConfig {
            customer_id: "cust_1".into(),
            client_id: "c".into(),
            client_secret: "s".into(),
            webhook_secret: "w".into(),
            base_url: "https://api.uber.com/".into(),
            token_url: crate::config::DEFAULT_TOKEN_URL.into(),
            scope: crate::config::DEFAULT_SCOPE.into(),
        };
        assert_eq!(cfg.create_delivery_url(), "https://api.uber.com/v1/customers/cust_1/deliveries");
    }
}
