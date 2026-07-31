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
