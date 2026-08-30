//! The rider identity read model (#639 part A; ADR-20260818-094500 ruling A, ADR-20260818-004646):
//! `Rider-{id}` facts → the `Rider` projector group → a `rider` row carrying the
//! `auth_ref -> rider_id` bridge. Needs a real Postgres (`DATABASE_URL`); the binary's gate refuses
//! to pass silently without one (#474).
//!
//! Nothing regresses here — there was no rider read model at all before this change — so the risk
//! being tested is WIRING and PARTIAL-UPDATE semantics, not column mapping (which is generated).

use infrastructure::ProjectionWorker;
use sqlx::{PgPool, Row};

async fn append_event(
    pool: &PgPool,
    stream_name: &str,
    version: i32,
    event_type: &str,
    payload: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO domain_events \
         (id, stream_name, version, user_id, user_type, correlation_id, cause_id, event_type, payload, metadata, occurred_at) \
         VALUES ($1, $2, $3, $4, 5, $5, NULL, $6, $7, NULL, now())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(stream_name)
    .bind(version)
    .bind(uuid::Uuid::nil())
    .bind(uuid::Uuid::new_v4())
    .bind(event_type)
    .bind(payload)
    .execute(pool)
    .await
    .expect("append event");
}

/// The fold, asserted as ONE tuple, and the shape of that assertion is load-bearing rather than
/// stylistic.
///
/// `RiderInfoUpdated` carries `displayName` but NOT `phone`, and `RiderStatusChanged` carries
/// neither: a projector that rebuilt the row from the latest event — or a spec that declared
/// `display_name`/`phone` nullable, which makes the emitter write a BLIND assignment instead of an
/// `if let Some(v)` guard — leaves `phone` empty and still passes any test that checks `status`
/// alone. Checking all five together is what catches it.
///
/// The three events are appended BEFORE the worker's first run on purpose (beck): a group with no
/// `projection_checkpoint` row starts at position 0, so this also pins the free backfill the
/// registry comment claims — the property that breaks silently if anyone seeds a checkpoint row in
/// the migration.
#[tokio::test]
async fn the_rider_fold_survives_a_partial_update_and_backfills_from_position_zero() {
    let Some(db) = crate::common::TestDb::acquire("rider_projection").await else { return };
    let pool = db.pool();

    let rider_id = uuid::Uuid::new_v4();
    let stream = format!("Rider-{rider_id}");
    // Fixtures verbatim from specs/tests.yaml.
    append_event(
        &pool,
        &stream,
        1,
        "RiderRegistered",
        serde_json::json!({
            "riderId": rider_id,
            "authRef": "auth-supabase-9",
            "displayName": "Léa",
            "phone": "+33611223344",
            "status": "OFFLINE"
        }),
    )
    .await;
    // A PARTIAL update: a new name, no phone.
    append_event(
        &pool,
        &stream,
        2,
        "RiderInfoUpdated",
        serde_json::json!({ "riderId": rider_id, "displayName": "Léa B." }),
    )
    .await;
    append_event(
        &pool,
        &stream,
        3,
        "RiderStatusChanged",
        serde_json::json!({ "riderId": rider_id, "status": "AVAILABLE" }),
    )
    .await;

    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once");

    let row = sqlx::query("SELECT auth_ref, display_name, phone, status FROM rider WHERE rider_id = $1")
        .bind(rider_id)
        .fetch_one(&pool)
        .await
        .expect("the rider row exists after one drain");
    let folded = (
        row.get::<String, _>("auth_ref"),
        row.get::<String, _>("display_name"),
        row.get::<String, _>("phone"),
        row.get::<String, _>("status"),
    );
    assert_eq!(
        folded,
        (
            "auth-supabase-9".to_string(),
            "Léa B.".to_string(),
            "+33611223344".to_string(),
            "AVAILABLE".to_string()
        ),
        "the partial RiderInfoUpdated must overwrite the name and LEAVE THE PHONE ALONE"
    );

    // The half that is the point of the whole chunk: the bridge resolves, and only for its own
    // subject. Asserted as a pair — a nullable indexed column nothing can be queried BY is a
    // column, not a read model.
    let resolve = |auth_ref: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query("SELECT rider_id FROM rider WHERE auth_ref = $1")
                .bind(auth_ref)
                .fetch_optional(&pool)
                .await
                .expect("lookup by auth_ref")
                .map(|r| r.get::<uuid::Uuid, _>("rider_id"))
        }
    };
    assert_eq!(resolve("auth-supabase-9").await, Some(rider_id), "the auth subject resolves");
    assert_eq!(resolve("auth-supabase-stranger").await, None, "and a stranger's does not");
}

/// `auth_ref` is UNIQUE, not merely indexed, and the difference is a security property: the
/// repository lookup is `fetch_optional`, which on multiplicity returns an ARBITRARY row rather
/// than an error, and `ScopeMembership` keys its grants on `member_id = rider_id` — so two riders
/// sharing an auth subject would hand one of them the other's order scope, silently and
/// plan-dependently. The constraint turns that into a visible failure.
///
/// Asserted against the DATABASE rather than against the DDL text: a spec that dropped `unique:`
/// would still emit a column and still pass a schema-shape test.
#[tokio::test]
async fn two_riders_cannot_share_one_auth_subject() {
    let Some(db) = crate::common::TestDb::acquire("rider_auth_ref_unique").await else { return };
    let pool = db.pool();

    let insert = |rider: uuid::Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO rider (rider_id, auth_ref, display_name, phone, status, created_at, updated_at) \
                 VALUES ($1, 'auth-supabase-shared', 'Léa', '+33611223344', 'OFFLINE', now(), now())",
            )
            .bind(rider)
            .execute(&pool)
            .await
        }
    };
    insert(uuid::Uuid::new_v4()).await.expect("the first rider takes the subject");
    let second = insert(uuid::Uuid::new_v4()).await;
    assert!(
        second.is_err(),
        "a second rider on the same auth subject must be REFUSED by the database — without the \
         constraint the bridge silently resolves to whichever row the planner returns first"
    );
}
