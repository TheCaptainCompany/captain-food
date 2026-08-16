//! The lane SINK — where a ROUTED `deliver:` step puts its intent
//! (ADR-20260816-040239 "`deliver:` is a lane ENQUEUE, not a foreign-stream append").
//!
//! A `deliver: <event> to: <actor>` step is a TELL. The process manager legitimately DECIDES that
//! the fact shall exist — it holds the frozen snapshot and the trigger's outcome — but **being the
//! birth AUTHORITY licenses the DECISION, never the APPEND**: writing the target aggregate's stream
//! from the saga puts two aggregates in one transaction and bypasses the target's own mailbox,
//! which is the serialization point for its writer (vernon).
//!
//! So the routed step STAGES an enqueue here, and the delivery glue
//! (`infrastructure::mailbox`) converts every staged intent into an `inbound_messages` row **inside
//! the same delivery transaction** — a degenerate outbox: same database, same commit, no dual
//! write. The target actor's lane worker then performs the append, its aggregate absorbs it
//! idempotently, and the delivery's `Recorded` verdict is what the schedules key on.
//!
//! Three constraints this module exists to make hard to break:
//!
//! 1. **Never in `prepare`.** `actor_runtime::completion` re-runs `prepare` with NO transaction
//!    open, and re-runs it on redelivery. Staging is inert by construction — nothing becomes true
//!    until the glue flushes the buffer into the fenced transaction — so a prepare-phase run that
//!    is thrown away enqueues nothing.
//! 2. **The insert rides the passed `&mut Transaction`**, never a pool handle. That is the glue's
//!    obligation; this module only hands it the intent.
//! 3. **The door identity is FROZEN**: `inbound_message_id(source, external_id)` where
//!    `external_id` is the TARGET AGGREGATE's id — never the triggering message's id, which
//!    changes on every redelivery and would mint a second birth. Same treatment as
//!    `actor_client::surrogate_actor_id`: changing this derivation re-mints the identity of every
//!    in-flight and future routed message.
//!
//! A duplicate enqueue is a **SUCCESS** outcome to the process manager, never an error: the door
//! collides on the primary key and the run completes. That is why [`LaneSink::stage`] cannot fail.

use std::sync::Mutex;

/// One staged lane ENQUEUE — the intent a routed `deliver:` step produces.
///
/// Business payload plus addressing only. The ENVELOPE (`cause_id`, `user_id`/`user_type`,
/// `correlation_id`, `channel`, `partition`) is stamped by the delivery glue from the mailbox row
/// that triggered the saga hop — infrastructure adds the envelope, exactly as everywhere else
/// (ADR-0041).
#[derive(Debug, Clone, PartialEq)]
pub struct LaneEnqueue {
    /// The target actor type (`actors.yaml` key) — its lane receives the message.
    pub actor_type: &'static str,
    /// The target aggregate's id — the lane the message is partitioned onto.
    pub actor_id: uuid::Uuid,
    /// `events.yaml` key; the target must declare it in `receives` (validator rule `pm-deliver`).
    pub event_type: &'static str,
    /// The ADJACENTLY-TAGGED `DomainEvent` form (`{"eventType", "payload"}`) — exactly what the
    /// EVENT delivery route deserializes.
    pub payload: serde_json::Value,
    /// FROZEN dedup axis, half one: the ROUTE identity (`pm:{ProcessManager}:{Event}`), so two
    /// different routed steps addressing the same aggregate can never collide.
    pub source: String,
    /// FROZEN dedup axis, half two: the TARGET AGGREGATE's id as text. Never the trigger's
    /// message id.
    pub external_id: String,
}

/// Where a routed `deliver:` step puts its intent. Infallible on purpose (see the module docs:
/// staging is inert, and a duplicate is a success), and `Debug` so it can sit on the trigger
/// envelope.
pub trait LaneSink: Send + Sync + std::fmt::Debug {
    /// Buffer one enqueue for the delivery glue to convert inside the fenced transaction.
    fn stage(&self, enqueue: LaneEnqueue);
}

/// The in-memory collector the mailbox delivery hands the saga — one instance per delivery, never
/// shared across messages (the buffer IS the delivery's uncommitted truth, mirroring
/// [`crate::staging::StagingEventStore`] exactly).
#[derive(Debug, Default)]
pub struct StagingLaneSink {
    staged: Mutex<Vec<LaneEnqueue>>,
}

impl StagingLaneSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drain the buffer for the in-tx flush (called once, after the handler returned Ok).
    pub fn take_staged(&self) -> Vec<LaneEnqueue> {
        std::mem::take(&mut self.staged.lock().expect("lane staging buffer poisoned"))
    }
}

impl LaneSink for StagingLaneSink {
    fn stage(&self, enqueue: LaneEnqueue) {
        self.staged.lock().expect("lane staging buffer poisoned").push(enqueue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(id: u128) -> LaneEnqueue {
        LaneEnqueue {
            actor_type: "Order",
            actor_id: uuid::Uuid::from_u128(id),
            event_type: "OrderPlaced",
            payload: serde_json::json!({ "eventType": "OrderPlaced", "payload": {} }),
            source: "pm:PlaceOrderProcess:OrderPlaced".into(),
            external_id: uuid::Uuid::from_u128(id).to_string(),
        }
    }

    /// Staging is INERT and ORDERED, and draining it twice yields nothing the second time — the
    /// property that makes a re-run `prepare` (no transaction open) harmless.
    #[test]
    fn staged_enqueues_drain_once_in_order() {
        let sink = StagingLaneSink::new();
        sink.stage(intent(1));
        sink.stage(intent(2));
        let first = sink.take_staged();
        assert_eq!(first, vec![intent(1), intent(2)]);
        assert!(sink.take_staged().is_empty(), "the buffer is consumed by the flush, once");
    }
}
