//! Stripe webhook Anti-Corruption Layer — INBOUND integration events (CLAUDE.md "Commands vs inbound
//! (integration) events"). Stripe PUSHes signed webhooks reporting facts that have ALREADY happened on
//! its side; there is nothing to validate and nothing to reject, so no command is involved: the ACL
//! translates the Stripe wire shape into the already-modelled domain events and records them as facts:
//!
//! - `payment_intent.succeeded`      → `PaymentCaptured`
//! - `payment_intent.payment_failed` → `PaymentFailed`
//! - `charge.refunded`               → `PaymentRefunded`
//!
//! Any other event type is acknowledged and ignored (Stripe sends whatever the endpoint subscribes to).
//!
//! # Boundary translation (Stripe idioms stay OUT of the domain)
//!
//! - Stripe amounts are integer **minor units** (`amount_received`, `refund.amount`) and lowercase ISO
//!   currency (`"eur"`) — mapped to the domain `Money { amountCents, currency }` value object with the
//!   currency uppercased ([`to_money`]). No floats, no string money.
//! - `restaurantId` / `orderId` are OUR ids: Stripe cannot know them unless we put them there, so the
//!   checkout flow MUST set them in the PaymentIntent's `metadata` (keys `restaurantId`, `orderId`) and
//!   they are read back from the webhook object's `metadata` here. A verified event whose metadata lacks
//!   a required id is unmappable — logged and acknowledged, never guessed.
//! - `payment_intent.id` / `refund.id` map to the `PaymentIntentId` / `RefundId` provider-reference
//!   scalars (which are Stripe ids by design, see `scalars.yaml`).
//!
//! # Signature verification ([`verify_signature`])
//!
//! `Stripe-Signature: t=<unix>,v1=<hexHmac>[,v1=…]` — the expected MAC is
//! `HMAC-SHA256(key = webhook secret, msg = "<t>.<raw body bytes>")`. Verification uses the RAW request
//! body (never a re-serialized JSON — any byte difference breaks the MAC), compares in CONSTANT TIME
//! (`hmac::Mac::verify_slice`), and rejects timestamps outside the ±[`SIGNATURE_TOLERANCE_SECS`] replay
//! window. The HTTP endpoint (in `server`) fails CLOSED when `STRIPE_WEBHOOK_SECRET` is unset.
//!
//! # Idempotency (redelivered webhooks are no-ops)
//!
//! Stripe retries until it sees a 2xx, so redelivery is normal. The adapter is a STATELESS
//! translator (ADR-20260719-193500): it maps the webhook and delivers the fact to the Payment
//! AGGREGATE via `application::payments::record_inbound_payment_event`, which dedups by the
//! aggregate's own fold ("already recorded?") — no `StripeEvent-%` envelope streams, no adapter
//! idempotency table. A redelivery is absorbed as [`StripeIngestOutcome::Duplicate`]. The envelope
//! `correlation_id` is the UUIDv5 of the Stripe event id for traceability.

use std::sync::Arc;

use application::ports::{is_version_conflict, Actor, EventStore};
use domain::generated::entities::Money;
use domain::generated::events::{DomainEvent, PaymentCaptured, PaymentFailed, PaymentRefunded};
use domain::generated::scalars::{
    CurrencyCode, MoneyCents, OrderId, PaymentIntentId, RefundId, RestaurantId,
};
use domain::shared::errors::DomainError;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Env var holding the endpoint's signing secret (`whsec_…`, from the Stripe dashboard/CLI). Read by
/// the `server` endpoint, which fails CLOSED (503) when it is unset.
pub const STRIPE_WEBHOOK_SECRET_ENV: &str = "STRIPE_WEBHOOK_SECRET";

/// Replay window: reject a signature whose `t=` is further than this from now (Stripe's documented
/// default tolerance).
pub const SIGNATURE_TOLERANCE_SECS: i64 = 300;

/// `UserType::EXTERNAL` ordinal for the event envelope (enums stored as declaration-order ints,
/// ADR-0037/0041) — Stripe facts are recorded as the fixed external system principal.
const EXTERNAL_USER_TYPE: i32 = 6;

