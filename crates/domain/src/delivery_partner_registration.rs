//! DeliveryPartnerRegistration aggregate — the PURE write-side state fold (#61, ADR-0035), mirroring
//! `prospect.rs`. A delivery partner's self-registered availability to serve ONE city on ONE catalog
//! channel (`specs/actors.yaml#/DeliveryPartnerRegistration`); id = registrationId. The registration is
//! BORN by its first `DeliveryPartnerAvailabilityRequested` fact and moves through a review status
//! (PENDING → APPROVED / REVOKED). The fold tracks only what the review invariants read — existence
//! (`DeliveryPartnerAvailabilityNotFound`) and the status (`DeliveryPartnerAvailabilityNotPending`).
//! Referential FK integrity (channel/city existence) is a boundary concern, not folded here. No I/O.

use crate::generated::events::DomainEvent;
use crate::generated::scalars::CityAvailabilityStatus;

/// What the DeliveryPartnerRegistration command handlers need to accept or reject a command. `None`
/// (from [`fold`]) means no `DeliveryPartnerAvailabilityRequested` yet — the registration does not exist.
#[derive(Debug, Clone, PartialEq)]
pub struct DeliveryPartnerRegistrationState {
    /// Review status: PENDING on request, APPROVED once an admin approves, REVOKED once withdrawn/disabled.
    pub status: CityAvailabilityStatus,
}

/// Fold a DeliveryPartnerRegistration stream (events in version order) into its current state. `None`
/// ⇔ the stream has no `DeliveryPartnerAvailabilityRequested` yet, i.e. the registration does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<DeliveryPartnerRegistrationState> {
    events.iter().fold(None, apply)
}

/// Apply one event — a pure transition, total over the whole event union.
fn apply(
    state: Option<DeliveryPartnerRegistrationState>,
    event: &DomainEvent,
) -> Option<DeliveryPartnerRegistrationState> {
    match event {
        // The birth fact: (re)establishes the registration in PENDING review.
        DomainEvent::DeliveryPartnerAvailabilityRequested(_) => {
            Some(DeliveryPartnerRegistrationState { status: CityAvailabilityStatus::PENDING })
        }
        DomainEvent::DeliveryPartnerAvailabilityApproved(_) => {
            let mut s = state?;
            s.status = CityAvailabilityStatus::APPROVED;
            Some(s)
        }
        DomainEvent::DeliveryPartnerAvailabilityRevoked(_) => {
            let mut s = state?;
            s.status = CityAvailabilityStatus::REVOKED;
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
        DeliveryPartnerAvailabilityApproved, DeliveryPartnerAvailabilityRequested,
        DeliveryPartnerAvailabilityRevoked,
    };
    use crate::generated::scalars::{
        CityId, DeliveryChannelKey, DeliveryPartnerName, DeliveryPartnerRegistrationId, EmailAddress,
    };

    fn requested() -> DomainEvent {
        DomainEvent::DeliveryPartnerAvailabilityRequested(DeliveryPartnerAvailabilityRequested {
            registration_id: DeliveryPartnerRegistrationId(uuid::Uuid::nil()),
            channel: DeliveryChannelKey("uber_direct".into()),
            city_id: CityId(uuid::Uuid::nil()),
            partner_name: DeliveryPartnerName("Uber Direct".into()),
            contact_email: EmailAddress("ops@uberdirect.example".into()),
        })
    }
    fn approved() -> DomainEvent {
        DomainEvent::DeliveryPartnerAvailabilityApproved(DeliveryPartnerAvailabilityApproved {
            registration_id: DeliveryPartnerRegistrationId(uuid::Uuid::nil()),
        })
    }
    fn revoked() -> DomainEvent {
        DomainEvent::DeliveryPartnerAvailabilityRevoked(DeliveryPartnerAvailabilityRevoked {
            registration_id: DeliveryPartnerRegistrationId(uuid::Uuid::nil()),
            reason: None,
        })
    }

    #[test]
    fn no_request_means_no_registration() {
        assert_eq!(fold(&[]), None);
        // A decision fact without a birth never materializes a registration.
        assert_eq!(fold(&[approved()]), None);
    }

    #[test]
    fn request_births_the_registration_pending() {
        let s = fold(&[requested()]).unwrap();
        assert_eq!(s.status, CityAvailabilityStatus::PENDING);
    }

    #[test]
    fn approval_then_revocation_fold_in_order() {
        assert_eq!(fold(&[requested(), approved()]).unwrap().status, CityAvailabilityStatus::APPROVED);
        assert_eq!(
            fold(&[requested(), approved(), revoked()]).unwrap().status,
            CityAvailabilityStatus::REVOKED
        );
    }

    #[test]
    fn stream_name_matches_the_aggregate_format() {
        let id = uuid::Uuid::nil();
        assert_eq!(
            DeliveryPartnerRegistrationState::stream(DeliveryPartnerRegistrationId(id)),
            format!("DeliveryPartnerRegistration-{id}")
        );
    }
}
