//! `RestaurantInvitation` aggregate — the PURE write-side state fold (ADR-0035), mirroring
//! `restaurant_membership.rs` (#639 part C step 6-iv, ADR-20260905-101349 §2/§3): the roster and
//! the invitation, its own aggregate, its own stream, its own lane (FORK 1 Option A again). No
//! I/O, no serialization logic (dependency rule).
//!
//! `lifecycle:` in the DSL is PENDING -> {ACCEPTED, REVOKED, EXPIRED}, all terminal; `state`
//! carries exactly what the handlers need to decide: the invited email + minted `MemberId` (for
//! `AcceptRestaurantInvitation`'s comparison and `GrantRestaurantAccess`'s derivation),
//! `restaurant_id`/`authority` (also for the grant leg's derivation), and which terminal state (if
//! any) the invitation has already reached, plus the `AuthSubject` the accept recorded.

use crate::generated::events::DomainEvent;
use crate::generated::scalars::{AuthSubject, EmailAddress, MemberAuthority, MemberId, RestaurantId};

/// What a terminal `RestaurantInvitation` has become — `None` in [`RestaurantInvitationState`]
/// means still PENDING.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestaurantInvitationTerminal {
    Accepted,
    Revoked,
    Expired,
}

/// What the RestaurantInvitation command handlers (AND `GrantRestaurantAccess`'s `MEMBER_INVITATION`
/// leg, reading across streams) need to know. `None` (from [`fold`]) means no
/// `RestaurantInvitationSent` yet on this stream.
#[derive(Debug, Clone, PartialEq)]
pub struct RestaurantInvitationState {
    pub restaurant_id: RestaurantId,
    pub invited_email: EmailAddress,
    pub authority: MemberAuthority,
    /// CALLER-MINTED on `InviteRestaurantMember` (ADR-0034, corrected from an earlier handler-mint
    /// draft, round 1 STOP finding) — "ours, so it exists before any credential does" holds either
    /// way, since the platform still mints it before the invitee ever authenticates. The SAME id
    /// rides on the eventual `RestaurantAccessGranted` (or the reservation holder's, round 2 §4).
    pub member_id: MemberId,
    /// `Some` once `RestaurantInvitationAccepted` has landed — `verify_email_token`'s OUTPUT,
    /// never a payload field (ADR-0041). `GrantRestaurantAccess`'s `MEMBER_INVITATION` leg reads
    /// this to populate the grant's own `authSubject`.
    pub accepted_auth_subject: Option<AuthSubject>,
    /// `None` while PENDING; `Some` once the invitation has reached a terminal state.
    pub terminal: Option<RestaurantInvitationTerminal>,
}

impl RestaurantInvitationState {
    /// Whether `AcceptRestaurantInvitation`/`GrantRestaurantAccess(MEMBER_INVITATION)` may proceed
    /// against this invitation right now — PENDING, and only PENDING
    /// (`rules.yaml#/RestaurantInvitationAcceptIsUniformlyRefused`).
    pub fn is_pending(&self) -> bool {
        self.terminal.is_none()
    }

    /// Whether the invitation has been ACCEPTED (`GrantRestaurantAccess`'s `MEMBER_INVITATION`
    /// proof, `rules.yaml#/MemberInvitationGrantDerivesFromInvitation`).
    pub fn is_accepted(&self) -> bool {
        matches!(self.terminal, Some(RestaurantInvitationTerminal::Accepted))
    }
}

/// Fold a RestaurantInvitation stream (events in version order) into its current state. `None` ⇔
/// the stream has no `RestaurantInvitationSent` yet, i.e. the invitation does not exist.
pub fn fold(events: &[DomainEvent]) -> Option<RestaurantInvitationState> {
    events.iter().fold(None, apply)
}

