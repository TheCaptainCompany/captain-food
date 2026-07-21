//! OUTBOUND Stripe client: the real adapter behind the generated [`PaymentService`] port
//! (services.yaml `payment`, issue #26 — replaces the composition root's
//! `FailClosedPaymentGateway` stand-in when `STRIPE_SECRET_KEY` is configured).
//!
//! - `payment.request` → `POST {base}/v1/payment_intents` (form-encoded), tagging the intent's
//!   `metadata` with the [`ServiceCallMeta`] business refs (`orderId`/`restaurantId`/`cartId`,
//!   copied VERBATIM) so the INBOUND webhook ACL (acl.rs) can map `payment_intent.*` facts back
//!   onto our aggregates. `confirm=false`: the FRONTEND confirms with the returned `client_secret`
//!   (specs/PRODUCT_SPEC_WEB_CLIENT.md checkout).
//! - `payment.refund` → `POST {base}/v1/refunds` (`payment_intent` + `amount`); the refund OUTCOME
//!   (`PaymentRefunded`) stays an inbound webhook fact, never this call's return value.
//!
//! Error mapping (the port contract): a Stripe `card_error`/decline → the canonical
//! `errors.yaml#/PaymentDeclined` rejection (`DomainError::Invariant("PaymentDeclined: …")`);
//! transport failures / 5xx / unparseable bodies → `DomainError::Repository`.
//!
//! The base URL is injected (default `https://api.stripe.com`) so tests can point at a local mock;
//! the request encoding and response/error mapping are PURE functions, unit-tested without network.

use application::generated::services::{
    PaymentRefundInput, PaymentRequestInput, PaymentRequestOutput, PaymentService, ServiceCallMeta,
};
use async_trait::async_trait;
use domain::generated::scalars::PaymentIntentId;
use domain::shared::errors::DomainError;

pub const DEFAULT_BASE_URL: &str = "https://api.stripe.com";

/// The real outbound Stripe [`PaymentService`] adapter.
pub struct StripePaymentGateway {
    http: reqwest::Client,
    base_url: String,
    secret_key: String,
}

impl StripePaymentGateway {
    /// Production constructor: `https://api.stripe.com` + the account's secret key.
    pub fn new(secret_key: impl Into<String>) -> Self {
        Self::with_base_url(DEFAULT_BASE_URL, secret_key)
    }

    /// Test seam: point the client at a local Stripe mock.
    pub fn with_base_url(base_url: impl Into<String>, secret_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            secret_key: secret_key.into(),
        }
    }

    async fn post_form(
        &self,
        path: &str,
        form: &[(String, String)],
    ) -> Result<(u16, String), DomainError> {
        let response = self
            .http
            .post(format!("{}{}", self.base_url, path))
            .bearer_auth(&self.secret_key)
            .form(form)
            .send()
            .await
            .map_err(|e| DomainError::Repository(format!("stripe: transport error on {path}: {e}")))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|e| DomainError::Repository(format!("stripe: body read error on {path}: {e}")))?;
        Ok((status, body))
    }
}

#[async_trait]
impl PaymentService for StripePaymentGateway {
    async fn request(
        &self,
        input: PaymentRequestInput,
        meta: &ServiceCallMeta,
    ) -> Result<PaymentRequestOutput, DomainError> {
        let form = encode_create_intent_form(&input, meta);
        let (status, body) = self.post_form("/v1/payment_intents", &form).await?;
        decode_create_intent_response(status, &body)
    }

    async fn refund(&self, input: PaymentRefundInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        let form = encode_refund_form(&input);
        let (status, body) = self.post_form("/v1/refunds", &form).await?;
        decode_refund_response(status, &body)
    }
}

// ------------------------------------------------------------------------------------------------
// Pure encoding/decoding (unit-tested without network)
// ------------------------------------------------------------------------------------------------

/// Form body for `POST /v1/payment_intents`. `currency` is lowercased (Stripe convention); the
/// envelope's business refs are copied VERBATIM into the intent's `metadata` — the checkout call
/// site sets EXACTLY the keys the inbound webhook ACL reads back (`restaurantId`/`orderId`, plus
/// `cartId` for traceability); a call without them creates an intent the webhook cannot map
/// (fail-closed downstream, acl.rs).
pub fn encode_create_intent_form(
    input: &PaymentRequestInput,
    meta: &ServiceCallMeta,
) -> Vec<(String, String)> {
    let mut form = vec![
        ("amount".into(), input.amount.amount_cents.0.to_string()),
        ("currency".into(), input.amount.currency.0.to_lowercase()),
        ("payment_method".into(), input.payment_method_id.0.clone()),
    ];
    for (key, value) in &meta.refs {
        form.push((format!("metadata[{key}]"), value.clone()));
    }
    form.push(("confirm".into(), "false".into()));
    form
}

