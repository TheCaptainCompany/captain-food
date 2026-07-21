//! OUTBOUND CoopCycle client: the real adapter behind the generated [`DeliveryService`] port
//! (services.yaml `delivery`, issue #58 — a third implementation alongside Avelo37, replacing the
//! composition root's `NoopDeliveryService` stand-in when `COOPCYCLE_INSTANCES` is configured).
//!
//! FEDERATION (specs/integrations/coopcycle.md §5): unlike Avelo37's single endpoint + static key,
//! CoopCycle is many co-op instances. `offer_job`:
//!   1. resolves the job to an instance by **dropoff postal-code prefix** (fail-closed if none covers it),
//!   2. fetches/refreshes that instance's **OAuth2 client-credentials** token (cached per instance),
//!   3. POSTs `POST {base}/deliveries` carrying OUR `job_reference` (= `deliveryJobId`) — the read-back
//!      key the instance echoes on every webhook so the inbound ACL (acl.rs) maps facts onto the stream.
//!
//! A 2xx means the offer was RECEIVED; acceptance/decline always come back asynchronously as inbound
//! webhook facts, never through this call's return value (services.yaml). The request encoding is a
//! PURE function, unit-tested without network.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use application::generated::services::{DeliveryOfferJobInput, DeliveryService, ServiceCallMeta};
use async_trait::async_trait;
use domain::generated::entities::Address;
use domain::shared::errors::DomainError;
use serde::Deserialize;

use crate::config::{CoopCycleInstance, CoopCycleRegistry};

/// A cached per-instance OAuth2 token with its expiry (refreshed shortly before it lapses).
struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

/// Refresh a token this long BEFORE its stated expiry, to avoid racing the boundary.
const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(60);

/// The real outbound CoopCycle [`DeliveryService`] adapter over the instance registry.
pub struct CoopCycleDeliveryGateway {
    http: reqwest::Client,
    registry: CoopCycleRegistry,
    /// Per-instance OAuth2 token cache (keyed by instance id).
    tokens: Mutex<HashMap<String, CachedToken>>,
}

impl CoopCycleDeliveryGateway {
    /// Construct from a parsed registry.
    pub fn new(registry: CoopCycleRegistry) -> Self {
        Self { http: reqwest::Client::new(), registry, tokens: Mutex::new(HashMap::new()) }
    }

    /// Build from the environment: `Some` when `COOPCYCLE_INSTANCES` holds a valid registry, `None`
    /// when unset/empty (the caller keeps the no-op stand-in). A SET-but-invalid registry is a
    /// misconfiguration surfaced as `Err` — the operator must see it, not get a silent no-op.
    pub fn from_env() -> Result<Option<Self>, String> {
        Ok(CoopCycleRegistry::from_env()?.map(Self::new))
    }

    /// A valid cached token for `instance`, fetching a fresh one via the OAuth2 client-credentials
    /// grant on miss/expiry. The network fetch happens OUTSIDE the cache lock (never held across
    /// await).
    async fn token_for(&self, instance: &CoopCycleInstance) -> Result<String, DomainError> {
        if let Some(tok) = self.tokens.lock().expect("token cache poisoned").get(&instance.id) {
            if tok.expires_at > Instant::now() {
                return Ok(tok.access_token.clone());
            }
        }
        let (access_token, expires_in) = self.fetch_token(instance).await?;
        let expires_at = Instant::now()
            + expires_in.saturating_sub(TOKEN_REFRESH_SKEW).max(Duration::from_secs(1));
        self.tokens
            .lock()
            .expect("token cache poisoned")
            .insert(instance.id.clone(), CachedToken { access_token: access_token.clone(), expires_at });
        Ok(access_token)
    }

