//! CoopCycle webhook Anti-Corruption Layer — INBOUND integration events (CLAUDE.md "Commands vs
//! inbound (integration) events"), the FEDERATED sibling of the Avelo37 ACL. A co-op instance PUSHes
//! signed webhooks reporting facts that ALREADY happened on its side; there is nothing to validate
//! and nothing to reject, so no command is involved — the ACL translates the co-op wire shape into
//! the already-modelled (partner-generic) domain events and records them as facts:
//!
//! - `task.accepted`       → `DeliveryAcceptedByPartner` (courier assigned)
//! - `task.declined`       → `DeliveryRejectedByPartner` (the saga re-offers, bounded — ADR-20260720-004556)
//! - `task.status_updated` → `DeliveryStatusUpdated` (progress up to DELIVERED/FAILED)
//!
//! Any other event type is acknowledged and ignored.
//!
//! # Federation (specs/integrations/coopcycle.md §5)
//!
//! CoopCycle is many self-hosted instances, so every fact is scoped by its `instance_id` (the
//! `{instance}` webhook path segment). Provider event ids are unique only PER instance, so the
//! staging/dedupe key is the namespaced `"{instance_id}:{event id}"`, and signatures are verified
//! with THAT instance's per-instance secret (looked up in the registry by the http shell).
//!
//! # Boundary translation, signature scheme, durable inbox
//!
//! Identical in shape to the Avelo37 ACL: raw co-op status strings are mapped to `DeliveryStatus`
//! HERE (never crossing the boundary); `job_reference` is OUR `DeliveryJobId` echoed back from the
//! outbound offer; the co-op task id maps to `partnerRef`. Signatures use the Stripe timestamped-HMAC
//! scheme adopted as our partner contract (`CoopCycle-Signature: t=…,v1=…`, ±300s replay window,
//! constant-time compare). Ingestion is verify → mirror verbatim into `external_coopcycle_events` →
//! translate → stage the adapted event into `inbound_events` (`source = 'coopcycle'`) → ACK; the
//! domain append happens later in the `InboundEventsDrainWorker` through the normal write path.

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

/// Replay window: reject a signature whose `t=` is further than this from now (same tolerance as the
/// Avelo37/Stripe seams).
pub const SIGNATURE_TOLERANCE_SECS: i64 = 300;

// ---------------------------------------------------------------------------------------------
// Envelope identity (ADR-0041) — deterministic, like the Avelo37/Stripe/SIRENE ACLs'
// ---------------------------------------------------------------------------------------------

/// Fixed UUIDv5 namespace for every id this ACL derives. NEVER change it: derived ids are stable
/// across deliveries and deployments.
fn coopcycle_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/integrations/coopcycle")
}

/// Fixed system user id stamping the event envelope (`domain_events.user_id`) for facts a co-op reports.
pub fn coopcycle_system_user_id() -> uuid::Uuid {
    uuid::Uuid::new_v5(&coopcycle_namespace(), b"system:coopcycle-webhook")
}

/// The namespaced staging/dedupe key for one co-op event: `"{instance_id}:{provider event id}"`.
/// Federation makes provider ids unique only per instance, so this is the globally-unique idempotent
/// key (the `external_coopcycle_events` pk and the `inbound_events.external_id`).
pub fn staging_key(instance_id: &str, event_id: &str) -> String {
    format!("{instance_id}:{event_id}")
}

/// Deterministic envelope `correlation_id` for a co-op event — every fact recorded from the same
/// delivery (and any redelivery attempt) correlates to the same value.
pub fn coopcycle_correlation_id(instance_id: &str, event_id: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&coopcycle_namespace(), staging_key(instance_id, event_id).as_bytes())
}

// ---------------------------------------------------------------------------------------------
// Signature verification (the Stripe scheme, per-instance secret)
// ---------------------------------------------------------------------------------------------

