//! Avelo37 webhook Anti-Corruption Layer — INBOUND integration events (CLAUDE.md "Commands vs
//! inbound (integration) events"). The delivery partner PUSHes signed webhooks reporting facts that
//! have ALREADY happened on its side; there is nothing to validate and nothing to reject, so no
//! command is involved: the ACL translates the partner wire shape into the already-modelled domain
//! events and records them as facts:
//!
//! - `delivery.accepted`       → `DeliveryAcceptedByPartner` (courier assigned)
//! - `delivery.declined`       → `DeliveryRejectedByPartner` (the saga re-offers, bounded — ADR-20260720-004556)
//! - `delivery.status_updated` → `DeliveryStatusUpdated` (progress up to DELIVERED/FAILED)
//!
//! Any other event type is acknowledged and ignored.
//!
//! # Boundary translation (partner idioms stay OUT of the domain)
//!
//! - The partner's snake_case status strings (`picked_up`, `out_for_delivery`, …) are mapped to the
//!   `DeliveryStatus` enum HERE ([`map_partner_status`]) — raw strings never cross the boundary
//!   (specs/integrations/avelo37.md §4).
//! - `job_reference` is OUR `DeliveryJobId`, echoed back by the partner: the outbound `offer_job`
//!   call (outbound.rs) sends it when creating the partner-side delivery — the exact Stripe
//!   `metadata` read-back pattern. A verified event without a parsable `job_reference` is
//!   unmappable — logged and acknowledged, never guessed.
//! - The partner-side delivery id (`dlv_…`) maps to the `partnerRef` `ExternalReference` scalar —
//!   the idempotent key of partner-reported facts (specs/integrations/avelo37.md §3).
//!
//! # Signature verification ([`verify_signature`])
//!
//! `Avelo37-Signature: t=<unix>,v1=<hexHmac>` — the Stripe scheme adopted verbatim as OUR contract
//! for the partner (timestamped HMAC beats a bare body HMAC: it bounds replay): the expected MAC is
//! `HMAC-SHA256(key = webhook secret, msg = "<t>.<raw body bytes>")`, compared in CONSTANT TIME,
//! timestamps outside ±[`SIGNATURE_TOLERANCE_SECS`] rejected. The HTTP endpoint fails CLOSED when
//! `AVELO37_WEBHOOK_SECRET` is unset.
//!
//! # Durable inbox + idempotency (ADR-20260720-015400: inbound event sourcing)
//!
//! Ingestion is verify → mirror the VERBATIM body into the adapter-owned `external_avelo37_events`
//! staging table → translate → stage the ADAPTED business event into `inbound_events` → ACK. The
//! domain append happens later, in the `InboundEventsDrainWorker`, through the normal write path
//! (`application::deliveries::record_inbound_delivery_event`) — where the DeliveryJob aggregate's
//! fold + declared lifecycle stay the authoritative dedupe/guard. Delivery-level dedupe is the
//! staging `(source, external_id)` unique: a redelivery is absorbed as
//! [`Avelo37IngestOutcome::Duplicate`].

use std::sync::Arc;

use domain::generated::entities::Courier;
use domain::generated::events::{
    DeliveryAcceptedByPartner, DeliveryRejectedByPartner, DeliveryStatusUpdated, DomainEvent,
};
use domain::generated::scalars::{DeliveryJobId, DeliveryStatus, ExternalReference, PhoneNumber};
use domain::shared::errors::DomainError;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Env var holding the endpoint's signing secret (agreed with the partner). Read by the HTTP
/// shell, which fails CLOSED (503) when it is unset.
pub const AVELO37_WEBHOOK_SECRET_ENV: &str = "AVELO37_WEBHOOK_SECRET";

/// Replay window: reject a signature whose `t=` is further than this from now (same tolerance as
/// the Stripe seam).
pub const SIGNATURE_TOLERANCE_SECS: i64 = 300;

// ---------------------------------------------------------------------------------------------
// Envelope identity (ADR-0041) — deterministic, like the Stripe/SIRENE ACLs'
// ---------------------------------------------------------------------------------------------

/// Fixed UUIDv5 namespace for every id this ACL derives. NEVER change it: derived ids are stable
/// across deliveries and deployments.
fn avelo37_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/integrations/avelo37")
}

