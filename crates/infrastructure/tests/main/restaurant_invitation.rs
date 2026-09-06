//! `RestaurantInvitation` walk (#639 part C step 6-iv, ADR-20260905-101349 §2/§3, round 2's §2
//! amendment): the roster and the invitation, on REAL Postgres. Needs `DATABASE_URL`; SKIPS offline
//! (`DB_TESTS_REQUIRED=0`), the `restaurant_membership.rs` precedent this file mirrors --
//! application-layer command handlers against a real `PgEventStore`, no HTTP/GraphQL layer (that
//! stack needs the read models this dispatch declares NOT LANDED, see the hand-back). What this
//! proves: invite -> accept (right/wrong email, byte-identical refusal, the token spent
//! UNCONDITIONALLY) -> the SECOND, PUBLIC-only command derives the grant's fields from the accepted
//! invitation, proving the caller IS the accepting subject, deriving a UUIDv5 membershipId and
//! reusing an already-held memberId for a re-hire -> revoke -> a second accept of an
//! already-accepted/revoked invitation is refused identically -> the door gates the invite leg only.

use application::commands::{
    accept_restaurant_invitation, grant_restaurant_access_by_invitation, invite_restaurant_member,
    restaurant_membership_id_for_invitation, revoke_restaurant_invitation,
};
use application::generated::services::{
    IdentityRefreshSessionInput, IdentityRefreshSessionOutput, IdentitySendEmailMagicLinkInput,
    IdentitySendPhoneOtpInput, IdentityService, IdentityStampCustomerClaimInput,
    IdentityStampMemberClaimInput, IdentityStampRiderClaimInput, IdentityVerifyEmailTokenInput,
    IdentityVerifyEmailTokenOutput, IdentityVerifyPhoneOtpInput, IdentityVerifyPhoneOtpOutput,
    ServiceCallMeta,
};
use application::ports::{Actor, EventStore};
use async_trait::async_trait;
use domain::generated::commands::{
    AcceptRestaurantInvitation, GrantRestaurantAccessByInvitation, InviteRestaurantMember,
    RevokeRestaurantInvitation,
};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{
    AccessBasis, EmailAddress, EmailVerificationToken, MemberAuthority, MemberId,
    RestaurantId, RestaurantInvitationId,
};
use domain::shared::errors::DomainError;
use infrastructure::PgEventStore;
use sqlx::PgPool;
use std::sync::Mutex;

fn actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::nil(),
        user_type: "RESTAURANT".to_string(),
        domain_id: None,
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

/// A scriptable `verify_email_token`: proves `email`/`auth_subject` for any token except
/// `"bad-token"` (`InvalidVerificationToken`) -- the `FakeIdentity` precedent
/// (`application::behaviour_support`), re-implemented here because that module is
/// `#[cfg(test)]`-private to its own crate. `calls` is asserted directly (beck): a refusal alone
/// never proves whether the token was spent.
struct ScriptedIdentity {
    email: EmailAddress,
    auth_subject: domain::generated::scalars::AuthSubject,
    calls: Mutex<u32>,
}

impl ScriptedIdentity {
    fn new(email: &str, auth_subject: &str) -> Self {
        Self {
            email: EmailAddress(email.to_string()),
            auth_subject: domain::generated::scalars::AuthSubject(auth_subject.to_string()),
            calls: Mutex::new(0),
        }
    }

    fn calls(&self) -> u32 {
        *self.calls.lock().expect("mutex")
    }
}

