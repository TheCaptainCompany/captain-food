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
//!
//! The status machine is the DECLARED lifecycle (`specs/actors.yaml#/DeliveryJob/lifecycle`,
//! ADR-20260721-093027): static edges for the operational facts plus DYNAMIC (event-carried) edges
//! for `DeliveryStatusUpdated`/`DeliveryPartnerStatusUpdated`, whose `status` payload field names
//! the target state. The fold moves `status` exclusively through the GENERATED tables
//! ([`lifecycle::initial`] births it, [`lifecycle::target`] applies recorded facts) and the command
//! handlers guard with [`lifecycle::transition`].

use crate::generated::events::DomainEvent;
pub use crate::generated::lifecycles::delivery_job as lifecycle;
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
/// touching the folded fields are no-ops, so a fatter stream never breaks rehydration). The status
/// moves ONLY through the generated lifecycle table (a rider decline or partner rejection is not in
/// the machine, so it folds as a status no-op); the hand-written part is the assignment fields and
/// the issue flag.
fn apply(state: Option<DeliveryJobState>, event: &DomainEvent) -> Option<DeliveryJobState> {
    if let Some(status) = lifecycle::initial(event) {
        return Some(DeliveryJobState {
            status,
            rider_id: None,
            assigned: false,
            partner_ref: None,
            open_issue: false,
        });
    }
    let mut s = state?;
    // The recorded fact wins at fold time: `target` maps a lifecycle event to its state — for the
    // dynamic events the event-carried `status` payload field — regardless of the current one
    // (legality is `transition`'s job, enforced by the handlers at append time).
    if let Some(next) = lifecycle::target(event) {
        s.status = next;
    }
    match event {
        DomainEvent::DeliveryAcceptedByRider(e) => {
            s.rider_id = Some(e.rider_id);
            s.assigned = true;
        }
        DomainEvent::DeliveryAcceptedByPartner(e) => {
            s.assigned = true;
            s.partner_ref = Some(e.partner_ref.clone());
        }
        DomainEvent::DeliveryAssignedToPartner(e) => {
            s.assigned = true;
            s.partner_ref = Some(e.partner_ref.clone());
        }
        DomainEvent::DeliveryUnassignedFromPartner(_) => {
            // Back to PENDING (the lifecycle edge) so the job is re-offerable.
            s.assigned = false;
            s.partner_ref = None;
        }
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
