//! The write-side MAILBOX port (#242 Runtime C3, PROP-20260728-152752 §2): the typed door every
//! command submission enters through — an `inbound_messages` insert, idempotent by `message_id`
//! (same payload hash → the original acceptance replays; a different one → the caller raises the
//! synchronous Conflict), addressed by `(actor_type, actor_id)` with the partition stamped by the
//! FROZEN routing hash. The mailbox worker (not a spawned task) delivers.
//!
//! Since #290 phase 1 (PROP-20260802-130500 D1) this module is the COMPILER-ENFORCED boundary:
//! [`MailboxEntry`] has `pub(crate)` fields, so a row can be assembled only inside this crate —
//! by the shared constructors in `crate::enqueue` and `crate::reminders`, behind the generated
//! typed clients. Everyone else reads through the getters; obtaining an entry anywhere else does
//! not compile. Cross-crate TESTS go through [`fixtures`] (the D5 `test-fixtures` feature), never
//! through a public constructor.

use async_trait::async_trait;
use domain::generated::scalars::InboundMessageStatus;
use domain::shared::errors::DomainError;

/// One mailbox insert — the envelope is the columns (ADR-0041), `payload` is business-only.
///
/// Fields are `pub(crate)` ON PURPOSE: the crate is the permission (PROP-20260802-130500). The
/// SQL side (`infrastructure::PgMailbox`) binds columns through the getters below.
#[derive(Debug, Clone)]
pub struct MailboxEntry {
    pub(crate) message_id: uuid::Uuid,
    /// COMMAND | EVENT | MESSAGE.
    pub(crate) kind: String,
    pub(crate) actor_type: String,
    pub(crate) actor_id: uuid::Uuid,
    /// `stable_partition(actor_id, width)` — stamped by the constructors in this crate (the
    /// client knows the partitioning; the table stays a dumb mailbox).
    pub(crate) partition: i16,
    pub(crate) message_type: String,
    pub(crate) payload: serde_json::Value,
    pub(crate) payload_hash: String,
    /// GRAPHQL | WORKER | EXTERNAL.
    pub(crate) channel: String,
    pub(crate) user_id: Option<uuid::Uuid>,
    pub(crate) user_type: String,
    pub(crate) correlation_id: uuid::Uuid,
    pub(crate) cause_id: Option<uuid::Uuid>,
    pub(crate) session_id: Option<uuid::Uuid>,
    pub(crate) trace_id: Option<String>,
    /// Owning adapter for kind EVENT ('stripe', …) — with `external_id`, the delivery-level dedupe.
    pub(crate) source: Option<String>,
    pub(crate) external_id: Option<String>,
}

/// The READ surface of an entry: one getter per column, for the SQL adapter that binds them and
/// the tests that assert them. No setter, no public constructor — that is the point.
impl MailboxEntry {
    pub fn message_id(&self) -> uuid::Uuid {
        self.message_id
    }
    /// COMMAND | EVENT | MESSAGE.
    pub fn kind(&self) -> &str {
        &self.kind
    }
    pub fn actor_type(&self) -> &str {
        &self.actor_type
    }
    pub fn actor_id(&self) -> uuid::Uuid {
        self.actor_id
    }
    pub fn partition(&self) -> i16 {
        self.partition
    }
    pub fn message_type(&self) -> &str {
        &self.message_type
    }
    pub fn payload(&self) -> &serde_json::Value {
        &self.payload
    }
    pub fn payload_hash(&self) -> &str {
        &self.payload_hash
    }
    /// GRAPHQL | WORKER | EXTERNAL.
    pub fn channel(&self) -> &str {
        &self.channel
    }
    pub fn user_id(&self) -> Option<uuid::Uuid> {
        self.user_id
    }
    pub fn user_type(&self) -> &str {
        &self.user_type
    }
    pub fn correlation_id(&self) -> uuid::Uuid {
        self.correlation_id
    }
    pub fn cause_id(&self) -> Option<uuid::Uuid> {
        self.cause_id
    }
    pub fn session_id(&self) -> Option<uuid::Uuid> {
        self.session_id
    }
    pub fn trace_id(&self) -> Option<&str> {
        self.trace_id.as_deref()
    }
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
    pub fn external_id(&self) -> Option<&str> {
        self.external_id.as_deref()
    }
}

