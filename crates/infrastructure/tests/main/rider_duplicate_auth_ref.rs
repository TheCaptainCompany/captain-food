//! The duplicate-`authRef` chain through the projector, pinned as a CLASSIFICATION (#639 part C
//! step 2b; `beck`, level 3 — the `projection_checkpoint_halt.rs` shape).
//!
//! Since step 2a the write side refuses a second `RegisterRider` for an already-bound login
//! (`auth_subject_reservations`, `RiderAuthSubjectBoundOnce`), so two `RiderRegistered` with the
//! same `authRef` and different `riderId`s can no longer be APPENDED — but they can still be
//! REPLAYED: any history recorded before #794 is exactly this shape, and a projector replays
//! whatever the log holds. This test therefore stays, and pins what the default `DbFaultPolicy::Skip`
//! does with that history rather than what the write side now prevents:
//!
//!   appended -> the projector's upsert hits `rider.auth_ref UNIQUE` -> the checkpoint ADVANCES ->
//!   the second rider has NO row -> and nothing but a log line says so (no counter fires).
//!
//! The door then reads the projection, so the FIRST rider keeps the login and the second is
//! `Public` forever. Deliberately NOT flipping the policy default here: that is its own recorded
//! decision (`worker.rs`, `DbFaultPolicy`, #474). What this test buys is that the day it flips,
//! this file goes red and the rider consequence is named in the diff.

use application::queries::RiderIdentityRepository;
use domain::generated::scalars::{AuthSubject, RiderId};
use infrastructure::{PgRiderRepository, ProjectionWorker};
use sqlx::{PgPool, Row};

async fn append_rider_registered(pool: &PgPool, rider_id: uuid::Uuid, auth_ref: &str) -> i64 {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, 1, $3, 5, $4, NULL, 'RiderRegistered', $5, NULL, now()) RETURNING position",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(format!("Rider-{rider_id}"))
    .bind(uuid::Uuid::nil())
    .bind(uuid::Uuid::new_v4())
    .bind(serde_json::json!({
        "riderId": rider_id,
        "authRef": auth_ref,
        "displayName": "Léa",
        "phone": "+33611223344",
        "status": "OFFLINE"
    }))
    .fetch_one(pool)
    .await
    .expect("append RiderRegistered")
    .get::<i64, _>("position")
}

async fn checkpoint(pool: &PgPool) -> Option<i64> {
    sqlx::query_scalar("SELECT position FROM projection_checkpoint WHERE projector = 'Rider'")
        .fetch_optional(pool)
        .await
        .expect("read the Rider checkpoint")
}

async fn riders_bound_to(pool: &PgPool, auth_ref: &str) -> Vec<uuid::Uuid> {
    sqlx::query_scalar("SELECT rider_id FROM rider WHERE auth_ref = $1 ORDER BY rider_id")
        .bind(auth_ref)
        .fetch_all(pool)
        .await
        .expect("read rider rows")
}

/// The classification under the DEFAULT policy: a replayed duplicate is skipped, the checkpoint
/// advances past it, the second rider never gets a row, and the door's reader answers the FIRST
/// rider — never an arbitrary one, never the second.
#[tokio::test]
async fn a_replayed_duplicate_auth_ref_is_skipped_the_checkpoint_advances_and_the_door_reads_the_first_rider() {
    let Some(db) = crate::common::TestDb::acquire("rider_duplicate_auth_ref").await else { return };
    let pool = db.pool();
    let auth_ref = "auth-supabase-dup";
    let rider_a = uuid::Uuid::new_v4();
    let rider_b = uuid::Uuid::new_v4();

    append_rider_registered(&pool, rider_a, auth_ref).await;
    let position_b = append_rider_registered(&pool, rider_b, auth_ref).await;

    ProjectionWorker::new(pool.clone())
        .run_once()
        .await
        .expect("the default policy (Skip) reports a clean drain over the duplicate");

    assert_eq!(
        checkpoint(&pool).await,
        Some(position_b),
        "Skip advances the Rider checkpoint PAST the rejected fold — the duplicate is behind it now"
    );
    assert_eq!(
        riders_bound_to(&pool, auth_ref).await,
        vec![rider_a],
        "exactly one row holds the login, and it is the FIRST registration — the second never landed"
    );

    // What the sign-in door actually reads: the projection, through the port the seam uses.
    let reader = PgRiderRepository::new(pool.clone());
    assert_eq!(
        reader.rider_id_by_auth_subject(AuthSubject(auth_ref.to_string())).await.expect("probe"),
        Some(RiderId(rider_a)),
        "the door resolves the login to the first rider; the second is Public forever (no row)"
    );
    assert_eq!(
        reader.rider_id_by_auth_subject(AuthSubject("auth-supabase-nobody".into())).await.expect("probe"),
        None,
        "and an unknown subject is nobody — NoMapping at the seam, never a guess"
    );
}
