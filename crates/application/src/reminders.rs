//! Reminder IDENTITY derivations — the pure half of the actors.yaml `reminders:` + `schedules:`
//! DSL (ADR-20260731-214500 §2, semantics ADR-20260731-150500/-153000): the fixed inbound UUIDv5
//! namespace, the deterministic reminder `message_id`, and the adjacently-tagged reminder payload.
//! The generated `generated::reminders::REMINDER_SCHEDULES` table says WHICH receives declare
//! WHAT; the mailbox-ROW construction (`scheduled_entry`, `declare`) moved to
//! `actor_client::reminders` with #290 phase 1 (PROP-20260802-130500 D1) — building a
//! `MailboxEntry` is the actor_client crate's exclusive permission, and this layer sits beneath it.

use crate::generated::reminders::ReminderSchedule;

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

