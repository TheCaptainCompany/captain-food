//! Uber Direct webhook Anti-Corruption Layer — INBOUND integration events (CLAUDE.md "Commands vs
//! inbound (integration) events"), the sibling of the Avelo37/CoopCycle ACLs. Uber PUSHes signed
//! webhooks reporting facts that ALREADY happened on its side; there is nothing to validate and
//! nothing to reject, so no command is involved — the ACL translates the Uber wire shape into the
//! already-modelled (partner-generic) domain events and records them as facts:
//!
//! - courier assigned (`event.delivery_status`, status `pickup`) → `DeliveryAcceptedByPartner`
//! - undeliverable before assignment (`canceled`/`returned`, no courier) → `DeliveryRejectedByPartner`
//!   (the saga advances the ranked walk — #60)
//! - every other status transition → `DeliveryStatusUpdated` (progress up to DELIVERED/FAILED)
//!
//! Any other event kind is acknowledged and ignored.
//!
//! # Signature scheme — the Uber DELTA from Avelo37/CoopCycle
//!
//! Unlike the Stripe-style timestamped `t=…,v1=…` scheme those partners adopted, Uber signs with a
//! **plain `X-Uber-Signature` = hex(HMAC-SHA256(webhook_secret, raw_body))** — no timestamp, so no
//! replay window (Uber's contract). Verification is a constant-time compare over the raw body.
//!
//! # Boundary translation, durable inbox
//!
//! Identical in shape to the Avelo37 ACL: raw Uber status strings are mapped to `DeliveryStatus`
//! HERE (never crossing the boundary); `external_id` is OUR `DeliveryJobId` echoed back from the
//! outbound create-delivery; the Uber delivery id maps to `partnerRef`. Ingestion is verify → mirror
//! verbatim into `external_uber_direct_events` → translate → stage the adapted event into
//! `inbound_events` (`source = 'uber_direct'`) → ACK; the domain append happens later in the
//! `InboundEventsDrainWorker` through the normal write path.

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

// ---------------------------------------------------------------------------------------------
// Envelope identity (ADR-0041) — deterministic, like the Avelo37/CoopCycle/Stripe/SIRENE ACLs'
// ---------------------------------------------------------------------------------------------

/// Fixed UUIDv5 namespace for every id this ACL derives. NEVER change it: derived ids are stable
/// across deliveries and deployments.
fn uber_direct_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/integrations/uber_direct")
}

/// Fixed system user id stamping the event envelope (`domain_events.user_id`) for facts Uber reports.
pub fn uber_direct_system_user_id() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uber_direct_namespace(), b"system:uber-direct-webhook")
}

/// Deterministic envelope `correlation_id` for an Uber event — every fact recorded from the same
/// event id (and any redelivery attempt) correlates to the same value.
pub fn uber_direct_correlation_id(event_id: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&uber_direct_namespace(), event_id.as_bytes())
}

// ---------------------------------------------------------------------------------------------
// Signature verification (Uber's plain raw-body HMAC — no timestamp)
// ---------------------------------------------------------------------------------------------

/// Why an `X-Uber-Signature` header was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureError {
    /// The header was not valid hex.
    MalformedSignature,
    /// The HMAC over the raw body did not match the header.
    NoMatchingSignature,
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedSignature => write!(f, "X-Uber-Signature is not valid hex"),
            Self::NoMatchingSignature => write!(f, "X-Uber-Signature does not match the payload"),
        }
    }
}

/// Verify an `X-Uber-Signature` header against the RAW request body: `sig == hex(HMAC-SHA256(secret,
/// body))`, compared in constant time.
pub fn verify_signature(secret: &str, header: &str, body: &[u8]) -> Result<(), SignatureError> {
    let provided = hex::decode(header.trim()).map_err(|_| SignatureError::MalformedSignature)?;
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC-SHA256 accepts keys of any length");
    mac.update(body);
    mac.verify_slice(&provided).map_err(|_| SignatureError::NoMatchingSignature)
}

// ---------------------------------------------------------------------------------------------
// Wire types — the Uber Direct subset this ACL reads (unknown fields are ignored by serde)
// ---------------------------------------------------------------------------------------------