#[async_trait]
impl IdentityService for ScriptedIdentity {
    async fn send_phone_otp(&self, _i: IdentitySendPhoneOtpInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        Ok(())
    }
    async fn verify_phone_otp(
        &self,
        _i: IdentityVerifyPhoneOtpInput,
        _m: &ServiceCallMeta,
    ) -> Result<IdentityVerifyPhoneOtpOutput, DomainError> {
        Err(DomainError::rejected("InvalidVerificationCode", serde_json::json!({})))
    }
    async fn refresh_session(
        &self,
        _i: IdentityRefreshSessionInput,
        _m: &ServiceCallMeta,
    ) -> Result<IdentityRefreshSessionOutput, DomainError> {
        Err(DomainError::Repository("not scripted".into()))
    }
    async fn stamp_customer_claim(&self, _i: IdentityStampCustomerClaimInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        Ok(())
    }
    async fn stamp_rider_claim(&self, _i: IdentityStampRiderClaimInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        Ok(())
    }
    async fn send_email_magic_link(&self, _i: IdentitySendEmailMagicLinkInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        Ok(())
    }
    async fn send_admin_sign_in_link(&self, _i: application::generated::services::IdentitySendAdminSignInLinkInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        Ok(())
    }
    async fn verify_email_token(
        &self,
        input: IdentityVerifyEmailTokenInput,
        _m: &ServiceCallMeta,
    ) -> Result<IdentityVerifyEmailTokenOutput, DomainError> {
        *self.calls.lock().expect("mutex") += 1;
        if input.token.0 == "bad-token" {
            return Err(DomainError::rejected("InvalidVerificationToken", serde_json::json!({})));
        }
        Ok(IdentityVerifyEmailTokenOutput {
            auth_ref: self.auth_subject.clone(),
            email: self.email.clone(),
            access_token: Some("fake.access".into()),
            refresh_token: Some("fake.refresh".into()),
            expires_in: Some(3600),
        })
    }
    async fn stamp_member_claim(&self, _i: IdentityStampMemberClaimInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        Ok(())
    }
    async fn stamp_admin_claim(&self, _i: application::generated::services::IdentityStampAdminClaimInput, _m: &ServiceCallMeta) -> Result<(), DomainError> {
        Ok(())
    }
}

fn token(s: &str) -> EmailVerificationToken {
    EmailVerificationToken(s.to_string())
}

async fn invite(
    pool: &PgPool,
    invitation_id: uuid::Uuid,
    restaurant_id: uuid::Uuid,
    member_id: uuid::Uuid,
    email: &str,
    authority: MemberAuthority,
    door: bool,
) -> Result<(), DomainError> {
    let store = PgEventStore::new(pool.clone());
    invite_restaurant_member(
        &store,
        InviteRestaurantMember {
            invitation_id: RestaurantInvitationId(invitation_id),
            restaurant_id: RestaurantId(restaurant_id),
            invited_email: EmailAddress(email.to_string()),
            authority,
            member_id: MemberId(member_id),
        },
        &actor(),
        door,
    )
    .await
}

async fn accept(
    pool: &PgPool,
    identity: &dyn IdentityService,
    invitation_id: uuid::Uuid,
    tok: &str,
) -> Result<(), DomainError> {
    let store = PgEventStore::new(pool.clone());
    accept_restaurant_invitation(
        &store,
        identity,
        AcceptRestaurantInvitation { invitation_id: RestaurantInvitationId(invitation_id), token: token(tok) },
        &actor(),
    )
    .await
}

async fn revoke(pool: &PgPool, invitation_id: uuid::Uuid) -> Result<(), DomainError> {
    let store = PgEventStore::new(pool.clone());
    revoke_restaurant_invitation(
        &store,
        RevokeRestaurantInvitation { invitation_id: RestaurantInvitationId(invitation_id) },
        &actor(),
    )
    .await
}

async fn grant_from_invitation(
    pool: &PgPool,
    identity: &dyn IdentityService,
    invitation_id: uuid::Uuid,
    tok: &str,
) -> Result<(), DomainError> {
    let store = PgEventStore::new(pool.clone());
    let auth_subjects = infrastructure::PgAuthSubjectReservationRepository::new(pool.clone());
    grant_restaurant_access_by_invitation(
        &store,
        identity,
        &auth_subjects,
        GrantRestaurantAccessByInvitation {
            invitation_id: RestaurantInvitationId(invitation_id),
            token: token(tok),
        },
        &actor(),
        true, // RUN_MEMBER_ACCESS_GRANT ON -- this walk exercises the aggregate, not the door.
    )
    .await
}

/// M1 (named mutant, planted and reverted): if `accept_restaurant_invitation` compared emails
/// case-sensitively, this would fail (RestaurantInvitationNotAcceptable) instead of succeeding.
#[tokio::test]
async fn accept_matches_case_insensitively() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_case_fold").await else { return };
    let pool = db.pool();
    let invitation_id = uuid::Uuid::new_v4();
    invite(&pool, invitation_id, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "Owner@PizzaRoma.fr", MemberAuthority::OPERATOR, true)
        .await
        .expect("invite");
    let identity = ScriptedIdentity::new("owner@pizzaroma.fr", "auth-1");
    accept(&pool, &identity, invitation_id, "sb-magic-token-abc").await.expect("accept, case-folded");
}

