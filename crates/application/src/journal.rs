//! Write-path JOURNAL ports (ADR-20260720-015300 / -015400) — the row types and store traits for the
//! two journal tables of `specs/database/tables/journals.yaml`: `command_journal` (one row per command
//! submission, persisted BEFORE handling — the idempotency key and the source of the operationStatus
//! surface) and `inbound_events` (adapted inbound BUSINESS events staged by adapter ACLs, drained
//! through the normal write path). Journals NEVER write `domain_events` and are never replayed as
//! state — the event log stays the single source of truth.
//!
//! The application defines the ports; `infrastructure` implements them over Postgres (ADR-0035). The
//! [`mem`] submodule provides in-memory implementations for dispatch/drain tests.

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use domain::generated::scalars::{CommandChannel, CommandJournalStatus, InboundEventStatus};
use domain::shared::errors::DomainError;

/// What the dispatch layer knows at acceptance time — everything but the lifecycle columns.
/// The envelope lives in the FIELDS; `payload` is the business command only (ADR-0041).
#[derive(Debug, Clone, PartialEq)]
pub struct CommandJournalEntry {
    /// Client-suppliable (MetadataInput) or server-generated UUIDv7 — the acceptance handle.
    pub message_id: uuid::Uuid,
    /// Effective correlation (client-supplied or defaulted to `message_id`).
    pub correlation_id: uuid::Uuid,
    /// Parent message/event id when this command was caused by another message.
    pub cause_id: Option<uuid::Uuid>,
    /// `X-SESSION-ID` header — the anonymous ownership scope for `operationStatus`.
    pub session_id: Option<uuid::Uuid>,
    /// W3C trace-id (traceparent-derived or server-started).
    pub trace_id: Option<String>,
    /// Acting user's auth subject (`None` for anonymous) — the authenticated ownership scope.
    pub user_id: Option<uuid::Uuid>,
    /// `UserType` ordinal (declaration-order integer, ADR-0037).
    pub user_type: i32,
    pub channel: CommandChannel,
    /// commands.yaml key, e.g. `PlaceOrder`.
    pub command_type: String,
    /// The BUSINESS command payload only — the envelope is the fields above (ADR-0041).
    pub payload: serde_json::Value,
    /// sha256 hex over the canonical (serde_json) payload — the duplicate-vs-conflict discriminator.
    pub payload_hash: String,
}

/// One stored `command_journal` row: the entry plus its lifecycle.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandJournalRow {
    pub entry: CommandJournalEntry,
    pub status: CommandJournalStatus,
    /// `{ code, context }` on REJECTED/FAILED — the errors.yaml code surfaced as `Operation.errorCode`.
    pub error: Option<serde_json::Value>,
    pub received_at: DateTime<Utc>,
    /// Terminal transition time; `None` while RECEIVED.
    pub completed_at: Option<DateTime<Utc>>,
}

/// Outcome of [`CommandJournal::insert`]: either a fresh acceptance or a replayed `message_id`.
/// On `Duplicate` the caller compares `payload_hash`: a match is an idempotent replay (acknowledge
/// with the original's `status`, `duplicate: true`); a mismatch is a client bug (`Conflict`).
#[derive(Debug, Clone, PartialEq)]
pub enum JournalInsertOutcome {
    Inserted,
    Duplicate { status: CommandJournalStatus, payload_hash: String },
}

/// Durable command journal (`command_journal`, ADR-20260720-015300).
#[async_trait]
pub trait CommandJournal: Send + Sync {
    /// Persist the entry as RECEIVED. A `message_id` collision returns the existing row's
    /// status + hash instead of inserting (the caller decides replay vs conflict).
    async fn insert(&self, entry: &CommandJournalEntry) -> Result<JournalInsertOutcome, DomainError>;

    /// Terminal transition: RECEIVED → SUCCEEDED | REJECTED | FAILED (stamps `completed_at`).
    async fn complete(
        &self,
        message_id: uuid::Uuid,
        status: CommandJournalStatus,
        error: Option<serde_json::Value>,
    ) -> Result<(), DomainError>;