/// Form body for `POST /v1/refunds`.
pub fn encode_refund_form(input: &PaymentRefundInput) -> Vec<(String, String)> {
    vec![
        ("payment_intent".into(), input.payment_intent_id.0.clone()),
        ("amount".into(), input.amount.amount_cents.0.to_string()),
    ]
}

#[derive(serde::Deserialize)]
struct PaymentIntentBody {
    id: String,
    client_secret: Option<String>,
}

#[derive(serde::Deserialize)]
struct StripeErrorEnvelope {
    error: StripeErrorBody,
}

#[derive(serde::Deserialize)]
struct StripeErrorBody {
    #[serde(rename = "type")]
    kind: Option<String>,
    code: Option<String>,
    message: Option<String>,
}

/// Map a non-2xx Stripe response: card-type declines (`card_error`, or any 4xx carrying a
/// `*declined*`/`card_*` code) → the canonical `PaymentDeclined` rejection; everything else
/// (5xx, unparseable bodies, other API errors) → `DomainError::Repository`.
fn map_error(context: &str, status: u16, body: &str) -> DomainError {
    if status < 500 {
        if let Ok(envelope) = serde_json::from_str::<StripeErrorEnvelope>(body) {
            let err = envelope.error;
            let code = err.code.as_deref().unwrap_or("");
            let is_decline = err.kind.as_deref() == Some("card_error")
                || code.contains("declined")
                || code.starts_with("card_")
                || code.starts_with("insufficient_");
            let message = err.message.unwrap_or_else(|| "payment declined".into());
            if is_decline {
                let code_suffix = if code.is_empty() { String::new() } else { format!(" ({code})") };
                return DomainError::Invariant(format!("PaymentDeclined: {message}{code_suffix}"));
            }
            return DomainError::Repository(format!(
                "stripe: {context} rejected (HTTP {status}, code '{code}'): {message}"
            ));
        }
    }
    DomainError::Repository(format!("stripe: {context} failed (HTTP {status}): {body}"))
}

/// Parse a `POST /v1/payment_intents` response into the port's [`PaymentRequestOutput`].
pub fn decode_create_intent_response(
    status: u16,
    body: &str,
) -> Result<PaymentRequestOutput, DomainError> {
    if !(200..300).contains(&status) {
        return Err(map_error("create_payment_intent", status, body));
    }
    let intent: PaymentIntentBody = serde_json::from_str(body).map_err(|e| {
        DomainError::Repository(format!("stripe: unparseable PaymentIntent response: {e}"))
    })?;
    let client_secret = intent.client_secret.ok_or_else(|| {
        DomainError::Repository(format!(
            "stripe: PaymentIntent {} response carries no client_secret",
            intent.id
        ))
    })?;
    Ok(PaymentRequestOutput { payment_intent_id: PaymentIntentId(intent.id), client_secret })
}

