//! Reclamation aggregate — the PURE write-side state fold (#151, #153), mirroring `conversation.rs`.
//! A customer claim/dispute over a delivered order (`specs/actors.yaml#/Reclamation`); id =
//! reclamationId (its own identity, MULTIPLE reclamations may exist per order). The reclamation is BORN
//! by its `ReclamationOpened` fact and moves through OPEN -> RESOLVED/REJECTED; a decided reclamation may
//! be reopened back to OPEN. The fold tracks only what the invariants read: existence
//! (`ReclamationNotFound` / `ReclamationAlreadyExists`) and the lifecycle status
//! (`ReclamationNotOpen` / `ReclamationNotReopenable`). No I/O.
//!
//! This slice records the resolution DECISION only; the refund money-move, the credit ledger and
//! replacement orders are downstream slices that react to `ReclamationResolved`. The 14-day window and
//! order-eligibility are cross-aggregate/temporal invariants enforced in the application layer, not here.

use crate::generated::events::DomainEvent;
use crate::generated::scalars::OrderId;

/// The lifecycle state of a reclamation. A PLAIN domain enum (not a generated scalar): this slice adds
/// no `ReclamationStatus` scalar — the status is derived and belongs with the read model (#154).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReclamationStatus {
    /// Awaiting a decision.
    OPEN,
    /// A resolution was recorded (full/partial refund, replacement, goodwill credit).
    RESOLVED,
    /// The claim was declined with a reason.
    REJECTED,
}

/// What the Reclamation command handlers need to accept or reject a command. `None` (from [`fold`])
/// means no `ReclamationOpened` yet — the reclamation does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReclamationState {
    /// Lifecycle status — OPEN on birth, RESOLVED/REJECTED once decided, OPEN again after a reopen.
    pub status: ReclamationStatus,
    /// The order this claim is about, established at `ReclamationOpened` (#153). Carried in state so the
    /// decision commands can stamp it onto their emitted events — which lets the claim lifecycle be woven
    /// into the per-order conversation thread, keyed by order (§2.5, #155), without the client re-supplying it.
    pub order_id: OrderId,
}

/// Fold a Reclamation stream (events in version order) into its current state. `None` ⇔ the stream has
/// no `ReclamationOpened` yet, i.e. the reclamation does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<ReclamationState> {
    events.iter().fold(None, apply)
}

/// Apply one event — a pure transition, total over the whole event union.
fn apply(state: Option<ReclamationState>, event: &DomainEvent) -> Option<ReclamationState> {
    match event {
        // The birth fact: establishes the reclamation OPEN, capturing the order it is about.
        DomainEvent::ReclamationOpened(e) => {
            Some(ReclamationState { status: ReclamationStatus::OPEN, order_id: e.order_id })
        }
        // A decision was recorded: the reclamation is RESOLVED. Impossible without a birth.
        DomainEvent::ReclamationResolved(_) => {
            let mut s = state?;
            s.status = ReclamationStatus::RESOLVED;
            Some(s)
        }
        // The claim was declined: the reclamation is REJECTED.
        DomainEvent::ReclamationRejected(_) => {
            let mut s = state?;
            s.status = ReclamationStatus::REJECTED;
            Some(s)
        }
        // A decided reclamation was reopened: back to OPEN.
        DomainEvent::ReclamationReopened(_) => {
            let mut s = state?;
            s.status = ReclamationStatus::OPEN;
            Some(s)
        }
        _ => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::Aggregate;
    use crate::generated::events::{
        ReclamationOpened, ReclamationRejected, ReclamationReopened, ReclamationResolved,
    };
    use crate::generated::scalars::{
        CustomerId, OrderId, ReclamationCategory, ReclamationDescription, ReclamationId,
        ReclamationReason, ReclamationResolution, RestaurantId,
    };

    fn opened() -> DomainEvent {
        DomainEvent::ReclamationOpened(ReclamationOpened {
            reclamation_id: ReclamationId(uuid::Uuid::nil()),
            order_id: OrderId(uuid::Uuid::nil()),
            customer_id: CustomerId(uuid::Uuid::nil()),
            restaurant_id: RestaurantId(uuid::Uuid::nil()),
            category: ReclamationCategory::MISSING_ITEM,
            description: ReclamationDescription("Drinks missing.".into()),
            requested_resolution: None,
        })
    }
    fn resolved() -> DomainEvent {
        DomainEvent::ReclamationResolved(ReclamationResolved {
            reclamation_id: ReclamationId(uuid::Uuid::nil()),
            order_id: OrderId(uuid::Uuid::nil()),
            resolution: ReclamationResolution::FULL_REFUND,
            note: None,
            refund_amount: None,
        })
    }
    fn rejected() -> DomainEvent {
        DomainEvent::ReclamationRejected(ReclamationRejected {
            reclamation_id: ReclamationId(uuid::Uuid::nil()),
            order_id: OrderId(uuid::Uuid::nil()),
            reason: ReclamationReason("All items were delivered.".into()),
        })
    }
    fn reopened() -> DomainEvent {
        DomainEvent::ReclamationReopened(ReclamationReopened {
            reclamation_id: ReclamationId(uuid::Uuid::nil()),
            order_id: OrderId(uuid::Uuid::nil()),
            reason: None,
        })
    }

    #[test]
    fn no_open_means_no_reclamation() {
        assert_eq!(fold(&[]), None);
        // A decision fact without a birth never materializes a reclamation.
        assert_eq!(fold(&[resolved()]), None);
    }

    #[test]
    fn open_births_the_reclamation_open() {
        assert_eq!(fold(&[opened()]).unwrap().status, ReclamationStatus::OPEN);
    }

    #[test]
    fn resolve_and_reject_move_out_of_open() {
        assert_eq!(fold(&[opened(), resolved()]).unwrap().status, ReclamationStatus::RESOLVED);
        assert_eq!(fold(&[opened(), rejected()]).unwrap().status, ReclamationStatus::REJECTED);
    }

    #[test]
    fn reopen_returns_a_decided_reclamation_to_open() {
        assert_eq!(fold(&[opened(), rejected(), reopened()]).unwrap().status, ReclamationStatus::OPEN);
        assert_eq!(fold(&[opened(), resolved(), reopened()]).unwrap().status, ReclamationStatus::OPEN);
    }

    #[test]
    fn stream_name_matches_the_aggregate_format() {
        let id = uuid::Uuid::nil();
        assert_eq!(ReclamationState::stream(ReclamationId(id)), format!("Reclamation-{id}"));
    }
}
