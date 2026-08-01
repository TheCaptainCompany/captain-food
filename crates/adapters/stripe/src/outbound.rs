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
use tracing::Instrument as _;

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
        idempotency_key: Option<&str>,
    ) -> Result<(u16, String), DomainError> {
        let mut request = self
            .http
            .post(format!("{}{}", self.base_url, path))
            .bearer_auth(&self.secret_key)
            .form(form);
        // Stripe's native retry-safety seam (ADR-20260801-023000): the SAME key re-submitted
        // returns the SAME object, so a mailbox redelivery that re-runs the prepare phase can
        // never create a second intent (or a second refund) for the same business identity.
        if let Some(key) = idempotency_key {
            request = request.header("Idempotency-Key", key);
        }
        let response = request
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
        // payment.intent.create (CLIENT) — the `place-order` contract's riskiest span, and the one its
        // success condition is written against (`business.result == 'captured'`). PROP-170500's worked
        // example of a Friday-night investigation is a 3.9s timeout on exactly this leg.
        //
        // `result` here is the intent CREATION outcome, not the capture: capture arrives later as an
        // inbound Stripe webhook fact. `created` therefore means "Stripe accepted the intent", and the
        // contract's `captured` value is recorded by the webhook path, not here — conflating the two
        // would make a created-but-never-captured payment look successful, which is the precise shape
        // of "a paid order nobody was told about".
        let span = telemetry::spans::payment_intent_create();
        let result = async {
            let form = encode_create_intent_form(&input, meta);
            // Idempotency key = the orderId ref (ADR-20260801-023000): the checkout call site
            // always sets it, so a re-run of the prepare phase (crash between the Stripe call
            // and the fenced commit, then redelivery) receives the SAME intent, no duplicate.
            let key = intent_idempotency_key(meta);
            let (status, body) =
                self.post_form("/v1/payment_intents", &form, key.as_deref()).await?;
            decode_create_intent_response(status, &body)
        }
        .instrument(span.clone())
        .await;
        telemetry::spans::record_payment_result(
            &span,
            match &result {
                Ok(_) => "created",
                Err(_) => "failed",
            },
        );
        if result.is_err() {
            // BUSINESS metric: the checkout-failure counter is what answers "are customers unable to
            // pay right now" without reading a single trace.
            telemetry::meters::place_order::payment_failure("intent_create_failed");
        }
        result
    }

    async fn refund(&self, input: PaymentRefundInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        let form = encode_refund_form(&input);
        // Deterministic per (intent, amount): a redelivered ApproveRefund re-runs the call and
        // Stripe returns the SAME refund instead of moving the money twice.
        let key = refund_idempotency_key(&input);
        let (status, body) = self.post_form("/v1/refunds", &form, Some(&key)).await?;
        decode_refund_response(status, &body)
    }
}

/// The create-intent idempotency key: the `orderId` business ref when the call site set it
/// (the checkout always does), else none — a keyless call keeps Stripe's default behavior.
pub fn intent_idempotency_key(meta: &ServiceCallMeta) -> Option<String> {
    meta.refs.get("orderId").map(|order_id| format!("intent:{order_id}"))
}

