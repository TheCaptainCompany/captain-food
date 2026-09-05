//! `RestaurantInvitation` walk (#639 part C step 6-iv, ADR-20260905-101349 §2/§3): the roster and
//! the invitation, on REAL Postgres. Needs `DATABASE_URL`; SKIPS offline (`DB_TESTS_REQUIRED=0`),
//! the `restaurant_membership.rs` precedent this file mirrors -- application-layer command
//! handlers against a real `PgEventStore`, no HTTP/GraphQL layer (that stack needs the read
//! models this dispatch declares NOT LANDED, see the hand-back). What this proves: invite ->
//! accept (right/wrong email, byte-identical refusal) -> the SECOND command derives the grant's
//! fields from the accepted invitation, never the client's copies -> revoke -> a second accept of
//! an already-accepted/revoked invitation is refused identically -> the door gates the invite leg
//! only.

use application::commands::{
    accept_restaurant_invitation, grant_restaurant_access, invite_restaurant_member,
    revoke_restaurant_invitation,
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
    AcceptRestaurantInvitation, GrantRestaurantAccess, InviteRestaurantMember,
    RevokeRestaurantInvitation,
};
use domain::generated::events::DomainEvent;
use domain::generated::scalars::{
    AccessBasis, EmailAddress, EmailVerificationToken, MemberAuthority, MemberId, MembershipId,
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

/// A scriptable `verify_email_token`: proves `email` for any token except `"bad-token"`
/// (`InvalidVerificationToken`) -- the `FakeIdentity` precedent (`application::behaviour_support`),
/// re-implemented here because that module is `#[cfg(test)]`-private to its own crate.
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
    membership_id: uuid::Uuid,
    invitation_id: uuid::Uuid,
) -> Result<(), DomainError> {
    let store = PgEventStore::new(pool.clone());
    let auth_subjects = infrastructure::PgAuthSubjectReservationRepository::new(pool.clone());
    grant_restaurant_access(
        &store,
        &auth_subjects,
        GrantRestaurantAccess {
            membership_id: MembershipId(membership_id),
            scope_type: None,
            scope_id: None,
            member_id: None,
            auth_subject: None,
            authority: None,
            basis: AccessBasis::MEMBER_INVITATION,
            invitation_id: Some(RestaurantInvitationId(invitation_id)),
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

/// The two-lane accept, end to end: AcceptRestaurantInvitation, then GrantRestaurantAccess derives
/// EVERY field (scopeId/memberId/authority/authSubject) from the invitation's own recorded facts
/// -- never from the (deliberately absent) client copies on the second command. A second
/// submission of the same accepted invitationId is the idempotent-replay path, not a privilege
/// question (M3, named mutant: if the handler trusted a client-supplied field instead of deriving
/// it, this test's assertions on the recorded event would fail).
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

    let membership_id = uuid::Uuid::new_v4();
    grant_from_invitation(&pool, membership_id, invitation_id).await.expect("grant derives from the invitation");

    let store = PgEventStore::new(pool.clone());
    let (events, _version) = store.load(&format!("RestaurantMembership-{membership_id}")).await.expect("load membership stream");
    let DomainEvent::RestaurantAccessGranted(granted) = events.first().expect("one event") else {
        panic!("expected RestaurantAccessGranted, got {:?}", events.first());
    };
    assert_eq!(granted.scope_id.0, restaurant_id, "scopeId derived from the invitation, not a client copy");
    assert_eq!(granted.member_id.0, member_id, "memberId derived from the invitation, not a client copy");
    assert_eq!(granted.authority, MemberAuthority::OPERATOR, "authority derived from the invitation, not a client copy");
    assert_eq!(granted.auth_subject.0, "auth-two-lane", "authSubject derived from the ACCEPTED fact, not a client copy");
    assert_eq!(granted.basis, AccessBasis::MEMBER_INVITATION);

    // A second submission for the same accepted invitationId is idempotent-replay (a DIFFERENT
    // membershipId still names the SAME already-granted relationship's proof; the underlying
    // GrantRestaurantAccess.membershipId idempotency key governs, so this call targets the SAME
    // membershipId to prove replay, not a fresh grant).
    grant_from_invitation(&pool, membership_id, invitation_id).await.expect("idempotent replay, not rejected");
    let (events_after, _) = store.load(&format!("RestaurantMembership-{membership_id}")).await.expect("reload");
    assert_eq!(events_after.len(), 1, "no second RestaurantAccessGranted for the same membershipId");
}

/// M3b (named mutant, stronger than the derivation assertions above): the client's OWN copies of
/// scopeId/memberId/authority/authSubject on the grant leg are actively WRONG here, and the
/// handler must still land the invitation's values -- planted by having the handler prefer
/// `cmd.member_id` when present (`cmd.member_id.unwrap_or(invitation.member_id)`), which this test
/// catches (unlike the derivation test above, whose command always sends `None`).
#[tokio::test]
async fn client_supplied_fields_on_the_grant_leg_are_ignored() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_ignore_client_fields").await else { return };
    let pool = db.pool();
    let invitation_id = uuid::Uuid::new_v4();
    let restaurant_id = uuid::Uuid::new_v4();
    let member_id = uuid::Uuid::new_v4();
    invite(&pool, invitation_id, restaurant_id, member_id, "owner@pizzaroma.fr", MemberAuthority::MANAGER, true)
        .await
        .expect("invite");
    let identity = ScriptedIdentity::new("owner@pizzaroma.fr", "auth-real");
    accept(&pool, &identity, invitation_id, "sb-magic-token-abc").await.expect("accept");

    let membership_id = uuid::Uuid::new_v4();
    let store = PgEventStore::new(pool.clone());
    let auth_subjects = infrastructure::PgAuthSubjectReservationRepository::new(pool.clone());
    grant_restaurant_access(
        &store,
        &auth_subjects,
        GrantRestaurantAccess {
            membership_id: MembershipId(membership_id),
            scope_type: Some(domain::generated::scalars::ScopeType::RESTAURANT),
            scope_id: Some(RestaurantId(uuid::Uuid::new_v4())), // WRONG, must be ignored
            member_id: Some(MemberId(uuid::Uuid::new_v4())),    // WRONG, must be ignored
            auth_subject: Some(domain::generated::scalars::AuthSubject("attacker-supplied".into())), // WRONG, must be ignored
            authority: Some(MemberAuthority::OPERATOR),          // WRONG (invitation says MANAGER), must be ignored
            basis: AccessBasis::MEMBER_INVITATION,
            invitation_id: Some(RestaurantInvitationId(invitation_id)),
        },
        &actor(),
        true,
    )
    .await
    .expect("grant derives from the invitation, not the client's fields");

    let (events, _) = store.load(&format!("RestaurantMembership-{membership_id}")).await.expect("load");
    let DomainEvent::RestaurantAccessGranted(granted) = events.first().expect("one event") else {
        panic!("expected RestaurantAccessGranted");
    };
    assert_eq!(granted.scope_id.0, restaurant_id);
    assert_eq!(granted.member_id.0, member_id);
    assert_eq!(granted.authority, MemberAuthority::MANAGER);
    assert_eq!(granted.auth_subject.0, "auth-real");
}

/// M4 (named mutant): the grant leg's proof is invitationId-presence + ACCEPTED status, never a
/// client-supplied authSubject -- an invitation that was never accepted refuses the grant leg
/// identically to an unknown invitationId (no enumeration oracle carried into the grant leg
/// either).
#[tokio::test]
async fn grant_from_an_unaccepted_invitation_is_refused() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_invitation_grant_unaccepted").await else { return };
    let pool = db.pool();
    let invitation_id = uuid::Uuid::new_v4();
    invite(&pool, invitation_id, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), "owner@pizzaroma.fr", MemberAuthority::MANAGER, true)
        .await
        .expect("invite, never accepted");
    let err = grant_from_invitation(&pool, uuid::Uuid::new_v4(), invitation_id).await.unwrap_err();
    assert!(matches!(err, DomainError::Rejected { code, .. } if code == "RestaurantInvitationNotAcceptable"));

    let missing_proof = {
        let store = PgEventStore::new(pool.clone());
        let auth_subjects = infrastructure::PgAuthSubjectReservationRepository::new(pool.clone());
        grant_restaurant_access(
            &store,
            &auth_subjects,
            GrantRestaurantAccess {
                membership_id: MembershipId(uuid::Uuid::new_v4()),
                scope_type: None,
                scope_id: None,
                member_id: None,
                auth_subject: None,
                authority: None,
                basis: AccessBasis::MEMBER_INVITATION,
                invitation_id: None,
            },
            &actor(),
            true,
        )
        .await
        .unwrap_err()
    };
    assert!(matches!(missing_proof, DomainError::Rejected { code, .. } if code == "InvitationProofRequired"));
}

/// The invite door gates the invite leg only; the revoke leg is NEVER gated (M5, named mutant: a
/// door check accidentally added to `revoke_restaurant_invitation` would make this red).
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
