//! OUTBOUND Stripe client: the real adapter behind the generated [`PaymentService`] port
//! (services.yaml `payment`, issue #26 — replaces the composition root's
//! `FailClosedPaymentGateway` stand-in when `STRIPE_SECRET_KEY` is configured).
//!
//! - `payment.request` → `POST {base}/v1/payment_intents` (form-encoded,
//!   `capture_method=manual` — authorize-then-capture, ADR-20260808-195315 §1.2: confirmation
//!   HOLDS the funds, the money moves at capture), tagging the intent's `metadata` with the
//!   [`ServiceCallMeta`] business refs (`orderId`/`restaurantId`/`cartId`, copied VERBATIM) so the
//!   INBOUND webhook ACL (acl.rs) can map `payment_intent.*` facts back onto our aggregates.
//!   `confirm=false`: the FRONTEND confirms with the returned `client_secret`
//!   (specs/PRODUCT_SPEC_WEB_CLIENT.md checkout).
//! - `payment.capture` → `POST {base}/v1/payment_intents/{id}/capture` (full authorized amount;
//!   PaymentSettlementProcess's fulfilment leg); the settled `PaymentCaptured` stays an inbound
//!   webhook fact. A failed capture ALSO bumps `payment_capture_failed_total{reason}` here — the
//!   paging half of the observability contract (specs/observability.yaml#/payment-settlement).
//! - `payment.release` → `POST {base}/v1/payment_intents/{id}/cancel` (void an uncaptured hold;
//!   the rejection/timeout path needs no refund because no capture — §1.3); the settled
//!   `PaymentReleased` arrives as the `payment_intent.canceled` webhook fact. Failures bump the
//!   warn-level `payment_release_failed_total{reason}` (a dead void self-heals by expiry).
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
    PaymentCaptureInput, PaymentRefundInput, PaymentReleaseInput, PaymentRequestInput,
    PaymentRequestOutput, PaymentService, ServiceCallMeta,
};
use application::ports::payment_gateway_refused;
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
            // The SPAN-side twin of the counter below (#623/#624 part 1). Without it the
            // `place-order` contract's `technical_error: any_span_errors` rule cannot fire, so a
            // Stripe refusal on the riskiest leg of checkout exports as a successful CLIENT span and
            // the error budget never learns about it. Set here, at the framework boundary, and never
            // in a handler: business code stays independent of the telemetry SDK.
            telemetry::spans::record_payment_intent_create_error(&span);
            // BUSINESS metric: the checkout-failure counter is what answers "are customers unable to
            // pay right now" without reading a single trace.
            telemetry::meters::place_order::payment_failure("intent_create_failed");
        }
        result
    }

    async fn capture(&self, input: PaymentCaptureInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        // Full-amount capture (no amount_to_capture: Stripe captures the whole authorization by
        // default — partial capture is the recorded open tips discussion, ADR-20260808-195315
        // §1.4). Deterministic per intent: a redelivered fulfilment trigger re-runs the call and
        // Stripe answers with the SAME capture instead of erroring on the second attempt.
        let key = capture_idempotency_key(&input);
        let path = format!("/v1/payment_intents/{}/capture", input.payment_intent_id.0);
        let (status, body) = self.post_form(&path, &[], Some(&key)).await?;
        let result = decode_capture_response(status, &body);
        if let Err(e) = &result {
            // The PAGING half of the payment-settlement contract: food cooked, money did not move.
            // Reason mirrors the wrapper's CaptureFailureReason classification over the same error
            // contract (application::process_managers::payment_settlement::classify_capture_error).
            telemetry::meters::payment_settlement::capture_failed(capture_failure_label(e));
        }
        result
    }

    async fn release(&self, input: PaymentReleaseInput, _meta: &ServiceCallMeta) -> Result<(), DomainError> {
        // Void of an UNCAPTURED authorization ("no need to refund because no capture", §1.3).
        // Same idempotency posture as capture: a redelivered abort trigger lands on the same
        // cancellation. The released fact arrives as the `payment_intent.canceled` webhook.
        let key = release_idempotency_key(&input);
        let path = format!("/v1/payment_intents/{}/cancel", input.payment_intent_id.0);
        let (status, body) = self.post_form(&path, &[], Some(&key)).await?;
        let result = decode_cancel_response(status, &body);
        if let Err(e) = &result {
            // Warn-level (never a page): a failed void self-heals — the hold expires within ~7 days.
            let reason = match e {
                DomainError::Repository(_) => "transient",
                _ => "deterministic",
            };
            telemetry::meters::payment_settlement::release_failed(reason);
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

/// The capture idempotency key: deterministic per intent (full-amount capture, one per payment).
pub fn capture_idempotency_key(input: &PaymentCaptureInput) -> String {
    format!("capture:{}", input.payment_intent_id.0)
}

/// The release (void) idempotency key: deterministic per intent.
pub fn release_idempotency_key(input: &PaymentReleaseInput) -> String {
    format!("release:{}", input.payment_intent_id.0)
}

/// The `payment_capture_failed_total{reason}` label for a capture-call error — the SAME
/// classification the saga wrapper records on the PaymentCaptureFailed fact (typed
/// scalars.yaml#/CaptureFailureReason values), derived from this adapter's own error contract.
fn capture_failure_label(err: &DomainError) -> &'static str {
    match err {
        DomainError::Invariant(msg) if msg.starts_with("PaymentDeclined") => "CARD_DECLINED",
        DomainError::Invariant(msg)
            if msg.contains("expired") || msg.contains("canceled") || msg.contains("cancelled") =>
        {
            "AUTHORIZATION_EXPIRED"
        }
        DomainError::Invariant(_) => "GATEWAY_REFUSED",
        _ => "GATEWAY_UNAVAILABLE",
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
    // Authorize-then-capture (ADR-20260808-195315 §1.2): confirmation HOLDS the funds
    // (`payment_intent.amount_capturable_updated` → PaymentAuthorized); the money moves only at
    // the explicit capture on fulfilment. Without this, Stripe captures automatically at confirm.
    form.push(("capture_method".into(), "manual".into()));
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
            // `idempotency_error` is two DIFFERENT conditions: HTTP 400 = the same key was
            // reused with different parameters (deterministic — a keying bug, terminal), but
            // HTTP 409 = a request with the SAME key is concurrently in flight — transient by
            // definition, and one the runtime now MANUFACTURES routinely: a rebalance steal
            // redelivers a head row whose victim's prepare is still executing, so thief and
            // victim run the same `intent:{orderId}` key at once. Terminal here would fail a
            // checkout the winning arm was completing (#272 review MAJOR, 2026-08-01) — retry
            // in place instead; the settled gateway object answers the retry.
            if kind == "idempotency_error" && status == 409 {
                return DomainError::Repository(format!(
                    "stripe: {context} idempotency key in flight (HTTP 409): {message}"
                ));
            }
            if kind == "invalid_request_error" || kind == "idempotency_error" {
                // The status travels STRUCTURALLY (application::ports encodes it in a shape its
                // own reader parses back) so the mailbox can put `401` in the journal row without
                // putting `{message}` there — Stripe's 401 message is literally
                // "Invalid API Key provided: sk_test_…" (#623/#625).
                return payment_gateway_refused(
                    Some(status),
                    &format!(
                        "stripe {context} refused deterministically (code '{code}'): {message}"
                    ),
                );
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

/// Parse a `POST /v1/payment_intents/{id}/capture` response: 2xx = capture ACCEPTED (settlement
/// arrives as the inbound `PaymentCaptured` webhook fact); errors map like create-intent — a
/// decline is the canonical `PaymentDeclined`, a deterministic refusal (already captured/canceled,
/// expired authorization) `PaymentGatewayRefused`, transient failures `Repository`.
pub fn decode_capture_response(status: u16, body: &str) -> Result<(), DomainError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    Err(map_error("capture_payment_intent", status, body))
}

/// Parse a `POST /v1/payment_intents/{id}/cancel` response: 2xx = void ACCEPTED (the released
/// fact arrives as the inbound `PaymentReleased` webhook — `payment_intent.canceled`).
pub fn decode_cancel_response(status: u16, body: &str) -> Result<(), DomainError> {
    if (200..300).contains(&status) {
        return Ok(());
    }
    Err(map_error("cancel_payment_intent", status, body))
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

    /// ADR-20260808-195315 §1.2: `capture_method=manual` is what makes the confirm an
    /// AUTHORIZATION (funds held) instead of an automatic capture — the money moves only at the
    /// explicit capture on fulfilment. Losing this one pair silently reverts the whole posture.
    #[test]
    fn create_intent_form_encodes_manual_capture_metadata_and_no_confirm() {
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
                ("capture_method".to_string(), "manual".to_string()),
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
        // Capture/release keys are per-intent: a redelivered fulfilment/abort trigger must land
        // on the SAME gateway object (rules.yaml#/PaymentCapturedOnFulfilment idempotency half).
        assert_eq!(
            capture_idempotency_key(&PaymentCaptureInput {
                payment_intent_id: PaymentIntentId("pi_123".into()),
            }),
            "capture:pi_123"
        );
        assert_eq!(
            release_idempotency_key(&PaymentReleaseInput {
                payment_intent_id: PaymentIntentId("pi_123".into()),
            }),
            "release:pi_123"
        );
    }

    /// Capture/cancel responses: 2xx accepted; a decline stays the canonical PaymentDeclined; a
    /// deterministic refusal (already captured, expired auth → canceled intent) is terminal.
    #[test]
    fn capture_and_cancel_responses_map_like_create_intent() {
        assert!(decode_capture_response(200, r#"{"id":"pi_1","status":"succeeded"}"#).is_ok());
        assert!(decode_cancel_response(200, r#"{"id":"pi_1","status":"canceled"}"#).is_ok());
        let declined = r#"{"error":{"type":"card_error","code":"card_declined","message":"Your card was declined."}}"#;
        match decode_capture_response(402, declined) {
            Err(DomainError::Invariant(msg)) => assert!(msg.starts_with("PaymentDeclined: "), "{msg}"),
            other => panic!("expected PaymentDeclined, got {other:?}"),
        }
        let already = r#"{"error":{"type":"invalid_request_error","code":"payment_intent_unexpected_state","message":"This PaymentIntent could not be captured because it has a status of canceled."}}"#;
        match decode_capture_response(400, already) {
            Err(DomainError::Invariant(msg)) => {
                assert!(msg.starts_with("PaymentGatewayRefused: "), "{msg}")
            }
            other => panic!("expected terminal PaymentGatewayRefused, got {other:?}"),
        }
        match decode_cancel_response(500, "oops") {
            Err(DomainError::Repository(msg)) => assert!(msg.contains("HTTP 500"), "{msg}"),
            other => panic!("expected Repository, got {other:?}"),
        }
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

    /// #272 D3 review MAJOR: HTTP 409 `idempotency_error` = the SAME key concurrently in flight
    /// (a rebalance steal redelivering while the victim's prepare still runs) — transient, must
    /// retry in place, never a terminal refusal that fails a checkout the winning arm was
    /// completing. Only the 400 params-mismatch form stays terminal (asserted above).
    #[test]
    fn in_flight_idempotency_conflict_409_is_retryable() {
        let body = r#"{"error":{"type":"idempotency_error","code":"","message":"There is currently another in-progress request using this Idempotency Key."}}"#;
        match decode_create_intent_response(409, body) {
            Err(DomainError::Repository(msg)) => assert!(msg.contains("in flight"), "{msg}"),
            other => panic!("expected retryable Repository error, got {other:?}"),
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
