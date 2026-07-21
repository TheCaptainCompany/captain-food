//! The on-app inbound-events drain worker (ADR-20260720-015400) — the delivery half of inbound event
//! sourcing, mirroring the SIRENE worker's drain pattern (ADR-0045).
//!
//! Adapter ACLs stage adapted BUSINESS events (events.yaml vocabulary) into the `inbound_events`
//! inbox after verifying + mirroring the raw payload in their own `external_*` table. THIS worker
//! drains the `RECEIVED` rows and delivers each through the NORMAL write path
//! ([`application::payments::record_inbound_payment_event`] for the Stripe payment facts,
//! [`application::deliveries::record_inbound_delivery_event`] for the Avelo37 delivery-partner
//! facts — issue #28 — → `Repository` → `domain_events`), with
//! an EXTERNAL actor whose `cause_id = inbound_event_id` — so every appended fact chains back to the
//! exact inbound record that carried it. The aggregate's fold-based dedupe stays AUTHORITATIVE: an
//! `AlreadyRecorded` outcome still marks the row `DELIVERED`.
//!
//! The worker also runs the `command_journal` stale-`RECEIVED` sweep (ADR-20260720-015300 crash
//! hygiene): a journal row whose handler spawned but never completed is marked FAILED after
//! [`STALE_COMMAND_SWEEP`], so `operationStatus` never reports a dead run as pending forever.
//!
//! Primary trigger = the adapter's nudge after staging (near-zero lag); the poll loop is the missed-
//! nudge safety net. Single-flight like the SIRENE worker: concurrent triggers coalesce.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use application::journal::{CommandJournal, InboundEventRow, InboundEvents};
use application::deliveries::record_inbound_delivery_event;
use application::payments::{record_inbound_payment_event, RecordOutcome};
use application::ports::{Actor, EventStore};
use domain::generated::events::DomainEvent;

/// Safety-net poll interval; the adapter nudge is the primary trigger.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Pending rows are drained in batches of this size (ordered by `received_at`, then id).
const BATCH_SIZE: i64 = 100;
/// A `command_journal` row still RECEIVED after this long is swept FAILED (crash between insert and
/// complete — the spawned handler cannot still be running).
const STALE_COMMAND_SWEEP: chrono::Duration = chrono::Duration::minutes(10);

/// `UserType::EXTERNAL` ordinal (declaration-order int, ADR-0037) — inbound facts are delivered as
/// the external system principal.
const EXTERNAL_USER_TYPE: i32 = 6;

/// Fixed UUIDv5 namespace for the per-source system user id (mirrors the adapters' own namespaces —
/// deterministic, stable across deliveries and deployments).
fn inbound_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/integrations/inbound")
}

/// One drain pass's outcome counters (logged; surfaced by the internal trigger).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InboundDrainSummary {
    /// Rows delivered through the write path (including the aggregate's already-recorded no-ops).
    pub delivered: u64,
    /// Of `delivered`, how many the aggregate had already recorded (webhook redelivery tail).
    pub already_recorded: u64,
    /// Rows marked FAILED (kept for retry/inspection — only infra failures abort the pass).
    pub failed: u64,
    /// Stale `command_journal` RECEIVED rows swept FAILED this pass.
    pub swept_commands: u64,
}

/// The drain worker. Constructed unconditionally with the pool-backed ports; the poll loop is
/// env-gated in the composition root (`RUN_INBOUND_DRAIN`), the nudge path stays always-on.
pub struct InboundEventsDrainWorker {
    inbox: Arc<dyn InboundEvents>,
    journal: Arc<dyn CommandJournal>,
    store: Arc<dyn EventStore>,
    draining: AtomicBool,
}

impl InboundEventsDrainWorker {
    pub fn new(
        inbox: Arc<dyn InboundEvents>,
        journal: Arc<dyn CommandJournal>,
        store: Arc<dyn EventStore>,
    ) -> Self {
        Self { inbox, journal, store, draining: AtomicBool::new(false) }
    }