    /// OAuth2 client-credentials token fetch against the instance's `token_url`.
    async fn fetch_token(
        &self,
        instance: &CoopCycleInstance,
    ) -> Result<(String, Duration), DomainError> {
        let form = [
            ("grant_type", "client_credentials"),
            ("client_id", instance.oauth.client_id.as_str()),
            ("client_secret", instance.oauth.client_secret.as_str()),
        ];
        let response = self
            .http
            .post(&instance.oauth.token_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| {
                DomainError::Repository(format!(
                    "coopcycle[{}]: OAuth2 token transport error: {e}",
                    instance.id
                ))
            })?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(DomainError::Repository(format!(
                "coopcycle[{}]: OAuth2 token request failed (HTTP {status}): {text}",
                instance.id
            )));
        }
        let token: OAuthTokenResponse = serde_json::from_str(&text).map_err(|e| {
            DomainError::Repository(format!(
                "coopcycle[{}]: unparsable OAuth2 token response: {e}",
                instance.id
            ))
        })?;
        // `expires_in` is optional in the wild; default to a conservative 5 minutes when absent.
        Ok((token.access_token, Duration::from_secs(token.expires_in.unwrap_or(300))))
    }
}

#[async_trait]
impl DeliveryService for CoopCycleDeliveryGateway {
    async fn offer_job(
        &self,
        input: DeliveryOfferJobInput,
        meta: &ServiceCallMeta,
    ) -> Result<(), DomainError> {
        let postal = &input.job.dropoff.postal_code.0;
        // Fail closed: a job with no covering co-op instance is NOT silently dropped — the saga leg
        // surfaces the error (the job stays open to independent riders).
        let instance = self.registry.resolve_by_postal(postal).ok_or_else(|| {
            DomainError::Repository(format!(
                "coopcycle: no co-op instance covers dropoff postal '{postal}'"
            ))
        })?;
        let token = self.token_for(instance).await?;
        let body = encode_offer_body(&input, meta);
        let response = self
            .http
            .post(format!("{}/deliveries", instance.base_url.trim_end_matches('/')))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                DomainError::Repository(format!(
                    "coopcycle[{}]: transport error on /deliveries: {e}",
                    instance.id
                ))
            })?;
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        decode_offer_response(&instance.id, status, &text)
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

fn encode_address(address: &Address) -> serde_json::Value {
    serde_json::json!({
        "line1": address.line1.0,
        "line2": address.line2.as_ref().map(|l| l.0.clone()),
        "postal_code": address.postal_code.0,
        "city": address.city.0,
        "country": address.country.0,
    })
}

/// JSON body for `POST /deliveries`. `job_reference` is the read-back key: the instance MUST echo it
/// on every webhook (`data.delivery.job_reference`) so the inbound ACL can map facts onto the
/// `DeliveryJob` stream (the exact Avelo37 / Stripe-`metadata` read-back pattern).
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

/// Map a `POST /deliveries` response: 2xx = offer RECEIVED (the answer arrives as an inbound webhook
/// fact); everything else is an infrastructure error the saga leg surfaces.
pub fn decode_offer_response(instance_id: &str, status: u16, body: &str) -> Result<(), DomainError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    Err(DomainError::Repository(format!(
        "coopcycle[{instance_id}]: offer_job failed (HTTP {status}): {body}"
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
            channel: domain::generated::scalars::DeliveryChannelKey("coopcycle".into()),
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
        assert_eq!(body["dropoff"]["postal_code"], "37000");
        assert_eq!(body["metadata"]["orderId"], uid(2).to_string());
    }

    #[test]
    fn twoxx_is_accepted_and_everything_else_is_a_repository_error() {
        assert!(decode_offer_response("tours", 201, r#"{"id":"task_77"}"#).is_ok());
        match decode_offer_response("tours", 503, "instance down") {
            Err(DomainError::Repository(msg)) => {
                assert!(msg.contains("HTTP 503"), "{msg}");
                assert!(msg.contains("coopcycle[tours]"), "{msg}");
            }
            other => panic!("expected Repository error, got {other:?}"),
        }
    }
}
