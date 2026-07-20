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
use crate::generated::scalars::{DeliveryStatus, ExternalReference, RiderId};

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
    /// The partner the job is assigned/accepted to, when partner-fulfilled — partner status reports
    /// must correlate to it; `None` on a PENDING or rider-fulfilled job.
    pub partner_ref: Option<ExternalReference>,
    /// Whether a reported delivery issue is still open — `ResolveDeliveryIssue` needs one to resolve.
    pub open_issue: bool,
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
            partner_ref: None,
            open_issue: false,
        });
    }
    let mut s = state?;
    match event {
        DomainEvent::DeliveryAcceptedByRider(e) => {
            s.status = DeliveryStatus::ASSIGNED;
            s.rider_id = Some(e.rider_id);
            s.assigned = true;
        }
        DomainEvent::DeliveryAcceptedByPartner(e) => {
            s.status = DeliveryStatus::ASSIGNED;
            s.assigned = true;
            s.partner_ref = Some(e.partner_ref.clone());
        }
        DomainEvent::DeliveryAssignedToPartner(e) => {
            s.status = DeliveryStatus::ASSIGNED;
            s.assigned = true;
            s.partner_ref = Some(e.partner_ref.clone());
        }
        DomainEvent::DeliveryUnassignedFromPartner(_) => {
            // Back to PENDING so the job is re-offerable (assign/accept both require PENDING).
            s.status = DeliveryStatus::PENDING;
            s.assigned = false;
            s.partner_ref = None;
        }
        DomainEvent::DeliveryPickedUp(_) => s.status = DeliveryStatus::PICKED_UP,
        DomainEvent::DeliveryStatusUpdated(e) => s.status = e.status,
        DomainEvent::DeliveryPartnerStatusUpdated(e) => s.status = e.status,
        DomainEvent::DeliveryCompleted(_) => s.status = DeliveryStatus::DELIVERED,
        DomainEvent::DeliveryCancelled(_) => s.status = DeliveryStatus::CANCELLED,
        // A rider decline leaves the job PENDING and re-offerable — nothing to fold.
        DomainEvent::DeliveryDeclinedByRider(_) => {}
        // Terminal dispatch failure (offer cap exhausted, ADR-20260720-004556): the job is FAILED
        // and surfaced for manual handling.
        DomainEvent::DeliveryDispatchFailed(_) => s.status = DeliveryStatus::FAILED,
        DomainEvent::DeliveryIssueReported(_) => s.open_issue = true,
        DomainEvent::DeliveryIssueResolved(_) => s.open_issue = false,
        _ => {}
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::entities::Address;
    use crate::generated::events::{
        DeliveryAcceptedByRider, DeliveryAssignedToPartner, DeliveryDeclinedByRider,
        DeliveryIssueReported, DeliveryIssueResolved, DeliveryPartnerStatusUpdated,
        DeliveryRequested, DeliveryUnassignedFromPartner,
    };
    use crate::generated::scalars::{
        AddressLine, CityName, CountryCode, DeliveryJobId, OrderId, PostalCode, RestaurantId,
    };

    fn job_id() -> DeliveryJobId {
        DeliveryJobId(uuid::Uuid::nil())
    }
    fn address() -> Address {
        Address {
            line1: AddressLine("1 rue Nationale".into()),
            line2: None,
            postal_code: PostalCode("37000".into()),
            city: CityName("Tours".into()),
            country: CountryCode("FR".into()),
        }
    }
    fn requested() -> DomainEvent {
        DomainEvent::DeliveryRequested(DeliveryRequested {
            mode: None,
            delivery_job_id: job_id(),
            order_id: OrderId(uuid::Uuid::nil()),
            restaurant_id: RestaurantId(uuid::Uuid::nil()),
            pickup: address(),
            dropoff: address(),
            provider: None,
        })
    }
    fn assigned_to_partner(partner_ref: &str) -> DomainEvent {
        DomainEvent::DeliveryAssignedToPartner(DeliveryAssignedToPartner {
            delivery_job_id: job_id(),
            partner_ref: ExternalReference(partner_ref.into()),
        })
    }
    fn unassigned() -> DomainEvent {
        DomainEvent::DeliveryUnassignedFromPartner(DeliveryUnassignedFromPartner {
            delivery_job_id: job_id(),
            reason: None,
        })
    }

    #[test]
    fn requested_births_a_pending_unassigned_job() {
        let s = fold(&[requested()]).unwrap();
        assert_eq!(s.status, DeliveryStatus::PENDING);
        assert_eq!(s.rider_id, None);
        assert!(!s.assigned);
        assert_eq!(s.partner_ref, None);
        assert!(!s.open_issue);
    }

    #[test]
    fn rider_acceptance_assigns_the_job_to_that_rider() {
        let rider = RiderId(uuid::Uuid::nil());
        let accept = DomainEvent::DeliveryAcceptedByRider(DeliveryAcceptedByRider {
            delivery_job_id: job_id(),
            rider_id: rider,
        });
        let s = fold(&[requested(), accept]).unwrap();
        assert_eq!(s.status, DeliveryStatus::ASSIGNED);
        assert_eq!(s.rider_id, Some(rider));
        assert!(s.assigned);
    }

    #[test]
    fn partner_assignment_and_unassignment_round_trip_to_pending() {
        let s = fold(&[requested(), assigned_to_partner("avelo37")]).unwrap();
        assert_eq!(s.status, DeliveryStatus::ASSIGNED);
        assert!(s.assigned);
        assert_eq!(s.partner_ref, Some(ExternalReference("avelo37".into())));
        // Unassigned → PENDING and re-offerable again.
        let s = fold(&[requested(), assigned_to_partner("avelo37"), unassigned()]).unwrap();
        assert_eq!(s.status, DeliveryStatus::PENDING);
        assert!(!s.assigned);
        assert_eq!(s.partner_ref, None);
    }

    #[test]
    fn partner_status_report_moves_the_status_machine() {
        let report = DomainEvent::DeliveryPartnerStatusUpdated(DeliveryPartnerStatusUpdated {
            delivery_job_id: job_id(),
            partner_ref: Some(ExternalReference("avelo37".into())),
            status: DeliveryStatus::PICKED_UP,
            occurred_at: None,
        });
        let s = fold(&[requested(), assigned_to_partner("avelo37"), report]).unwrap();
        assert_eq!(s.status, DeliveryStatus::PICKED_UP);
    }

    #[test]
    fn rider_decline_leaves_the_job_pending() {
        let decline = DomainEvent::DeliveryDeclinedByRider(DeliveryDeclinedByRider {
            delivery_job_id: job_id(),
            rider_id: RiderId(uuid::Uuid::nil()),
            reason: None,
        });
        assert_eq!(fold(&[requested(), decline]), fold(&[requested()]));
    }

    #[test]
    fn issue_report_and_resolution_toggle_the_open_issue_flag() {
        let reported = DomainEvent::DeliveryIssueReported(DeliveryIssueReported {
            delivery_job_id: job_id(),
            rider_id: None,
            issue: "customer unreachable".into(),
            reported_at: None,
        });
        let resolved = DomainEvent::DeliveryIssueResolved(DeliveryIssueResolved {
            delivery_job_id: job_id(),
            resolution: "reached by phone".into(),
            resolved_at: None,
        });
        let s = fold(&[requested(), reported.clone()]).unwrap();
        assert!(s.open_issue);
        let s = fold(&[requested(), reported, resolved]).unwrap();
        assert!(!s.open_issue);
    }
}