/// An Uber Direct webhook envelope (the subset we read). Uber wraps the delivery in `data` and names
/// the event in `kind` (e.g. `event.delivery_status`), with a stable `event_id`.
#[derive(Debug, Clone, Deserialize)]
pub struct UberEvent {
    /// Provider event id — the idempotency key (globally unique on Uber's side).
    pub event_id: String,
    /// Event kind, e.g. `event.delivery_status` / `event.courier_update`.
    pub kind: String,
    pub data: UberDelivery,
}

/// Uber `delivery` object subset.
#[derive(Debug, Clone, Deserialize)]
pub struct UberDelivery {
    /// Uber-side delivery id → `partnerRef`.
    id: Option<String>,
    /// OUR `DeliveryJobId`, echoed back from the outbound create-delivery (outbound.rs).
    external_id: Option<String>,
    /// Delivery status: `pending` / `pickup` / `pickup_complete` / `dropoff` / `delivered` /
    /// `canceled` / `returned`.
    status: Option<String>,
    courier: Option<UberCourier>,
    #[serde(rename = "undeliverable_reason")]
    undeliverable_reason: Option<String>,
    #[serde(rename = "pickup_eta")]
    pickup_eta: Option<String>,
    #[serde(rename = "dropoff_eta")]
    dropoff_eta: Option<String>,
    updated: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct UberCourier {
    name: String,
    #[serde(rename = "phone_number")]
    phone_number: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Mapping — the Anti-Corruption boundary
// ---------------------------------------------------------------------------------------------

/// Result of translating one verified Uber event.
#[derive(Debug, Clone, PartialEq)]
pub enum UberMapOutcome {
    /// One of the three delivery-partner facts, ready to stage.
    Mapped(DomainEvent),
    /// An event kind/status this ACL does not consume — acknowledged, nothing recorded.
    Ignored,
}

/// The Uber Direct status vocabulary → the domain `DeliveryStatus`. The ONLY place these strings are
/// allowed to exist; an unknown value is unmappable (never guessed). `pending`/`pickup` are handled
/// upstream (creation ack / courier assignment) and never reach here as a plain status update.
fn map_delivery_status(raw: &str) -> Result<DeliveryStatus, String> {
    match raw {
        "pickup" => Ok(DeliveryStatus::ASSIGNED),
        "pickup_complete" => Ok(DeliveryStatus::PICKED_UP),
        "dropoff" => Ok(DeliveryStatus::OUT_FOR_DELIVERY),
        "delivered" => Ok(DeliveryStatus::DELIVERED),
        "canceled" => Ok(DeliveryStatus::CANCELLED),
        "returned" => Ok(DeliveryStatus::FAILED),
        other => Err(format!("unknown Uber delivery status '{other}'")),
    }
}

fn job_reference(delivery: &UberDelivery, context: &str) -> Result<DeliveryJobId, String> {
    let raw = delivery
        .external_id
        .as_deref()
        .ok_or_else(|| format!("{context}: delivery carries no external_id"))?;
    uuid::Uuid::parse_str(raw)
        .map(DeliveryJobId)
        .map_err(|e| format!("{context}: external_id is not a uuid: {e}"))
}

fn partner_ref(delivery: &UberDelivery) -> Option<ExternalReference> {
    delivery.id.clone().map(ExternalReference)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Translate a (signature-verified) Uber event into the domain fact it reports. `Err` = an event we
/// DO consume whose payload cannot be mapped (missing external_id, unknown status…) — the caller logs
/// it and acknowledges (a retry would not fix the payload).
pub fn map_uber_event(event: &UberEvent) -> Result<UberMapOutcome, String> {
    // We only consume delivery-status events; courier-location updates and everything else are ACKed
    // and ignored (they carry no domain fact we model).
    if event.kind != "event.delivery_status" {
        return Ok(UberMapOutcome::Ignored);
    }
    let delivery = &event.data;
    let status = delivery
        .status
        .as_deref()
        .ok_or("event.delivery_status: delivery carries no status")?;

    match status {
        // Creation ack — no courier yet, nothing to record.
        "pending" => Ok(UberMapOutcome::Ignored),
        // Courier assigned & heading to the restaurant: the Uber analogue of a partner acceptance.
        "pickup" => {
            let delivery_job_id = job_reference(delivery, "event.delivery_status")?;
            let partner_ref =
                partner_ref(delivery).ok_or("event.delivery_status: delivery carries no id")?;
            let courier = delivery
                .courier
                .as_ref()
                .ok_or("event.delivery_status(pickup): delivery carries no courier")?;
            Ok(UberMapOutcome::Mapped(DomainEvent::DeliveryAcceptedByPartner(
                DeliveryAcceptedByPartner {
                    delivery_job_id,
                    partner_ref,
                    // entities.yaml#/Courier: a PARTNER courier has no riderId (not a Captain rider).
                    courier: Courier {
                        display_name: truncate_chars(&courier.name, 140),
                        phone: courier.phone_number.clone().map(PhoneNumber),
                        rider_id: None,
                    },
                    estimated_pickup_at: delivery.pickup_eta.clone(),
                    estimated_dropoff_at: delivery.dropoff_eta.clone(),
                },
            )))
        }
        // Undeliverable BEFORE a courier ever picked up (no courier on the payload): Uber could not
        // fulfil it — treated as a partner decline so the saga advances the ranked walk (#60).
        "canceled" | "returned" if delivery.courier.is_none() => {
            let delivery_job_id = job_reference(delivery, "event.delivery_status")?;
            Ok(UberMapOutcome::Mapped(DomainEvent::DeliveryRejectedByPartner(
                DeliveryRejectedByPartner {
                    delivery_job_id,
                    partner_ref: partner_ref(delivery),
                    // events.yaml caps `reason` at 500 chars — truncate on a char boundary.
                    reason: delivery.undeliverable_reason.as_deref().map(|r| truncate_chars(r, 500)),
                },
            )))
        }
        // Every other transition (pickup_complete / dropoff / delivered, or a post-assignment
        // cancel/return) is progress on an accepted job.
        other => {
            let delivery_job_id = job_reference(delivery, "event.delivery_status")?;
            let status = map_delivery_status(other).map_err(|e| format!("event.delivery_status: {e}"))?;
            Ok(UberMapOutcome::Mapped(DomainEvent::DeliveryStatusUpdated(DeliveryStatusUpdated {
                delivery_job_id,
                partner_ref: partner_ref(delivery),
                status,
                occurred_at: delivery.updated.clone(),
                note: delivery.undeliverable_reason.as_deref().map(|n| truncate_chars(n, 500)),
            })))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Ingestor — mirror the raw delivery, stage the adapted fact
// ---------------------------------------------------------------------------------------------

/// Adapter-owned raw mirror (`external_uber_direct_events`): the verified event is UPSERTed verbatim
/// BEFORE interpretation. Trait so the ingest flow is unit-testable in memory;
/// [`PgRawUberDirectEvents`](crate::raw::PgRawUberDirectEvents) is the Postgres impl. The pk is the
/// provider `event_id`.
#[async_trait::async_trait]
pub trait RawUberDirectEvents: Send + Sync {
    /// UPSERT the verified raw event; `Ok(true)` = newly mirrored, `Ok(false)` = already known.
    async fn upsert(
        &self,
        uber_event_id: &str,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<bool, DomainError>;

    /// Stamp the translation high-water mark once the event has been interpreted.
    async fn mark_processed(&self, uber_event_id: &str) -> Result<(), DomainError>;
}

/// What the ingestor did with one verified event (all four are ACKed with 2xx by the endpoint).
#[derive(Debug, Clone, PartialEq)]
pub enum UberIngestOutcome {
    Recorded { event_type: String },
    Duplicate,
    Ignored { event_type: String },
    Unmappable { reason: String },
}

/// Ingests one verified event: raw mirror UPSERT → ACL translation → `inbound_events` staging. The
/// domain append happens later in the `InboundEventsDrainWorker`. Only infrastructure failures
/// surface as `Err` (5xx → Uber retries); everything else is definitive.
pub struct UberDirectWebhookIngestor {
    raw: Arc<dyn RawUberDirectEvents>,
    inbox: Arc<dyn application::journal::InboundEvents>,
    on_staged: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl UberDirectWebhookIngestor {
    pub fn new(
        raw: Arc<dyn RawUberDirectEvents>,
        inbox: Arc<dyn application::journal::InboundEvents>,
    ) -> Self {
        Self { raw, inbox, on_staged: None }
    }

    /// Wire the post-staging nudge (spawns the drain pass; must not block).
    pub fn with_nudge(mut self, nudge: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.on_staged = Some(nudge);
        self
    }

    /// Mirror + translate + stage one verified event. `raw_body` is the VERBATIM parsed request body.
    /// Crash-safe ordering: the raw mirror lands first; staging dedupes on `(source, external_id)`
    /// where `external_id` is the provider `event_id`.
    pub async fn ingest(
        &self,
        event: &UberEvent,
        raw_body: &serde_json::Value,
    ) -> Result<UberIngestOutcome, DomainError> {
        self.raw.upsert(&event.event_id, &event.kind, raw_body).await?;

        let domain_event = match map_uber_event(event) {
            Ok(UberMapOutcome::Mapped(e)) => e,
            Ok(UberMapOutcome::Ignored) => {
                self.raw.mark_processed(&event.event_id).await?;
                return Ok(UberIngestOutcome::Ignored { event_type: event.kind.clone() });
            }
            Err(reason) => {
                self.raw.mark_processed(&event.event_id).await?;
                return Ok(UberIngestOutcome::Unmappable { reason });
            }
        };

        // Stage the ADAPTED business event (Uber vocabulary stops here). The tagged serde form
        // (`{"eventType": …, "payload": …}`) is what the drain worker deserializes back.
        let tagged = serde_json::to_value(&domain_event).map_err(|e| {
            DomainError::Repository(format!("adapted event for {} unserializable: {e}", event.event_id))
        })?;
        let event_type =
            tagged.get("eventType").and_then(|t| t.as_str()).unwrap_or("unknown").to_owned();
        let row = application::journal::InboundEventRow {
            inbound_event_id: uuid::Uuid::now_v7(),
            source: "uber_direct".into(),
            external_id: event.event_id.clone(),
            correlation_id: uber_direct_correlation_id(&event.event_id),
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
                UberIngestOutcome::Recorded { event_type: event.kind.clone() }
            }
            application::journal::StageOutcome::Duplicate => UberIngestOutcome::Duplicate,
        };
        self.raw.mark_processed(&event.event_id).await?;
        Ok(outcome)
    }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const SECRET: &str = "uber_whsec";
    const JOB_ID: &str = "11111111-1111-4111-8111-111111111111";

    fn sign(secret: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    #[test]
    fn valid_signature_verifies_and_a_wrong_secret_does_not() {
        let body = br#"{"event_id":"evt_1","kind":"event.delivery_status"}"#;
        let header = sign(SECRET, body);
        assert!(verify_signature(SECRET, &header, body).is_ok());
        assert_eq!(verify_signature("other", &header, body), Err(SignatureError::NoMatchingSignature));
    }

    #[test]
    fn malformed_signature_is_rejected() {
        assert_eq!(verify_signature(SECRET, "not-hex!!", b"{}"), Err(SignatureError::MalformedSignature));
    }

    fn pickup_event() -> UberEvent {
        UberEvent {
            event_id: "evt_1".into(),
            kind: "event.delivery_status".into(),
            data: serde_json::from_value(serde_json::json!({
                "id": "del_77",
                "external_id": JOB_ID,
                "status": "pickup",
                "courier": { "name": "Léa", "phone_number": "+33611223344" }
            }))
            .unwrap(),
        }
    }

    #[test]
    fn maps_courier_assignment_to_the_partner_generic_acceptance() {
        match map_uber_event(&pickup_event()).unwrap() {
            UberMapOutcome::Mapped(DomainEvent::DeliveryAcceptedByPartner(e)) => {
                assert_eq!(e.delivery_job_id.0.to_string(), JOB_ID);
                assert_eq!(e.partner_ref.0, "del_77");
                assert_eq!(e.courier.display_name, "Léa");
                assert!(e.courier.rider_id.is_none());
            }
            other => panic!("expected DeliveryAcceptedByPartner, got {other:?}"),
        }
    }

    #[test]
    fn maps_progress_and_undeliverable_and_ignores_the_rest() {
        // A post-assignment status transition → DeliveryStatusUpdated.
        let mut progress = pickup_event();
        progress.data = serde_json::from_value(serde_json::json!({
            "id": "del_77", "external_id": JOB_ID, "status": "delivered",
            "courier": { "name": "Léa" }
        }))
        .unwrap();
        match map_uber_event(&progress).unwrap() {
            UberMapOutcome::Mapped(DomainEvent::DeliveryStatusUpdated(e)) => {
                assert_eq!(e.status, DeliveryStatus::DELIVERED);
            }
            other => panic!("expected DeliveryStatusUpdated, got {other:?}"),
        }

        // Undeliverable with no courier → DeliveryRejectedByPartner (the saga re-offers).
        let mut undeliverable = pickup_event();
        undeliverable.data = serde_json::from_value(serde_json::json!({
            "id": "del_77", "external_id": JOB_ID, "status": "canceled",
            "undeliverable_reason": "no courier available"
        }))
        .unwrap();
        match map_uber_event(&undeliverable).unwrap() {
            UberMapOutcome::Mapped(DomainEvent::DeliveryRejectedByPartner(e)) => {
                assert_eq!(e.reason.as_deref(), Some("no courier available"));
            }
            other => panic!("expected DeliveryRejectedByPartner, got {other:?}"),
        }

        // An unknown status is unmappable; a non-delivery-status kind is ignored.
        let mut weird = pickup_event();
        weird.data = serde_json::from_value(serde_json::json!({
            "id": "del_77", "external_id": JOB_ID, "status": "teleported", "courier": { "name": "x" }
        }))
        .unwrap();
        assert!(map_uber_event(&weird).is_err());

        let mut other_kind = pickup_event();
        other_kind.kind = "event.courier_update".into();
        assert_eq!(map_uber_event(&other_kind).unwrap(), UberMapOutcome::Ignored);
    }

    // ----- ingest flow over in-memory ports -----

    use application::journal::{InboundEventRow, InboundEvents, StageOutcome};

    #[derive(Default)]
    struct MemInbox {
        rows: Mutex<Vec<(String, String)>>, // (source, external_id)
    }
    #[async_trait::async_trait]
    impl InboundEvents for MemInbox {
        async fn stage(&self, row: &InboundEventRow) -> Result<StageOutcome, DomainError> {
            let mut rows = self.rows.lock().unwrap();
            if rows.iter().any(|(s, e)| s == &row.source && e == &row.external_id) {
                return Ok(StageOutcome::Duplicate);
            }
            rows.push((row.source.clone(), row.external_id.clone()));
            Ok(StageOutcome::Staged)
        }
        async fn pending(&self, _limit: i64) -> Result<Vec<InboundEventRow>, DomainError> {
            Ok(vec![])
        }
        async fn mark_delivered(&self, _id: uuid::Uuid) -> Result<(), DomainError> {
            Ok(())
        }
        async fn mark_failed(
            &self,
            _id: uuid::Uuid,
            _error: serde_json::Value,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemRaw {
        rows: Mutex<Vec<String>>,
    }
    #[async_trait::async_trait]
    impl RawUberDirectEvents for MemRaw {
        async fn upsert(
            &self,
            uber_event_id: &str,
            _event_type: &str,
            _payload: &serde_json::Value,
        ) -> Result<bool, DomainError> {
            let mut rows = self.rows.lock().unwrap();
            if rows.iter().any(|k| k == uber_event_id) {
                return Ok(false);
            }
            rows.push(uber_event_id.to_string());
            Ok(true)
        }
        async fn mark_processed(&self, _uber_event_id: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn ingest_stages_the_fact_and_dedupes_redelivery() {
        let raw = Arc::new(MemRaw::default());
        let inbox = Arc::new(MemInbox::default());
        let ingestor = UberDirectWebhookIngestor::new(raw.clone(), inbox.clone());
        let event = pickup_event();
        let body = serde_json::json!({
            "event_id": event.event_id, "kind": event.kind, "data": event.data.clone_json()
        });

        let out = ingestor.ingest(&event, &body).await.unwrap();
        assert_eq!(out, UberIngestOutcome::Recorded { event_type: "event.delivery_status".into() });
        assert_eq!(raw.rows.lock().unwrap()[0], "evt_1");

        // Redelivery of the SAME event dedupes at the inbox.
        let dup = ingestor.ingest(&event, &body).await.unwrap();
        assert_eq!(dup, UberIngestOutcome::Duplicate);
    }

    // Small helper so the ingest test can re-serialize the parsed `data` back to JSON.
    impl UberDelivery {
        fn clone_json(&self) -> serde_json::Value {
            serde_json::json!({
                "id": self.id, "external_id": self.external_id, "status": self.status,
                "courier": self.courier.as_ref().map(|c| serde_json::json!({
                    "name": c.name, "phone_number": c.phone_number
                }))
            })
        }
    }
}
