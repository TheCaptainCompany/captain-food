//! The write-side MAILBOX port (#242 Runtime C3, PROP-20260728-152752 §2): the typed door every
//! command submission enters through once the resolvers flip — an `inbound_messages` insert,
//! idempotent by `message_id` (same payload hash → the original acceptance replays; a different
//! one → the caller raises the synchronous Conflict), addressed by `(actor_type, actor_id)` with
//! the partition stamped by the FROZEN routing hash. Replaces `journal::CommandJournal` as the
//! acceptance surface; the mailbox worker (not a spawned task) delivers.

use async_trait::async_trait;
use domain::generated::scalars::InboundMessageStatus;
use domain::shared::errors::DomainError;

/// One mailbox insert — the envelope is the columns (ADR-0041), `payload` is business-only.
#[derive(Debug, Clone)]
pub struct MailboxEntry {
    pub message_id: uuid::Uuid,
    /// COMMAND | EVENT | MESSAGE.
    pub kind: String,
    pub actor_type: String,
    pub actor_id: uuid::Uuid,
    /// `stable_partition(actor_id, width)` — stamped by the CALLER (the client knows the
    /// partitioning; the table stays a dumb mailbox).
    pub partition: i16,
    pub message_type: String,
    pub payload: serde_json::Value,
    pub payload_hash: String,
    /// GRAPHQL | WORKER | EXTERNAL.
    pub channel: String,
    pub user_id: Option<uuid::Uuid>,
    pub user_type: String,
    pub correlation_id: uuid::Uuid,
    pub cause_id: Option<uuid::Uuid>,
    pub session_id: Option<uuid::Uuid>,
    pub trace_id: Option<String>,
    /// Owning adapter for kind EVENT ('stripe', …) — with `external_id`, the delivery-level dedupe.
    pub source: Option<String>,
    pub external_id: Option<String>,
}

/// What an insert did — mirrors `journal::JournalInsertOutcome` so the acceptance contract
/// (ADR-20260720-015500) carries over unchanged.
#[derive(Debug, Clone, PartialEq)]
pub enum MailboxInsertOutcome {
    Inserted,
    /// `message_id` already present: the original row's status + payload hash — replay vs
    /// conflict is the caller's call.
    Duplicate { status: InboundMessageStatus, payload_hash: String },
}

/// The status-read projection of one mailbox row (`operationStatus` / ownership scoping).
#[derive(Debug, Clone)]
pub struct MailboxStatusRow {
    pub message_id: uuid::Uuid,
    pub correlation_id: uuid::Uuid,
    pub status: InboundMessageStatus,
    /// `{ code, context }` on REJECTED / FAILED.
    pub error: Option<serde_json::Value>,
    pub user_id: Option<uuid::Uuid>,
    pub session_id: Option<uuid::Uuid>,
    pub received_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[async_trait]
pub trait Mailbox: Send + Sync {
    /// Persist the entry as RECEIVED (immediate `position`). A `message_id` collision returns the
    /// existing row's status + hash instead of inserting.
    async fn insert(&self, entry: &MailboxEntry) -> Result<MailboxInsertOutcome, DomainError>;

    /// The row behind an acceptance handle (the `operationStatus` lookup).
    async fn by_message(
        &self,
        message_id: uuid::Uuid,
    ) -> Result<Option<MailboxStatusRow>, DomainError>;
}

/// In-memory double for tests (mirrors `journal::mem::MemCommandJournal`).
pub mod mem {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    pub struct MemMailbox {
        rows: Mutex<HashMap<uuid::Uuid, (MailboxEntry, InboundMessageStatus, Option<serde_json::Value>)>>,
    }

    impl MemMailbox {
        /// Snapshot of every stored entry, insertion-order-independent (test assertions).
        pub fn entries(&self) -> Vec<MailboxEntry> {
            self.rows.lock().expect("mem mailbox poisoned").values().map(|(e, _, _)| e.clone()).collect()
        }

        /// One entry by message id (test assertions).
        pub fn entry(&self, message_id: uuid::Uuid) -> Option<MailboxEntry> {
            self.rows
                .lock()
                .expect("mem mailbox poisoned")
                .get(&message_id)
                .map(|(e, _, _)| e.clone())
        }
    }

    #[async_trait]
    impl Mailbox for MemMailbox {
        async fn insert(&self, entry: &MailboxEntry) -> Result<MailboxInsertOutcome, DomainError> {
            let mut rows = self.rows.lock().expect("mem mailbox poisoned");
            if let Some((existing, status, _)) = rows.get(&entry.message_id) {
                return Ok(MailboxInsertOutcome::Duplicate {
                    status: *status,
                    payload_hash: existing.payload_hash.clone(),
                });
            }
            rows.insert(entry.message_id, (entry.clone(), InboundMessageStatus::RECEIVED, None));
            Ok(MailboxInsertOutcome::Inserted)
        }

        async fn by_message(
            &self,
            message_id: uuid::Uuid,
        ) -> Result<Option<MailboxStatusRow>, DomainError> {
            let rows = self.rows.lock().expect("mem mailbox poisoned");
            Ok(rows.get(&message_id).map(|(e, status, error)| MailboxStatusRow {
                message_id: e.message_id,
                correlation_id: e.correlation_id,
                status: *status,
                error: error.clone(),
                user_id: e.user_id,
                session_id: e.session_id,
                received_at: chrono::Utc::now(),
                completed_at: None,
            }))
        }
    }
}
