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

/// What a schedule did — the ADR-20260731-150500 outcome triple for kind MESSAGE. Deliberately a
/// SEPARATE enum from [`MailboxInsertOutcome`]: the generated GraphQL resolvers match the insert
/// outcome exhaustively, and a reminder outcome must never widen the acceptance contract they
/// implement.
#[derive(Debug, Clone, PartialEq)]
pub enum MailboxScheduleOutcome {
    /// Fresh SCHEDULED row — deliverable only once the promotion pass finds it due.
    Scheduled,
    /// The identity was still SCHEDULED: `scheduled_at` + payload moved IN PLACE — same row,
    /// same history, one pending occurrence per (actor, purpose) (ADR-20260731-150500 §1).
    Rescheduled,
    /// The pending occurrence is SPENT (promoted or terminal): the row is untouched
    /// (ADR-20260731-150500 §2) — replay vs conflict is the caller's call, as on insert.
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
    /// The accepted payload's hash — the cross-arm duplicate check compares against it
    /// (#272 review MAJOR-3: a legacy-arm retry of a mailbox-accepted messageId must replay,
    /// and a DIFFERENT payload under the same id must Conflict, exactly like same-store dedupe).
    pub payload_hash: String,
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

    /// Persist MANY entries as RECEIVED in one round-trip, returning the `message_id`s that were
    /// actually inserted; the rest collided on the pk and are already on the mailbox.
    ///
    /// Exists because a per-row `insert` makes a bulk producer latency-bound rather than
    /// throughput-bound: the SIRENE sweep ran at ~628 rows/min against ~3,800/min of ingest, and
    /// ~99% of that wall-clock was round-trips, not work (the same finding as #215/#216 on the
    /// ingest side). Callers that hand over a whole batch at once should not pay per row for it.
    ///
    /// The dedupe contract is deliberately WEAKER than `insert`'s: this reports *whether* a row was
    /// new, not the existing row's status and hash, because distinguishing `Deduplicated` from
    /// `PayloadConflict` requires reading every collided row back. A caller that needs that
    /// distinction must use `insert`. For an idempotent producer keyed on a content hash — where a
    /// collision means "this exact version is already here" — it is not needed.
    ///
    /// Default: a correct row-by-row fallback, so an implementation only overrides it to go faster.
    async fn insert_many(
        &self,
        entries: &[MailboxEntry],
    ) -> Result<Vec<uuid::Uuid>, DomainError> {
        let mut inserted = Vec::new();
        for entry in entries {
            if matches!(self.insert(entry).await?, MailboxInsertOutcome::Inserted) {
                inserted.push(entry.message_id);
            }
        }
        Ok(inserted)
    }

    /// The row behind an acceptance handle (the `operationStatus` lookup).
    async fn by_message(
        &self,
        message_id: uuid::Uuid,
    ) -> Result<Option<MailboxStatusRow>, DomainError>;

    /// Persist the entry as SCHEDULED (`position` NULL until the promotion pass stamps one).
    /// Re-declaring an identity that is still SCHEDULED postpones IN PLACE — `scheduled_at` and
    /// the payload move, nothing else (ADR-20260731-150500); a collision with a promoted or
    /// terminal row leaves it untouched.
    async fn schedule(
        &self,
        entry: &MailboxEntry,
        scheduled_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<MailboxScheduleOutcome, DomainError>;

    /// The explicit withdrawal: `SCHEDULED → CANCELLED` (ADR-20260731-150500 §3). `false` when
    /// the row is absent or no longer SCHEDULED — a cancellation that raced promotion and lost,
    /// which is a fact for the caller, not an error.
    async fn cancel_scheduled(&self, message_id: uuid::Uuid) -> Result<bool, DomainError>;
}

/// In-memory double for tests (mirrors `journal::mem::MemCommandJournal`).
pub mod mem {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    struct MemRow {
        entry: MailboxEntry,
        status: InboundMessageStatus,
        error: Option<serde_json::Value>,
        /// Reminder rows only — mirrors the `scheduled_at` column (NULL for immediate rows).
        scheduled_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    #[derive(Default)]
    pub struct MemMailbox {
        rows: Mutex<HashMap<uuid::Uuid, MemRow>>,
        /// When set, inserts land SUCCEEDED immediately — the fake stands in for a mailbox WITH a
        /// live worker, for flows that await a sent command's terminal status (HubRise connect).
        /// IMMEDIATE rows only: a scheduled row stays SCHEDULED (no worker delivers the undue).
        auto_succeed: bool,
    }