    /// The row behind an acceptance handle (`operationStatus` lookup).
    async fn by_message(
        &self,
        message_id: uuid::Uuid,
    ) -> Result<Option<CommandJournalRow>, DomainError>;

    /// Crash hygiene: mark rows still RECEIVED after `older_than` as FAILED (a handler spawned but
    /// never completed — process died between insert and complete). Returns how many were swept.
    async fn sweep_stale_received(&self, older_than: Duration) -> Result<u64, DomainError>;
}

/// One `inbound_events` row: an adapted inbound BUSINESS event (events.yaml vocabulary only) staged
/// by an adapter ACL, awaiting delivery through the normal write path.
#[derive(Debug, Clone, PartialEq)]
pub struct InboundEventRow {
    /// UUIDv7 minted at staging — becomes `domain_events.cause_id` on delivery.
    pub inbound_event_id: uuid::Uuid,
    /// Owning adapter, e.g. `stripe`.
    pub source: String,
    /// Provider event id (e.g. Stripe `evt_…`); UNIQUE with `source`.
    pub external_id: String,
    /// UUIDv5 of the provider event id (the established ACL convention).
    pub correlation_id: uuid::Uuid,
    /// events.yaml key, e.g. `PaymentCaptured`.
    pub event_type: String,
    /// The serialized domain event — business vocabulary only (the ACL already translated).
    pub payload: serde_json::Value,
    pub status: InboundEventStatus,
    pub error: Option<serde_json::Value>,
    pub received_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
}

/// Outcome of [`InboundEvents::stage`]: `Duplicate` = this `(source, external_id)` was already
/// staged (webhook redelivery) — a no-op, the original row keeps its lifecycle.
#[derive(Debug, Clone, PartialEq)]
pub enum StageOutcome {
    Staged,
    Duplicate,
}

/// Durable inbound-event inbox (`inbound_events`, ADR-20260720-015400).
#[async_trait]
pub trait InboundEvents: Send + Sync {
    /// Stage an adapted business event as RECEIVED; `(source, external_id)` dedupes redelivery.
    async fn stage(&self, row: &InboundEventRow) -> Result<StageOutcome, DomainError>;

    /// The oldest RECEIVED rows (by `received_at`, then id), at most `limit` — the drain's batch.
    async fn pending(&self, limit: i64) -> Result<Vec<InboundEventRow>, DomainError>;

    /// Delivery succeeded (including the aggregate's already-recorded no-op): RECEIVED → DELIVERED.
    async fn mark_delivered(&self, inbound_event_id: uuid::Uuid) -> Result<(), DomainError>;

    /// Delivery failed: RECEIVED → FAILED with the error detail (retryable/inspectable).
    async fn mark_failed(
        &self,
        inbound_event_id: uuid::Uuid,
        error: serde_json::Value,
    ) -> Result<(), DomainError>;
}

/// In-memory implementations (plain `Mutex` state) mirroring the Postgres semantics, for
/// dispatch/drain tests.
pub mod mem {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory [`CommandJournal`], keyed by `message_id`.
    #[derive(Default)]
    pub struct MemCommandJournal {
        rows: Mutex<HashMap<uuid::Uuid, CommandJournalRow>>,
    }

    #[async_trait]
    impl CommandJournal for MemCommandJournal {
        async fn insert(
            &self,
            entry: &CommandJournalEntry,
        ) -> Result<JournalInsertOutcome, DomainError> {
            let mut rows = self.rows.lock().unwrap();
            if let Some(existing) = rows.get(&entry.message_id) {
                return Ok(JournalInsertOutcome::Duplicate {
                    status: existing.status,
                    payload_hash: existing.entry.payload_hash.clone(),
                });
            }
            rows.insert(
                entry.message_id,
                CommandJournalRow {
                    entry: entry.clone(),
                    status: CommandJournalStatus::RECEIVED,
                    error: None,
                    received_at: Utc::now(),
                    completed_at: None,
                },
            );
            Ok(JournalInsertOutcome::Inserted)
        }