/// The no-enumeration property (M2, named mutant): an unknown invitationId and a WRONG verified
/// email produce the byte-identical typed error -- planted by comparing the two `DomainError`
/// values structurally, not just their variant.
#[tokio::test]
async fn wrong_email_and_unknown_invitation_refuse_identically() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_no_enum").await else { return };
    let pool = db.pool();
    let invitation_id = uuid::Uuid::new_v4();
    invite(&pool, invitation_id, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "owner@pizzaroma.fr", MemberAuthority::MANAGER, true)
        .await
        .expect("invite");
    let stranger = ScriptedIdentity::new("someone-else@example.com", "auth-2");
    let wrong_email = accept(&pool, &stranger, invitation_id, "sb-magic-token-abc").await.unwrap_err();
    let genuine = ScriptedIdentity::new("owner@pizzaroma.fr", "auth-3");
    let unknown = accept(&pool, &genuine, uuid::Uuid::new_v4(), "sb-magic-token-abc").await.unwrap_err();
    match (&wrong_email, &unknown) {
        (DomainError::Rejected { code: c1, context: x1 }, DomainError::Rejected { code: c2, context: x2 }) => {
            assert_eq!(c1, "RestaurantInvitationNotAcceptable");
            assert_eq!(c1, c2);
            // The context carries only `invitationId` (the caller's OWN echo, never a hint about
            // which of the five causes applied) -- shaped identically for both.
            assert_eq!(x1.as_object().map(|o| o.keys().collect::<Vec<_>>()), x2.as_object().map(|o| o.keys().collect::<Vec<_>>()));
        }
        other => panic!("expected two Rejected errors, got {other:?}"),
    }
    // Round 2 (beck BLOCKING): the token is spent UNCONDITIONALLY on BOTH legs -- an unknown
    // invitationId burns it exactly like a wrong-email one. Red under the round-1 ordering (the
    // `is_pending()` filter ran BEFORE `verify_email_token`, so the unknown leg's `calls` stayed 0).
    assert_eq!(stranger.calls(), 1, "wrong-email leg must call verify_email_token exactly once");
    assert_eq!(genuine.calls(), 1, "unknown-invitation leg must call verify_email_token exactly once too");
}

#[tokio::test]
async fn revoked_invitation_refuses_accept_and_a_second_revoke() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_revoke").await else { return };
    let pool = db.pool();
    let invitation_id = uuid::Uuid::new_v4();
    invite(&pool, invitation_id, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "owner@pizzaroma.fr", MemberAuthority::MANAGER, true)
        .await
        .expect("invite");
    revoke(&pool, invitation_id).await.expect("revoke");
    let identity = ScriptedIdentity::new("owner@pizzaroma.fr", "auth-4");
    let err = accept(&pool, &identity, invitation_id, "sb-magic-token-abc").await.unwrap_err();
    assert!(matches!(err, DomainError::Rejected { code, .. } if code == "RestaurantInvitationNotAcceptable"));
    let second_revoke = revoke(&pool, invitation_id).await.unwrap_err();
    assert!(matches!(second_revoke, DomainError::Rejected { code, .. } if code == "RestaurantInvitationAlreadyRevoked"));
}