/// The refund idempotency key: deterministic over the refunded intent and amount.
pub fn refund_idempotency_key(input: &PaymentRefundInput) -> String {
    format!("refund:{}:{}", input.payment_intent_id.0, input.amount.amount_cents.0)
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

/// Map a non-2xx Stripe response onto the THREE outcome classes the delivery paths distinguish
/// (#272 D1 review CRITICAL-1 — the classes drive retry-vs-terminal, so a wrong class either
/// loses a payment or wedges a mailbox lane forever):
///
/// - card-type declines (`card_error`, or any 4xx carrying a `*declined*`/`card_*` code) → the
///   canonical `PaymentDeclined` rejection (`Invariant` with the catalogued prefix — REJECTED);
/// - DETERMINISTIC request refusals (`invalid_request_error`, `idempotency_error`) → `Invariant`
///   with a non-catalogued prefix: a terminal FAILED on BOTH dispatch arms. Retrying these can
///   never succeed (a bogus payment method, a reused idempotency key with different params), and
///   on the mailbox arm a `Repository` here would retry the head row FOREVER — one crafted
///   `paymentMethodId` per partition could wedge every checkout lane;
/// - everything plausibly transient (5xx, `rate_limit_error`, `api_error`, auth/config errors,
///   transport failures, unparseable bodies) → `DomainError::Repository`: the legacy arm lands
///   FAILED, the mailbox arm retries in place — the class where retry can actually help.
fn map_error(context: &str, status: u16, body: &str) -> DomainError {
    if status < 500 {
        if let Ok(envelope) = serde_json::from_str::<StripeErrorEnvelope>(body) {
            let err = envelope.error;
            let code = err.code.as_deref().unwrap_or("");
            let kind = err.kind.as_deref().unwrap_or("");
            let is_decline = kind == "card_error"
                || code.contains("declined")
                || code.starts_with("card_")
                || code.starts_with("insufficient_");
            let message = err.message.unwrap_or_else(|| "payment declined".into());
            if is_decline {
                let code_suffix = if code.is_empty() { String::new() } else { format!(" ({code})") };
                return DomainError::Invariant(format!("PaymentDeclined: {message}{code_suffix}"));
            }
            if kind == "invalid_request_error" || kind == "idempotency_error" {
                return DomainError::Invariant(format!(
                    "PaymentGatewayRefused: stripe {context} refused deterministically (HTTP {status}, code '{code}'): {message}"
                ));
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

    /// ADR-20260801-023000: intent idempotency key = the orderId ref; refund key deterministic
    /// over (intent, amount) — a prepare-phase re-run must land on the SAME Stripe object.
    #[test]
    fn idempotency_keys_are_deterministic_over_business_identity() {
        assert_eq!(
            intent_idempotency_key(&meta()).as_deref(),
            Some("intent:11111111-1111-4111-8111-111111111111")
        );
        assert_eq!(intent_idempotency_key(&ServiceCallMeta::new(uuid::Uuid::nil())), None);
        let key = refund_idempotency_key(&PaymentRefundInput {
            payment_intent_id: PaymentIntentId("pi_123".into()),
            amount: Money { amount_cents: MoneyCents(500), currency: CurrencyCode("EUR".into()) },
        });
        assert_eq!(key, "refund:pi_123:500");
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

    /// #272 D1 review CRITICAL-1: a deterministic request refusal must be TERMINAL (Invariant,
    /// non-catalogued prefix → FAILED on both arms), never Repository — on the mailbox arm a
    /// Repository outcome retries the head row forever and wedges the partition.
    #[test]
    fn deterministic_request_refusals_are_terminal_never_retried() {
        for body in [
            r#"{"error":{"type":"invalid_request_error","code":"parameter_missing","message":"Missing required param: amount."}}"#,
            r#"{"error":{"type":"idempotency_error","code":"","message":"Keys for idempotent requests can only be used with the same parameters."}}"#,
        ] {
            match decode_create_intent_response(400, body) {
                Err(DomainError::Invariant(msg)) => {
                    assert!(msg.starts_with("PaymentGatewayRefused: "), "{msg}");
                }
                other => panic!("expected terminal PaymentGatewayRefused, got {other:?}"),
            }
        }
    }

    /// The transient classes STAY retryable: rate limiting and unrecognized 4xx kinds map to
    /// Repository (mailbox retry-in-place; legacy FAILED).
    #[test]
    fn transient_gateway_errors_stay_repository() {
        let body = r#"{"error":{"type":"rate_limit_error","message":"Too many requests."}}"#;
        match decode_create_intent_response(429, body) {
            Err(DomainError::Repository(msg)) => assert!(msg.contains("HTTP 429"), "{msg}"),
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
    fn refund_ok_is_accepted_and_deterministic_refusal_is_terminal() {
        assert!(decode_refund_response(200, r#"{"id":"re_1","object":"refund"}"#).is_ok());
        // charge_already_refunded is deterministic — retrying an ApproveRefund delivery on it
        // forever would wedge the RefundProcess lane (review CRITICAL-1).
        let body = r#"{"error":{"type":"invalid_request_error","code":"charge_already_refunded","message":"Charge ch_1 has already been refunded."}}"#;
        match decode_refund_response(400, body) {
            Err(DomainError::Invariant(msg)) => {
                assert!(msg.starts_with("PaymentGatewayRefused: "), "{msg}");
                assert!(msg.contains("charge_already_refunded"), "{msg}")
            }
            other => panic!("expected terminal PaymentGatewayRefused, got {other:?}"),
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
