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

use crate::mailbox::{Mailbox, MailboxStatusRow};
use crate::status_bus::{OperationStatusBus, OperationUpdate};

/// The one generic, actor-agnostic client: holds the read capability over operation status —
/// the durable row (pull) and the post-commit response stream (push).
pub struct ActorClient {
    mailbox: Arc<dyn Mailbox>,
    bus: OperationStatusBus,
}

impl ActorClient {
    pub fn new(mailbox: Arc<dyn Mailbox>, bus: OperationStatusBus) -> Self {
        Self { mailbox, bus }
    }

    /// The status of one operation by its globally-unique acceptance handle — the row behind
    /// `operationStatus` / the `operationStatusChanged` snapshot. `None` = unknown identity; the
    /// OWNERSHIP decision (no existence oracle, ADR-20260720-015500) stays with the caller, which
    /// alone knows the request principal.
    pub async fn get_operation_status(
        &self,
        message_id: uuid::Uuid,
    ) -> Result<Option<MailboxStatusRow>, DomainError> {
        self.mailbox.by_message(message_id).await
    }

    /// Subscribe to the response stream of ONE operation (§2.1 `watch`, #303): every post-commit
    /// transition published for `message_id`, in publish order, with lag made explicit. Subscribe
    /// BEFORE the snapshot read to close the subscribe/complete race — the returned watch only
    /// sees updates published after this call. The stream is notification, never truth: on
    /// [`OperationWatchEvent::Lagged`] re-read via [`ActorClient::get_operation_status`], and the
    /// OWNERSHIP decision (no existence oracle, ADR-20260720-015500) stays with the caller.
    pub fn watch(&self, message_id: uuid::Uuid) -> OperationWatch {
        OperationWatch { message_id, rx: self.bus.subscribe() }
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
        let mut watch = reader.watch(watched);
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

    /// A watcher that fell behind the bus's retention gets an explicit Lagged event — the cue to
    /// re-read the durable row — and the stream ends (None) when every publisher is gone.
    #[tokio::test]
    async fn watch_makes_lag_explicit_and_ends_when_the_bus_closes() {
        use domain::generated::scalars::InboundMessageStatus as M;

        use crate::status_bus::{OperationStatusBus, OperationUpdate};
        let bus = OperationStatusBus::new(1);
        let reader = ActorClient::new(Arc::new(MemMailbox::default()), bus.clone());
        let watched = uuid::Uuid::from_u128(0xC);
        let mut watch = reader.watch(watched);
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
