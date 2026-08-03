//! The generic READ door over operation status (PROP-20260802-130500 D4, #290 phase 1).
//!
//! The shape follows the data (product-owner directive, 2026-08-02): **operation status is
//! generic to all operations** — `message_id` is globally unique and the outcome
//! (PENDING/SUCCEEDED/REJECTED/…) is an ENVELOPE-level fact carrying nothing actor-specific — so
//! it is deliberately NOT a method on the per-actor typed clients (that would pretend a generic
//! read is actor-specific, sixteen times), and there is no separate `OperationStatusClient`
//! concept. The split: **per-actor typed clients = the write side** (send/record/schedule/cancel,
//! where WHICH actor matters at compile time); **the one generic [`ActorClient`] = the read
//! side** (where it does not).
//!
//! This is the ONLY sanctioned read path over `inbound_messages` status: the `operationStatus`
//! query and the `operationStatusChanged` snapshot both resolve through it, and nobody SELECTs
//! the table (the D3 capability allowlist keeps SQL out of every crate that could try).
//!
//! `watch(message_id)` (the §2.1 response-bus subscription, #303) is the push half: the
//! relocated [`crate::status_bus::OperationStatusBus`] lives BEHIND this door now, so a caller
//! subscribes to the response of its message through the same client that serves the snapshot —
//! pull first, then stream, the first-value-then-transitions contract of ADR-20260720-015500.

use std::sync::Arc;

use domain::shared::errors::DomainError;
use tokio::sync::broadcast;

use crate::mailbox::{Mailbox, MailboxAccess, MailboxStatusRow};
use crate::status_bus::{OperationStatusBus, OperationUpdate};

/// The one generic, actor-agnostic client: holds the read capability over operation status —
/// the durable row (pull) and the post-commit response stream (push).
pub struct ActorClient {
    mailbox: Arc<dyn Mailbox>,
    /// `None` on a [`ActorClient::pull_only`] door — see there for why that is a real deployment
    /// posture and not a degraded one.
    bus: Option<OperationStatusBus>,
}

impl ActorClient {
    /// The full door: durable row (pull) + post-commit response stream (push).
    pub fn new(mailbox: Arc<dyn Mailbox>, bus: OperationStatusBus) -> Self {
        Self { mailbox, bus: Some(bus) }
    }

    /// The PULL-ONLY door, for a caller that has no response bus to share (#304 review).
    ///
    /// A standalone adapter is exactly that: `run_standalone_workers` builds its own local,
    /// subscriber-less bus, so there is no process-wide bus to hand a client. Handing such a
    /// caller `OperationStatusBus::default()` would compile and then LIE — it constructs a live
    /// broadcast channel whose only sender is the client's own field, so a later `watch` would
    /// await forever: never a message (nothing publishes), never `Closed` (the client holds the
    /// sender). A hang, in one deployment topology only.
    ///
    /// This constructor puts that posture in the type instead: `watch` returns `None`, which the
    /// caller must handle, and `get_operation_status` — the durable read, correct in every
    /// topology — is unaffected.
    pub fn pull_only(mailbox: Arc<dyn Mailbox>) -> Self {
        Self { mailbox, bus: None }
    }

    /// The status of one operation by its globally-unique acceptance handle — the row behind
    /// `operationStatus` / the `operationStatusChanged` snapshot. `None` = unknown identity; the
    /// OWNERSHIP decision (no existence oracle, ADR-20260720-015500) stays with the caller, which
    /// alone knows the request principal.
    pub async fn get_operation_status(
        &self,
        message_id: uuid::Uuid,
    ) -> Result<Option<MailboxStatusRow>, DomainError> {
        self.mailbox.by_message(message_id, MailboxAccess::granted()).await
    }

    /// Subscribe to the response stream of ONE operation (§2.1 `watch`, #303): every post-commit
    /// transition published for `message_id`, in publish order, with lag made explicit. Subscribe
    /// BEFORE the snapshot read to close the subscribe/complete race — the returned watch only
    /// sees updates published after this call. The stream is notification, never truth: on
    /// [`OperationWatchEvent::Lagged`] re-read via [`ActorClient::get_operation_status`], and the
    /// OWNERSHIP decision (no existence oracle, ADR-20260720-015500) stays with the caller.
    /// `None` on a [`ActorClient::pull_only`] door, which has no response stream to offer — the
    /// caller falls back to polling [`ActorClient::get_operation_status`].
    pub fn watch(&self, message_id: uuid::Uuid) -> Option<OperationWatch> {
        Some(OperationWatch { message_id, rx: self.bus.as_ref()?.subscribe() })
    }
}

/// One event on an [`ActorClient::watch`] stream.
#[derive(Debug, Clone)]
pub enum OperationWatchEvent {
    /// A post-commit transition of the watched operation.
    Update(OperationUpdate),
    /// The watcher fell behind the bus's retention and transitions were dropped. The durable row
    /// is the pull truth — re-read via [`ActorClient::get_operation_status`].
    Lagged,
}

/// A live subscription to one operation's responses, from [`ActorClient::watch`]. Updates for
/// other operations on the shared bus are filtered out here, so the caller never sees them.
pub struct OperationWatch {
    message_id: uuid::Uuid,
    rx: broadcast::Receiver<OperationUpdate>,
}