    impl MemMailbox {
        /// A fake whose worker "delivers" instantly: every insert lands SUCCEEDED.
        pub fn instantly_delivered() -> Self {
            Self { auto_succeed: true, ..Self::default() }
        }

        /// Snapshot of every stored entry, insertion-order-independent (test assertions).
        pub fn entries(&self) -> Vec<MailboxEntry> {
            self.rows.lock().expect("mem mailbox poisoned").values().map(|r| r.entry.clone()).collect()
        }

        /// One entry by message id (test assertions).
        pub fn entry(&self, message_id: uuid::Uuid) -> Option<MailboxEntry> {
            self.rows
                .lock()
                .expect("mem mailbox poisoned")
                .get(&message_id)
                .map(|r| r.entry.clone())
        }

        /// One row's `scheduled_at` (test assertions) — `None` when the row is absent.
        pub fn scheduled_at(
            &self,
            message_id: uuid::Uuid,
        ) -> Option<chrono::DateTime<chrono::Utc>> {
            self.rows
                .lock()
                .expect("mem mailbox poisoned")
                .get(&message_id)
                .and_then(|r| r.scheduled_at)
        }
    }

    #[async_trait]
    impl Mailbox for MemMailbox {
        async fn insert(&self, entry: &MailboxEntry) -> Result<MailboxInsertOutcome, DomainError> {
            let mut rows = self.rows.lock().expect("mem mailbox poisoned");
            if let Some(existing) = rows.get(&entry.message_id) {
                return Ok(MailboxInsertOutcome::Duplicate {
                    status: existing.status,
                    payload_hash: existing.entry.payload_hash.clone(),
                });
            }
            let status = if self.auto_succeed {
                InboundMessageStatus::SUCCEEDED
            } else {
                InboundMessageStatus::RECEIVED
            };
            rows.insert(
                entry.message_id,
                MemRow { entry: entry.clone(), status, error: None, scheduled_at: None },
            );
            Ok(MailboxInsertOutcome::Inserted)
        }

        async fn by_message(
            &self,
            message_id: uuid::Uuid,
        ) -> Result<Option<MailboxStatusRow>, DomainError> {
            let rows = self.rows.lock().expect("mem mailbox poisoned");
            Ok(rows.get(&message_id).map(|r| MailboxStatusRow {
                message_id: r.entry.message_id,
                correlation_id: r.entry.correlation_id,
                status: r.status,
                error: r.error.clone(),
                payload_hash: r.entry.payload_hash.clone(),
                user_id: r.entry.user_id,
                session_id: r.entry.session_id,
                received_at: chrono::Utc::now(),
                completed_at: None,
            }))
        }

