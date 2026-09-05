//! `RestaurantRoster` read model (#639 part C step 6-iv round 2, ADR-20260905-101349 §2 amendment,
//! PROP-20260831-180622 §6.4): the executable rebuild recipe -- checkpoint reset, NEVER TRUNCATE
//! (the table's own `rules:`) -- plus the flat, GRANT-shaped predicates the query serves. No
//! predicate here is ever a revocation test (PROP §6.4).

use application::commands::{grant_restaurant_access, revoke_restaurant_access};
use application::ports::Actor;
use domain::generated::commands::{GrantRestaurantAccess, RevokeRestaurantAccess};
use domain::generated::scalars::{
    AccessBasis, AccessRevocationGround, AuthSubject, MemberAuthority, MemberId, MembershipId,
    RestaurantId, ScopeType,
};
use infrastructure::{PgAuthSubjectReservationRepository, PgEventStore, ProjectionWorker};
use sqlx::{PgPool, Row};

fn actor() -> Actor {
    Actor {
        user_id: uuid::Uuid::nil(),
        user_type: "ADMIN".to_string(),
        domain_id: None,
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    }
}

async fn grant(
    pool: &PgPool,
    membership_id: uuid::Uuid,
    scope_id: uuid::Uuid,
    member_id: uuid::Uuid,
    auth_subject: &str,
    authority: MemberAuthority,
) -> Result<(), domain::shared::errors::DomainError> {
    let store = PgEventStore::new(pool.clone());
    let auth_subjects = PgAuthSubjectReservationRepository::new(pool.clone());
    grant_restaurant_access(
        &store,
        &auth_subjects,
        GrantRestaurantAccess {
            membership_id: MembershipId(membership_id),
            scope_type: ScopeType::RESTAURANT,
            scope_id: RestaurantId(scope_id),
            member_id: MemberId(member_id),
            auth_subject: AuthSubject(auth_subject.to_string()),
            authority,
            basis: AccessBasis::CAPTAIN_ONBOARDING,
        },
        &actor(),
        true,
    )
    .await
}

async fn roster_row(pool: &PgPool, membership_id: uuid::Uuid) -> Option<(uuid::Uuid, String)> {
    sqlx::query("SELECT scope_id, authority FROM restaurantroster WHERE membership_id = $1")
        .bind(membership_id)
        .fetch_optional(pool)
        .await
        .expect("query restaurantroster")
        .map(|r| (r.get::<uuid::Uuid, _>("scope_id"), r.get::<String, _>("authority")))
}

/// A GRANT-shaped predicate: the row exists with the right values (never "absence" as a signal).
#[tokio::test]
async fn a_grant_projects_the_roster_row() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_roster_grant").await else { return };
    let pool = db.pool();
    let (membership, scope, member) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    grant(&pool, membership, scope, member, "auth-roster-1", MemberAuthority::MANAGER).await.expect("grant");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once");

    let row = roster_row(&pool, membership).await.expect("roster row exists");
    assert_eq!(row.0, scope);
    assert_eq!(row.1, "MANAGER");
}

/// The executable rebuild recipe (dba, PROP §6.4): checkpoint reset, NEVER TRUNCATE. Right after
/// the reset, BEFORE the replay runs, the row must still be there (a checkpoint reset alone never
/// hides a member) -- the opposite of `RestaurantInvitationList`'s TRUNCATE-together recipe.
#[tokio::test]
async fn checkpoint_reset_never_hides_a_member_and_replay_reproduces_the_row() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_roster_checkpoint_reset").await else { return };
    let pool = db.pool();
    let (membership, scope, member) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    grant(&pool, membership, scope, member, "auth-roster-2", MemberAuthority::OPERATOR).await.expect("grant");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (first drain)");
    assert!(roster_row(&pool, membership).await.is_some(), "roster row exists after the first drain");

    let reset = sqlx::query("UPDATE projection_checkpoint SET position = 0 WHERE projector = 'RestaurantRoster'")
        .execute(&pool)
        .await
        .expect("rewind the RestaurantRoster checkpoint");
    assert_eq!(
        reset.rows_affected(),
        1,
        "the projector name must match the registered 'RestaurantRoster' group -- a rename here \
         would pass vacuously with 0 rows touched (the Member precedent, R2-4)"
    );
    assert!(
        roster_row(&pool, membership).await.is_some(),
        "a checkpoint reset alone must never hide a member -- the row was never touched"
    );

    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (replay)");
    let row = roster_row(&pool, membership).await.expect("row survives the replay");
    assert_eq!(row.0, scope);
    assert_eq!(row.1, "OPERATOR");
}