        async fn complete(
            &self,
            message_id: uuid::Uuid,
            status: CommandJournalStatus,
            error: Option<serde_json::Value>,
        ) -> Result<(), DomainError> {
            let mut rows = self.rows.lock().unwrap();
            if let Some(row) = rows.get_mut(&message_id) {
                row.status = status;
                row.error = error;
                row.completed_at = Some(Utc::now());
            }
            Ok(())
        }

        async fn by_message(
            &self,
            message_id: uuid::Uuid,
        ) -> Result<Option<CommandJournalRow>, DomainError> {
            Ok(self.rows.lock().unwrap().get(&message_id).cloned())
        }

        async fn sweep_stale_received(&self, older_than: Duration) -> Result<u64, DomainError> {
            let cutoff = Utc::now() - older_than;
            let mut swept = 0;
            for row in self.rows.lock().unwrap().values_mut() {
                if row.status == CommandJournalStatus::RECEIVED && row.received_at < cutoff {
                    row.status = CommandJournalStatus::FAILED;
                    row.error = Some(serde_json::json!({
                        "code": "Internal",
                        "context": { "detail": "stale RECEIVED swept (handler never completed)" }
                    }));
                    row.completed_at = Some(Utc::now());
                    swept += 1;
                }
            }
            Ok(swept)
        }
    }

    /// In-memory [`InboundEvents`], deduped on `(source, external_id)`.
    #[derive(Default)]
    pub struct MemInboundEvents {
        rows: Mutex<Vec<InboundEventRow>>,
    }

    #[async_trait]
    impl InboundEvents for MemInboundEvents {
        async fn stage(&self, row: &InboundEventRow) -> Result<StageOutcome, DomainError> {
            let mut rows = self.rows.lock().unwrap();
            if rows.iter().any(|r| r.source == row.source && r.external_id == row.external_id) {
                return Ok(StageOutcome::Duplicate);
            }
            let mut stamped = row.clone();
            stamped.status = InboundEventStatus::RECEIVED;
            stamped.received_at = Utc::now();
            stamped.delivered_at = None;
            rows.push(stamped);
            Ok(StageOutcome::Staged)
        }

        async fn pending(&self, limit: i64) -> Result<Vec<InboundEventRow>, DomainError> {
            let rows = self.rows.lock().unwrap();
            let mut pending: Vec<InboundEventRow> = rows
                .iter()
                .filter(|r| r.status == InboundEventStatus::RECEIVED)
                .cloned()
                .collect();
            pending.sort_by(|a, b| {
                (a.received_at, a.inbound_event_id).cmp(&(b.received_at, b.inbound_event_id))
            });
            pending.truncate(limit.max(0) as usize);
            Ok(pending)
        }

        async fn mark_delivered(&self, inbound_event_id: uuid::Uuid) -> Result<(), DomainError> {
            for row in self.rows.lock().unwrap().iter_mut() {
                if row.inbound_event_id == inbound_event_id {
                    row.status = InboundEventStatus::DELIVERED;
                    row.delivered_at = Some(Utc::now());
                }
            }
            Ok(())
        }

        async fn mark_failed(
            &self,
            inbound_event_id: uuid::Uuid,
            error: serde_json::Value,
        ) -> Result<(), DomainError> {
            for row in self.rows.lock().unwrap().iter_mut() {
                if row.inbound_event_id == inbound_event_id {
                    row.status = InboundEventStatus::FAILED;
                    row.error = Some(error.clone());
                }
            }
            Ok(())
        }
    }
}

