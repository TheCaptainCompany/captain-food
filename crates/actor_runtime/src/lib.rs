//! The ACTOR MAILBOX RUNTIME (PROP-20260728-152752 §3/§3.1/§3.5, ADR-20260730-231500) — durable,
//! partitioned, lease-balanced message consumption over two Postgres tables:
//!
//! - `inbound_messages` — the mailbox: one row per message to one actor, addressed by
//!   `(actor_type, actor_id)`, consumed in `position` order per partition (head-of-line);
//! - `mailbox_partitions` — the registry: one row per `(actor_type, partition)` carrying the
//!   CHECKPOINT (everything at or below it is terminal), the LEASE (`claimed_by` + `lease_until`,
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

pub mod completion;
pub mod lease;
pub mod message;
pub mod partition;
pub mod worker;

pub use completion::{complete_fenced, CompletionError};
pub use lease::{claim_due_lanes, heartbeat, release_lane, seed_partitions, steal_lane, Lane};
pub use message::{Delivery, DeliveryObserver, HandlerVerdict, InboundMessage, MessageHandler};
pub use partition::stable_partition;
pub use worker::{MailboxWorker, WorkerConfig};