/// A colleague's row is untouched by ANOTHER membership's revoke -- the roster's own scope,
/// never a whole-restaurant fold (grant-shaped, never a revocation predicate).
#[tokio::test]
async fn one_membership_leaves_a_colleagues_roster_row_intact() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_roster_two_members").await else { return };
    let pool = db.pool();
    let scope = uuid::Uuid::new_v4();
    let (membership_a, member_a) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let (membership_b, member_b) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    grant(&pool, membership_a, scope, member_a, "auth-roster-a", MemberAuthority::MANAGER).await.expect("grant a");
    grant(&pool, membership_b, scope, member_b, "auth-roster-b", MemberAuthority::OPERATOR).await.expect("grant b");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once");

    assert!(roster_row(&pool, membership_a).await.is_some());
    assert!(roster_row(&pool, membership_b).await.is_some());
}

async fn revoke(
    pool: &PgPool,
    membership_id: uuid::Uuid,
) -> Result<(), domain::shared::errors::DomainError> {
    let store = PgEventStore::new(pool.clone());
    revoke_restaurant_access(
        &store,
        RevokeRestaurantAccess {
            membership_id: MembershipId(membership_id),
            ground: AccessRevocationGround::ACCESS_NO_LONGER_NEEDED,
        },
        &actor(),
    )
    .await
}

/// Round 3 (#639 part C step 6-iv, dba BLOCKING): the ADDITIVE `RestaurantAccessRevoked` DELETE
/// arm -- grant -> revoke -> the row is GONE, never a stale listing (the exact defect round 2
/// left named: "a revoked colleague stays listed until the revoke-removal follow-up lands").
#[tokio::test]
async fn a_revoke_deletes_the_roster_row() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_roster_revoke_deletes").await else { return };
    let pool = db.pool();
    let (membership, scope, member) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    grant(&pool, membership, scope, member, "auth-roster-revoke", MemberAuthority::OPERATOR)
        .await
        .expect("grant");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (grant)");
    assert!(roster_row(&pool, membership).await.is_some(), "row exists right after the grant");

    revoke(&pool, membership).await.expect("revoke");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (revoke)");
    assert!(
        roster_row(&pool, membership).await.is_none(),
        "the revoked membership's row is GONE, never a stale listing"
    );
}

/// The rebuild recipe stays deterministic under the table's OWN discipline
/// (checkpoint-reset-never-TRUNCATE) even with the new delete arm in the replay: a
/// granted-then-revoked membership must replay back to the SAME absent state, not resurrect the
/// row a naive "TRUNCATE lost my delete" bug would produce.
#[tokio::test]
async fn checkpoint_reset_replay_reproduces_the_revoked_absence() {
    let Some(db) = crate::common::TestDb::acquire("restaurant_roster_revoke_replay").await else { return };
    let pool = db.pool();
    let (membership, scope, member) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    grant(&pool, membership, scope, member, "auth-roster-revoke-replay", MemberAuthority::MANAGER)
        .await
        .expect("grant");
    revoke(&pool, membership).await.expect("revoke");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (grant + revoke, first drain)");
    assert!(roster_row(&pool, membership).await.is_none(), "absent right after the first drain");

    let reset = sqlx::query("UPDATE projection_checkpoint SET position = 0 WHERE projector = 'RestaurantRoster'")
        .execute(&pool)
        .await
        .expect("rewind the RestaurantRoster checkpoint");
    assert_eq!(reset.rows_affected(), 1, "the projector name must match the registered 'RestaurantRoster' group");

    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (replay)");
    assert!(
        roster_row(&pool, membership).await.is_none(),
        "a full replay of grant-then-revoke reproduces the SAME absence -- deterministic, never a \
         resurrected row"
    );
}
