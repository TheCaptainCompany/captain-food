//! `RestaurantMembership` / `Member` / `ScopeMembership` MEMBER-arm integration tests, on REAL
//! Postgres (#639 part C step 6-i, ADR-20260905-101349 §4/§5/§11). Needs `DATABASE_URL`; SKIPS
//! offline (a missing database FAILS this suite unless `DB_TESTS_REQUIRED=0`, #474).

use application::commands::{grant_restaurant_access, revoke_restaurant_access};
use application::ports::Actor;
use application::queries::{AuthSubjectReservationRepository, BoundPrincipal};
use domain::generated::commands::{GrantRestaurantAccess, RevokeRestaurantAccess};
use domain::generated::scalars::{
    AccessBasis, AccessRevocationGround, AuthSubject, MemberAuthority, MemberId, MembershipId,
    RestaurantId, ScopeType,
};
use infrastructure::persistence::member_store;
use infrastructure::{PgAuthSubjectReservationRepository, PgEventStore, ProjectionWorker};
use sqlx::{PgPool, Row};
use std::sync::Arc;

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
            authority: MemberAuthority::MANAGER,
            basis: AccessBasis::CAPTAIN_ONBOARDING,
        },
        &actor(),
        true, // RUN_MEMBER_ACCESS_GRANT ON -- these tests exercise the aggregate, not the door.
    )
    .await
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
            ground: AccessRevocationGround::LEFT_THE_RESTAURANT,
        },
        &actor(),
    )
    .await
}

// ─── B: the reservation refutations (§4, the auth_subject_reservation.rs precedent) ─────────────

/// Two members race for one login; Postgres decides, exactly one wins. The in-memory fake's mutex
/// would pass this for a read-then-write implementation too — this must hit a real database.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_concurrent_grants_for_one_subject_exactly_one_wins() {
    let Some(db) = crate::common::TestDb::acquire("member_reservation_race").await else { return };
    let pool = db.pool();
    let repo = Arc::new(PgAuthSubjectReservationRepository::new(pool.clone()));

    let subject = AuthSubject(format!("sub-{}", uuid::Uuid::new_v4()));
    let a = MemberId(uuid::Uuid::new_v4());
    let b = MemberId(uuid::Uuid::new_v4());

    let claim_a = tokio::spawn({
        let repo = repo.clone();
        let subject = subject.clone();
        async move { repo.reserve(subject, BoundPrincipal::Member(a)).await }
    });
    let claim_b = tokio::spawn({
        let repo = repo.clone();
        let subject = subject.clone();
        async move { repo.reserve(subject, BoundPrincipal::Member(b)).await }
    });
    let won_a = claim_a.await.expect("join a").expect("reserve a");
    let won_b = claim_b.await.expect("join b").expect("reserve b");
    assert!(won_a != won_b, "exactly one of two concurrent claims may win (a={won_a}, b={won_b})");
}

/// M1's refutation: the SAME subject held as RIDER is a SEPARATE row from MEMBER — the key is the
/// PAIR `(principal_kind, auth_subject)`, never the subject alone. Mutating `reserve`'s lookup to
/// key on `auth_subject` alone makes this test red (the MEMBER claim would see the RIDER row as
/// already holding the subject and refuse).
#[tokio::test]
async fn the_same_subject_reserved_as_rider_is_a_separate_row_from_member() {
    let Some(db) = crate::common::TestDb::acquire("member_reservation_kinds").await else { return };
    let pool = db.pool();
    let repo = PgAuthSubjectReservationRepository::new(pool.clone());

    let subject = AuthSubject(format!("sub-{}", uuid::Uuid::new_v4()));
    let rider = domain::generated::scalars::RiderId(uuid::Uuid::new_v4());
    let member = MemberId(uuid::Uuid::new_v4());

    assert!(
        repo.reserve(subject.clone(), BoundPrincipal::Rider(rider)).await.expect("reserve rider"),
        "the rider binding must succeed first"
    );
    assert!(
        repo.reserve(subject.clone(), BoundPrincipal::Member(member)).await.expect("reserve member"),
        "a person who is ALSO a restaurant member must hold BOTH bindings on one credential -- a \
         subject-only key would refuse this and permanently bar a rider from ever becoming staff"
    );

    let rows = sqlx::query(
        "SELECT principal_kind FROM auth_subject_reservations WHERE auth_subject = $1 ORDER BY principal_kind",
    )
    .bind(&subject.0)
    .fetch_all(&pool)
    .await
    .expect("read reservations");
    assert_eq!(rows.len(), 2, "two independent rows, one per principal kind");
    assert_eq!(rows[0].get::<String, _>("principal_kind"), "MEMBER");
    assert_eq!(rows[1].get::<String, _>("principal_kind"), "RIDER");
}

