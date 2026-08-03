//! MailboxSupervision aggregate — the PURE write-side state fold (#315, ADR-20260803-002712 Q1):
//! operator supervision actions over the actor mailbox itself, keyed by the SUPERVISED row's
//! `messageId` (stream `MailboxSupervision-{targetMessageId}`). The stream is a LEDGER of
//! interventions, not a state machine — every operator action on that row accumulates as a fact,
//! which is the audit trail SQL runbooks never leave. The actual row flip happens through the
//! application `MailboxRequeue` port (the arbiter of "is this row cap-poisoned"); the fold only
//! remembers what was recorded. No I/O.

use crate::generated::events::DomainEvent;
use crate::generated::scalars::{MailboxActorType, MessageId};

/// What the MailboxSupervision command handlers need from the stream. `None` (from [`fold`])
/// means no operator has ever intervened on this row — the common case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxSupervisionState {
    /// The supervised mailbox row (the aggregate identity).
    pub target_message_id: MessageId,
    /// The lane the supervised row belongs to, as recorded by the latest intervention.
    pub actor_type: Option<MailboxActorType>,
    /// How many requeues have been recorded — a row that keeps needing requeues is a signal the
    /// CAUSE was never actually fixed, surfaced to the operator rather than inferred.
    pub requeues: u32,
}

/// Fold a MailboxSupervision stream (events in version order) into its intervention ledger.
pub fn fold(events: &[DomainEvent]) -> Option<MailboxSupervisionState> {
    events.iter().fold(None, apply)
}

/// Apply one event — a pure transition, total over the whole event union.
fn apply(
    state: Option<MailboxSupervisionState>,
    event: &DomainEvent,
) -> Option<MailboxSupervisionState> {
    match event {
        DomainEvent::MailboxMessageRequeued(e) => {
            let mut s = state.unwrap_or(MailboxSupervisionState {
                target_message_id: e.target_message_id,
                actor_type: None,
                requeues: 0,
            });
            s.actor_type = Some(e.actor_type.clone());
            s.requeues += 1;
            Some(s)
        }
        _ => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::Aggregate;
    use crate::generated::events::MailboxMessageRequeued;

    fn requeued(target: u128, lane: &str) -> DomainEvent {
        DomainEvent::MailboxMessageRequeued(MailboxMessageRequeued {
            target_message_id: MessageId(uuid::Uuid::from_u128(target)),
            actor_type: MailboxActorType(lane.into()),
        })
    }

    #[test]
    fn no_intervention_means_no_state() {
        assert_eq!(fold(&[]), None);
    }

    #[test]
    fn requeues_accumulate_as_a_ledger() {
        let s = fold(&[requeued(1, "Payment"), requeued(1, "Payment")]).unwrap();
        assert_eq!(s.requeues, 2);
        assert_eq!(s.actor_type, Some(MailboxActorType("Payment".into())));
    }

    #[test]
    fn stream_name_matches_the_aggregate_format() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            MailboxSupervisionState::stream(MessageId(id)),
            format!("MailboxSupervision-{id}")
        );
    }
}