/// Fixed system user id stamping the event envelope (`domain_events.user_id`) for facts the
/// partner reports.
pub fn avelo37_system_user_id() -> uuid::Uuid {
    uuid::Uuid::new_v5(&avelo37_namespace(), b"system:avelo37-webhook")
}

/// Deterministic envelope `correlation_id` for a partner event id — every fact recorded from the
/// same delivery (and any redelivery attempt) correlates to the same value.
pub fn avelo37_correlation_id(avelo37_event_id: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&avelo37_namespace(), avelo37_event_id.as_bytes())
}

// ---------------------------------------------------------------------------------------------
// Signature verification (the Stripe scheme, adopted as our partner contract)
// ---------------------------------------------------------------------------------------------

/// Why an `Avelo37-Signature` header was rejected.
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
            Self::MissingTimestamp => write!(f, "no t= timestamp in Avelo37-Signature header"),
            Self::MissingSignature => write!(f, "no v1= signature in Avelo37-Signature header"),
            Self::StaleTimestamp { timestamp, now } => write!(
                f,
                "timestamp {timestamp} outside the {SIGNATURE_TOLERANCE_SECS}s replay window (now {now})"
            ),
            Self::NoMatchingSignature => write!(f, "no v1 signature matches the payload"),
        }
    }
}

/// Verify an `Avelo37-Signature` header against the RAW request body.
///
/// The signed payload is `"<t>.<body>"` where `<t>` is the timestamp EXACTLY as it appeared in the
/// header. Every `v1` candidate is checked with a constant-time comparison
/// ([`Mac::verify_slice`]); non-hex candidates simply never match. `now_unix` is injected for
/// testability.
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
            // Unknown/future schemes are ignored (secret-roll compatibility, like Stripe's v0).
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

    if candidates.into_iter().any(|candidate| mac.clone().verify_slice(&candidate).is_ok()) {
        Ok(())
    } else {
        Err(SignatureError::NoMatchingSignature)
    }
}

// ---------------------------------------------------------------------------------------------
// Wire types — the Avelo37 subset this ACL reads (unknown fields are ignored by serde)
// ---------------------------------------------------------------------------------------------