    /// One single-flight drain pass. Returns `None` when another pass is already running (the
    /// concurrent trigger coalesces into it).
    pub async fn run_once(self: &Arc<Self>) -> Option<InboundDrainSummary> {
        if self.draining.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return None;
        }
        let summary = self.drain().await;
        self.draining.store(false, Ordering::SeqCst);
        Some(summary)
    }

    /// The safety-net poll loop (the adapter nudge is the primary trigger).
    pub async fn run_loop(self: Arc<Self>) {
        loop {
            if let Some(s) = self.run_once().await {
                if s != InboundDrainSummary::default() {
                    println!(
                        "inbound drain: delivered={} (already={}) failed={} swept_commands={}",
                        s.delivered, s.already_recorded, s.failed, s.swept_commands
                    );
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    async fn drain(&self) -> InboundDrainSummary {
        let mut summary = InboundDrainSummary::default();
        // Journal crash hygiene rides on the same tick (cheap UPDATE, usually 0 rows).
        match self.journal.sweep_stale_received(STALE_COMMAND_SWEEP).await {
            Ok(swept) => summary.swept_commands = swept,
            Err(e) => eprintln!("inbound drain: stale-command sweep failed: {e}"),
        }
        loop {
            let batch = match self.inbox.pending(BATCH_SIZE).await {
                Ok(rows) => rows,
                Err(e) => {
                    eprintln!("inbound drain: pending read failed: {e}");
                    return summary;
                }
            };
            if batch.is_empty() {
                return summary;
            }
            let batch_len = batch.len() as i64;
            for row in batch {
                self.process_row(row, &mut summary).await;
            }
            if batch_len < BATCH_SIZE {
                return summary;
            }
        }
    }

    /// Deliver one inbox row through the normal write path; a per-row failure marks it FAILED and
    /// never aborts the pass (the row stays inspectable/retryable).
    async fn process_row(&self, row: InboundEventRow, summary: &mut InboundDrainSummary) {
        let id = row.inbound_event_id;
        match self.deliver(&row).await {
            Ok(already) => {
                summary.delivered += 1;
                if already {
                    summary.already_recorded += 1;
                }
                if let Err(e) = self.inbox.mark_delivered(id).await {
                    eprintln!("inbound drain: mark_delivered({id}) failed: {e}");
                }
            }
            Err(reason) => {
                summary.failed += 1;
                eprintln!("inbound drain: delivery of {id} ({}) failed: {reason}", row.event_type);
                let err = serde_json::json!({ "detail": reason });
                if let Err(e) = self.inbox.mark_failed(id, err).await {
                    eprintln!("inbound drain: mark_failed({id}) failed: {e}");
                }
            }
        }
    }

    /// Route the adapted event to its aggregate's recording path. Returns whether the aggregate had
    /// already recorded the fact (`Ok(true)` = benign redelivery tail).
    async fn deliver(&self, row: &InboundEventRow) -> Result<bool, String> {
        let event: DomainEvent = serde_json::from_value(row.payload.clone())
            .map_err(|e| format!("unparsable staged DomainEvent: {e}"))?;
        let actor = Actor {
            user_id: uuid::Uuid::new_v5(
                &inbound_namespace(),
                format!("system:{}", row.source).as_bytes(),
            ),
            user_type: EXTERNAL_USER_TYPE,
            correlation_id: row.correlation_id,
            // The causality link: the appended fact's cause is the inbound record that carried it.
            cause_id: Some(row.inbound_event_id),
        };
        match &event {
            DomainEvent::PaymentCaptured(_)
            | DomainEvent::PaymentFailed(_)
            | DomainEvent::PaymentRefunded(_) => {
                match record_inbound_payment_event(self.store.as_ref(), event, &actor).await {
                    Ok(RecordOutcome::Recorded) => Ok(false),
                    Ok(RecordOutcome::AlreadyRecorded) => Ok(true),
                    Err(e) if application::ports::is_version_conflict(&e) => Ok(true),
                    Err(e) => Err(e.to_string()),
                }
            }
            // Delivery-partner facts (issue #28): the Avelo37 ACL stages them, this route records
            // them on the DeliveryJob stream — the saga (DeliveryDispatchProcess) reacts from the log.
            DomainEvent::DeliveryAcceptedByPartner(_)
            | DomainEvent::DeliveryRejectedByPartner(_)
            | DomainEvent::DeliveryStatusUpdated(_) => {
                match record_inbound_delivery_event(self.store.as_ref(), event, &actor).await {
                    Ok(RecordOutcome::Recorded) => Ok(false),
                    Ok(RecordOutcome::AlreadyRecorded) => Ok(true),
                    Err(e) if application::ports::is_version_conflict(&e) => Ok(true),
                    Err(e) => Err(e.to_string()),
                }
            }
            other => Err(format!(
                "no delivery route for inbound event type '{}' (staged as {})",
                event_tag(other),
                row.event_type
            )),
        }
    }
}

/// The events.yaml tag of a [`DomainEvent`] (adjacently-tagged serde form).
fn event_tag(event: &DomainEvent) -> String {
    serde_json::to_value(event)
        .ok()
        .and_then(|v| v.get("eventType").and_then(|t| t.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use application::journal::mem::{MemCommandJournal, MemInboundEvents};
    use application::journal::{CommandJournalEntry, StageOutcome};
    use application::ports::version_conflict;
    use chrono::Utc;
    use domain::generated::entities::Money;
    use domain::generated::events::PaymentCaptured;
    use domain::generated::scalars::{
        CommandChannel, CommandJournalStatus, CurrencyCode, InboundEventStatus, MoneyCents,
        OrderId, PaymentIntentId, RestaurantId,
    };
    use domain::shared::errors::DomainError;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory [`EventStore`] recording the ACTOR envelope per append, so the tests can assert the
    /// causality chain (`cause_id = inbound_event_id`) the worker stamps.
    #[derive(Default)]
    struct MemStore {
        streams: Mutex<HashMap<String, Vec<(DomainEvent, Actor)>>>,
    }

    impl MemStore {
        fn stream(&self, stream: &str) -> Vec<DomainEvent> {
            self.streams
                .lock()
                .unwrap()
                .get(stream)
                .map(|rows| rows.iter().map(|(e, _)| e.clone()).collect())
                .unwrap_or_default()
        }

        fn actors(&self, stream: &str) -> Vec<Actor> {
            self.streams
                .lock()
                .unwrap()
                .get(stream)
                .map(|rows| rows.iter().map(|(_, a)| a.clone()).collect())
                .unwrap_or_default()
        }
    }

    #[async_trait::async_trait]
    impl EventStore for MemStore {
        async fn append(
            &self,
            stream_name: &str,
            expected_version: i64,
            events: &[DomainEvent],
            actor: &Actor,
        ) -> Result<i64, DomainError> {
            let mut streams = self.streams.lock().unwrap();
            let stream = streams.entry(stream_name.to_string()).or_default();
            if stream.len() as i64 != expected_version {
                return Err(version_conflict(stream_name, expected_version));
            }
            stream.extend(events.iter().map(|e| (e.clone(), actor.clone())));
            Ok(stream.len() as i64)
        }

        async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError> {
            let events = self.stream(stream_name);
            let version = events.len() as i64;
            Ok((events, version))
        }
    }

    fn captured_row(ext: &str) -> InboundEventRow {
        let event = DomainEvent::PaymentCaptured(PaymentCaptured {
            payment_intent_id: PaymentIntentId("pi_1".into()),
            order_id: Some(OrderId(uuid::Uuid::from_u128(1))),
            restaurant_id: RestaurantId(uuid::Uuid::from_u128(2)),
            amount: Money { amount_cents: MoneyCents(1960), currency: CurrencyCode("EUR".into()) },
        });
        InboundEventRow {
            inbound_event_id: uuid::Uuid::new_v4(),
            source: "stripe".into(),
            external_id: ext.into(),
            correlation_id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, ext.as_bytes()),
            event_type: "PaymentCaptured".into(),
            payload: serde_json::to_value(&event).unwrap(),
            status: InboundEventStatus::RECEIVED,
            error: None,
            received_at: Utc::now(),
            delivered_at: None,
        }
    }

    fn worker(
        inbox: Arc<MemInboundEvents>,
        journal: Arc<MemCommandJournal>,
        store: Arc<MemStore>,
    ) -> Arc<InboundEventsDrainWorker> {
        Arc::new(InboundEventsDrainWorker::new(inbox, journal, store))
    }

    #[tokio::test]
    async fn drains_a_staged_fact_into_the_event_log_with_causality() {
        let (inbox, journal, store) =
            (Arc::new(MemInboundEvents::default()), Arc::new(MemCommandJournal::default()), Arc::new(MemStore::default()));
        let row = captured_row("evt_1");
        assert_eq!(inbox.stage(&row).await.unwrap(), StageOutcome::Staged);

        let w = worker(inbox.clone(), journal, store.clone());
        let s = w.run_once().await.expect("single-flight");
        assert_eq!(s.delivered, 1);
        assert_eq!(s.failed, 0);

        // The fact landed on the Payment stream via the normal write path…
        let events = store.stream("Payment-pi_1");
        assert_eq!(events.len(), 1);
        // …with the inbound record as its cause (chain: webhook → inbound_events → domain_events).
        let actors = store.actors("Payment-pi_1");
        assert_eq!(actors[0].cause_id, Some(row.inbound_event_id));
        assert_eq!(actors[0].correlation_id, row.correlation_id);
        // …and the inbox row is DELIVERED (no longer pending).
        assert!(inbox.pending(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn drains_a_staged_delivery_fact_onto_the_delivery_job_stream() {
        use domain::generated::entities::{Address, Courier};
        use domain::generated::events::{DeliveryAcceptedByPartner, DeliveryRequested};
        use domain::generated::scalars::{
            AddressLine, CityName, CountryCode, DeliveryJobId, ExternalReference, PhoneNumber,
            PostalCode,
        };

        let (inbox, journal, store) =
            (Arc::new(MemInboundEvents::default()), Arc::new(MemCommandJournal::default()), Arc::new(MemStore::default()));
        let job = DeliveryJobId(uuid::Uuid::from_u128(7));
        let stream = format!("DeliveryJob-{}", job.0);
        let address = Address {
            line1: AddressLine("1 rue Nationale".into()),
            line2: None,
            postal_code: PostalCode("37000".into()),
            city: CityName("Tours".into()),
            country: CountryCode("FR".into()),
        };
        // The job exists (saga-delivered birth)…
        store
            .streams
            .lock()
            .unwrap()
            .entry(stream.clone())
            .or_default()
            .push((
                DomainEvent::DeliveryRequested(DeliveryRequested {
                    mode: None,
                    delivery_job_id: job,
                    order_id: OrderId(uuid::Uuid::from_u128(1)),
                    restaurant_id: RestaurantId(uuid::Uuid::from_u128(2)),
                    pickup: address.clone(),
                    dropoff: address,
                    provider: None,
                }),
                Actor { user_id: uuid::Uuid::nil(), user_type: 0, correlation_id: uuid::Uuid::nil(), cause_id: None },
            ));
        // …and the Avelo37 ACL staged the partner acceptance.
        let event = DomainEvent::DeliveryAcceptedByPartner(DeliveryAcceptedByPartner {
            delivery_job_id: job,
            partner_ref: ExternalReference("avelo-77".into()),
            courier: Courier {
                display_name: "Léa".into(),
                phone: Some(PhoneNumber("+33611223344".into())),
                rider_id: None,
            },
            estimated_pickup_at: None,
            estimated_dropoff_at: None,
        });
        let row = InboundEventRow {
            inbound_event_id: uuid::Uuid::new_v4(),
            source: "avelo37".into(),
            external_id: "evt_av_1".into(),
            correlation_id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"evt_av_1"),
            event_type: "DeliveryAcceptedByPartner".into(),
            payload: serde_json::to_value(&event).unwrap(),
            status: InboundEventStatus::RECEIVED,
            error: None,
            received_at: Utc::now(),
            delivered_at: None,
        };
        inbox.stage(&row).await.unwrap();

        let w = worker(inbox.clone(), journal, store.clone());
        let s = w.run_once().await.unwrap();
        assert_eq!(s.delivered, 1);
        assert_eq!(s.failed, 0);
        // The fact landed on the DeliveryJob stream with the inbound causality chain.
        let events = store.stream(&stream);
        assert_eq!(events.len(), 2);
        assert!(matches!(events[1], DomainEvent::DeliveryAcceptedByPartner(_)));
        let actors = store.actors(&stream);
        assert_eq!(actors[1].cause_id, Some(row.inbound_event_id));
        assert!(inbox.pending(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_redelivered_fact_is_a_no_op_and_still_delivers() {
        let (inbox, journal, store) =
            (Arc::new(MemInboundEvents::default()), Arc::new(MemCommandJournal::default()), Arc::new(MemStore::default()));
        // Two DIFFERENT provider deliveries carrying the SAME fact (Stripe redelivery after a lost
        // ACK): both stage (different external ids), the second is the aggregate's no-op.
        inbox.stage(&captured_row("evt_1")).await.unwrap();
        inbox.stage(&captured_row("evt_2")).await.unwrap();

        let w = worker(inbox.clone(), journal, store.clone());
        let s = w.run_once().await.unwrap();
        assert_eq!(s.delivered, 2);
        assert_eq!(s.already_recorded, 1);
        assert_eq!(store.stream("Payment-pi_1").len(), 1, "the fact recorded exactly once");
    }

    #[tokio::test]
    async fn an_unroutable_event_is_marked_failed_not_lost() {
        let (inbox, journal, store) =
            (Arc::new(MemInboundEvents::default()), Arc::new(MemCommandJournal::default()), Arc::new(MemStore::default()));
        let mut row = captured_row("evt_bad");
        row.payload = serde_json::json!({ "eventType": "NotAnEvent", "payload": {} });
        inbox.stage(&row).await.unwrap();

        let w = worker(inbox.clone(), journal, store.clone());
        let s = w.run_once().await.unwrap();
        assert_eq!(s.failed, 1);
        assert_eq!(s.delivered, 0);
        assert!(inbox.pending(10).await.unwrap().is_empty(), "FAILED rows are not re-drained");
        assert!(store.stream("Payment-pi_1").is_empty());
    }

    #[tokio::test]
    async fn sweeps_stale_received_commands_on_the_same_tick() {
        let (inbox, journal, store) =
            (Arc::new(MemInboundEvents::default()), Arc::new(MemCommandJournal::default()), Arc::new(MemStore::default()));
        let id = uuid::Uuid::new_v4();
        journal
            .insert(&CommandJournalEntry {
                message_id: id,
                correlation_id: id,
                cause_id: None,
                session_id: None,
                trace_id: None,
                user_id: None,
                user_type: 0,
                channel: CommandChannel::GRAPHQL,
                command_type: "AddCartLine".into(),
                payload: serde_json::json!({}),
                payload_hash: "h".into(),
            })
            .await
            .unwrap();

        // The mem sweep uses the same threshold the worker passes — a fresh row is NOT stale.
        let w = worker(inbox, journal.clone(), store);
        let s = w.run_once().await.unwrap();
        assert_eq!(s.swept_commands, 0);
        let row = journal.by_message(id).await.unwrap().unwrap();
        assert_eq!(row.status, CommandJournalStatus::RECEIVED);
    }
}