/// Why a `CoopCycle-Signature` header was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureError {
    MissingTimestamp,
    MissingSignature,
    StaleTimestamp { timestamp: i64, now: i64 },
    NoMatchingSignature,
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTimestamp => write!(f, "no t= timestamp in CoopCycle-Signature header"),
            Self::MissingSignature => write!(f, "no v1= signature in CoopCycle-Signature header"),
            Self::StaleTimestamp { timestamp, now } => write!(
                f,
                "timestamp {timestamp} outside the {SIGNATURE_TOLERANCE_SECS}s replay window (now {now})"
            ),
            Self::NoMatchingSignature => write!(f, "no v1 signature matches the payload"),
        }
    }
}

/// Verify a `CoopCycle-Signature` header against the RAW request body under one instance's secret.
/// The signed payload is `"<t>.<body>"`; every `v1` candidate is checked in constant time. `now_unix`
/// is injected for testability.
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
            Some(("v1", v)) => {
                if let Ok(bytes) = hex::decode(v) {
                    candidates.push(bytes);
                }
            }
            _ => {}
        }
    }

    let timestamp_raw = timestamp_raw.ok_or(SignatureError::MissingTimestamp)?;
    let timestamp: i64 = timestamp_raw.parse().map_err(|_| SignatureError::MissingTimestamp)?;
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
// Wire types — the CoopCycle subset this ACL reads (unknown fields are ignored by serde)
// ---------------------------------------------------------------------------------------------