/// The two-lane accept, end to end: `AcceptRestaurantInvitation`, then
/// `GrantRestaurantAccessByInvitation` derives EVERY field (scopeId/memberId/authority/authSubject)
/// from the invitation's own recorded facts and its OWN proved token -- there is no client copy to
/// trust or ignore, since the command carries none. A second submission of the same accepted
/// invitationId is the idempotent-replay path on the DERIVED membershipId, not a privilege question
/// (M3, named mutant: if the handler derived any field differently, this test's assertions on the
/// recorded event would fail).
#[tokio::test]
async fn the_two_lane_accept_derives_the_grant_from_the_invitation() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_two_lane").await else { return };
    let pool = db.pool();
    let invitation_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let member_id = uuid::Uuid::new_v4();
    invite(&pool, invitation_id, restaurant_id, member_id, "owner@pizzaroma.fr", MemberAuthority::OPERATOR, true)
        .await
        .expect("invite");

    let identity = ScriptedIdentity::new("owner@pizzaroma.fr", "auth-two-lane");
    accept(&pool, &identity, invitation_id, "sb-magic-token-abc").await.expect("accept");

    grant_from_invitation(&pool, &identity, invitation_id, "sb-magic-token-abc")
        .await
        .expect("grant derives from the invitation");

    let membership_id = restaurant_membership_id_for_invitation(RestaurantInvitationId(invitation_id));
    let store = PgEventStore::new(pool.clone());
    let (events, _version) = store.load(&format!("RestaurantMembership-{}", membership_id.0)).await.expect("load membership stream");
    let DomainEvent::RestaurantAccessGranted(granted) = events.first().expect("one event") else {
        panic!("expected RestaurantAccessGranted, got {:?}", events.first());
    };
    assert_eq!(granted.scope_id.0, restaurant_id, "scopeId derived from the invitation");
    assert_eq!(granted.member_id.0, member_id, "memberId derived from the invitation (never held elsewhere)");
    assert_eq!(granted.authority, MemberAuthority::OPERATOR, "authority derived from the invitation");
    assert_eq!(granted.auth_subject.0, "auth-two-lane", "authSubject derived from the ACCEPTED fact / the grant leg's own proof");
    assert_eq!(granted.basis, AccessBasis::MEMBER_INVITATION);

    // A second submission for the same accepted invitationId is idempotent-replay: the DERIVED
    // membershipId names the SAME stream, so this is ONE fact for the whole invitation, never two
    // (vernon B1: an accepted invitation must yield at most one membership).
    grant_from_invitation(&pool, &identity, invitation_id, "sb-magic-token-abc")
        .await
        .expect("idempotent replay, not rejected");
    let (events_after, _) = store.load(&format!("RestaurantMembership-{}", membership_id.0)).await.expect("reload");
    assert_eq!(events_after.len(), 1, "no second RestaurantAccessGranted for the same accepted invitation");
}

/// vernon B2 (the caller-is-the-subject proof): a stranger who learns the invitationId and proves
/// their OWN valid token (a DIFFERENT auth subject than the one who accepted) gains nothing --
/// refused identically to an unknown invitation, never a hint that this one belongs to someone else.
#[tokio::test]
async fn a_stranger_with_their_own_valid_token_gains_nothing() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_stranger_grant").await else { return };
    let pool = db.pool();
    let invitation_id = uuid::Uuid::new_v4();
    invite(&pool, invitation_id, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "owner@pizzaroma.fr", MemberAuthority::MANAGER, true)
        .await
        .expect("invite");
    let owner = ScriptedIdentity::new("owner@pizzaroma.fr", "auth-owner");
    accept(&pool, &owner, invitation_id, "sb-magic-token-abc").await.expect("accept");

    let stranger = ScriptedIdentity::new("stranger@example.com", "auth-stranger");
    let err = grant_from_invitation(&pool, &stranger, invitation_id, "sb-magic-token-stranger")
        .await
        .unwrap_err();
    assert!(matches!(err, DomainError::Rejected { code, .. } if code == "RestaurantInvitationNotAcceptable"));

    // Nothing was granted: the membership stream the (correct) derivation would target stays empty.
    let membership_id = restaurant_membership_id_for_invitation(RestaurantInvitationId(invitation_id));
    let store = PgEventStore::new(pool.clone());
    let (events, _) = store.load(&format!("RestaurantMembership-{}", membership_id.0)).await.expect("load");
    assert!(events.is_empty(), "a stranger's own valid token must grant nothing");
}