        async fn schedule(
            &self,
            entry: &MailboxEntry,
            scheduled_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<MailboxScheduleOutcome, DomainError> {
            let mut rows = self.rows.lock().expect("mem mailbox poisoned");
            match rows.get_mut(&entry.message_id) {
                None => {
                    rows.insert(
                        entry.message_id,
                        MemRow {
                            entry: entry.clone(),
                            status: InboundMessageStatus::SCHEDULED,
                            error: None,
                            scheduled_at: Some(scheduled_at),
                        },
                    );
                    Ok(MailboxScheduleOutcome::Scheduled)
                }
                Some(row) if row.status == InboundMessageStatus::SCHEDULED => {
                    row.scheduled_at = Some(scheduled_at);
                    row.entry.payload = entry.payload.clone();
                    row.entry.payload_hash = entry.payload_hash.clone();
                    Ok(MailboxScheduleOutcome::Rescheduled)
                }
                Some(row) => Ok(MailboxScheduleOutcome::Duplicate {
                    status: row.status,
                    payload_hash: row.entry.payload_hash.clone(),
                }),
            }
        }

        async fn cancel_scheduled(&self, message_id: uuid::Uuid) -> Result<bool, DomainError> {
            let mut rows = self.rows.lock().expect("mem mailbox poisoned");
            match rows.get_mut(&message_id) {
                Some(row) if row.status == InboundMessageStatus::SCHEDULED => {
                    row.status = InboundMessageStatus::CANCELLED;
                    Ok(true)
                }
                _ => Ok(false),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mem::MemMailbox;
    use super::*;

    fn entry(payload: serde_json::Value) -> MailboxEntry {
        MailboxEntry {
            message_id: uuid::Uuid::from_u128(0x5EED),
            kind: "MESSAGE".into(),
            actor_type: "Order".into(),
            actor_id: uuid::Uuid::from_u128(0x0AD1),
            partition: 0,
            message_type: "OrderExpired".into(),
            payload_hash: crate::journal::payload_hash(&payload),
            payload,
            channel: "WORKER".into(),
            user_id: None,
            user_type: "EXTERNAL".into(),
            correlation_id: uuid::Uuid::from_u128(0xC0),
            cause_id: None,
            session_id: None,
            trace_id: None,
            source: None,
            external_id: None,
        }
    }

    /// ADR-20260731-150500 §1: a SCHEDULED-row collision reschedules IN PLACE — one row, the
    /// latest time, the latest payload.
    #[tokio::test]
    async fn mem_schedule_reschedules_in_place_while_scheduled() {
        let mailbox = MemMailbox::default();
        let t1 = chrono::Utc::now() + chrono::Duration::hours(1);
        let t2 = t1 + chrono::Duration::hours(1);
        let first = entry(serde_json::json!({"eventType": "OrderExpired"}));
        assert_eq!(
            mailbox.schedule(&first, t1).await.unwrap(),
            MailboxScheduleOutcome::Scheduled
        );
        let second = entry(serde_json::json!({"eventType": "OrderExpired", "window": "P2Y"}));
        assert_eq!(
            mailbox.schedule(&second, t2).await.unwrap(),
            MailboxScheduleOutcome::Rescheduled
        );
        assert_eq!(mailbox.entries().len(), 1, "one pending occurrence per (actor, purpose)");
        assert_eq!(mailbox.scheduled_at(first.message_id), Some(t2));
        assert_eq!(mailbox.entry(first.message_id).unwrap().payload_hash, second.payload_hash);
    }

    /// ADR-20260731-150500 §2/§3: a spent occurrence is a Duplicate, and cancellation only wins
    /// while the row is still SCHEDULED.
    #[tokio::test]
    async fn mem_spent_rows_are_duplicates_and_cancel_is_scheduled_only() {
        let mailbox = MemMailbox::default();
        let e = entry(serde_json::json!({"eventType": "OrderExpired"}));
        // An immediate insert (RECEIVED) is a spent identity for any later schedule.
        mailbox.insert(&e).await.unwrap();
        match mailbox.schedule(&e, chrono::Utc::now()).await.unwrap() {
            MailboxScheduleOutcome::Duplicate { status, .. } => {
                assert_eq!(status, InboundMessageStatus::RECEIVED)
            }
            other => panic!("expected Duplicate, got {other:?}"),
        }
        assert!(!mailbox.cancel_scheduled(e.message_id).await.unwrap(), "RECEIVED is not cancellable");

        let scheduled = MailboxEntry { message_id: uuid::Uuid::from_u128(0x5EED2), ..entry(serde_json::json!({})) };
        mailbox.schedule(&scheduled, chrono::Utc::now()).await.unwrap();
        assert!(mailbox.cancel_scheduled(scheduled.message_id).await.unwrap());
        assert!(!mailbox.cancel_scheduled(scheduled.message_id).await.unwrap(), "already CANCELLED");
        assert_eq!(
            mailbox.by_message(scheduled.message_id).await.unwrap().unwrap().status,
            InboundMessageStatus::CANCELLED
        );
    }
}