/// M2's refutation, driven through the REAL handlers end to end: `revoke_restaurant_access` never
/// releases the `(MEMBER, authSubject)` binding, so granting a SECOND, DIFFERENT member on the
/// SAME login after a revoke is still refused — the lifetime binding (PROP §7). Mutating
/// `revoke_restaurant_access` to release the reservation on revoke makes this test red.
#[tokio::test]
async fn after_revoke_reserving_the_subject_for_another_member_is_still_refused() {
    let Some(db) = crate::common::TestDb::acquire("member_reservation_no_release").await else { return };
    let pool = db.pool();

    let subject = format!("sub-{}", uuid::Uuid::new_v4());
    let membership_a = uuid::Uuid::new_v4();
    let member_a = uuid::Uuid::new_v4();
    let scope = uuid::Uuid::new_v4();

    grant(&pool, membership_a, scope, member_a, &subject).await.expect("grant A");
    revoke(&pool, membership_a).await.expect("revoke A");

    // A DIFFERENT member, SAME login, a fresh membershipId (so idempotency never masks this) --
    // must still be refused: the binding is a lifetime identifier binding, never released.
    let membership_b = uuid::Uuid::new_v4();
    let member_b = uuid::Uuid::new_v4();
    let err = grant(&pool, membership_b, scope, member_b, &subject).await.expect_err(
        "a revoked member's login must stay bound to them forever -- granting a second member on \
         it must be refused, never silently re-bound",
    );
    match err {
        domain::shared::errors::DomainError::Rejected { code, .. } => {
            assert_eq!(code, "MemberAuthSubjectAlreadyBound");
        }
        other => panic!("expected MemberAuthSubjectAlreadyBound, got {other:?}"),
    }
}

// ─── D: the targeted revoke, never the broad role-wide delete ───────────────────────────────────

/// Two members hold access to ONE restaurant scope; revoking one must leave the OTHER's row
/// intact -- never the broad `revoke_role` arm, which would strip BOTH (every MEMBER row on that
/// scope). M3's refutation.
#[tokio::test]
async fn revoking_one_member_leaves_the_others_access_intact() {
    let Some(db) = crate::common::TestDb::acquire("member_targeted_revoke").await else { return };
    let pool = db.pool();

    let scope = uuid::Uuid::new_v4();
    let (membership_a, member_a) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let (membership_b, member_b) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    grant(&pool, membership_a, scope, member_a, &format!("auth-{membership_a}")).await.expect("grant A");
    grant(&pool, membership_b, scope, member_b, &format!("auth-{membership_b}")).await.expect("grant B");

    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (two grants)");

    let is_member_row = |member_id: uuid::Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM scopemembership WHERE scope_type = 'RESTAURANT' AND scope_id = $1 AND member_type = 'MEMBER' AND member_id = $2",
            )
            .bind(scope)
            .bind(member_id)
            .fetch_one(&pool)
            .await
            .expect("count")
        }
    };
    assert_eq!(is_member_row(member_a).await, 1, "member A granted");
    assert_eq!(is_member_row(member_b).await, 1, "member B granted");

    revoke(&pool, membership_a).await.expect("revoke A");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (revoke A)");

    assert_eq!(is_member_row(member_a).await, 0, "member A's row is gone");
    assert_eq!(is_member_row(member_b).await, 1, "member B's row survives the TARGETED revoke");
}