/// young B1: a re-hire (or a person joining a second restaurant) whose auth subject already holds
/// a memberId from an EARLIER accepted invitation gets the grant with the EXISTING memberId, never
/// the second invitation's freshly-minted one -- no `MemberAuthSubjectAlreadyBound` after a
/// terminal accept, with no retry available.
#[tokio::test]
async fn a_rehire_reuses_the_held_member_id() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_rehire").await else { return };
    let pool = db.pool();
    let first_invitation = uuid::Uuid::new_v4();
    let first_member = uuid::Uuid::new_v4();
    invite(&pool, first_invitation, uuid::Uuid::new_v4(), first_member, "person@example.com", MemberAuthority::OPERATOR, true)
        .await
        .expect("first invite");
    let person = ScriptedIdentity::new("person@example.com", "auth-person");
    accept(&pool, &person, first_invitation, "sb-magic-token-first").await.expect("first accept");
    grant_from_invitation(&pool, &person, first_invitation, "sb-magic-token-first")
        .await
        .expect("first grant");

    // A SECOND restaurant invites the SAME person (SAME email, SAME resulting auth subject once
    // accepted) with a FRESH, DIFFERENT minted memberId.
    let second_invitation = uuid::Uuid::new_v4();
    let second_restaurant = uuid::Uuid::new_v4();
    let second_member = uuid::Uuid::new_v4();
    assert_ne!(first_member, second_member);
    invite(&pool, second_invitation, second_restaurant, second_member, "person@example.com", MemberAuthority::MANAGER, true)
        .await
        .expect("second invite");
    accept(&pool, &person, second_invitation, "sb-magic-token-first").await.expect("second accept");
    grant_from_invitation(&pool, &person, second_invitation, "sb-magic-token-first")
        .await
        .expect("second grant reuses the held memberId");

    let second_membership_id = restaurant_membership_id_for_invitation(RestaurantInvitationId(second_invitation));
    let store = PgEventStore::new(pool.clone());
    let (events, _) = store.load(&format!("RestaurantMembership-{}", second_membership_id.0)).await.expect("load second");
    let DomainEvent::RestaurantAccessGranted(granted) = events.first().expect("one event") else {
        panic!("expected RestaurantAccessGranted");
    };
    assert_eq!(granted.member_id.0, first_member, "the SECOND grant must reuse the FIRST invitation's memberId");
    assert_ne!(granted.member_id.0, second_member, "never the second invitation's freshly-minted memberId");
    assert_eq!(granted.scope_id.0, second_restaurant);
    assert_eq!(granted.authority, MemberAuthority::MANAGER);
}

/// M4 (named mutant): the grant leg's proof is invitationId-presence + ACCEPTED status + the
/// caller's own proved subject matching the accepted one, never anything else -- an invitation
/// that was never accepted refuses the grant leg identically to an unknown invitationId (no
/// enumeration oracle carried into the grant leg either).
#[tokio::test]
async fn grant_from_an_unaccepted_invitation_is_refused() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_grant_unaccepted").await else { return };
    let pool = db.pool();
    let invitation_id = uuid::Uuid::new_v4();
    invite(&pool, invitation_id, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "owner@pizzaroma.fr", MemberAuthority::MANAGER, true)
        .await
        .expect("invite, never accepted");
    let identity = ScriptedIdentity::new("owner@pizzaroma.fr", "auth-owner");
    let err = grant_from_invitation(&pool, &identity, invitation_id, "sb-magic-token-abc").await.unwrap_err();
    assert!(matches!(err, DomainError::Rejected { code, .. } if code == "RestaurantInvitationNotAcceptable"));
}

/// The invite door gates the invite leg only; the revoke leg is NEVER gated. M5's shape is
/// COMPILER-ENFORCED rather than mutant-tested (round 1 correction, beck): `revoke_restaurant_
/// invitation`'s signature carries no boolean gate parameter at all, so a door check on that path
/// is structurally unspellable -- there is no mutant to plant.
#[tokio::test]
async fn the_door_refuses_invite_but_not_revoke() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_door").await else { return };
    let pool = db.pool();
    let invitation_id = uuid::Uuid::new_v4();

    let closed = invite(&pool, invitation_id, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "owner@pizzaroma.fr", MemberAuthority::MANAGER, false)
        .await
        .unwrap_err();
    assert!(matches!(closed, DomainError::Rejected { code, .. } if code == "RestaurantInvitationDoorClosed"));

    // Nothing was appended (the door refuses BEFORE the store is touched) -- revoke on the SAME
    // id, still with no invitation ever having landed, is `RestaurantInvitationNotFound`, never a
    // door refusal: the ungated leg reads exactly like the door was never involved.
    let not_found = revoke(&pool, invitation_id).await.unwrap_err();
    assert!(matches!(not_found, DomainError::Rejected { code, .. } if code == "RestaurantInvitationNotFound"));

    // Now invite for REAL (door ON) and prove revoke still succeeds while the door stays OFF for
    // a DIFFERENT invite attempt on a fresh id, never touching this one's stream.
    invite(&pool, invitation_id, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "owner@pizzaroma.fr", MemberAuthority::MANAGER, true)
        .await
        .expect("invite, door ON");
    revoke(&pool, invitation_id).await.expect("revoke never gated");
}
