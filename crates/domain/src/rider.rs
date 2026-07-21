//! Rider aggregate — the PURE write-side state fold (ADR-0035), mirroring `customer.rs`. A rider
//! identity linked to the auth provider user (`specs/actors.yaml#/Rider`); the fold tracks just what
//! the declared invariants read: existence (`RiderNotFound`), profile fields, and the availability
//! machine (`errors.yaml#/InvalidRiderStatusTransition`). No I/O, no serialization logic
//! (dependency rule).
//!
//! The availability machine is the DECLARED lifecycle (`specs/actors.yaml#/Rider/lifecycle`,
//! ADR-20260721-093027) in its dynamic-target form: `RiderRegistered` and `RiderStatusChanged` carry
//! the target state in their `status` payload field, so the fold moves `status` exclusively through
//! the GENERATED tables ([`lifecycle::initial`] births from the payload, [`lifecycle::target`]
//! applies recorded facts) and the command handlers guard with [`lifecycle::transition`].

use crate::generated::events::DomainEvent;
pub use crate::generated::lifecycles::rider as lifecycle;
use crate::generated::scalars::{PhoneNumber, RiderStatus};

/// What the Rider command handlers need to know to accept or reject a command. `None` (from
/// [`fold`]) means no `RiderRegistered` yet on this stream.
#[derive(Debug, Clone, PartialEq)]
pub struct RiderState {
    /// Availability/lifecycle machine (OFFLINE/AVAILABLE/ON_DELIVERY/SUSPENDED) — guarded by
    /// [`lifecycle::transition`].
    pub status: RiderStatus,
    /// Current display name (profile field, edited via UpdateRiderInfo).
    pub display_name: String,
    /// Current canonical E.164 phone (profile field, edited via UpdateRiderInfo).
    pub phone: PhoneNumber,
}

/// Fold a Rider stream (events in version order) into its current state. `None` ⇔ the stream has no
/// `RiderRegistered` yet, i.e. the rider does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<RiderState> {
    events.iter().fold(None, apply)
}

