//! The write-side arbiter of "one login credential, one rider" on REAL Postgres (#639 part C step
//! 2a, #794): two CONCURRENT `reserve` calls for one credential and two rider ids, exactly one
//! `true`. Postgres decides, via `INSERT … ON CONFLICT (principal_kind, auth_subject) DO NOTHING`.
//!
//! This must hit a real database. The in-memory fake in `application::behaviour_support` would pass
//! this assertion for a read-then-write implementation too (its mutex serialises the calls), so it
//! cannot distinguish the arbitration the table exists to provide from the race it exists to close.
//!
//! Needs `DATABASE_URL`: since #474 a missing database FAILS this suite; only an explicit
//! `DB_TESTS_REQUIRED=0` skips it, and that leaves a receipt (`crates/db_test_gate`).

use std::sync::Arc;

use application::queries::{AuthSubjectReservationRepository, BoundPrincipal};
use domain::generated::scalars::{AuthSubject, RiderId};
use infrastructure::PgAuthSubjectReservationRepository;
use sqlx::Row;

/// Two riders race for one login; one wins, the loser stays refused on every retry, the winner's
/// replay is idempotent, and the same login under ANOTHER principal kind is a different row — the
/// key is the pair. The port has one arm today (`BoundPrincipal::Rider`), so the last property is
/// asserted at the table, where the constraint lives.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_concurrent_claims_of_one_login_bind_exactly_one_rider() {
    let Some(db) = crate::common::TestDb::acquire("auth_subject_reservation").await else { return };
    let pool = db.pool();
    let repo = Arc::new(PgAuthSubjectReservationRepository::new(pool.clone()));

    let subject = AuthSubject(format!("sub-{}", uuid::Uuid::new_v4()));
    let a = RiderId(uuid::Uuid::new_v4());
    let b = RiderId(uuid::Uuid::new_v4());

    let claim_a = tokio::spawn({
        let repo = repo.clone();
        let subject = subject.clone();
        async move { repo.reserve(subject, BoundPrincipal::Rider(a)).await }
    });
    let claim_b = tokio::spawn({
        let repo = repo.clone();
        let subject = subject.clone();
        async move { repo.reserve(subject, BoundPrincipal::Rider(b)).await }
    });
    let won_a = claim_a.await.expect("join a").expect("reserve a");
    let won_b = claim_b.await.expect("join b").expect("reserve b");
    assert!(
        won_a != won_b,
        "exactly one of two concurrent claims may win (a={won_a}, b={won_b})"
    );
    let (winner, loser) = if won_a { (a, b) } else { (b, a) };

    // The winner's replay is idempotent (the handler may crash between reserving and appending);
    // the loser is refused again, forever -- there is no release.
    assert!(repo.reserve(subject.clone(), BoundPrincipal::Rider(winner)).await.expect("replay"));
    assert!(!repo.reserve(subject.clone(), BoundPrincipal::Rider(loser)).await.expect("retry"));

    let rows = sqlx::query(
        "SELECT principal_kind, principal_id FROM auth_subject_reservations WHERE auth_subject = $1",
    )
    .bind(&subject.0)
    .fetch_all(&pool)
    .await
    .expect("read reservations");
    assert_eq!(rows.len(), 1, "one binding per (kind, credential)");
    assert_eq!(rows[0].get::<String, _>("principal_kind"), "RIDER");
    assert_eq!(rows[0].get::<uuid::Uuid, _>("principal_id"), winner.0);

    // THE KEY IS THE PAIR: the same credential under CUSTOMER is a different row, so a rider who is
    // also a customer keeps both bindings. Asserted at the table because the port's only arm is
    // Rider today.
    let customer_binding = sqlx::query(
        "INSERT INTO auth_subject_reservations \
           (principal_kind, auth_subject, principal_id, reserved_at) \
         VALUES ('CUSTOMER', $1, $2, now()) \
         ON CONFLICT (principal_kind, auth_subject) DO NOTHING",
    )
    .bind(&subject.0)
    .bind(uuid::Uuid::new_v4())
    .execute(&pool)
    .await
    .expect("customer binding")
    .rows_affected();
    assert_eq!(customer_binding, 1, "a subject-only key would have refused this");
}