impl OperationWatch {
    /// The next event for this operation; `None` once the bus closes (every publisher dropped —
    /// process teardown in practice).
    pub async fn next(&mut self) -> Option<OperationWatchEvent> {
        loop {
            match self.rx.recv().await {
                Ok(update) if update.message_id == self.message_id => {
                    return Some(OperationWatchEvent::Update(update));
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    return Some(OperationWatchEvent::Lagged);
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domain::generated::scalars::InboundMessageStatus;

    use super::ActorClient;
    use crate::mailbox::mem::MemMailbox;
    use crate::mailbox::{Envelope, Mailbox};

    /// The read door answers with the very row the write door accepted — same identity, same
    /// status the mem worker left it in — and `None` for an unknown handle.
    #[tokio::test]
    async fn get_operation_status_reads_the_accepted_row_and_none_for_unknown() {
        let mem = Arc::new(MemMailbox::default());
        let restaurant_id = uuid::Uuid::from_u128(0xF00D);
        let message_id = uuid::Uuid::from_u128(0x0B5);
        let cmd = domain::generated::commands::MarkRestaurantClosed {
            restaurant_id: domain::generated::scalars::RestaurantId(restaurant_id),
            reason: None,
        };
        let writer = crate::generated::actor_clients::RestaurantClient::new(
            mem.clone() as Arc<dyn Mailbox>,
            restaurant_id,
        );
        writer
            .send(
                cmd,
                Envelope {
                    message_id,
                    correlation_id: message_id,
                    cause_id: None,
                    session_id: None,
                    trace_id: None,
                    user_id: None,
                    user_type: "EXTERNAL".into(),
                    channel: "WORKER".into(),
                },
            )
            .await
            .expect("typed send");

        let reader = ActorClient::new(mem.clone(), crate::status_bus::OperationStatusBus::default());
        let row = reader
            .get_operation_status(message_id)
            .await
            .expect("read")
            .expect("the accepted row is visible through the read door");
        assert_eq!(row.message_id, message_id);
        assert_eq!(row.correlation_id, message_id);
        assert_eq!(row.status, InboundMessageStatus::RECEIVED);

        assert!(
            reader
                .get_operation_status(uuid::Uuid::from_u128(0xDEAD))
                .await
                .expect("read")
                .is_none(),
            "an unknown handle is None — the ownership/oracle policy stays with the caller"
        );
    }

    /// `watch` is keyed: it delivers the watched operation's transitions in publish order and
    /// silently drops everything else on the shared bus.
    #[tokio::test]
    async fn watch_filters_to_the_watched_operation() {
        use domain::generated::scalars::InboundMessageStatus as M;

        use crate::status_bus::{OperationStatusBus, OperationUpdate};
        let bus = OperationStatusBus::default();
        let reader = ActorClient::new(Arc::new(MemMailbox::default()), bus.clone());
        let watched = uuid::Uuid::from_u128(0xA);
        let other = uuid::Uuid::from_u128(0xB);
        let mut watch = reader.watch(watched).expect("a bus-backed door watches");
        for (id, status) in [(other, M::SUCCEEDED), (watched, M::RECEIVED), (watched, M::REJECTED)] {
            bus.publish(OperationUpdate {
                message_id: id,
                correlation_id: id,
                status,
                error_code: (status == M::REJECTED).then(|| "RestaurantNotFound".into()),
                message: None,
            });
        }
        let super::OperationWatchEvent::Update(first) = watch.next().await.expect("first") else {
            panic!("expected an update, not lag")
        };
        assert_eq!((first.message_id, first.status), (watched, M::RECEIVED));
        let super::OperationWatchEvent::Update(second) = watch.next().await.expect("second") else {
            panic!("expected an update, not lag")
        };
        assert_eq!((second.message_id, second.status), (watched, M::REJECTED));
        assert_eq!(second.error_code.as_deref(), Some("RestaurantNotFound"));
    }

    /// A pull-only door has no response stream to offer, and says so — rather than handing back
    /// a watch that would await forever on a bus nobody publishes to (#304 review).
    #[tokio::test]
    async fn a_pull_only_door_offers_no_watch() {
        let reader = ActorClient::pull_only(Arc::new(MemMailbox::default()));
        assert!(
            reader.watch(uuid::Uuid::from_u128(0xD)).is_none(),
            "a door with no bus must return None, never a stream that hangs"
        );
        assert!(
            reader.get_operation_status(uuid::Uuid::from_u128(0xD)).await.expect("read").is_none(),
            "the durable read still works — it is the half a pull-only door exists for"
        );
    }

    /// A watcher that fell behind the bus's retention gets an explicit Lagged event — the cue to
    /// re-read the durable row — and the stream ends (None) when every publisher is gone.
    #[tokio::test]
    async fn watch_makes_lag_explicit_and_ends_when_the_bus_closes() {
        use domain::generated::scalars::InboundMessageStatus as M;

        use crate::status_bus::{OperationStatusBus, OperationUpdate};
        let bus = OperationStatusBus::new(1);
        let reader = ActorClient::new(Arc::new(MemMailbox::default()), bus.clone());
        let watched = uuid::Uuid::from_u128(0xC);
        let mut watch = reader.watch(watched).expect("a bus-backed door watches");
        for status in [M::RECEIVED, M::SUCCEEDED] {
            bus.publish(OperationUpdate {
                message_id: watched,
                correlation_id: watched,
                status,
                error_code: None,
                message: None,
            });
        }
        assert!(
            matches!(watch.next().await, Some(super::OperationWatchEvent::Lagged)),
            "capacity 1 with two publishes must surface as Lagged, never a silent drop"
        );
        // The retained tail is still delivered after the lag marker…
        let Some(super::OperationWatchEvent::Update(kept)) = watch.next().await else {
            panic!("the retained update survives the lag")
        };
        assert_eq!(kept.status, M::SUCCEEDED);
        // …and the stream ends once every publisher is gone.
        drop(bus);
        drop(reader);
        assert!(watch.next().await.is_none(), "a closed bus ends the watch");
    }
}