fn apply(
    state: Option<RestaurantInvitationState>,
    event: &DomainEvent,
) -> Option<RestaurantInvitationState> {
    match event {
        DomainEvent::RestaurantInvitationSent(e) => {
            // Birth. One InviteRestaurantMember per invitationId in practice (the aggregate's own
            // id is caller-minted per-invite, the GrantRestaurantAccess/membershipId precedent),
            // so a second birth on the SAME stream is not a modeled case; folding defensively
            // keeps the FIRST birth's facts rather than resetting them.
            Some(state.unwrap_or(RestaurantInvitationState {
                restaurant_id: e.restaurant_id,
                invited_email: e.invited_email.clone(),
                authority: e.authority,
                member_id: e.member_id,
                accepted_auth_subject: None,
                terminal: None,
            }))
        }
        // First-terminal-wins on replay (round 2, young): the lifecycle only ever transitions OUT
        // of PENDING once, but a fold must stay correct even against a stray/duplicate terminal
        // event on the stream -- a later Expired must never overwrite an already-recorded
        // Accepted, because the cross-stream grant read's safety (vernon) rests on ACCEPTED being
        // durably terminal once folded.
        DomainEvent::RestaurantInvitationAccepted(e) => {
            let mut s = state?;
            if s.terminal.is_none() {
                s.accepted_auth_subject = Some(e.auth_subject.clone());
                s.terminal = Some(RestaurantInvitationTerminal::Accepted);
            }
            Some(s)
        }
        DomainEvent::RestaurantInvitationRevoked(_) => {
            let mut s = state?;
            if s.terminal.is_none() {
                s.terminal = Some(RestaurantInvitationTerminal::Revoked);
            }
            Some(s)
        }
        DomainEvent::RestaurantInvitationExpired(_) => {
            let mut s = state?;
            if s.terminal.is_none() {
                s.terminal = Some(RestaurantInvitationTerminal::Expired);
            }
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
        RestaurantInvitationAccepted, RestaurantInvitationExpired, RestaurantInvitationRevoked,
        RestaurantInvitationSent,
    };
    use crate::generated::scalars::RestaurantInvitationId;

    fn sent(id: uuid::Uuid) -> DomainEvent {
        DomainEvent::RestaurantInvitationSent(RestaurantInvitationSent {
            invitation_id: RestaurantInvitationId(id),
            restaurant_id: RestaurantId(uuid::Uuid::from_u128(2)),
            invited_email: EmailAddress("colleague@example.com".into()),
            authority: MemberAuthority::OPERATOR,
            member_id: MemberId(uuid::Uuid::from_u128(3)),
        })
    }

    fn accepted(id: uuid::Uuid) -> DomainEvent {
        DomainEvent::RestaurantInvitationAccepted(RestaurantInvitationAccepted {
            invitation_id: RestaurantInvitationId(id),
            auth_subject: AuthSubject("auth-supabase-1".into()),
        })
    }

    fn revoked(id: uuid::Uuid) -> DomainEvent {
        DomainEvent::RestaurantInvitationRevoked(RestaurantInvitationRevoked {
            invitation_id: RestaurantInvitationId(id),
        })
    }

    fn expired(id: uuid::Uuid) -> DomainEvent {
        DomainEvent::RestaurantInvitationExpired(RestaurantInvitationExpired {
            invitation_id: RestaurantInvitationId(id),
        })
    }

    #[test]
    fn no_events_means_no_invitation() {
        assert_eq!(fold(&[]), None);
    }

    #[test]
    fn sent_is_born_pending() {
        let id = uuid::Uuid::from_u128(1);
        let state = fold(&[sent(id)]).expect("invitation exists");
        assert!(state.is_pending());
        assert!(!state.is_accepted());
    }

    #[test]
    fn accepted_carries_the_verified_auth_subject_and_is_terminal() {
        let id = uuid::Uuid::from_u128(1);
        let state = fold(&[sent(id), accepted(id)]).expect("invitation exists");
        assert!(!state.is_pending());
        assert!(state.is_accepted());
        assert_eq!(state.accepted_auth_subject, Some(AuthSubject("auth-supabase-1".into())));
    }

    #[test]
    fn revoked_is_terminal_and_not_accepted() {
        let id = uuid::Uuid::from_u128(1);
        let state = fold(&[sent(id), revoked(id)]).expect("invitation exists");
        assert!(!state.is_pending());
        assert!(!state.is_accepted());
    }

    #[test]
    fn expired_is_terminal_and_not_accepted() {
        let id = uuid::Uuid::from_u128(1);
        let state = fold(&[sent(id), expired(id)]).expect("invitation exists");
        assert!(!state.is_pending());
        assert!(!state.is_accepted());
    }

    /// Round 2 (young): a stray `Expired` after an already-recorded `Accepted` must never
    /// overwrite it -- the cross-stream grant read's safety rests on ACCEPTED being durably
    /// terminal once folded.
    #[test]
    fn a_stray_expired_after_accepted_never_overwrites_it() {
        let id = uuid::Uuid::from_u128(1);
        let state = fold(&[sent(id), accepted(id), expired(id)]).expect("invitation exists");
        assert!(state.is_accepted(), "first-terminal-wins: ACCEPTED must survive a later Expired");
        assert_eq!(state.accepted_auth_subject, Some(AuthSubject("auth-supabase-1".into())));
    }

    /// Round 2 (vernon): ACCEPTED's terminality is pinned directly -- the grant-by-invitation
    /// leg's whole proof rests on `is_pending()` being false and `is_accepted()` being true once
    /// this fact has landed, whatever else replays after it.
    #[test]
    fn accepted_is_pinned_terminal() {
        let id = uuid::Uuid::from_u128(1);
        let state = fold(&[sent(id), accepted(id)]).expect("invitation exists");
        assert!(!state.is_pending());
        assert!(state.is_accepted());
    }

    #[test]
    fn stream_name_matches_the_invitation_id() {
        let id = RestaurantInvitationId(uuid::Uuid::nil());
        assert_eq!(RestaurantInvitationState::stream(id), format!("RestaurantInvitation-{}", id.0));
    }
}
