//! DeliveryJob aggregate — the PURE write-side state fold (ADR-0031/0035/0046). Command handlers
//! rehydrate a [`DeliveryJobState`] by folding the stream's events (loaded through the `EventStore`
//! port) and then enforce the invariants declared in `specs/actors.yaml`/`specs/errors.yaml` against it
//! (the delivery status machine + single assignment). Deliberately MINIMAL; no I/O, no serialization
//! logic (dependency rule).
//!
//! The stream is born by `DeliveryRequested` (emitted by DeliveryDispatchProcess) → PENDING, then
//! fulfilled by EITHER an independent Captain rider (the commands folded here) OR a delivery partner
//! (inbound `DeliveryAcceptedByPartner`/`DeliveryStatusUpdated` facts, folded too so a partner-tracked
//! job rejects rider commands with the right status).

use crate::generated::events::DomainEvent;
use crate::generated::scalars::{DeliveryStatus, RiderId};

/// What the DeliveryJob command handlers need to know about the aggregate to accept or reject a
/// command. `None` (from [`fold`]) means the job does not exist → `DeliveryJobNotFound`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeliveryJobState {
    /// Delivery status machine (PENDING → ASSIGNED → PICKED_UP → …) — `InvalidDeliveryStatus`.
    pub status: DeliveryStatus,
    /// The independent rider the job is assigned to, when rider-fulfilled — pickup/completion must come
    /// from this rider; `None` on a PENDING or partner-fulfilled job.
    pub rider_id: Option<RiderId>,
    /// Whether ANY courier (independent rider or partner) already took the job —
    /// `DeliveryAlreadyAssigned`.
    pub assigned: bool,
}

/// Fold a DeliveryJob stream (events in version order) into its current state. `None` ⇔ the stream has
/// no `DeliveryRequested` yet, i.e. the job does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<DeliveryJobState> {
    events.iter().fold(None, apply)
}

/// Apply one event to the state — a pure transition, total over the whole event union (events not
/// touching the folded fields are no-ops, so a fatter stream never breaks rehydration).
fn apply(state: Option<DeliveryJobState>, event: &DomainEvent) -> Option<DeliveryJobState> {
    if let DomainEvent::DeliveryRequested(_) = event {
        return Some(DeliveryJobState {
            status: DeliveryStatus::PENDING,
            rider_id: None,
            assigned: false,
        });
    }
    let mut s = state?;
    match event {
        DomainEvent::DeliveryAcceptedByRider(e) => {
            s.status = DeliveryStatus::ASSIGNED;
            s.rider_id = Some(e.rider_id);
            s.assigned = true;
        }
        DomainEvent::DeliveryAcceptedByPartner(_) => {
            s.status = DeliveryStatus::ASSIGNED;
            s.assigned = true;
        }
        DomainEvent::DeliveryPickedUp(_) => s.status = DeliveryStatus::PICKED_UP,
        DomainEvent::DeliveryStatusUpdated(e) => s.status = e.status,
        DomainEvent::DeliveryCompleted(_) => s.status = DeliveryStatus::DELIVERED,
        DomainEvent::DeliveryCancelled(_) => s.status = DeliveryStatus::CANCELLED,
        _ => {}
    }
    Some(s)
}