// ---------------------------------------------------------------------------------------------
// Envelope identity (ADR-0041) — deterministic, like the SIRENE ACL's
// ---------------------------------------------------------------------------------------------

/// Fixed UUIDv5 namespace for every id this ACL derives. NEVER change it: derived ids are stable
/// across deliveries and deployments.
fn stripe_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/integrations/stripe")
}

/// Fixed system user id stamping the event envelope (`domain_events.user_id`) for facts Stripe reports.
pub fn stripe_system_user_id() -> uuid::Uuid {
    uuid::Uuid::new_v5(&stripe_namespace(), b"system:stripe-webhook")
}

/// Deterministic envelope `correlation_id` for a Stripe event id — every fact recorded from the same
/// delivery (and any redelivery attempt) correlates to the same value.
pub fn stripe_correlation_id(stripe_event_id: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&stripe_namespace(), stripe_event_id.as_bytes())
}

// ---------------------------------------------------------------------------------------------
// Signature verification
// ---------------------------------------------------------------------------------------------

/// Why a `Stripe-Signature` header was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureError {
    /// No parsable `t=<unix>` element in the header.
    MissingTimestamp,
    /// The header carries no `v1=` candidate at all.
    MissingSignature,
    /// `|now - t|` exceeds [`SIGNATURE_TOLERANCE_SECS`] (replay window).
    StaleTimestamp { timestamp: i64, now: i64 },
    /// No `v1` candidate matched the HMAC of `"<t>.<body>"` under our secret.
    NoMatchingSignature,
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTimestamp => write!(f, "no t= timestamp in Stripe-Signature header"),
            Self::MissingSignature => write!(f, "no v1= signature in Stripe-Signature header"),
            Self::StaleTimestamp { timestamp, now } => write!(
                f,
                "timestamp {timestamp} outside the {SIGNATURE_TOLERANCE_SECS}s replay window (now {now})"
            ),
            Self::NoMatchingSignature => write!(f, "no v1 signature matches the payload"),
        }
    }
}

/// Verify a `Stripe-Signature` header against the RAW request body.
///
/// The signed payload is `"<t>.<body>"` where `<t>` is the timestamp EXACTLY as it appeared in the
/// header (never re-formatted — a leading zero would change the MAC). Every `v1` candidate is checked
/// with a constant-time comparison ([`Mac::verify_slice`]); non-hex or wrong-length candidates simply
/// never match. `now_unix` is injected for testability.
pub fn verify_signature(
    secret: &str,
    header: &str,
    body: &[u8],
    now_unix: i64,
) -> Result<(), SignatureError> {
    let mut timestamp_raw: Option<&str> = None;
    let mut candidates: Vec<Vec<u8>> = Vec::new();
    for element in header.split(',') {
        match element.trim().split_once('=') {
            Some(("t", v)) => timestamp_raw = Some(v),
            // `v0` (test-mode scheme) and any future/unknown schemes are ignored, per Stripe docs.
            Some(("v1", v)) => {
                if let Ok(bytes) = hex::decode(v) {
                    candidates.push(bytes);
                }
            }
            _ => {}
        }
    }

    let timestamp_raw = timestamp_raw.ok_or(SignatureError::MissingTimestamp)?;
    let timestamp: i64 =
        timestamp_raw.parse().map_err(|_| SignatureError::MissingTimestamp)?;
    if candidates.is_empty() {
        return Err(SignatureError::MissingSignature);
    }
    if (now_unix - timestamp).abs() > SIGNATURE_TOLERANCE_SECS {
        return Err(SignatureError::StaleTimestamp { timestamp, now: now_unix });
    }

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts keys of any length");
    mac.update(timestamp_raw.as_bytes());
    mac.update(b".");
    mac.update(body);

    // `verify_slice` is the constant-time comparison (subtle::ConstantTimeEq under the hood).
    if candidates.into_iter().any(|candidate| mac.clone().verify_slice(&candidate).is_ok()) {
        Ok(())
    } else {
        Err(SignatureError::NoMatchingSignature)
    }
}