/// Cross-crate TEST access to the raw entry shape (PROP-20260802-130500 D5): compiled only for
/// this crate's own tests or under the `test-fixtures` feature, which only `[dev-dependencies]`
/// may enable (CI-guarded). The exhaustiveness CONTRACT: both conversions below list every field
/// with no `..` rest pattern, so adding an 18th `MailboxEntry` column breaks THIS module's
/// compilation — which forces [`EntryFixture`] to grow with it, which keeps every out-of-crate
/// full-destructure freeze test (e.g. `graphql_typed_send`) exhaustive by construction.
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures {
    use super::MailboxEntry;

    /// The all-public mirror of [`MailboxEntry`] for tests: seed rows with
    /// `EntryFixture { .. }.into()`, freeze rows with `entry.into_fixture()` + full destructure.
    #[derive(Debug, Clone)]
    pub struct EntryFixture {
        pub message_id: uuid::Uuid,
        pub kind: String,
        pub actor_type: String,
        pub actor_id: uuid::Uuid,
        pub partition: i16,
        pub message_type: String,
        pub payload: serde_json::Value,
        pub payload_hash: String,
        pub channel: String,
        pub user_id: Option<uuid::Uuid>,
        pub user_type: String,
        pub correlation_id: uuid::Uuid,
        pub cause_id: Option<uuid::Uuid>,
        pub session_id: Option<uuid::Uuid>,
        pub trace_id: Option<String>,
        pub source: Option<String>,
        pub external_id: Option<String>,
    }

    impl From<EntryFixture> for MailboxEntry {
        fn from(f: EntryFixture) -> Self {
            // FULL destructure + full construction, no `..`: the compile-time lockstep guarantee.
            let EntryFixture {
                message_id,
                kind,
                actor_type,
                actor_id,
                partition,
                message_type,
                payload,
                payload_hash,
                channel,
                user_id,
                user_type,
                correlation_id,
                cause_id,
                session_id,
                trace_id,
                source,
                external_id,
            } = f;
            MailboxEntry {
                message_id,
                kind,
                actor_type,
                actor_id,
                partition,
                message_type,
                payload,
                payload_hash,
                channel,
                user_id,
                user_type,
                correlation_id,
                cause_id,
                session_id,
                trace_id,
                source,
                external_id,
            }
        }
    }

    impl MailboxEntry {
        /// The entry as its all-public test mirror — the read half of the fixture contract.
        pub fn into_fixture(self) -> EntryFixture {
            let MailboxEntry {
                message_id,
                kind,
                actor_type,
                actor_id,
                partition,
                message_type,
                payload,
                payload_hash,
                channel,
                user_id,
                user_type,
                correlation_id,
                cause_id,
                session_id,
                trace_id,
                source,
                external_id,
            } = self;
            EntryFixture {
                message_id,
                kind,
                actor_type,
                actor_id,
                partition,
                message_type,
                payload,
                payload_hash,
                channel,
                user_id,
                user_type,
                correlation_id,
                cause_id,
                session_id,
                trace_id,
                source,
                external_id,
            }
        }
    }
}

/// The transport ENVELOPE a caller supplies with one typed message (#284 slice 1,
/// PROP-20260728-152752 §2.1) — identity, causation and provenance of the REQUEST, nothing else.
///
/// The split is deliberate: the envelope carries what only the CALLER knows (which request this
/// is, what caused it, who is acting, over which channel), while the typed actor client owns
/// everything the SPEC knows — the payload (a typed command/fact), the actor addressing
/// (`actor_type`, `actor_id`, the frozen partition) and the message kind. An envelope can
/// therefore never mis-address a message, and a client can never forge a caller identity.
#[derive(Debug, Clone)]
pub struct Envelope {
    pub message_id: uuid::Uuid,
    pub correlation_id: uuid::Uuid,
    pub cause_id: Option<uuid::Uuid>,
    pub session_id: Option<uuid::Uuid>,
    pub trace_id: Option<String>,
    pub user_id: Option<uuid::Uuid>,
    /// `UserType` TEXT value, stored verbatim (ADR-20260728).
    pub user_type: String,
    /// GRAPHQL | WORKER | EXTERNAL.
    pub channel: String,
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

    /// The row behind an acceptance handle. Callers outside this crate read it through
    /// [`crate::ActorClient::get_operation_status`] — the D4 read door — never by naming this
    /// method on a bare pool.
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

/// In-memory double for tests (mirrors `journal::mem::MemCommandJournal`). Compiled only for this
/// crate's own tests or under the D5 `test-fixtures` feature — a mem double in a release
/// artifact would be a mailbox that silently swallows commands.
#[cfg(any(test, feature = "test-fixtures"))]
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
            payload_hash: application::journal::payload_hash(&payload),
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