/// Apply one event — a pure transition, total over the whole event union. The status moves ONLY
/// through the generated lifecycle table; the hand-written part is payload extraction.
fn apply(state: Option<RiderState>, event: &DomainEvent) -> Option<RiderState> {
    if let Some(status) = lifecycle::initial(event) {
        if let DomainEvent::RiderRegistered(e) = event {
            return Some(RiderState {
                status,
                display_name: e.display_name.clone(),
                phone: e.phone.clone(),
            });
        }
    }
    let mut s = state?;
    // The recorded fact wins at fold time: `target` reads the event-carried status regardless of
    // the current state (legality is `transition`'s job, enforced by the handlers at append time).
    if let Some(next) = lifecycle::target(event) {
        s.status = next;
    }
    if let DomainEvent::RiderInfoUpdated(e) = event {
        // Partial update: only the provided profile fields change; the status never does.
        if let Some(name) = &e.display_name {
            s.display_name = name.clone();
        }
        if let Some(phone) = &e.phone {
            s.phone = phone.clone();
        }
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::Aggregate;
    use crate::generated::events::{RiderInfoUpdated, RiderRegistered, RiderStatusChanged};
    use crate::generated::scalars::{ExternalReference, RiderId};

    fn registered(status: RiderStatus) -> DomainEvent {
        DomainEvent::RiderRegistered(RiderRegistered {
            rider_id: RiderId(uuid::Uuid::nil()),
            auth_ref: ExternalReference("auth_1".into()),
            display_name: "Sam".into(),
            phone: PhoneNumber("+33600000000".into()),
            status,
        })
    }
    fn status_changed(status: RiderStatus) -> DomainEvent {
        DomainEvent::RiderStatusChanged(RiderStatusChanged {
            rider_id: RiderId(uuid::Uuid::nil()),
            status,
        })
    }

    #[test]
    fn no_registration_means_no_rider() {
        assert_eq!(fold(&[]), None);
        assert_eq!(fold(&[status_changed(RiderStatus::AVAILABLE)]), None);
    }

    #[test]
    fn registration_births_the_rider_with_the_event_status() {
        let s = fold(&[registered(RiderStatus::OFFLINE)]).unwrap();
        assert_eq!(s.status, RiderStatus::OFFLINE);
        assert_eq!(s.display_name, "Sam");
        assert_eq!(s.phone, PhoneNumber("+33600000000".into()));
    }

    #[test]
    fn info_update_is_partial_and_never_touches_the_status() {
        let update = DomainEvent::RiderInfoUpdated(RiderInfoUpdated {
            rider_id: RiderId(uuid::Uuid::nil()),
            display_name: Some("Sam R.".into()),
            phone: None, // omitted → keeps the current phone
        });
        let s = fold(&[registered(RiderStatus::AVAILABLE), update]).unwrap();
        assert_eq!(s.display_name, "Sam R.");
        assert_eq!(s.phone, PhoneNumber("+33600000000".into()));
        assert_eq!(s.status, RiderStatus::AVAILABLE);
    }

    #[test]
    fn status_changes_fold_in_order() {
        let s = fold(&[
            registered(RiderStatus::OFFLINE),
            status_changed(RiderStatus::AVAILABLE),
            status_changed(RiderStatus::ON_DELIVERY),
        ])
        .unwrap();
        assert_eq!(s.status, RiderStatus::ON_DELIVERY);
    }

    /// The generated table IS the declared machine (actors.yaml#/Rider/lifecycle, dynamic-target
    /// form: `RiderStatusChanged` carries the target in `status`) — the same legality set the old
    /// hand `can_transition` encoded, now spec-checked (rules.yaml#/RiderLifecycle).
    #[test]
    fn generated_transition_table_matches_the_declared_machine() {
        use RiderStatus::*;
        let t = |from: RiderStatus, to: RiderStatus| lifecycle::transition(from, &status_changed(to));
        // The legal moves.
        assert_eq!(t(OFFLINE, AVAILABLE), Some(AVAILABLE));
        assert_eq!(t(AVAILABLE, OFFLINE), Some(OFFLINE));
        assert_eq!(t(AVAILABLE, ON_DELIVERY), Some(ON_DELIVERY));
        assert_eq!(t(ON_DELIVERY, AVAILABLE), Some(AVAILABLE));
        assert_eq!(t(SUSPENDED, OFFLINE), Some(OFFLINE)); // reinstate
        // Suspension is admin-imposed from anywhere, idempotently (SUSPENDED → SUSPENDED).
        assert_eq!(t(OFFLINE, SUSPENDED), Some(SUSPENDED));
        assert_eq!(t(AVAILABLE, SUSPENDED), Some(SUSPENDED));
        assert_eq!(t(ON_DELIVERY, SUSPENDED), Some(SUSPENDED));
        assert_eq!(t(SUSPENDED, SUSPENDED), Some(SUSPENDED));
        // The notable invalid jumps.
        assert_eq!(t(OFFLINE, ON_DELIVERY), None);
        assert_eq!(t(ON_DELIVERY, OFFLINE), None);
        assert_eq!(t(SUSPENDED, AVAILABLE), None);
        assert_eq!(t(SUSPENDED, ON_DELIVERY), None);
        assert_eq!(t(OFFLINE, OFFLINE), None);
        assert_eq!(t(AVAILABLE, AVAILABLE), None);
        // The birth is event-carried too, and no state is terminal.
        assert_eq!(lifecycle::initial(&registered(AVAILABLE)), Some(AVAILABLE));
        assert!(lifecycle::TERMINAL.is_empty());
    }

    #[test]
    fn stream_name_matches_the_aggregate_format() {
        let id = uuid::Uuid::nil();
        assert_eq!(RiderState::stream(RiderId(id)), format!("Rider-{id}"));
    }
}
