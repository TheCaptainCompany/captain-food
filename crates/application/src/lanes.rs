//! The lane SINK — where a ROUTED `deliver:` or `send:` step puts its intent
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
//! 3. **The door identity is FROZEN, and its second half is DECLARED — not inherited from the
//!    target**: `inbound_message_id(source, external_id)` where `source` is the ROUTE
//!    (`pm:{ProcessManager}:{Message}`) and `external_id` is that route's DEDUP AXIS. Never the
//!    triggering message's id, which changes on every redelivery and would mint a second birth.
//!    The axis is the value that makes two enqueues THE SAME REQUEST to the target, and it is the
//!    target aggregate's id only where the two coincide:
//!
//!    * a `deliver:` carries a fact the target absorbs onto its own stream, so the axis IS the
//!      target aggregate's id;
//!    * a `send:` takes whatever its spec step declares in `dedup_by:` — mandatory, with
//!      deliberately NO default (validator rule `pm-send-dedup`, plus a pre-emitter `panic!`).
//!      `GrantCustomerCredit` is keyed on the RECLAMATION while its ledger is keyed by CUSTOMER,
//!      and a customer legitimately receives many goodwill credits: a target-inherited axis would
//!      have keyed that door on the ledger and swallowed every credit after the first — money
//!      owed, never paid, no error raised anywhere.
//!
//!    Read the axis as "the same request", NOT as "the key the target handler is idempotent on":
//!    a target may REJECT a repeat rather than absorb it. `MarkOrderDelivered` is not idempotent —
//!    it refuses anything but `READY → DELIVERED` — so on that route the door is the only thing
//!    collapsing a partner report racing a rider completion, and it collapses them because both
//!    name the same ORDER, not because the handler would have tolerated the second. The corollary
//!    is the sharp edge of a rejecting target: a door minted by a REJECTED first attempt stays
//!    minted, so a later legitimate attempt on the same axis is absorbed with no effect
//!    (tracked as [#811](https://github.com/TheCaptainCompany/captain-food/issues/811), a
//!    property of every routed COMMAND door, not of one route).
//!
//!    Same treatment as `actor_client::surrogate_actor_id`: changing either half re-mints the
//!    identity of every in-flight and future routed message.
//!
//! A duplicate enqueue is a **SUCCESS** outcome to the process manager, never an error: the door
//! collides on the primary key and the run completes. That is why [`LaneSink::stage`] cannot fail.

use std::sync::Mutex;

/// Which DOOR a staged enqueue goes through — the runtime half of the DSL's own fact/command
/// distinction, made a type so a route cannot pick the wrong one by passing a string (vernon).
///
/// The two are not interchangeable, and the difference is what happens when the target REFUSES:
///
/// * [`LaneMessageKind::Event`] is a `deliver:` — a FACT the authority already decided. The target
///   records it idempotently; there is no verdict to disagree with, only a redelivery to absorb.
/// * [`LaneMessageKind::Command`] is a `send:` — a REQUEST the target may reject on its own
///   invariants. The lane worker runs the handler, a rejection lands a REJECTED verdict on a
///   supervisable row, and (the reason this variant exists at all) the delivery declares the
///   `schedules:` reminders the spec attaches to that `(actor, command)` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneMessageKind {
    /// `inbound_messages.kind = 'EVENT'` — the recorded-fact route.
    Event,
    /// `inbound_messages.kind = 'COMMAND'` — the rejectable-request route.
    Command,
}

impl LaneMessageKind {
    /// The `inbound_messages.kind` token. A closed set on both sides: the column's own CHECK
    /// constraint names the same strings.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Event => "EVENT",
            Self::Command => "COMMAND",
        }
    }
}

/// One staged lane ENQUEUE — the intent a routed `deliver:` or `send:` step produces.
///
/// Business payload plus addressing only. The ENVELOPE (`cause_id`, `user_id`/`user_type`,
/// `correlation_id`, `channel`, `partition`) is stamped by the delivery glue from the mailbox row
/// that triggered the saga hop — infrastructure adds the envelope, exactly as everywhere else
/// (ADR-0041).
#[derive(Debug, Clone, PartialEq)]
pub struct LaneEnqueue {
    /// Which door this intent goes through — and therefore which delivery route runs it.
    pub kind: LaneMessageKind,
    /// The target actor type (`actors.yaml` key) — its lane receives the message.
    pub actor_type: &'static str,
    /// The target aggregate's id — the lane the message is partitioned onto.
    pub actor_id: uuid::Uuid,
    /// The message name: an `events.yaml` key for [`LaneMessageKind::Event`] (the target must
    /// declare it in `receives` — validator rule `pm-deliver`), a `commands.yaml` key for
    /// [`LaneMessageKind::Command`] (`pm-sends-no-inbox`). Lands on `inbound_messages.message_type`
    /// either way, which is what both delivery routes and `reminder_schedules_for` match on.
    pub message_type: &'static str,
    /// EVENT: the ADJACENTLY-TAGGED `DomainEvent` form (`{"eventType", "payload"}`), exactly what
    /// the EVENT delivery route deserializes. COMMAND: the BARE command payload, exactly what
    /// `dispatch_command` deserializes into the generated command struct — the two routes read the
    /// column differently, so the shape is chosen with [`Self::kind`], never guessed.
    pub payload: serde_json::Value,
    /// FROZEN dedup axis, half one: the ROUTE identity (`pm:{ProcessManager}:{Message}`), so two
    /// different routed steps addressing the same aggregate can never collide.
    pub source: String,
    /// FROZEN dedup axis, half two: this ROUTE's DECLARED axis as text — the value that makes two
    /// enqueues the same request to the target. Never the trigger's message id. A `deliver:` uses
    /// the TARGET AGGREGATE's id; a `send:` uses the property its spec step names in `dedup_by:`
    /// (`pm-send-dedup`, no default), which is the target aggregate's id only where the two
    /// coincide — see the module docs for the money defect an inherited default would cause, and
    /// for why "the key the target handler is idempotent on" does not describe a REJECTING target.
    pub external_id: String,
}

/// Where a routed `deliver:` or `send:` step puts its intent. Infallible on purpose (see the
/// module docs: staging is inert, and a duplicate is a success), and `Debug` so it can sit on the
/// trigger envelope.
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
            kind: LaneMessageKind::Event,
            actor_type: "Order",
            actor_id: uuid::Uuid::from_u128(id),
            message_type: "OrderPlaced",
            payload: serde_json::json!({ "eventType": "OrderPlaced", "payload": {} }),
            source: "pm:PlaceOrderProcess:OrderPlaced".into(),
            external_id: uuid::Uuid::from_u128(id).to_string(),
        }
    }

    /// The two doors are DISTINCT staged values even for the same target — the property that stops
    /// a fact and a request being interchangeable once they are both just rows.
    #[test]
    fn the_door_kind_is_part_of_the_staged_intent() {
        let fact = intent(1);
        let request = LaneEnqueue { kind: LaneMessageKind::Command, ..intent(1) };
        assert_ne!(fact, request);
        assert_eq!(fact.kind.as_str(), "EVENT");
        assert_eq!(request.kind.as_str(), "COMMAND");
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