/// A CoopCycle webhook envelope (the subset we read).
#[derive(Debug, Clone, Deserialize)]
pub struct CoopCycleEvent {
    /// Provider event id — unique PER instance (namespaced into the idempotency key with the instance).
    pub id: String,
    /// Event type, e.g. `task.accepted`.
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: CoopCycleEventData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CoopCycleEventData {
    /// The affected co-op delivery/task, shape depending on `type` — kept raw and re-read per type.
    pub delivery: serde_json::Value,
}

/// Co-op `delivery`/`task` object subset.
#[derive(Debug, Clone, Deserialize)]
struct CoopCycleDelivery {
    /// Co-op-side task id → `partnerRef`.
    id: Option<String>,
    /// OUR `DeliveryJobId`, echoed back from the outbound offer (outbound.rs).
    job_reference: Option<String>,
    courier: Option<CoopCycleCourier>,
    status: Option<String>,
    reason: Option<String>,
    note: Option<String>,
    eta_pickup_at: Option<String>,
    eta_dropoff_at: Option<String>,
    occurred_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CoopCycleCourier {
    name: String,
    phone: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Mapping — the Anti-Corruption boundary
// ---------------------------------------------------------------------------------------------

/// Result of translating one verified CoopCycle event.
#[derive(Debug, Clone, PartialEq)]
pub enum CoopCycleMapOutcome {
    /// One of the three delivery-partner facts, ready to stage.
    Mapped(DomainEvent),
    /// An event type this ACL does not consume — acknowledged, nothing recorded.
    Ignored,
}

/// The co-op's status vocabulary → the domain `DeliveryStatus`. The ONLY place these strings are
/// allowed to exist; an unknown value is unmappable (never guessed). `en_route` is accepted as a
/// synonym for OUT_FOR_DELIVERY (co-op vocabulary varies).
fn map_partner_status(raw: &str) -> Result<DeliveryStatus, String> {
    match raw {
        "assigned" => Ok(DeliveryStatus::ASSIGNED),
        "picked_up" | "picked" => Ok(DeliveryStatus::PICKED_UP),
        "out_for_delivery" | "en_route" => Ok(DeliveryStatus::OUT_FOR_DELIVERY),
        "delivered" => Ok(DeliveryStatus::DELIVERED),
        "failed" => Ok(DeliveryStatus::FAILED),
        "cancelled" => Ok(DeliveryStatus::CANCELLED),
        other => Err(format!("unknown co-op delivery status '{other}'")),
    }
}

fn job_reference(delivery: &CoopCycleDelivery, context: &str) -> Result<DeliveryJobId, String> {
    let raw = delivery
        .job_reference
        .as_deref()
        .ok_or_else(|| format!("{context}: delivery carries no job_reference"))?;
    uuid::Uuid::parse_str(raw)
        .map(DeliveryJobId)
        .map_err(|e| format!("{context}: job_reference is not a uuid: {e}"))
}

fn partner_ref(delivery: &CoopCycleDelivery) -> Option<ExternalReference> {
    delivery.id.clone().map(ExternalReference)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Translate a (signature-verified) CoopCycle event into the domain fact it reports. `Err` = an event
/// type we DO consume whose payload cannot be mapped (missing job_reference/courier, unknown
/// status…) — the caller logs it and acknowledges (a retry would not fix the payload).
pub fn map_coopcycle_event(event: &CoopCycleEvent) -> Result<CoopCycleMapOutcome, String> {
    let parse = |context: &str| -> Result<CoopCycleDelivery, String> {
        serde_json::from_value(event.data.delivery.clone())
            .map_err(|e| format!("{context}: unparsable data.delivery: {e}"))
    };
    match event.event_type.as_str() {
        "task.accepted" => {
            let delivery = parse("task.accepted")?;
            let delivery_job_id = job_reference(&delivery, "task.accepted")?;
            let partner_ref =
                partner_ref(&delivery).ok_or("task.accepted: delivery carries no task id")?;
            let courier =
                delivery.courier.as_ref().ok_or("task.accepted: delivery carries no courier")?;
            Ok(CoopCycleMapOutcome::Mapped(DomainEvent::DeliveryAcceptedByPartner(
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
        "task.declined" => {
            let delivery = parse("task.declined")?;
            let delivery_job_id = job_reference(&delivery, "task.declined")?;
            Ok(CoopCycleMapOutcome::Mapped(DomainEvent::DeliveryRejectedByPartner(
                DeliveryRejectedByPartner {
                    delivery_job_id,
                    partner_ref: partner_ref(&delivery),
                    // events.yaml caps `reason` at 500 chars — truncate on a char boundary.
                    reason: delivery.reason.as_deref().map(|r| truncate_chars(r, 500)),
                },
            )))
        }
        "task.status_updated" => {
            let delivery = parse("task.status_updated")?;
            let delivery_job_id = job_reference(&delivery, "task.status_updated")?;
            let raw_status = delivery
                .status
                .as_deref()
                .ok_or("task.status_updated: delivery carries no status")?;
            let status = map_partner_status(raw_status)
                .map_err(|e| format!("task.status_updated: {e}"))?;
            Ok(CoopCycleMapOutcome::Mapped(DomainEvent::DeliveryStatusUpdated(
                DeliveryStatusUpdated {
                    delivery_job_id,
                    partner_ref: partner_ref(&delivery),
                    status,
                    occurred_at: delivery.occurred_at.clone(),
                    note: delivery.note.as_deref().map(|n| truncate_chars(n, 500)),
                },
            )))
        }
        _ => Ok(CoopCycleMapOutcome::Ignored),
    }
}

// ---------------------------------------------------------------------------------------------
// Ingestor — mirror the raw delivery (with its instance), stage the adapted fact
// ---------------------------------------------------------------------------------------------

/// Adapter-owned raw mirror (`external_coopcycle_events`): the verified event is UPSERTed verbatim
/// BEFORE interpretation, scoped by its originating `instance_id`. Trait so the ingest flow is
/// unit-testable in memory; [`PgRawCoopCycleEvents`](crate::raw::PgRawCoopCycleEvents) is the
/// Postgres impl. The pk is the namespaced [`staging_key`].
#[async_trait::async_trait]
pub trait RawCoopCycleEvents: Send + Sync {
    /// UPSERT the verified raw event; `Ok(true)` = newly mirrored, `Ok(false)` = already known.
    async fn upsert(
        &self,
        coopcycle_event_id: &str,
        instance_id: &str,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<bool, DomainError>;

    /// Stamp the translation high-water mark once the event has been interpreted.
    async fn mark_processed(&self, coopcycle_event_id: &str) -> Result<(), DomainError>;
}

/// What the ingestor did with one verified event (all four are ACKed with 2xx by the endpoint).
#[derive(Debug, Clone, PartialEq)]
pub enum CoopCycleIngestOutcome {
    Recorded { event_type: String },
    Duplicate,
    Ignored { event_type: String },
    Unmappable { reason: String },
}

/// Ingests one verified event for a given instance: raw mirror UPSERT → ACL translation →
/// `inbound_events` staging. The domain append happens later in the `InboundEventsDrainWorker`. Only
/// infrastructure failures surface as `Err` (5xx → co-op retries); everything else is definitive.
pub struct CoopCycleWebhookIngestor {
    raw: Arc<dyn RawCoopCycleEvents>,
    mailbox: Arc<dyn application::mailbox::Mailbox>,
    on_staged: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl CoopCycleWebhookIngestor {
    pub fn new(
        raw: Arc<dyn RawCoopCycleEvents>,
        mailbox: Arc<dyn application::mailbox::Mailbox>,
    ) -> Self {
        Self { raw, mailbox, on_staged: None }
    }

    /// Wire the post-staging nudge (spawns the drain pass; must not block).
    pub fn with_nudge(mut self, nudge: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.on_staged = Some(nudge);
        self
    }

    /// Mirror + translate + stage one verified event under its `instance_id`. `raw_body` is the
    /// VERBATIM parsed request body. Crash-safe ordering: the raw mirror lands first; staging dedupes
    /// on `(source, external_id)` where `external_id` is the namespaced [`staging_key`].
    pub async fn ingest(
        &self,
        instance_id: &str,
        event: &CoopCycleEvent,
        raw_body: &serde_json::Value,
    ) -> Result<CoopCycleIngestOutcome, DomainError> {
        let key = staging_key(instance_id, &event.id);
        self.raw.upsert(&key, instance_id, &event.event_type, raw_body).await?;

        let domain_event = match map_coopcycle_event(event) {
            Ok(CoopCycleMapOutcome::Mapped(e)) => e,
            Ok(CoopCycleMapOutcome::Ignored) => {
                self.raw.mark_processed(&key).await?;
                return Ok(CoopCycleIngestOutcome::Ignored { event_type: event.event_type.clone() });
            }
            Err(reason) => {
                self.raw.mark_processed(&key).await?;
                return Ok(CoopCycleIngestOutcome::Unmappable { reason });
            }
        };

        // Enqueue the ADAPTED business event on its actor lane (ADR-20260731-122500) (co-op vocabulary stops here). The tagged serde form
        // (`{"eventType": …, "payload": …}`) is what the kind-EVENT route deserializes back.
        let tagged = serde_json::to_value(&domain_event).map_err(|e| {
            DomainError::Repository(format!("adapted event for {key} unserializable: {e}"))
        })?;
        let fact = match infrastructure::mailbox::inbound_fact_for(
            "coopcycle",
            &key,
            coopcycle_correlation_id(instance_id, &event.id),
            tagged,
        ) {
            Ok(f) => f,
            // A mapped family the lane resolver cannot address is an ACL/schema drift — recorded
            // as unmappable (mirror kept), never a 5xx retry storm.
            Err(e) => {
                self.raw.mark_processed(&key).await?;
                return Ok(CoopCycleIngestOutcome::Unmappable { reason: e.to_string() });
            }
        };
        let outcome = match infrastructure::mailbox::enqueue_inbound_fact(self.mailbox.as_ref(), fact).await? {
            infrastructure::mailbox::EnqueueOutcome::Enqueued => {
                if let Some(nudge) = &self.on_staged {
                    nudge();
                }
                CoopCycleIngestOutcome::Recorded { event_type: event.event_type.clone() }
            }
            _ => CoopCycleIngestOutcome::Duplicate,
        };
        self.raw.mark_processed(&key).await?;
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

    const SECRET: &str = "whs_tours_secret";
    const JOB_ID: &str = "11111111-1111-4111-8111-111111111111";

    fn sign(secret: &str, t: i64, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{t}.").as_bytes());
        mac.update(body);
        format!("t={t},v1={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn valid_signature_verifies_and_a_wrong_secret_does_not() {
        let body = br#"{"id":"evt_1","type":"task.accepted"}"#;
        let header = sign(SECRET, 1_000, body);
        assert!(verify_signature(SECRET, &header, body, 1_000).is_ok());
        assert_eq!(
            verify_signature("other", &header, body, 1_000),
            Err(SignatureError::NoMatchingSignature)
        );
    }

    #[test]
    fn stale_timestamp_is_rejected() {
        let body = br#"{}"#;
        let header = sign(SECRET, 1_000, body);
        match verify_signature(SECRET, &header, body, 1_000 + SIGNATURE_TOLERANCE_SECS + 1) {
            Err(SignatureError::StaleTimestamp { .. }) => {}
            other => panic!("expected StaleTimestamp, got {other:?}"),
        }
    }

    fn accepted_event() -> CoopCycleEvent {
        CoopCycleEvent {
            id: "evt_1".into(),
            event_type: "task.accepted".into(),
            data: CoopCycleEventData {
                delivery: serde_json::json!({
                    "id": "task_77",
                    "job_reference": JOB_ID,
                    "courier": { "name": "Léa", "phone": "+33611223344" }
                }),
            },
        }
    }

    #[test]
    fn maps_accepted_to_the_partner_generic_fact() {
        match map_coopcycle_event(&accepted_event()).unwrap() {
            CoopCycleMapOutcome::Mapped(DomainEvent::DeliveryAcceptedByPartner(e)) => {
                assert_eq!(e.delivery_job_id.0.to_string(), JOB_ID);
                assert_eq!(e.partner_ref.0, "task_77");
                assert_eq!(e.courier.display_name, "Léa");
                assert!(e.courier.rider_id.is_none());
            }
            other => panic!("expected DeliveryAcceptedByPartner, got {other:?}"),
        }
    }

    #[test]
    fn unknown_status_is_unmappable_and_unknown_type_is_ignored() {
        let mut bad = accepted_event();
        bad.event_type = "task.status_updated".into();
        bad.data.delivery = serde_json::json!({ "job_reference": JOB_ID, "status": "teleported" });
        assert!(map_coopcycle_event(&bad).is_err());

        let mut other = accepted_event();
        other.event_type = "task.note_added".into();
        assert_eq!(map_coopcycle_event(&other).unwrap(), CoopCycleMapOutcome::Ignored);
    }

    // ----- ingest flow over in-memory ports -----

    /// Local in-memory [`InboundEvents`], deduped on `(source, external_id)` — the cross-crate test

    #[derive(Default)]
    struct MemRaw {
        rows: Mutex<Vec<(String, String)>>, // (key, instance_id)
    }
    #[async_trait::async_trait]
    impl RawCoopCycleEvents for MemRaw {
        async fn upsert(
            &self,
            coopcycle_event_id: &str,
            instance_id: &str,
            _event_type: &str,
            _payload: &serde_json::Value,
        ) -> Result<bool, DomainError> {
            let mut rows = self.rows.lock().unwrap();
            if rows.iter().any(|(k, _)| k == coopcycle_event_id) {
                return Ok(false);
            }
            rows.push((coopcycle_event_id.to_string(), instance_id.to_string()));
            Ok(true)
        }
        async fn mark_processed(&self, _coopcycle_event_id: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn ingest_namespaces_the_key_by_instance_and_stages_the_fact() {
        let raw = Arc::new(MemRaw::default());
        let inbox = Arc::new(application::mailbox::mem::MemMailbox::default());
        let ingestor = CoopCycleWebhookIngestor::new(raw.clone(), inbox.clone());
        let event = accepted_event();
        let body = serde_json::to_value(&serde_json::json!({
            "id": event.id, "type": event.event_type,
            "data": { "delivery": event.data.delivery }
        }))
        .unwrap();

        let out = ingestor.ingest("tours", &event, &body).await.unwrap();
        assert_eq!(out, CoopCycleIngestOutcome::Recorded { event_type: "task.accepted".into() });
        // The staged inbound row is keyed by the namespaced "{instance}:{event id}".
        assert_eq!(raw.rows.lock().unwrap()[0].0, "tours:evt_1");

        // Same event id from a DIFFERENT instance is a distinct fact (not a duplicate).
        let out2 = ingestor.ingest("national", &event, &body).await.unwrap();
        assert_eq!(out2, CoopCycleIngestOutcome::Recorded { event_type: "task.accepted".into() });
        // Redelivery of the SAME (instance,event) dedupes at the inbox.
        let dup = ingestor.ingest("tours", &event, &body).await.unwrap();
        assert_eq!(dup, CoopCycleIngestOutcome::Duplicate);
    }
}
