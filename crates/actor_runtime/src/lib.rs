//! The ACTOR MAILBOX RUNTIME (PROP-20260728-152752 §3/§3.1/§3.5, ADR-20260730-231500) — durable,
//! partitioned, lease-balanced message consumption over two Postgres tables:
//!
//! - `inbound_messages` — the mailbox: one row per message to one actor, addressed by
//!   `(actor_type, actor_id)`, consumed in `position` order per partition (head-of-line);
//! - `mailbox_partitions` — the registry: one row per `(actor_type, partition)` carrying the
//!   CHECKPOINT (a monotonic delivered-position high-water mark — supervision visibility and the
//!   fence's write target, NOT a consumption filter: sequence-allocated positions are not
//!   commit-ordered, so filtering on it would strand late-committing rows; `status = 'RECEIVED'`
//!   alone defines what is undelivered), the LEASE (`claimed_by` + `lease_until`,
//!   heartbeat-renewed, expiry-takeover — no coordinator), and the OWNERSHIP_VERSION fencing
//!   counter (incremented on every ownership change, asserted inside every completion
//!   transaction: a stale owner's commit matches 0 rows and the WHOLE transaction rolls back,
//!   handler effects included).
//!
//! What Proto.Actor taught us, applied (ADR-20260730-234918 D2.1): there is no durable mailbox to
//! borrow anywhere — this one is ours; the fencing must be LIVE, not decorative (Go's is dead
//! code); duplicates are PREVENTED by the fence, not repaired after the fact (.NET repairs); an
//! ownership change invalidates in-flight work (the topology-validity-token idea — here, the
//! fence makes stale work unable to commit, which is stronger than cancelling it).
//!
//! EXTRACTION-READY: generic over [`MessageHandler`]; zero Captain.Food domain dependencies
//! (enforced by `tests/dependency_rule.rs`). The host product supplies the handler (its command
//! dispatch) and the seeding widths (its actor catalog); this crate owns claim, drain, fence,
//! checkpoint, and nothing else.

pub mod activation;
pub mod completion;
pub mod lease;
pub mod message;
pub mod schedule;
pub mod worker;

pub use activation::ActivationCache;
pub use completion::{complete_fenced, CompletionError};
pub use lease::{
    claim_due_lanes, heartbeat, ownership_census, release_lane, seed_partitions, steal_from,
    steal_lane, Lane, OwnershipCensus,
};
pub use message::{Delivery, DeliveryObserver, HandlerVerdict, InboundMessage, MessageHandler, Prepared};
// NOTE (#290 phase 1, PROP-20260802-130500 D1): `stable_partition` — the FROZEN producer-side
// routing hash — moved to the `actor_client` boundary crate. This runtime never computes a
// partition itself (producers stamp the column; the drain trusts it), so the function was an
// export of convenience here, and keeping it would have forced the client crate to depend on the
// whole runtime for one pure function.
pub use schedule::promote_due;
pub use worker::{LaneEvents, MailboxWorker, WorkerConfig};
