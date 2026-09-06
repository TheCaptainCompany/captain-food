//! `PlatformMembership` aggregate — the PURE write-side state fold (ADR-0035), mirroring
//! `restaurant_membership.rs` (#639 part C step 6-v, ADR-20260905-223957 §1-§3): platform standing
//! is its OWN relationship, one aggregate, one stream, one lane. No I/O, no serialization logic
//! (dependency rule).
//!
//! No `lifecycle:` machine and no `revoked` field: a `PlatformMembership` is born by
//! `PlatformAccessGranted` and NOTHING else can happen to it yet — no revoke command exists
//! (ADR-20260905-223957 §3, deferred until a second admin exists). `state` therefore tracks only
//! existence, which is all the declared invariant (`PlatformAccessGrantIsIdempotent`) reads.

use crate::generated::events::DomainEvent;

/// What the `PlatformMembership` command handler needs to know to accept or reject a command.
/// `None` (from [`fold`]) means no `PlatformAccessGranted` yet on this stream. A unit struct
/// rather than `bool`/`()` directly: `Some(PlatformMembershipState)` reads as "this platform
/// membership exists", the same shape every other aggregate's state fold uses, and it leaves room
/// for a field the day a revoke command is built (the `RestaurantMembershipState::revoked`
/// precedent) without reshaping every call site that already matches on `Option<Self>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlatformMembershipState;

/// Fold a `PlatformMembership` stream (events in version order) into its current state. `None` ⇔
/// the stream has no `PlatformAccessGranted` yet, i.e. the membership does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<PlatformMembershipState> {
    events.iter().fold(None, apply)
}

fn apply(
    state: Option<PlatformMembershipState>,
    event: &DomainEvent,
) -> Option<PlatformMembershipState> {
    match event {
        // Birth. A replayed grant on an EXISTING stream (the idempotent-retry path,
        // `rules.yaml#/PlatformAccessGrantIsIdempotent` — the SAME shape a re-run of the one-shot
        // bootstrap relies on) never reaches this fold twice in practice: the handler
        // short-circuits before appending a second time.
        DomainEvent::PlatformAccessGranted(_) => Some(state.unwrap_or_default()),
        _ => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::Aggregate;
    use crate::generated::events::PlatformAccessGranted;
    use crate::generated::scalars::{AuthSubject, PlatformAccessBasis, PlatformMembershipId};

    fn granted(platform_membership_id: uuid::Uuid) -> DomainEvent {
        DomainEvent::PlatformAccessGranted(PlatformAccessGranted {
            platform_membership_id: PlatformMembershipId(platform_membership_id),
            auth_subject: AuthSubject("auth-admin-1".to_string()),
            basis: PlatformAccessBasis::CAPTAIN_ONBOARDING,
        })
    }

    #[test]
    fn no_events_means_no_membership() {
        assert_eq!(fold(&[]), None);
    }

    #[test]
    fn granted_is_born() {
        let id = uuid::Uuid::from_u128(1);
        assert!(fold(&[granted(id)]).is_some());
    }

    #[test]
    fn a_replayed_grant_stays_one_state() {
        let id = uuid::Uuid::from_u128(1);
        assert_eq!(fold(&[granted(id)]), fold(&[granted(id), granted(id)]));
    }

    #[test]
    fn stream_name_matches_the_platform_membership_id() {
        let id = PlatformMembershipId(uuid::Uuid::nil());
        assert_eq!(
            PlatformMembershipState::stream(id),
            format!("PlatformMembership-{}", id.0)
        );
    }
}
