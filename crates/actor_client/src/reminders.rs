//! Reminder-row construction — the mailbox-entry half of the actors.yaml `reminders:` +
//! `schedules:` DSL (ADR-20260731-214500 §2, semantics ADR-20260731-150500/-153000). Moved here
//! from `application::reminders` with #290 phase 1 (PROP-20260802-130500 D1): building a
//! [`MailboxEntry`] is this crate's exclusive permission, so the constructor lives behind the
//! same wall as every other one. The pure identity derivations (`inbound_namespace`,
//! `reminder_message_id`, `tagged_payload`) stay in `application::reminders` — they build ids and
//! payloads, not rows.

use application::generated::reminders::ReminderSchedule;
use application::reminders::{reminder_message_id, tagged_payload};
use domain::shared::errors::DomainError;

use crate::mailbox::{Mailbox, MailboxAccess, MailboxEntry, MailboxScheduleOutcome};

/// Re-exported for the generated behaviour tests in `application` (dev-dep cycle, D5): inside
/// application's own test build, `crate::generated::reminders::ReminderSchedule` and the
/// `application` THIS crate links are two distinct compilations of the same source — passing one
/// to [`declare`] (which expects the other) is a type error. Reading the schedule table through
/// this re-export keeps the whole test block on ONE side of that seam.
pub use application::generated::reminders::reminder_schedules_for;

/// The full mailbox entry a declaration writes: kind MESSAGE, channel WORKER, `message_type` =
/// the payload FACT type (ADR-20260731-153000 §1a — the scheduled message is an event, recorded
/// with record semantics at delivery). The scheduling principal is the system scheduler
/// (ADR-0041 envelope metadata). The infrastructure delivery glue (`apply_schedules_in_tx`) binds
/// this construction inside the completion transaction, reading columns through the getters.
pub fn scheduled_entry(
    spec: &ReminderSchedule,
    actor_id: uuid::Uuid,
    partition: i16,
    correlation_id: uuid::Uuid,
    cause_id: Option<uuid::Uuid>,
) -> MailboxEntry {
    let payload = tagged_payload(spec, actor_id);
    let payload_hash = application::journal::payload_hash(&payload);
    MailboxEntry {
        message_id: reminder_message_id(actor_id, spec.reminder),
        kind: "MESSAGE".into(),
        actor_type: spec.actor_type.into(),
        actor_id,
        partition,
        message_type: spec.payload_event.into(),
        payload,
        payload_hash,
        channel: "WORKER".into(),
        user_id: Some(uuid::Uuid::new_v5(&application::reminders::inbound_namespace(), b"system:scheduler")),
        user_type: "EXTERNAL".into(),
        correlation_id,
        cause_id,
        session_id: None,
        trace_id: None,
        source: None,
        external_id: None,
    }
}

/// Declare (or re-declare) one reminder through the [`Mailbox`] port: "schedule it for the time
/// you NOW want" — idempotent and self-postponing (ADR-20260731-150500). Used by tests and
/// pool-side callers; the mailbox delivery glue applies [`scheduled_entry`] inside its completion
/// transaction instead, so a committed delivery can never lose its declared reminder.
pub async fn declare(
    mailbox: &dyn Mailbox,
    spec: &ReminderSchedule,
    actor_id: uuid::Uuid,
    partition: i16,
    scheduled_at: chrono::DateTime<chrono::Utc>,
    correlation_id: uuid::Uuid,
) -> Result<MailboxScheduleOutcome, DomainError> {
    mailbox
        .schedule(
            &scheduled_entry(spec, actor_id, partition, correlation_id, None),
            scheduled_at,
            MailboxAccess::granted(),
        )
        .await
}
