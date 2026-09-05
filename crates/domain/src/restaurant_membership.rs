//! `RestaurantMembership` aggregate — the PURE write-side state fold (ADR-0035), mirroring
//! `rider.rs` (#639 part C step 6-i, ADR-20260905-101349 §1-§2): the bridge and the grant, one
//! aggregate, one stream, one lane (FORK 1 Option A). No I/O, no serialization logic (dependency
//! rule).
//!
//! No `lifecycle:` machine — a membership is born by `RestaurantAccessGranted` and, at most once,
//! ends by `RestaurantAccessRevoked`; `state` tracks exactly what the declared invariants read:
//! existence (`RestaurantMembershipNotFound`) and whether it has been revoked
//! (`RestaurantMembershipAlreadyRevoked`, `RestaurantAccessGrantIsIdempotent`).

use crate::generated::events::DomainEvent;

/// What the RestaurantMembership command handlers need to know to accept or reject a command.
/// `None` (from [`fold`]) means no `RestaurantAccessGranted` yet on this stream.
#[derive(Debug, Clone, PartialEq)]
pub struct RestaurantMembershipState {
    /// `true` once `RestaurantAccessRevoked` has landed — a membership is revoked at most once
    /// (`rules.yaml#/RestaurantMembershipRevocationIsFinal`); the ADR-20260904-014136-style Art.
    /// 11-log discipline never overwrites a revocation with a second one.
    pub revoked: bool,
}

/// Fold a RestaurantMembership stream (events in version order) into its current state. `None` ⇔
/// the stream has no `RestaurantAccessGranted` yet, i.e. the membership does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<RestaurantMembershipState> {
    events.iter().fold(None, apply)
}

fn apply(
    state: Option<RestaurantMembershipState>,
    event: &DomainEvent,
) -> Option<RestaurantMembershipState> {
    match event {
        DomainEvent::RestaurantAccessGranted(_) => {
            // Birth. A replayed grant on an EXISTING stream (the idempotent-retry path,
            // `rules.yaml#/RestaurantAccessGrantIsIdempotent`) never reaches this fold twice in
            // practice — the handler short-circuits before appending a second time — but folding
            // it defensively preserves whatever `revoked` already holds rather than resetting it.
            Some(state.unwrap_or(RestaurantMembershipState { revoked: false }))
        }
        DomainEvent::RestaurantAccessRevoked(_) => {
            let mut s = state?;
            s.revoked = true;
            Some(s)
        }
        _ => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::Aggregate;
    use crate::generated::events::{RestaurantAccessGranted, RestaurantAccessRevoked};
    use crate::generated::scalars::{
        AccessBasis, AccessRevocationGround, AuthSubject, MemberAuthority, MemberId, MembershipId,
        PrincipalKind, RestaurantId, ScopeType,
    };

    fn granted(membership_id: uuid::Uuid) -> DomainEvent {
        DomainEvent::RestaurantAccessGranted(RestaurantAccessGranted {
            membership_id: MembershipId(membership_id),
            scope_type: ScopeType::RESTAURANT,
            scope_id: RestaurantId(uuid::Uuid::from_u128(2)),
            principal_kind: PrincipalKind::MEMBER,
            member_id: MemberId(uuid::Uuid::from_u128(3)),
            auth_subject: AuthSubject("auth-1".to_string()),
            authority: MemberAuthority::MANAGER,
            basis: AccessBasis::CAPTAIN_ONBOARDING,
        })
    }

    fn revoked(membership_id: uuid::Uuid) -> DomainEvent {
        DomainEvent::RestaurantAccessRevoked(RestaurantAccessRevoked {
            membership_id: MembershipId(membership_id),
            ground: AccessRevocationGround::LEFT_THE_RESTAURANT,
        })
    }

    #[test]
    fn no_events_means_no_membership() {
        assert_eq!(fold(&[]), None);
    }

    #[test]
    fn granted_is_born_unrevoked() {
        let id = uuid::Uuid::from_u128(1);
        let state = fold(&[granted(id)]).expect("membership exists");
        assert!(!state.revoked);
    }

    #[test]
    fn revoked_after_granted_is_final() {
        let id = uuid::Uuid::from_u128(1);
        let state = fold(&[granted(id), revoked(id)]).expect("membership exists");
        assert!(state.revoked);
    }

    #[test]
    fn stream_name_matches_the_membership_id() {
        let id = MembershipId(uuid::Uuid::nil());
        assert_eq!(RestaurantMembershipState::stream(id), format!("RestaurantMembership-{}", id.0));
    }
}
