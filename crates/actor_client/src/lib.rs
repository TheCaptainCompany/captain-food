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
//!   (`get_operation_status(message_id)`, PROP-20260802-130500 D4): status is an envelope-level
//!   outcome keyed by the globally-unique `message_id`, so the read side is actor-agnostic while
//!   the write side stays per-actor.
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

/// GENERATED surface (do not edit by hand): the per-actor typed clients and the frozen
/// command-addressing tables, emitted by tools/codegen-rs from specs/actors.yaml.
pub mod generated;

pub use client::ActorClient;
pub use enqueue::{
    enqueue_inbound_facts, inbound_message_id, inbound_namespace, reminder_message_id,
    surrogate_actor_id, EnqueueOutcome, InboundFact, ScheduleOutcome,
};
pub use mailbox::{
    Envelope, Mailbox, MailboxEntry, MailboxInsertOutcome, MailboxScheduleOutcome,
    MailboxStatusRow,
};
pub use partition::stable_partition;

// The drift-guard REFERENCE implementations (test-only, PROP-20260802-130500 D5): visible to
// other crates' tests through the `test-fixtures` feature, never to a release artifact.
#[cfg(any(test, feature = "test-fixtures"))]
pub use enqueue::{cancel_reminder, schedule_reminder};