/// An Avelo37 webhook delivery envelope (the subset we read).
#[derive(Debug, Clone, Deserialize)]
pub struct Avelo37Event {
    /// Globally unique partner event id (`evt_…`) — OUR idempotency key.
    pub id: String,
    /// Event type, e.g. `delivery.accepted`.
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Avelo37EventData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Avelo37EventData {
    /// The affected partner delivery, shape depending on `type` — kept raw and re-read per type.
    pub delivery: serde_json::Value,
}

/// Partner `delivery` object subset.
#[derive(Debug, Clone, Deserialize)]
struct Avelo37Delivery {
    /// Partner-side delivery id (`dlv_…`) → `partnerRef`.
    id: Option<String>,
    /// OUR `DeliveryJobId`, echoed back from the outbound offer (outbound.rs).
    job_reference: Option<String>,
    courier: Option<Avelo37Courier>,
    status: Option<String>,
    reason: Option<String>,
    note: Option<String>,
    eta_pickup_at: Option<String>,
    eta_dropoff_at: Option<String>,
    occurred_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Avelo37Courier {
    name: String,
    phone: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Mapping — the actual Anti-Corruption boundary
// ---------------------------------------------------------------------------------------------

/// Result of translating one verified Avelo37 event.
#[derive(Debug, Clone, PartialEq)]
pub enum Avelo37MapOutcome {
    /// One of the three delivery-partner facts, ready to stage.
    Mapped(DomainEvent),
    /// An event type this ACL does not consume — acknowledged, nothing recorded.
    Ignored,
}

/// The partner's snake_case status vocabulary → the domain `DeliveryStatus`. The ONLY place the
/// partner status strings are allowed to exist; an unknown value is unmappable (never guessed).
fn map_partner_status(raw: &str) -> Result<DeliveryStatus, String> {
    match raw {
        "assigned" => Ok(DeliveryStatus::ASSIGNED),
        "picked_up" => Ok(DeliveryStatus::PICKED_UP),
        "out_for_delivery" => Ok(DeliveryStatus::OUT_FOR_DELIVERY),
        "delivered" => Ok(DeliveryStatus::DELIVERED),
        "failed" => Ok(DeliveryStatus::FAILED),
        "cancelled" => Ok(DeliveryStatus::CANCELLED),
        other => Err(format!("unknown partner delivery status '{other}'")),
    }
}

fn job_reference(delivery: &Avelo37Delivery, context: &str) -> Result<DeliveryJobId, String> {
    let raw = delivery
        .job_reference
        .as_deref()
        .ok_or_else(|| format!("{context}: delivery carries no job_reference"))?;
    uuid::Uuid::parse_str(raw)
        .map(DeliveryJobId)
        .map_err(|e| format!("{context}: job_reference is not a uuid: {e}"))
}

fn partner_ref(delivery: &Avelo37Delivery) -> Option<ExternalReference> {
    delivery.id.clone().map(ExternalReference)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Translate a (signature-verified) Avelo37 event into the domain fact it reports. `Err` = an
/// event type we DO consume whose payload cannot be mapped (missing job_reference/courier, unknown
/// status…) — the caller logs it and acknowledges the delivery (a retry would not fix the payload).
pub fn map_avelo37_event(event: &Avelo37Event) -> Result<Avelo37MapOutcome, String> {
    let parse = |context: &str| -> Result<Avelo37Delivery, String> {
        serde_json::from_value(event.data.delivery.clone())
            .map_err(|e| format!("{context}: unparsable data.delivery: {e}"))
    };
    match event.event_type.as_str() {
        "delivery.accepted" => {
            let delivery = parse("delivery.accepted")?;
            let delivery_job_id = job_reference(&delivery, "delivery.accepted")?;
            let partner_ref = partner_ref(&delivery)
                .ok_or("delivery.accepted: delivery carries no partner id")?;
            let courier =
                delivery.courier.as_ref().ok_or("delivery.accepted: delivery carries no courier")?;
            Ok(Avelo37MapOutcome::Mapped(DomainEvent::DeliveryAcceptedByPartner(
                DeliveryAcceptedByPartner {
                    delivery_job_id,
                    partner_ref,
                    // entities.yaml#/Courier: a PARTNER courier has no riderId (not a Captain rider).
                    courier: Courier {
                        display_name: truncate_chars(&courier.name, 140),
                        phone: courier.phone.clone().map(PhoneNumber),
                        rider_id: None,
                    },
                    estimated_pickup_at: delivery.eta_pickup_at.clone(),
                    estimated_dropoff_at: delivery.eta_dropoff_at.clone(),
                },
            )))
        }
        "delivery.declined" => {
            let delivery = parse("delivery.declined")?;
            let delivery_job_id = job_reference(&delivery, "delivery.declined")?;
            Ok(Avelo37MapOutcome::Mapped(DomainEvent::DeliveryRejectedByPartner(
                DeliveryRejectedByPartner {
                    delivery_job_id,
                    partner_ref: partner_ref(&delivery),
                    // events.yaml caps `reason` at 500 chars — truncate on a char boundary.
                    reason: delivery.reason.as_deref().map(|r| truncate_chars(r, 500)),
                },
            )))
        }
        "delivery.status_updated" => {
            let delivery = parse("delivery.status_updated")?;
            let delivery_job_id = job_reference(&delivery, "delivery.status_updated")?;
            let raw_status = delivery
                .status
                .as_deref()
                .ok_or("delivery.status_updated: delivery carries no status")?;
            let status = map_partner_status(raw_status)
                .map_err(|e| format!("delivery.status_updated: {e}"))?;
            Ok(Avelo37MapOutcome::Mapped(DomainEvent::DeliveryStatusUpdated(
                DeliveryStatusUpdated {
                    delivery_job_id,
                    partner_ref: partner_ref(&delivery),
                    status,
                    occurred_at: delivery.occurred_at.clone(),
                    note: delivery.note.as_deref().map(|n| truncate_chars(n, 500)),
                },
            )))
        }
        _ => Ok(Avelo37MapOutcome::Ignored),
    }
}

// ---------------------------------------------------------------------------------------------
// Ingestor — mirror the raw delivery, stage the adapted fact (ADR-20260720-015400)
// ---------------------------------------------------------------------------------------------

/// Adapter-owned raw mirror (`external_avelo37_events`,
/// `specs/database/tables/integration_staging.yaml`): the verified delivery is UPSERTed verbatim
/// BEFORE interpretation, so redelivery dedupes on the partner event id and replay/backfill never
/// needs the partner to resend. Trait so the ingest flow is unit-testable in memory;
/// [`PgRawAvelo37Events`](crate::raw::PgRawAvelo37Events) is the Postgres impl.
#[async_trait::async_trait]
pub trait RawAvelo37Events: Send + Sync {
    /// UPSERT the verified raw event; `Ok(true)` = newly mirrored, `Ok(false)` = already known.
    async fn upsert(
        &self,
        avelo37_event_id: &str,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<bool, DomainError>;

    /// Stamp the translation high-water mark once the delivery has been interpreted (staged into
    /// `inbound_events`, or definitively Ignored/Unmappable).
    async fn mark_processed(&self, avelo37_event_id: &str) -> Result<(), DomainError>;
}

/// What the ingestor did with one verified delivery (all four are ACKed with 2xx by the endpoint).
#[derive(Debug, Clone, PartialEq)]
pub enum Avelo37IngestOutcome {
    /// The adapted fact was staged into `inbound_events` for the drain worker to deliver.
    Recorded { event_type: String },
    /// This partner event id was already staged — redelivery absorbed as a no-op.
    Duplicate,
    /// An event type this ACL does not consume.
    Ignored { event_type: String },
    /// A consumed event type whose payload could not be mapped (logged; retrying would not help).
    Unmappable { reason: String },
}

/// Ingests one verified delivery: raw mirror UPSERT → ACL translation → `inbound_events` staging
/// (ADR-20260720-015400). The domain append happens later, in the `InboundEventsDrainWorker`,
/// through the normal write path — the ingestor never touches `domain_events`. Generic over the
/// ports so the flow is unit-testable in memory.
pub struct Avelo37WebhookIngestor {
    raw: Arc<dyn RawAvelo37Events>,
    inbox: Arc<dyn application::journal::InboundEvents>,
    /// Optional post-staging nudge (the composition root wires it to the drain worker's `run_once`)
    /// so delivery lag is near zero; the worker's poll loop is the safety net.
    on_staged: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Avelo37WebhookIngestor {
    pub fn new(
        raw: Arc<dyn RawAvelo37Events>,
        inbox: Arc<dyn application::journal::InboundEvents>,
    ) -> Self {
        Self { raw, inbox, on_staged: None }
    }

    /// Wire the post-staging nudge (spawns the drain pass; must not block).
    pub fn with_nudge(mut self, nudge: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.on_staged = Some(nudge);
        self
    }

    /// Mirror + translate + stage one verified delivery. `raw_body` is the VERBATIM parsed request
    /// body (never a re-serialization of the typed subset — the mirror keeps every field). Only
    /// infrastructure failures (DB unreachable) surface as `Err` — the endpoint answers 5xx and
    /// the partner retries; everything else is a definitive outcome. Crash-safe ordering: the raw
    /// mirror lands first; staging dedupes by `(source, external_id)`, so re-running any prefix of
    /// the flow is a no-op.
    pub async fn ingest(
        &self,
        event: &Avelo37Event,
        raw_body: &serde_json::Value,
    ) -> Result<Avelo37IngestOutcome, DomainError> {
        self.raw.upsert(&event.id, &event.event_type, raw_body).await?;

        let domain_event = match map_avelo37_event(event) {
            Ok(Avelo37MapOutcome::Mapped(e)) => e,
            Ok(Avelo37MapOutcome::Ignored) => {
                self.raw.mark_processed(&event.id).await?;
                return Ok(Avelo37IngestOutcome::Ignored { event_type: event.event_type.clone() });
            }
            Err(reason) => {
                self.raw.mark_processed(&event.id).await?;
                return Ok(Avelo37IngestOutcome::Unmappable { reason });
            }
        };

        // Stage the ADAPTED business event (external vocabulary stops here). The tagged serde form
        // (`{"eventType": …, "payload": …}`) is what the drain worker deserializes back.
        let tagged = serde_json::to_value(&domain_event).map_err(|e| {
            DomainError::Repository(format!("adapted event for {} unserializable: {e}", event.id))
        })?;
        let event_type = tagged
            .get("eventType")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown")
            .to_owned();
        let row = application::journal::InboundEventRow {
            inbound_event_id: uuid::Uuid::now_v7(),
            source: "avelo37".into(),
            external_id: event.id.clone(),
            correlation_id: avelo37_correlation_id(&event.id),
            event_type,
            payload: tagged,
            status: domain::generated::scalars::InboundEventStatus::RECEIVED,
            error: None,
            received_at: chrono::Utc::now(),
            delivered_at: None,
        };
        let outcome = match self.inbox.stage(&row).await? {
            application::journal::StageOutcome::Staged => {
                if let Some(nudge) = &self.on_staged {
                    nudge();
                }
                Avelo37IngestOutcome::Recorded { event_type: event.event_type.clone() }
            }
            application::journal::StageOutcome::Duplicate => Avelo37IngestOutcome::Duplicate,
        };
        self.raw.mark_processed(&event.id).await?;
        Ok(outcome)
    }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use application::journal::InboundEvents;
    use domain::shared::errors::DomainError;

    const SECRET: &str = "avwh_test_secret";
    const JOB_ID: &str = "11111111-1111-4111-8111-111111111111";

    /// Build a valid `Avelo37-Signature` header for `body` at `t` — the contract's construction.
    fn sign(secret: &str, t: i64, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{t}.").as_bytes());
        mac.update(body);
        format!("t={t},v1={}", hex::encode(mac.finalize().into_bytes()))
    }

    // ----- signature verification -----

    #[test]
    fn valid_signature_passes_and_tampering_fails() {
        let body = br#"{"id":"evt_av_1","type":"delivery.accepted"}"#;
        let now = 1_760_000_000;
        let header = sign(SECRET, now, body);
        assert_eq!(verify_signature(SECRET, &header, body, now), Ok(()));
        assert_eq!(
            verify_signature(SECRET, &header, b"tampered", now),
            Err(SignatureError::NoMatchingSignature)
        );
        assert_eq!(
            verify_signature("other_secret", &header, body, now),
            Err(SignatureError::NoMatchingSignature)
        );
    }

    #[test]
    fn stale_timestamp_fails_beyond_replay_window() {
        let body = b"payload";
        let t = 1_760_000_000;
        let header = sign(SECRET, t, body);
        assert!(matches!(
            verify_signature(SECRET, &header, body, t + SIGNATURE_TOLERANCE_SECS + 1),
            Err(SignatureError::StaleTimestamp { .. })
        ));
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
    }

    // ----- event mapping -----

    fn event_from_json(json: serde_json::Value) -> Avelo37Event {
        serde_json::from_value(json).expect("valid Avelo37Event json")
    }

    fn sample_accepted() -> Avelo37Event {
        event_from_json(serde_json::json!({
            "id": "evt_av_accepted",
            "type": "delivery.accepted",
            "data": { "delivery": {
                "id": "dlv_77",
                "job_reference": JOB_ID,
                "status": "assigned",
                "courier": { "name": "Léa", "phone": "+33611223344" },
                "eta_pickup_at": "2026-07-21T11:00:00Z",
                "eta_dropoff_at": "2026-07-21T11:25:00Z"
            } }
        }))
    }

    #[test]
    fn delivery_accepted_maps_to_delivery_accepted_by_partner() {
        let Avelo37MapOutcome::Mapped(DomainEvent::DeliveryAcceptedByPartner(accepted)) =
            map_avelo37_event(&sample_accepted()).unwrap()
        else {
            panic!("expected Mapped(DeliveryAcceptedByPartner)");
        };
        assert_eq!(accepted.delivery_job_id, DeliveryJobId(uuid::Uuid::parse_str(JOB_ID).unwrap()));
        assert_eq!(accepted.partner_ref, ExternalReference("dlv_77".into()));
        assert_eq!(accepted.courier.display_name, "Léa");
        assert_eq!(accepted.courier.phone, Some(PhoneNumber("+33611223344".into())));
        assert_eq!(accepted.courier.rider_id, None, "a partner courier is not a Captain rider");
        assert_eq!(accepted.estimated_pickup_at.as_deref(), Some("2026-07-21T11:00:00Z"));
    }

    #[test]
    fn delivery_declined_maps_to_delivery_rejected_by_partner() {
        let event = event_from_json(serde_json::json!({
            "id": "evt_av_declined",
            "type": "delivery.declined",
            "data": { "delivery": {
                "id": "dlv_77", "job_reference": JOB_ID, "reason": "No courier available"
            } }
        }));
        let Avelo37MapOutcome::Mapped(DomainEvent::DeliveryRejectedByPartner(rejected)) =
            map_avelo37_event(&event).unwrap()
        else {
            panic!("expected DeliveryRejectedByPartner");
        };
        assert_eq!(rejected.delivery_job_id, DeliveryJobId(uuid::Uuid::parse_str(JOB_ID).unwrap()));
        assert_eq!(rejected.partner_ref, Some(ExternalReference("dlv_77".into())));
        assert_eq!(rejected.reason.as_deref(), Some("No courier available"));
    }

    #[test]
    fn delivery_status_updated_maps_partner_status_vocabulary() {
        for (raw, expected) in [
            ("picked_up", DeliveryStatus::PICKED_UP),
            ("out_for_delivery", DeliveryStatus::OUT_FOR_DELIVERY),
            ("delivered", DeliveryStatus::DELIVERED),
            ("failed", DeliveryStatus::FAILED),
        ] {
            let event = event_from_json(serde_json::json!({
                "id": "evt_av_status",
                "type": "delivery.status_updated",
                "data": { "delivery": {
                    "id": "dlv_77", "job_reference": JOB_ID, "status": raw,
                    "occurred_at": "2026-07-21T11:10:00Z"
                } }
            }));
            let Avelo37MapOutcome::Mapped(DomainEvent::DeliveryStatusUpdated(updated)) =
                map_avelo37_event(&event).unwrap()
            else {
                panic!("expected DeliveryStatusUpdated for '{raw}'");
            };
            assert_eq!(updated.status, expected, "partner '{raw}'");
            assert_eq!(updated.occurred_at.as_deref(), Some("2026-07-21T11:10:00Z"));
        }
    }

    #[test]
    fn unknown_status_and_missing_job_reference_are_unmappable() {
        let unknown_status = event_from_json(serde_json::json!({
            "id": "evt_bad_status",
            "type": "delivery.status_updated",
            "data": { "delivery": { "id": "dlv_77", "job_reference": JOB_ID, "status": "teleported" } }
        }));
        let err = map_avelo37_event(&unknown_status).unwrap_err();
        assert!(err.contains("teleported"), "unexpected error: {err}");

        let no_job = event_from_json(serde_json::json!({
            "id": "evt_no_job",
            "type": "delivery.accepted",
            "data": { "delivery": { "id": "dlv_77", "courier": { "name": "Léa" } } }
        }));
        let err = map_avelo37_event(&no_job).unwrap_err();
        assert!(err.contains("job_reference"), "unexpected error: {err}");
    }

    #[test]
    fn unconsumed_event_type_is_ignored() {
        let event = event_from_json(serde_json::json!({
            "id": "evt_other", "type": "courier.location_updated", "data": { "delivery": {} }
        }));
        assert_eq!(map_avelo37_event(&event).unwrap(), Avelo37MapOutcome::Ignored);
    }

    // ----- ingest: mirror + stage, idempotently (in-memory port doubles) -----

    /// Minimal in-memory [`RawAvelo37Events`] mirroring the `external_avelo37_events` semantics.
    #[derive(Default)]
    struct MemRawAvelo37Events {
        rows: std::sync::Mutex<std::collections::HashMap<String, (serde_json::Value, bool)>>,
    }

    #[async_trait::async_trait]
    impl RawAvelo37Events for MemRawAvelo37Events {
        async fn upsert(
            &self,
            avelo37_event_id: &str,
            _event_type: &str,
            payload: &serde_json::Value,
        ) -> Result<bool, DomainError> {
            let mut rows = self.rows.lock().unwrap();
            if rows.contains_key(avelo37_event_id) {
                return Ok(false);
            }
            rows.insert(avelo37_event_id.to_string(), (payload.clone(), false));
            Ok(true)
        }

        async fn mark_processed(&self, avelo37_event_id: &str) -> Result<(), DomainError> {
            if let Some(row) = self.rows.lock().unwrap().get_mut(avelo37_event_id) {
                row.1 = true;
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_delivery_is_mirrored_and_staged_for_the_drain() {
        let raw = Arc::new(MemRawAvelo37Events::default());
        let inbox = Arc::new(application::journal::mem::MemInboundEvents::default());
        let ingestor = Avelo37WebhookIngestor::new(raw.clone(), inbox.clone());
        let event = sample_accepted();
        let raw_body = serde_json::json!({ "id": event.id, "verbatim": true });

        let outcome = ingestor.ingest(&event, &raw_body).await.unwrap();
        assert_eq!(
            outcome,
            Avelo37IngestOutcome::Recorded { event_type: "delivery.accepted".into() }
        );
        // The raw mirror holds the VERBATIM body and is marked processed…
        let rows = raw.rows.lock().unwrap();
        let (mirrored, processed) = rows.get(&event.id).expect("mirrored");
        assert_eq!(mirrored["verbatim"], true);
        assert!(processed);
        drop(rows);
        // …and the ADAPTED business event awaits the drain worker (no domain append here).
        let pending = inbox.pending(10).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].source, "avelo37");
        assert_eq!(pending[0].external_id, event.id);
        assert_eq!(pending[0].event_type, "DeliveryAcceptedByPartner");
        assert_eq!(pending[0].correlation_id, avelo37_correlation_id(&event.id));
        let staged: DomainEvent = serde_json::from_value(pending[0].payload.clone()).unwrap();
        assert!(matches!(staged, DomainEvent::DeliveryAcceptedByPartner(_)));
    }

    #[tokio::test]
    async fn redelivered_webhook_is_a_no_op() {
        let raw = Arc::new(MemRawAvelo37Events::default());
        let inbox = Arc::new(application::journal::mem::MemInboundEvents::default());
        let ingestor = Avelo37WebhookIngestor::new(raw, inbox.clone());
        let event = sample_accepted();
        let raw_body = serde_json::json!({ "id": event.id });

        let first = ingestor.ingest(&event, &raw_body).await.unwrap();
        assert_eq!(
            first,
            Avelo37IngestOutcome::Recorded { event_type: "delivery.accepted".into() }
        );
        // The partner redelivers the SAME event → the (source, external_id) staging dedupe absorbs it.
        let second = ingestor.ingest(&event, &raw_body).await.unwrap();
        assert_eq!(second, Avelo37IngestOutcome::Duplicate);
        assert_eq!(inbox.pending(10).await.unwrap().len(), 1, "staged exactly once");
    }

    #[tokio::test]
    async fn ignored_and_unmappable_deliveries_are_mirrored_but_never_staged() {
        let raw = Arc::new(MemRawAvelo37Events::default());
        let inbox = Arc::new(application::journal::mem::MemInboundEvents::default());
        let ingestor = Avelo37WebhookIngestor::new(raw.clone(), inbox.clone());

        let ignored = event_from_json(serde_json::json!({
            "id": "evt_other", "type": "courier.location_updated", "data": { "delivery": {} }
        }));
        let outcome = ingestor.ingest(&ignored, &serde_json::json!({})).await.unwrap();
        assert_eq!(
            outcome,
            Avelo37IngestOutcome::Ignored { event_type: "courier.location_updated".into() }
        );

        let unmappable = event_from_json(serde_json::json!({
            "id": "evt_no_job",
            "type": "delivery.accepted",
            "data": { "delivery": { "id": "dlv_77", "courier": { "name": "Léa" } } }
        }));
        let outcome = ingestor.ingest(&unmappable, &serde_json::json!({})).await.unwrap();
        assert!(matches!(outcome, Avelo37IngestOutcome::Unmappable { .. }));

        // Both are receipts in the mirror (processed — a retry would carry the same payload)…
        let rows = raw.rows.lock().unwrap();
        assert!(rows.get("evt_other").is_some_and(|r| r.1));
        assert!(rows.get("evt_no_job").is_some_and(|r| r.1));
        drop(rows);
        // …but nothing crossed into the domain handoff.
        assert!(inbox.pending(10).await.unwrap().is_empty());
    }
}