/// Parse a `POST /v1/refunds` response: 2xx = refund ACCEPTED (settlement arrives as the inbound
/// `PaymentRefunded` webhook fact); errors map like create-intent.
pub fn decode_refund_response(status: u16, body: &str) -> Result<(), DomainError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    Err(map_error("request_refund", status, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::generated::entities::Money;
    use domain::generated::scalars::{CurrencyCode, MoneyCents, PaymentMethodId};

    fn request() -> PaymentRequestInput {
        PaymentRequestInput {
            amount: Money { amount_cents: MoneyCents(2450), currency: CurrencyCode("EUR".into()) },
            payment_method_id: PaymentMethodId("pm_card_visa".into()),
        }
    }

    /// The checkout call site's envelope: the business refs the webhook ACL reads back.
    fn meta() -> ServiceCallMeta {
        ServiceCallMeta::new(uuid::Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap())
            .with_ref("orderId", "11111111-1111-4111-8111-111111111111")
            .with_ref("restaurantId", "22222222-2222-4222-8222-222222222222")
            .with_ref("cartId", "33333333-3333-4333-8333-333333333333")
    }

    #[test]
    fn create_intent_form_encodes_amount_lowercase_currency_metadata_and_no_confirm() {
        let form = encode_create_intent_form(&request(), &meta());
        assert_eq!(
            form,
            vec![
                ("amount".to_string(), "2450".to_string()),
                ("currency".to_string(), "eur".to_string()),
                ("payment_method".to_string(), "pm_card_visa".to_string()),
                (
                    "metadata[cartId]".to_string(),
                    "33333333-3333-4333-8333-333333333333".to_string()
                ),
                (
                    "metadata[orderId]".to_string(),
                    "11111111-1111-4111-8111-111111111111".to_string()
                ),
                (
                    "metadata[restaurantId]".to_string(),
                    "22222222-2222-4222-8222-222222222222".to_string()
                ),
                ("confirm".to_string(), "false".to_string()),
            ]
        );
    }

    #[test]
    fn refund_form_encodes_intent_and_amount() {
        let form = encode_refund_form(&PaymentRefundInput {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            amount: Money { amount_cents: MoneyCents(500), currency: CurrencyCode("EUR".into()) },
        });
        assert_eq!(
            form,
            vec![
                ("payment_intent".to_string(), "pi_123".to_string()),
                ("amount".to_string(), "500".to_string()),
            ]
        );
    }

    #[test]
    fn ok_response_maps_to_created_payment_intent() {
        let body = r#"{"id":"pi_3ABC","object":"payment_intent","client_secret":"pi_3ABC_secret_x"}"#;
        let created = decode_create_intent_response(200, body).unwrap();
        assert_eq!(created.payment_intent_id.0, "pi_3ABC");
        assert_eq!(created.client_secret, "pi_3ABC_secret_x");
    }

    #[test]
    fn missing_client_secret_is_a_repository_error() {
        let body = r#"{"id":"pi_3ABC","object":"payment_intent"}"#;
        match decode_create_intent_response(200, body) {
            Err(DomainError::Repository(msg)) => assert!(msg.contains("client_secret"), "{msg}"),
            other => panic!("expected Repository error, got {other:?}"),
        }
    }

    #[test]
    fn card_declined_maps_to_canonical_payment_declined_rejection() {
        let body = r#"{"error":{"type":"card_error","code":"card_declined","message":"Your card was declined."}}"#;
        match decode_create_intent_response(402, body) {
            Err(DomainError::Invariant(msg)) => {
                assert!(msg.starts_with("PaymentDeclined: "), "{msg}");
                assert!(msg.contains("Your card was declined."), "{msg}");
                assert!(msg.contains("card_declined"), "{msg}");
            }
            other => panic!("expected PaymentDeclined Invariant, got {other:?}"),
        }
    }

    #[test]
    fn non_card_api_error_maps_to_repository_error() {
        let body = r#"{"error":{"type":"invalid_request_error","code":"parameter_missing","message":"Missing required param: amount."}}"#;
        match decode_create_intent_response(400, body) {
            Err(DomainError::Repository(msg)) => {
                assert!(msg.contains("parameter_missing"), "{msg}");
                assert!(msg.contains("Missing required param"), "{msg}");
            }
            other => panic!("expected Repository error, got {other:?}"),
        }
    }

    #[test]
    fn server_error_maps_to_repository_error_even_with_error_body() {
        let body = r#"{"error":{"type":"api_error","message":"Stripe is down"}}"#;
        match decode_create_intent_response(500, body) {
            Err(DomainError::Repository(msg)) => assert!(msg.contains("HTTP 500"), "{msg}"),
            other => panic!("expected Repository error, got {other:?}"),
        }
    }

    #[test]
    fn refund_ok_is_accepted_and_decline_maps_like_create_intent() {
        assert!(decode_refund_response(200, r#"{"id":"re_1","object":"refund"}"#).is_ok());
        let body = r#"{"error":{"type":"invalid_request_error","code":"charge_already_refunded","message":"Charge ch_1 has already been refunded."}}"#;
        match decode_refund_response(400, body) {
            Err(DomainError::Repository(msg)) => {
                assert!(msg.contains("charge_already_refunded"), "{msg}")
            }
            other => panic!("expected Repository error, got {other:?}"),
        }
    }

    #[test]
    fn unparseable_error_body_maps_to_repository_error() {
        match decode_create_intent_response(400, "not json") {
            Err(DomainError::Repository(msg)) => assert!(msg.contains("HTTP 400"), "{msg}"),
            other => panic!("expected Repository error, got {other:?}"),
        }
    }
}