/// Canonical payload hash: sha256 hex over the serde_json serialization. The SAME function must be
/// used by every dispatch surface so replay detection never depends on key ordering differences
/// (serde_json preserves struct-declaration order for our generated commands).
pub fn payload_hash(payload: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(payload.to_string().as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::mem::*;
    use super::*;
    use serde_json::json;

    fn entry(id: uuid::Uuid, payload: serde_json::Value) -> CommandJournalEntry {
        CommandJournalEntry {
            message_id: id,
            correlation_id: id,
            cause_id: None,
            session_id: None,
            trace_id: None,
            user_id: None,
            user_type: 0,
            channel: CommandChannel::GRAPHQL,
            command_type: "RegisterRestaurant".into(),
            payload_hash: payload_hash(&payload),
            payload,
        }
    }

    #[tokio::test]
    async fn journal_lifecycle_and_duplicate_detection() {
        let journal = MemCommandJournal::default();
        let id = uuid::Uuid::new_v4();
        let e = entry(id, json!({ "restaurantId": "r1" }));

        assert_eq!(journal.insert(&e).await.unwrap(), JournalInsertOutcome::Inserted);
        let row = journal.by_message(id).await.unwrap().unwrap();
        assert_eq!(row.status, CommandJournalStatus::RECEIVED);
        assert!(row.completed_at.is_none());

        // A replay reports the original's status + hash — the caller discriminates replay vs conflict.
        match journal.insert(&e).await.unwrap() {
            JournalInsertOutcome::Duplicate { status, payload_hash } => {
                assert_eq!(status, CommandJournalStatus::RECEIVED);
                assert_eq!(payload_hash, e.payload_hash);
            }
            other => panic!("expected Duplicate, got {other:?}"),
        }

        journal
            .complete(id, CommandJournalStatus::REJECTED, Some(json!({ "code": "Conflict" })))
            .await
            .unwrap();
        let done = journal.by_message(id).await.unwrap().unwrap();
        assert_eq!(done.status, CommandJournalStatus::REJECTED);
        assert!(done.completed_at.is_some());
        assert_eq!(done.error.unwrap()["code"], "Conflict");
    }

    #[tokio::test]
    async fn stale_received_rows_are_swept_failed() {
        let journal = MemCommandJournal::default();
        let id = uuid::Uuid::new_v4();
        journal.insert(&entry(id, json!({}))).await.unwrap();
        // Nothing is stale yet.
        assert_eq!(journal.sweep_stale_received(Duration::minutes(5)).await.unwrap(), 0);
        // With a zero threshold the fresh RECEIVED row is already "stale".
        assert_eq!(journal.sweep_stale_received(Duration::zero()).await.unwrap(), 1);
        let row = journal.by_message(id).await.unwrap().unwrap();
        assert_eq!(row.status, CommandJournalStatus::FAILED);
    }

    #[tokio::test]
    async fn inbound_stage_dedupes_and_drains_in_order() {
        let inbox = MemInboundEvents::default();
        let mk = |ext: &str| InboundEventRow {
            inbound_event_id: uuid::Uuid::new_v4(),
            source: "stripe".into(),
            external_id: ext.into(),
            correlation_id: uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, ext.as_bytes()),
            event_type: "PaymentCaptured".into(),
            payload: json!({ "paymentIntentId": "pi_1" }),
            status: InboundEventStatus::RECEIVED,
            error: None,
            received_at: Utc::now(),
            delivered_at: None,
        };

        let first = mk("evt_1");
        assert_eq!(inbox.stage(&first).await.unwrap(), StageOutcome::Staged);
        assert_eq!(inbox.stage(&mk("evt_1")).await.unwrap(), StageOutcome::Duplicate);
        assert_eq!(inbox.stage(&mk("evt_2")).await.unwrap(), StageOutcome::Staged);

        let pending = inbox.pending(10).await.unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].external_id, "evt_1");

        inbox.mark_delivered(first.inbound_event_id).await.unwrap();
        let pending = inbox.pending(10).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].external_id, "evt_2");

        inbox
            .mark_failed(pending[0].inbound_event_id, json!({ "detail": "boom" }))
            .await
            .unwrap();
        assert!(inbox.pending(10).await.unwrap().is_empty());
    }

    #[test]
    fn payload_hash_discriminates_content_not_instance() {
        let a = payload_hash(&json!({ "x": 1 }));
        let b = payload_hash(&json!({ "x": 1 }));
        let c = payload_hash(&json!({ "x": 2 }));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