// ─── E: the rebuild recipes, as executable tests (§5) ────────────────────────────────────────────

async fn member_auth_subject(pool: &PgPool, member_id: uuid::Uuid) -> Option<String> {
    member_store::load(pool, MemberId(member_id)).await.expect("load").map(|r| r.auth_subject.0)
}

/// (1) `Member`: checkpoint reset (NEVER TRUNCATE) never denies. The row is never deleted by a
/// reset alone, so it resolves at every point of the drain -- including the instant immediately
/// after the reset, before any replay has run at all. M4's refutation lives in the SIBLING test
/// below (swap this recipe for TRUNCATE and watch the very same assertion go red).
#[tokio::test]
async fn member_resolves_at_every_point_of_a_checkpoint_reset_replay() {
    let Some(db) = crate::common::TestDb::acquire("member_checkpoint_reset").await else { return };
    let pool = db.pool();

    let (membership, member) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    grant(&pool, membership, uuid::Uuid::new_v4(), member, "auth-reset-1").await.expect("grant");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (first drain)");
    assert!(member_auth_subject(&pool, member).await.is_some(), "member resolves after the first drain");

    // Checkpoint reset, never TRUNCATE: rewind the Member checkpoint to 0.
    let reset = sqlx::query("UPDATE projection_checkpoint SET position = 0 WHERE projector = 'Member'")
        .execute(&pool)
        .await
        .expect("rewind the Member checkpoint");
    assert_eq!(
        reset.rows_affected(),
        1,
        "the projector name must match the registered 'Member' group -- a rename here would pass \
         vacuously with 0 rows touched (round-2 beck finding, R2-4)"
    );
    // The claim, checked at its strongest point: RIGHT AFTER the reset, BEFORE the replay runs.
    assert!(
        member_auth_subject(&pool, member).await.is_some(),
        "a checkpoint reset alone must never deny -- the row was never touched"
    );

    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (replay)");
    assert_eq!(
        member_auth_subject(&pool, member).await.as_deref(),
        Some("auth-reset-1"),
        "the replay reproduces the same row"
    );
}

/// (2) TRUNCATE + reset creates a REAL denial window: prove it, do not comment it
/// (ADR-20260904-014136's shape). Right after the TRUNCATE, before the replay completes, the
/// member is GONE -- the opposite of the checkpoint-reset-only recipe above.
#[tokio::test]
async fn truncate_then_reset_opens_a_real_denial_window() {
    let Some(db) = crate::common::TestDb::acquire("member_truncate_denial").await else { return };
    let pool = db.pool();

    let (membership, member) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    grant(&pool, membership, uuid::Uuid::new_v4(), member, "auth-truncate-1").await.expect("grant");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (first drain)");
    assert!(member_auth_subject(&pool, member).await.is_some());

    sqlx::query("TRUNCATE member").execute(&pool).await.expect("truncate member");
    sqlx::query("DELETE FROM projection_checkpoint WHERE projector = 'Member'")
        .execute(&pool)
        .await
        .expect("reset the Member checkpoint to 0 (no row = starts at 0)");

    // THE DENIAL WINDOW: right after TRUNCATE, before the replay has run, the member is DENIED.
    assert_eq!(
        member_auth_subject(&pool, member).await,
        None,
        "TRUNCATE opens a real window in which a member who exists resolves to nobody -- this is \
         exactly why the table's own `rules:` forbid it"
    );

    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (replay heals it)");
    assert_eq!(
        member_auth_subject(&pool, member).await.as_deref(),
        Some("auth-truncate-1"),
        "the replay eventually heals the row, but the window above is the point"
    );
}