// ---------------------------------------------------------------------------------------------
// Wire types — the Stripe subset this ACL reads (unknown fields are ignored by serde)
// ---------------------------------------------------------------------------------------------

/// A Stripe webhook delivery envelope (the subset we read).
#[derive(Debug, Clone, Deserialize)]
pub struct StripeEvent {
    /// Globally unique Stripe event id (`evt_…`) — OUR idempotency key.
    pub id: String,
    /// Event type, e.g. `payment_intent.succeeded`.
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: StripeEventData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StripeEventData {
    /// The affected API object, shape depending on `type` — kept raw and re-read per event type.
    pub object: serde_json::Value,
}

/// `payment_intent` object subset (for `payment_intent.succeeded` / `.payment_failed`).
#[derive(Debug, Clone, Deserialize)]
struct StripePaymentIntent {
    id: String,
    amount: i64,
    amount_received: Option<i64>,
    currency: String,
    #[serde(default)]
    metadata: std::collections::HashMap<String, String>,
    last_payment_error: Option<StripeApiError>,
}

#[derive(Debug, Clone, Deserialize)]
struct StripeApiError {
    code: Option<String>,
    message: Option<String>,
}

/// `charge` object subset (for `charge.refunded`).
#[derive(Debug, Clone, Deserialize)]
struct StripeCharge {
    payment_intent: Option<String>,
    #[serde(default)]
    metadata: std::collections::HashMap<String, String>,
    refunds: Option<StripeRefundList>,
}

#[derive(Debug, Clone, Deserialize)]
struct StripeRefundList {
    #[serde(default)]
    data: Vec<StripeRefund>,
}

#[derive(Debug, Clone, Deserialize)]
struct StripeRefund {
    id: String,
    amount: i64,
    currency: String,
    reason: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Mapping — the actual Anti-Corruption boundary
// ---------------------------------------------------------------------------------------------

/// Result of translating one verified Stripe event.
#[derive(Debug, Clone, PartialEq)]
pub enum StripeMapOutcome {
    /// One of the three payment facts, ready to append.
    Mapped(DomainEvent),
    /// An event type this ACL does not consume — acknowledged, nothing recorded.
    Ignored,
}

/// Stripe minor units + lowercase ISO code → the domain `Money` value object. The ONLY place the
/// Stripe money shape is allowed to exist.
fn to_money(amount_minor_units: i64, currency: &str) -> Money {
    Money { amount_cents: MoneyCents(amount_minor_units), currency: CurrencyCode(currency.to_uppercase()) }
}

/// Read one of OUR uuids back from the object's `metadata` (set at PaymentIntent creation).
fn metadata_uuid(
    metadata: &std::collections::HashMap<String, String>,
    key: &str,
) -> Result<uuid::Uuid, String> {
    let raw = metadata.get(key).ok_or_else(|| format!("metadata key '{key}' missing"))?;
    uuid::Uuid::parse_str(raw).map_err(|e| format!("metadata key '{key}' is not a uuid: {e}"))
}

fn metadata_uuid_opt(
    metadata: &std::collections::HashMap<String, String>,
    key: &str,
) -> Result<Option<uuid::Uuid>, String> {
    match metadata.get(key) {
        None => Ok(None),
        Some(raw) => uuid::Uuid::parse_str(raw)
            .map(Some)
            .map_err(|e| format!("metadata key '{key}' is not a uuid: {e}")),
    }
}

/// Translate a (signature-verified) Stripe event into the domain fact it reports. `Err` = an event
/// type we DO consume whose payload cannot be mapped (missing/invalid metadata, no refund…) — the
/// caller logs it and acknowledges the delivery (a retry would not fix the payload).
pub fn map_stripe_event(event: &StripeEvent) -> Result<StripeMapOutcome, String> {
    match event.event_type.as_str() {
        "payment_intent.succeeded" => {
            let pi: StripePaymentIntent = parse_object(event)?;
            let restaurant_id = metadata_uuid(&pi.metadata, "restaurantId")
                .map_err(|e| format!("{}: {e}", event.event_type))?;
            // Null when captured before the Order aggregate is created in the saga (events.yaml).
            let order_id = metadata_uuid_opt(&pi.metadata, "orderId")
                .map_err(|e| format!("{}: {e}", event.event_type))?;
            // `amount_received` is what was actually captured; fall back to the intent amount.
            let captured = pi.amount_received.filter(|a| *a > 0).unwrap_or(pi.amount);
            Ok(StripeMapOutcome::Mapped(DomainEvent::PaymentCaptured(PaymentCaptured {
                payment_intent_id: PaymentIntentId(pi.id.clone()),
                order_id: order_id.map(OrderId),
                restaurant_id: RestaurantId(restaurant_id),
                amount: to_money(captured, &pi.currency),
            })))
        }
        "payment_intent.payment_failed" => {
            let pi: StripePaymentIntent = parse_object(event)?;
            let restaurant_id = metadata_uuid(&pi.metadata, "restaurantId")
                .map_err(|e| format!("{}: {e}", event.event_type))?;
            let reason = pi
                .last_payment_error
                .and_then(|e| e.message.or(e.code))
                .unwrap_or_else(|| "payment failed (no error detail reported by Stripe)".to_string());
            Ok(StripeMapOutcome::Mapped(DomainEvent::PaymentFailed(PaymentFailed {
                payment_intent_id: PaymentIntentId(pi.id),
                restaurant_id: RestaurantId(restaurant_id),
                // events.yaml caps `reason` at 500 chars — truncate on a char boundary.
                reason: truncate_chars(&reason, 500),
            })))
        }
        "charge.refunded" => {
            let charge: StripeCharge = parse_object(event)?;
            let payment_intent_id = charge
                .payment_intent
                .clone()
                .ok_or("charge.refunded: charge has no payment_intent")?;
            // PaymentRefunded requires both ids (events.yaml `required`).
            let restaurant_id = metadata_uuid(&charge.metadata, "restaurantId")
                .map_err(|e| format!("charge.refunded: {e}"))?;
            let order_id = metadata_uuid(&charge.metadata, "orderId")
                .map_err(|e| format!("charge.refunded: {e}"))?;
            // `refunds.data` is most-recent-first; the newest refund is the fact THIS delivery
            // reports (an earlier partial refund had its own event id and was recorded then).
            let refund = charge
                .refunds
                .as_ref()
                .and_then(|list| list.data.first())
                .ok_or("charge.refunded: charge carries no refund entry")?;
            Ok(StripeMapOutcome::Mapped(DomainEvent::PaymentRefunded(PaymentRefunded {
                refund_id: RefundId(refund.id.clone()),
                payment_intent_id: PaymentIntentId(payment_intent_id),
                order_id: OrderId(order_id),
                restaurant_id: RestaurantId(restaurant_id),
                amount: to_money(refund.amount, &refund.currency),
                reason: refund.reason.clone().map(|r| truncate_chars(&r, 500)),
            })))
        }
        _ => Ok(StripeMapOutcome::Ignored),
    }
}

fn parse_object<T: serde::de::DeserializeOwned>(event: &StripeEvent) -> Result<T, String> {
    serde_json::from_value(event.data.object.clone())
        .map_err(|e| format!("{}: unparsable data.object: {e}", event.event_type))
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

// ---------------------------------------------------------------------------------------------
// Ingestor — record the fact, idempotently
// ---------------------------------------------------------------------------------------------

/// What the ingestor did with one verified delivery (all four are ACKed with 2xx by the endpoint).
#[derive(Debug, Clone, PartialEq)]
pub enum StripeIngestOutcome {
    /// The fact was appended to `domain_events`.
    Recorded { event_type: String },
    /// This Stripe event id was already recorded — redelivery absorbed as a no-op.
    Duplicate,
    /// An event type this ACL does not consume.
    Ignored { event_type: String },
    /// A consumed event type whose payload could not be mapped (logged; retrying would not help).
    Unmappable { reason: String },
}

/// Records Stripe-reported payment facts through the ordinary event-store append port. Generic over
/// the port (not the Pg adapter) so the idempotency behaviour is unit-testable in memory.
pub struct StripeWebhookIngestor {
    store: Arc<dyn EventStore>,
}

impl StripeWebhookIngestor {
    pub fn new(store: Arc<dyn EventStore>) -> Self {
        Self { store }
    }

    /// Map + append one verified delivery. Only infrastructure failures (DB unreachable) surface as
    /// `Err` — the endpoint answers 5xx and Stripe retries; everything else is a definitive outcome.
    pub async fn ingest(&self, event: &StripeEvent) -> Result<StripeIngestOutcome, DomainError> {
        let domain_event = match map_stripe_event(event) {
            Ok(StripeMapOutcome::Mapped(e)) => e,
            Ok(StripeMapOutcome::Ignored) => {
                return Ok(StripeIngestOutcome::Ignored { event_type: event.event_type.clone() })
            }
            Err(reason) => return Ok(StripeIngestOutcome::Unmappable { reason }),
        };

        let actor = Actor {
            user_id: stripe_system_user_id(),
            user_type: EXTERNAL_USER_TYPE,
            correlation_id: stripe_correlation_id(&event.id),
            cause_id: None,
        };

        // The adapter is a stateless translator (ADR-20260719-193500): the fact is delivered to the
        // Payment AGGREGATE, which owns it — dedup is the actor's fold ("already recorded"), not an
        // adapter envelope. No `StripeEvent-{id}` streams, nothing synthetic in the log.
        match application::payments::record_inbound_payment_event(self.store.as_ref(), domain_event, &actor).await {
            Ok(application::payments::RecordOutcome::Recorded) => {
                Ok(StripeIngestOutcome::Recorded { event_type: event.event_type.clone() })
            }
            Ok(application::payments::RecordOutcome::AlreadyRecorded) => Ok(StripeIngestOutcome::Duplicate),
            Err(e) if is_version_conflict(&e) => Ok(StripeIngestOutcome::Duplicate),
            Err(e) => Err(e),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use application::ports::version_conflict;

    const SECRET: &str = "whsec_test_secret";

    /// Build a valid `Stripe-Signature` header for `body` at `t` — the same construction Stripe uses.
    fn sign(secret: &str, t: i64, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{t}.").as_bytes());
        mac.update(body);
        format!("t={t},v1={}", hex::encode(mac.finalize().into_bytes()))
    }

    // ----- signature verification -----

    #[test]
    fn valid_signature_passes() {
        let body = br#"{"id":"evt_1","type":"payment_intent.succeeded"}"#;
        let now = 1_760_000_000;
        let header = sign(SECRET, now, body);
        assert_eq!(verify_signature(SECRET, &header, body, now), Ok(()));
    }

    #[test]
    fn valid_signature_passes_among_multiple_v1_candidates() {
        let body = b"payload";
        let now = 1_760_000_000;
        let good = sign(SECRET, now, body);
        // Prepend a bogus v1 (Stripe sends several during secret rolls) — one match must suffice.
        let header = format!("t={now},v1={},{}", "ab".repeat(32), &good[good.find("v1=").unwrap()..]);
        assert_eq!(verify_signature(SECRET, &header, body, now), Ok(()));
    }

    #[test]
    fn tampered_body_fails() {
        let now = 1_760_000_000;
        let header = sign(SECRET, now, b"original body");
        assert_eq!(
            verify_signature(SECRET, &header, b"tampered body", now),
            Err(SignatureError::NoMatchingSignature)
        );
    }

    #[test]
    fn wrong_secret_fails() {
        let body = b"payload";
        let now = 1_760_000_000;
        let header = sign("whsec_other_secret", now, body);
        assert_eq!(
            verify_signature(SECRET, &header, body, now),
            Err(SignatureError::NoMatchingSignature)
        );
    }

    #[test]
    fn stale_timestamp_fails_beyond_replay_window() {
        let body = b"payload";
        let t = 1_760_000_000;
        let header = sign(SECRET, t, body);
        // 301s later: outside the window even though the MAC itself is correct.
        assert_eq!(
            verify_signature(SECRET, &header, body, t + SIGNATURE_TOLERANCE_SECS + 1),
            Err(SignatureError::StaleTimestamp { timestamp: t, now: t + 301 })
        );
        // Future-dated beyond the window is equally rejected (|now - t|).
        assert!(matches!(
            verify_signature(SECRET, &header, body, t - SIGNATURE_TOLERANCE_SECS - 1),
            Err(SignatureError::StaleTimestamp { .. })
        ));
        // Exactly at the tolerance edge still passes.
        assert_eq!(verify_signature(SECRET, &header, body, t + SIGNATURE_TOLERANCE_SECS), Ok(()));
    }

    #[test]
    fn missing_parts_fail() {
        let body = b"payload";
        let now = 1_760_000_000;
        assert_eq!(
            verify_signature(SECRET, &format!("v1={}", "ab".repeat(32)), body, now),
            Err(SignatureError::MissingTimestamp)
        );
        assert_eq!(
            verify_signature(SECRET, &format!("t={now}"), body, now),
            Err(SignatureError::MissingSignature)
        );
        // A non-hex v1 can never match.
        assert_eq!(
            verify_signature(SECRET, &format!("t={now},v1=not-hex"), body, now),
            Err(SignatureError::MissingSignature)
        );
    }

    // ----- event mapping -----

    const RESTAURANT_ID: &str = "11111111-1111-4111-8111-111111111111";
    const ORDER_ID: &str = "22222222-2222-4222-8222-222222222222";

    fn event_from_json(json: serde_json::Value) -> StripeEvent {
        serde_json::from_value(json).expect("valid StripeEvent json")
    }

    fn sample_succeeded() -> StripeEvent {
        event_from_json(serde_json::json!({
            "id": "evt_1PXsucceeded",
            "object": "event",
            "api_version": "2024-06-20",
            "type": "payment_intent.succeeded",
            "data": {
                "object": {
                    "id": "pi_3NabcSample",
                    "object": "payment_intent",
                    "amount": 2350,
                    "amount_received": 2350,
                    "currency": "eur",
                    "status": "succeeded",
                    "metadata": { "restaurantId": RESTAURANT_ID, "orderId": ORDER_ID }
                }
            }
        }))
    }

    #[test]
    fn payment_intent_succeeded_maps_to_payment_captured() {
        let outcome = map_stripe_event(&sample_succeeded()).unwrap();
        let StripeMapOutcome::Mapped(DomainEvent::PaymentCaptured(captured)) = outcome else {
            panic!("expected Mapped(PaymentCaptured), got {outcome:?}");
        };
        assert_eq!(captured.payment_intent_id, PaymentIntentId("pi_3NabcSample".into()));
        assert_eq!(captured.order_id, Some(OrderId(uuid::Uuid::parse_str(ORDER_ID).unwrap())));
        assert_eq!(
            captured.restaurant_id,
            RestaurantId(uuid::Uuid::parse_str(RESTAURANT_ID).unwrap())
        );
        // Stripe minor units + lowercase code → Money { amountCents, uppercased ISO currency }.
        assert_eq!(captured.amount, Money { amount_cents: MoneyCents(2350), currency: CurrencyCode("EUR".into()) });
    }

    #[test]
    fn payment_intent_failed_maps_to_payment_failed() {
        let event = event_from_json(serde_json::json!({
            "id": "evt_1PXfailed",
            "type": "payment_intent.payment_failed",
            "data": { "object": {
                "id": "pi_3Nfailed",
                "amount": 900,
                "currency": "eur",
                "metadata": { "restaurantId": RESTAURANT_ID },
                "last_payment_error": { "code": "card_declined", "message": "Your card was declined." }
            } }
        }));
        let StripeMapOutcome::Mapped(DomainEvent::PaymentFailed(failed)) =
            map_stripe_event(&event).unwrap()
        else {
            panic!("expected PaymentFailed");
        };
        assert_eq!(failed.payment_intent_id, PaymentIntentId("pi_3Nfailed".into()));
        assert_eq!(failed.reason, "Your card was declined.");
    }

    #[test]
    fn charge_refunded_maps_to_payment_refunded_from_latest_refund() {
        let event = event_from_json(serde_json::json!({
            "id": "evt_1PXrefunded",
            "type": "charge.refunded",
            "data": { "object": {
                "id": "ch_3Nabc",
                "object": "charge",
                "payment_intent": "pi_3NabcSample",
                "amount_refunded": 2350,
                "currency": "eur",
                "metadata": { "restaurantId": RESTAURANT_ID, "orderId": ORDER_ID },
                "refunds": { "object": "list", "data": [
                    { "id": "re_3Nlatest", "amount": 1000, "currency": "eur", "reason": "requested_by_customer" },
                    { "id": "re_3Nolder", "amount": 1350, "currency": "eur", "reason": null }
                ] }
            } }
        }));
        let StripeMapOutcome::Mapped(DomainEvent::PaymentRefunded(refunded)) =
            map_stripe_event(&event).unwrap()
        else {
            panic!("expected PaymentRefunded");
        };
        assert_eq!(refunded.refund_id, RefundId("re_3Nlatest".into()));
        assert_eq!(refunded.payment_intent_id, PaymentIntentId("pi_3NabcSample".into()));
        assert_eq!(refunded.order_id, OrderId(uuid::Uuid::parse_str(ORDER_ID).unwrap()));
        assert_eq!(refunded.amount, Money { amount_cents: MoneyCents(1000), currency: CurrencyCode("EUR".into()) });
        assert_eq!(refunded.reason.as_deref(), Some("requested_by_customer"));
    }

    #[test]
    fn unconsumed_event_type_is_ignored() {
        let event = event_from_json(serde_json::json!({
            "id": "evt_other", "type": "customer.created", "data": { "object": {} }
        }));
        assert_eq!(map_stripe_event(&event).unwrap(), StripeMapOutcome::Ignored);
    }

    #[test]
    fn missing_restaurant_metadata_is_unmappable() {
        let event = event_from_json(serde_json::json!({
            "id": "evt_nometa",
            "type": "payment_intent.succeeded",
            "data": { "object": { "id": "pi_x", "amount": 100, "currency": "eur", "metadata": {} } }
        }));
        let err = map_stripe_event(&event).unwrap_err();
        assert!(err.contains("restaurantId"), "unexpected error: {err}");
    }

    // ----- idempotent ingestion (in-memory event store) -----

    /// Minimal in-memory port double reproducing the UNIQUE(stream_name, version) guard.
    struct InMemoryEventStore {
        appended: std::sync::Mutex<std::collections::HashMap<String, Vec<DomainEvent>>>,
    }

    #[async_trait::async_trait]
    impl EventStore for InMemoryEventStore {
        async fn append(
            &self,
            stream_name: &str,
            expected_version: i64,
            events: &[DomainEvent],
            _actor: &Actor,
        ) -> Result<i64, DomainError> {
            let mut streams = self.appended.lock().unwrap();
            let stream = streams.entry(stream_name.to_string()).or_default();
            if stream.len() as i64 != expected_version {
                return Err(version_conflict(stream_name, expected_version));
            }
            stream.extend_from_slice(events);
            Ok(stream.len() as i64)
        }

        async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
            let streams = self.appended.lock().unwrap();
            let events = streams.get(stream_name).cloned().unwrap_or_default();
            let version = events.len() as i64;
            Ok((events, version))
        }
    }

    #[tokio::test]
    async fn redelivered_webhook_is_a_no_op() {
        let store = Arc::new(InMemoryEventStore { appended: Default::default() });
        let ingestor = StripeWebhookIngestor::new(store.clone());
        let event = sample_succeeded();

        let first = ingestor.ingest(&event).await.unwrap();
        assert_eq!(
            first,
            StripeIngestOutcome::Recorded { event_type: "payment_intent.succeeded".into() }
        );
        // Stripe redelivers the SAME event → absorbed by the Payment aggregate's dedup, nothing
        // appended twice. The fact lands on the Payment stream (ADR-20260719-193500) — no
        // StripeEvent-% envelope streams.
        let second = ingestor.ingest(&event).await.unwrap();
        assert_eq!(second, StripeIngestOutcome::Duplicate);

        let (events, version) = store.load("Payment-pi_3NabcSample").await.unwrap();
        assert_eq!(version, 1);
        assert!(matches!(events.as_slice(), [DomainEvent::PaymentCaptured(_)]));
    }
}
