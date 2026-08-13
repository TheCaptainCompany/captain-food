//! The in-process operation-response bus (ADR-20260720-015500, generalized per
//! PROP-20260728-152752 §2.1): a `tokio::sync::broadcast` fan-out of post-commit operation
//! completions, keyed by `message_id`. Relocated from `infrastructure` behind this boundary
//! crate (#303, PROP-20260802-130500 D4) so the response stream — like the row read — is served
//! by the one generic [`crate::ActorClient`], and re-keyed from the retired `CommandJournalStatus`
//! to the mailbox-native [`InboundMessageStatus`].
//!
//! Publishers (the write side of the bus, post-commit only): the mailbox delivery observer
//! (`infrastructure::mailbox::StatusBusObserver`) with the honest verdicts — SUCCEEDED, REJECTED,
//! FAILED, IGNORED, DUPLICATE — and the legacy journal+spawn dispatch, whose
//! `CommandJournalStatus` maps losslessly into the mailbox enum. Consumption goes through
//! [`crate::ActorClient::watch`] alone: [`OperationStatusBus::subscribe`] is crate-internal, so
//! the typed watch is the ONLY read surface (ADR-20260802-170059 — dead or bypassable surface is
//! an open door).
//!
//! Deliberately a SEPARATE bus from the domain-event bus: completion ticks must never compete
//! with domain-event fan-out for channel capacity, and their subscriber sets differ. Same
//! guarantees — notification not source of truth (the durable row is; watchers re-read via
//! [`crate::ActorClient::get_operation_status`] on lag), best-effort publish, single-process
//! scope (V0, ADR-0042).

use domain::generated::scalars::InboundMessageStatus;
use tokio::sync::broadcast;

/// One operation lifecycle transition: the acceptance (RECEIVED) and the terminal completion.
#[derive(Debug, Clone)]
pub struct OperationUpdate {
    /// The acceptance handle (`inbound_messages.message_id`
    /// — one keyspace, globally unique).
    pub message_id: uuid::Uuid,
    pub correlation_id: uuid::Uuid,
    pub status: InboundMessageStatus,
    /// The errors.yaml code on REJECTED/FAILED (surfaced as `Operation.errorCode`).
    pub error_code: Option<String>,
    /// Interpolated human-readable summary, when one exists.
    pub message: Option<String>,
}

/// Cloneable handle over the broadcast channel: the delivery observer and the legacy dispatch
/// publish; [`crate::ActorClient::watch`] subscribes (via the crate-internal receiver).
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

    /// Broadcast an operation transition. Best effort: no live subscribers is a no-op — the
    /// durable row has already committed and the pull read serves the truth.
    pub fn publish(&self, update: OperationUpdate) {
        let _ = self.tx.send(update);
    }

    /// A fresh receiver seeing every update published from now on. Crate-internal: the sanctioned
    /// consumer surface is [`crate::ActorClient::watch`], which filters to one `message_id` and
    /// makes lag explicit.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<OperationUpdate> {
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
            status: InboundMessageStatus::SUCCEEDED,
            error_code: None,
            message: None,
        });
    }
}