/// (3) `ScopeMembership`: DELETE + checkpoint reset + full replay reproduces the MEMBER grant
/// rows; the reservation table (a NON-replayed, domain-owned write) is diffed and must be
/// byte-identical before and after.
#[tokio::test]
async fn scope_membership_delete_and_full_replay_reproduces_grants_reservation_unchanged() {
    let Some(db) = crate::common::TestDb::acquire("scope_membership_member_rebuild").await else { return };
    let pool = db.pool();

    let (membership_a, member_a, scope_a) =
        (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let (membership_b, member_b, scope_b) =
        (uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    grant(&pool, membership_a, scope_a, member_a, "auth-rebuild-a").await.expect("grant A");
    grant(&pool, membership_b, scope_b, member_b, "auth-rebuild-b").await.expect("grant B");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (two grants)");

    let member_rows = |pool: PgPool| async move {
        sqlx::query(
            "SELECT membership_id, scope_id, member_id FROM scopemembership \
             WHERE member_type = 'MEMBER' ORDER BY membership_id",
        )
        .fetch_all(&pool)
        .await
        .expect("read member rows")
        .into_iter()
        .map(|r| (r.get::<uuid::Uuid, _>("membership_id"), r.get::<uuid::Uuid, _>("scope_id"), r.get::<uuid::Uuid, _>("member_id")))
        .collect::<Vec<_>>()
    };
    let before = member_rows(pool.clone()).await;
    assert_eq!(before.len(), 2, "both grants materialized before the rebuild");

    let reservations_before: Vec<(String, String, uuid::Uuid)> = sqlx::query(
        "SELECT principal_kind, auth_subject, principal_id FROM auth_subject_reservations \
         WHERE principal_kind = 'MEMBER' ORDER BY auth_subject",
    )
    .fetch_all(&pool)
    .await
    .expect("read reservations before")
    .into_iter()
    .map(|r| (r.get(0), r.get(1), r.get(2)))
    .collect();
    assert_eq!(reservations_before.len(), 2, "both reservations exist before the rebuild");

    // The ScopeMembership recipe (PROP §6.4): DELETE + checkpoint reset + full replay.
    sqlx::query("DELETE FROM scopemembership").execute(&pool).await.expect("delete scopemembership");
    sqlx::query("UPDATE projection_checkpoint SET position = 0 WHERE projector = 'ScopeMembership'")
        .execute(&pool)
        .await
        .expect("rewind the ScopeMembership checkpoint");
    let start = std::time::Instant::now();
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (full replay)");
    let elapsed = start.elapsed();
    eprintln!(
        "scope_membership_delete_and_full_replay_reproduces_grants_reservation_unchanged: \
         from-zero replay over 2 seeded RestaurantAccessGranted rows (plus whatever the shared \
         schema/migration chain appended ahead of them) took {elapsed:?}"
    );

    let after = member_rows(pool.clone()).await;
    assert_eq!(before, after, "the full replay reproduces exactly the same MEMBER grant rows");

    let reservations_after: Vec<(String, String, uuid::Uuid)> = sqlx::query(
        "SELECT principal_kind, auth_subject, principal_id FROM auth_subject_reservations \
         WHERE principal_kind = 'MEMBER' ORDER BY auth_subject",
    )
    .fetch_all(&pool)
    .await
    .expect("read reservations after")
    .into_iter()
    .map(|r| (r.get(0), r.get(1), r.get(2)))
    .collect();
    assert_eq!(
        reservations_before, reservations_after,
        "the reservation table is NOT a projection -- it must be byte-identical, never replayed"
    );
}

/// M5's refutation: the CREATING arm must never write a grant-shaped column onto `Member` (the
/// table's own `rules:`) -- pinned as a schema assertion, since `member` carries only
/// `member_id`/`auth_subject`/`created_at`/`updated_at` today.
#[tokio::test]
async fn member_carries_no_grant_shaped_column() {
    let Some(db) = crate::common::TestDb::acquire("member_no_grant_shaped_column").await else { return };
    let pool = db.pool();
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns WHERE table_name = 'member' ORDER BY column_name",
    )
    .fetch_all(&pool)
    .await
    .expect("read member columns");
    assert_eq!(
        columns,
        vec!["auth_subject", "created_at", "member_id", "updated_at"],
        "a grant-shaped column (authority/basis/scopeId/...) here would break the checkpoint-reset \
         rebuild rule -- the table's own `rules:` holds only while this stays true"
    );
}

/// Round-2 dba finding (R2-8): a SECOND grant for the same `member_id`, under a FRESH
/// `membershipId` and a DIFFERENT `authSubject`, must never rebind the bridge -- "the binding
/// OUTLIVES any one grant" (the table's own `rules:`). Before the fix, `member_store::upsert`'s
/// `ON CONFLICT (member_id) DO UPDATE SET auth_subject = EXCLUDED.auth_subject` passed every other
/// belt (the idempotency key is `membershipId`; the reservation keys on the fresh subject) and
/// silently orphaned the first credential. First-write-wins: `Member.auth_subject` stays the FIRST
/// subject ever folded for that member.
#[tokio::test]
async fn a_second_grant_for_the_same_member_never_rebinds_the_auth_subject() {
    let Some(db) = crate::common::TestDb::acquire("member_first_write_wins").await else { return };
    let pool = db.pool();

    let member = uuid::Uuid::new_v4();
    let (m1, m2) = (uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    grant(&pool, m1, uuid::Uuid::new_v4(), member, "auth-first-subject").await.expect("grant 1");
    grant(&pool, m2, uuid::Uuid::new_v4(), member, "auth-second-subject").await.expect("grant 2");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once");

    assert_eq!(
        member_auth_subject(&pool, member).await.as_deref(),
        Some("auth-first-subject"),
        "a second grant with a new membershipId and a different authSubject must never rebind an \
         already-bound member's Member row -- first-write-wins, replay-deterministic"
    );
}

/// Round-2 beck finding (R2-5): `TestGrantRestaurantAccessDoorClosed` (the generated behaviour
/// test) only proves `assert_appended(&[])` -- moving the door check BELOW `auth_subjects.reserve`
/// would still pass that test while a login got bound to a never-granted member. The generated
/// harness's `SpecAuthSubjectReservations.held` map is private to `behaviour_support.rs` with no
/// accessor, so the harness cannot express this; asserted here instead, against the real table.
#[tokio::test]
async fn door_closed_never_touches_the_auth_subject_reservation() {
    let Some(db) = crate::common::TestDb::acquire("member_door_closed_no_reservation").await else { return };
    let pool = db.pool();
    let store = PgEventStore::new(pool.clone());
    let auth_subjects = PgAuthSubjectReservationRepository::new(pool.clone());
    let subject = format!("auth-door-closed-{}", uuid::Uuid::new_v4());
    let result = grant_restaurant_access(
        &store,
        &auth_subjects,
        GrantRestaurantAccess {
            membership_id: MembershipId(uuid::Uuid::new_v4()),
            scope_type: ScopeType::RESTAURANT,
            scope_id: RestaurantId(uuid::Uuid::new_v4()),
            member_id: MemberId(uuid::Uuid::new_v4()),
            auth_subject: AuthSubject(subject.clone()),
            authority: MemberAuthority::MANAGER,
            basis: AccessBasis::CAPTAIN_ONBOARDING,
        },
        &actor(),
        false, // RUN_MEMBER_ACCESS_GRANT OFF
    )
    .await;
    assert!(result.is_err(), "the door-closed call must be refused");

    let held: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM auth_subject_reservations WHERE auth_subject = $1",
    )
    .bind(&subject)
    .fetch_one(&pool)
    .await
    .expect("count reservations");
    assert_eq!(
        held, 0,
        "the door check must run BEFORE the reservation write -- a login must never get bound to a \
         never-granted member"
    );
}
