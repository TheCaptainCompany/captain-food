//! In-process operation-status bus (ADR-20260720-015500): a `tokio::sync::broadcast` fan-out of
//! `command_journal` lifecycle transitions, published by the acceptance-first dispatch after each
//! journal write. The GraphQL `operationStatusChanged` subscription subscribes to it to push status
//! updates (PENDING → SUCCEEDED | REJECTED | FAILED) without polling.
//!
//! Deliberately a SEPARATE bus from [`super::event_bus::EventBus`]: journal ticks must never compete
//! with domain-event fan-out for channel capacity, and their subscriber sets differ. Same guarantees
//! as the event bus — notification not source of truth (the journal row is; subscribers re-read via
//! `operationStatus` on lag), best-effort publish, single-process scope (V0, ADR-0042).

use domain::generated::scalars::CommandJournalStatus;
use tokio::sync::broadcast;

/// One journal lifecycle transition: the acceptance (RECEIVED) and the terminal completion.
#[derive(Debug, Clone)]
pub struct OperationUpdate {
    /// The acceptance handle (`command_journal.message_id`).
    pub message_id: uuid::Uuid,
    pub correlation_id: uuid::Uuid,
    pub status: CommandJournalStatus,
    /// The errors.yaml code on REJECTED/FAILED (surfaced as `Operation.errorCode`).
    pub error_code: Option<String>,
    /// Interpolated human-readable summary, when one exists.
    pub message: Option<String>,
}

/// Cloneable handle over the broadcast channel: the dispatch publishes, `operationStatusChanged`
/// subscribes (both via schema `.data(...)`).
#[derive(Clone)]
pub struct OperationStatusBus {
    tx: broadcast::Sender<OperationUpdate>,
}

impl OperationStatusBus {
    /// A bus retaining up to `capacity` in-flight updates per subscriber before it lags.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Broadcast a journal transition. Best effort: no live subscribers is a no-op — the journal
    /// row has already committed and the pull query serves the truth.
    pub fn publish(&self, update: OperationUpdate) {
        let _ = self.tx.send(update);
    }

    /// A fresh receiver seeing every update published from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<OperationUpdate> {
        self.tx.subscribe()
    }
}

impl Default for OperationStatusBus {
    /// Capacity generously above any realistic V0 burst (updates are ~100 bytes).
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_without_subscribers_is_a_noop() {
        OperationStatusBus::default().publish(OperationUpdate {
            message_id: uuid::Uuid::new_v4(),
            correlation_id: uuid::Uuid::new_v4(),
            status: CommandJournalStatus::SUCCEEDED,
            error_code: None,
            message: None,
        });
    }

    #[tokio::test]
    async fn subscriber_receives_lifecycle_updates() {
        let bus = OperationStatusBus::default();
        let mut rx = bus.subscribe();
        let id = uuid::Uuid::new_v4();
        bus.publish(OperationUpdate {
            message_id: id,
            correlation_id: id,
            status: CommandJournalStatus::REJECTED,
            error_code: Some("RestaurantNotFound".into()),
            message: Some("Restaurant not found.".into()),
        });
        let got = rx.recv().await.expect("update");
        assert_eq!(got.message_id, id);
        assert_eq!(got.status, CommandJournalStatus::REJECTED);
        assert_eq!(got.error_code.as_deref(), Some("RestaurantNotFound"));
    }
}
