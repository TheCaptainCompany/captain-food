//! Reminder declaration — the runtime half of the actors.yaml `reminders:` + `schedules:` DSL
//! (ADR-20260731-214500 §2, semantics ADR-20260731-150500/-153000). A successful delivery of a
//! `schedules:`-bearing receive (re)declares its reminders through [`declare`]: one SCHEDULED
//! mailbox row per (actor, reminder), deterministic identity, rescheduled IN PLACE on
//! re-declaration. The generated `generated::reminders::REMINDER_SCHEDULES` table says WHICH
//! receives declare WHAT; this module builds the entry and talks to the [`Mailbox`] port — the
//! infrastructure delivery glue applies the same construction inside the completion transaction.

use crate::generated::reminders::ReminderSchedule;
use crate::mailbox::{Mailbox, MailboxEntry, MailboxScheduleOutcome};
use domain::shared::errors::DomainError;

/// The fixed UUIDv5 namespace for inbound/scheduled identities (PROP-20260728-152752 §3.4) —
/// deterministic, stable across deliveries and deployments. Shared with the adapter enqueue
/// helpers (infrastructure re-exports it): one namespace, one dedupe axis.
pub fn inbound_namespace() -> uuid::Uuid {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/integrations/inbound")
}

/// The mailbox identity of a reminder: `UUIDv5(actor_id, reminder name)` — one pending occurrence
/// per (actor, purpose), which is what makes re-declaration a RESCHEDULE and never a duplicate
/// (ADR-20260731-150500 §1).
pub fn reminder_message_id(actor_id: uuid::Uuid, reminder_name: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(&inbound_namespace(), format!("{actor_id}:{reminder_name}").as_bytes())
}

/// The reminder's payload — the adjacently-tagged FACT (`{"eventType","payload"}`), its single
/// field the actor's identity property (enforced at generation: a reminder fact requiring more
/// fails the emitter, not this builder).
pub fn tagged_payload(spec: &ReminderSchedule, actor_id: uuid::Uuid) -> serde_json::Value {
    serde_json::json!({
        "eventType": spec.payload_event,
        "payload": { spec.identity_prop: actor_id.to_string() },
    })
}

/// The full mailbox entry a declaration writes: kind MESSAGE, channel WORKER, `message_type` =
/// the payload FACT type (ADR-20260731-153000 §1a — the scheduled message is an event, recorded
/// with record semantics at delivery). The scheduling principal is the system scheduler
/// (ADR-0041 envelope metadata).
pub fn scheduled_entry(
    spec: &ReminderSchedule,
    actor_id: uuid::Uuid,
    partition: i16,
    correlation_id: uuid::Uuid,
    cause_id: Option<uuid::Uuid>,
) -> MailboxEntry {
    let payload = tagged_payload(spec, actor_id);
    let payload_hash = crate::journal::payload_hash(&payload);
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
        user_id: Some(uuid::Uuid::new_v5(&inbound_namespace(), b"system:scheduler")),
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
    mailbox.schedule(&scheduled_entry(spec, actor_id, partition, correlation_id, None), scheduled_at).await
}
