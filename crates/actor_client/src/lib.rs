//! The ACTOR-CLIENT boundary crate (PROP-20260802-130500 D1/D4, #290 phase 1): the one door —
//! write AND read — to the `inbound_messages` actor mailbox.
//!
//! What lives here, and why HERE:
//!
//! - [`mailbox`] — the `Mailbox` PORT (implemented by `infrastructure::PgMailbox` over SQL), the
//!   [`mailbox::MailboxEntry`] row shape with **pub(crate) fields** (readable outside via getters,
//!   constructible nowhere else — the compiler is the door guard now), and the caller-supplied
//!   [`mailbox::Envelope`].
//! - [`generated::actor_clients`] — one strongly-typed client per mailbox actor (codegen from
//!   actors.yaml), the ONLY write door: sealed per-actor `Command`/`Fact` traits make sending a
//!   message the actor does not `receive` a COMPILE error.
//! - [`client::ActorClient`] — the one generic READ door over operation status
//!   (`get_operation_status(message_id)` + `watch(message_id)`, PROP-20260802-130500 D4, #303):
//!   status is an envelope-level outcome keyed by the globally-unique `message_id`, so the read
//!   side is actor-agnostic while the write side stays per-actor.
//! - [`status_bus`] — the in-process operation-response bus (§2.1 generalized, relocated from
//!   `infrastructure` under #303): publishers push post-commit verdicts, and the ONLY consumer
//!   surface is `ActorClient::watch` (`subscribe` is crate-internal).
//! - [`reminders`] — the reminder-row constructor (`scheduled_entry`) the in-transaction
//!   `schedules:` upsert binds from, and the pool-backed `declare`.
//! - [`stable_partition`] — the FROZEN partition routing hash (re-homed from `actor_runtime`,
//!   which never computes partitions itself: producers stamp them, and every producer lives
//!   behind this crate).
//!
//! Dependency direction: `actor_client -> application -> domain`; `infrastructure`, `server` and
//! the adapters depend on this crate. `actor_client` holds NO sqlx/reqwest — the D3 capability
//! allowlist (tools/codegen-rs `capability_dependencies_are_allowlisted`) keeps it that way.

pub mod client;
mod enqueue;
pub mod mailbox;
mod partition;
pub mod reminders;
pub mod status_bus;

/// GENERATED surface (do not edit by hand): the per-actor typed clients and the frozen
/// command-addressing tables, emitted by tools/codegen-rs from specs/actors.yaml.
pub mod generated;

pub use client::{ActorClient, OperationWatch, OperationWatchEvent};
pub use enqueue::{
    inbound_message_id, inbound_namespace, reminder_message_id, surrogate_actor_id,
    EnqueueOutcome, ScheduleOutcome,
};
// The D8-deferred UNTYPED bulk fact door (#290 review BLOCKING-1a): exported only under the
// `bulk-door` feature, which the guard test allows `infrastructure` alone to enable — its SIRENE
// sweep is the one sanctioned producer. The facts are receives-validated in `inbound_entry`, so
// even the bulk path cannot enqueue an event type its target actor does not declare.
#[cfg(feature = "bulk-door")]
pub use enqueue::{enqueue_inbound_facts, InboundFact};
pub use mailbox::{
    Envelope, Mailbox, MailboxEntry, MailboxInsertOutcome, MailboxScheduleOutcome,
    MailboxStatusRow,
};
pub use partition::stable_partition;
pub use status_bus::{OperationStatusBus, OperationUpdate};

// The drift-guard REFERENCE implementations (test-only, PROP-20260802-130500 D5): visible to
// other crates' tests through the `test-fixtures` feature, never to a release artifact.
#[cfg(any(test, feature = "test-fixtures"))]
pub use enqueue::{cancel_reminder, schedule_reminder};
