//! The rider identity read model (#639 part A; ADR-20260818-094500 ruling A, ADR-20260818-004646):
//! `Rider-{id}` facts → the `Rider` projector group → a `rider` row carrying the
//! `auth_ref -> rider_id` bridge. Needs a real Postgres (`DATABASE_URL`); the binary's gate refuses
//! to pass silently without one (#474).
//!
//! Nothing regresses here — there was no rider read model at all before this change — so the risk
//! being tested is WIRING and PARTIAL-UPDATE semantics, not column mapping (which is generated).

use application::ports::EventStore;
use infrastructure::{PgEventStore, ProjectionWorker};
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

    let row = sqlx::query("SELECT auth_ref, display_name, phone, status, standing FROM rider WHERE rider_id = $1")
        .bind(rider_id)
        .fetch_one(&pool)
        .await
        .expect("the rider row exists after one drain");
    let folded = (
        row.get::<String, _>("auth_ref"),
        row.get::<String, _>("display_name"),
        row.get::<String, _>("phone"),
        row.get::<String, _>("status"),
        row.get::<String, _>("standing"),
    );
    assert_eq!(
        folded,
        (
            "auth-supabase-9".to_string(),
            "Léa B.".to_string(),
            "+33611223344".to_string(),
            "AVAILABLE".to_string(),
            "ACTIVE".to_string(),
        ),
        "the partial RiderInfoUpdated must overwrite the name and LEAVE THE PHONE ALONE; the \
         creating arm's fold never writes anything but ACTIVE (#639 part C step 4-i)"
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

// ─── #639 part C step 4-i (ADR-20260904-081527 §1-3) — the standing fold ─────────────────────────

async fn registered(pool: &PgPool, rider_id: uuid::Uuid, auth_ref: &str) {
    append_event(
        pool,
        &format!("Rider-{rider_id}"),
        1,
        "RiderRegistered",
        serde_json::json!({
            "riderId": rider_id,
            "authRef": auth_ref,
            "displayName": "Léa",
            "phone": "+33611223344",
            "status": "OFFLINE"
        }),
    )
    .await;
}

async fn standing_of(pool: &PgPool, rider_id: uuid::Uuid) -> String {
    sqlx::query("SELECT standing FROM rider WHERE rider_id = $1")
        .bind(rider_id)
        .fetch_one(pool)
        .await
        .expect("rider row")
        .get::<String, _>("standing")
}

/// (4) A restriction fact flips `standing` to RESTRICTED; a reinstatement fact flips it back.
#[tokio::test]
async fn a_restricted_fact_flips_standing_and_a_reinstated_fact_flips_it_back() {
    let Some(db) = crate::common::TestDb::acquire("rider_standing_restrict_reinstate").await else { return };
    let pool = db.pool();
    let rider_id = uuid::Uuid::new_v4();
    registered(&pool, rider_id, "auth-supabase-restrict-1").await;
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        2,
        "RiderRestricted",
        serde_json::json!({
            "riderId": rider_id,
            "ground": "IDENTITY_MISMATCH",
            "decidedAt": "2026-01-06T12:00:00Z",
            "effectiveAt": "2026-01-06T12:00:00Z"
        }),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (restricted)");
    assert_eq!(standing_of(&pool, rider_id).await, "RESTRICTED");

    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        3,
        "RiderReinstated",
        serde_json::json!({ "riderId": rider_id }),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (reinstated)");
    assert_eq!(standing_of(&pool, rider_id).await, "ACTIVE");
}

/// (5) A legacy `RiderStatusChanged { SUSPENDED }` alone never restricts — the fold keys on the
/// FACT (RiderRestricted), never on `status` (ADR-20260904-014136 §5).
#[tokio::test]
async fn a_legacy_suspended_status_does_not_restrict_and_the_fact_does() {
    let Some(db) = crate::common::TestDb::acquire("rider_standing_legacy_suspended").await else { return };
    let pool = db.pool();
    let rider_id = uuid::Uuid::new_v4();
    registered(&pool, rider_id, "auth-supabase-legacy-1").await;
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        2,
        "RiderStatusChanged",
        serde_json::json!({ "riderId": rider_id, "status": "SUSPENDED" }),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (legacy SUSPENDED)");
    assert_eq!(
        standing_of(&pool, rider_id).await,
        "ACTIVE",
        "a legacy SUSPENDED status must never be read as a grant"
    );

    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        3,
        "RiderRestricted",
        serde_json::json!({
            "riderId": rider_id,
            "ground": "ACCOUNT_COMPROMISE",
            "decidedAt": "2026-01-06T12:00:00Z",
            "effectiveAt": "2026-01-06T12:00:00Z"
        }),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (the fact)");
    assert_eq!(standing_of(&pool, rider_id).await, "RESTRICTED", "the FACT restricts");
}

/// (6) dba's mutant: a checkpoint-reset replay-in-place over an EXISTING restricted row must never
/// re-grant it. Restrict, then re-run the creation event (`RiderRegistered`) through the SAME
/// projector without resetting the row — the replay-in-place shape a checkpoint reset produces
/// (the row survives; only the checkpoint rewinds).
#[tokio::test]
async fn replaying_rider_registered_over_a_restricted_row_keeps_it_restricted() {
    let Some(db) = crate::common::TestDb::acquire("rider_standing_replay_registered").await else { return };
    let pool = db.pool();
    let rider_id = uuid::Uuid::new_v4();
    registered(&pool, rider_id, "auth-supabase-replay-1").await;
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        2,
        "RiderRestricted",
        serde_json::json!({
            "riderId": rider_id,
            "ground": "IDENTITY_MISMATCH",
            "decidedAt": "2026-01-06T12:00:00Z",
            "effectiveAt": "2026-01-06T12:00:00Z"
        }),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (restricted)");
    assert_eq!(standing_of(&pool, rider_id).await, "RESTRICTED");

    // The replay-in-place: rewind the Rider checkpoint to 0 (never TRUNCATE — the row stays) and
    // drain again. `with_batch_size(1)` is load-bearing, not incidental: a single unbounded
    // `run_once()` would fold BOTH RiderRegistered and RiderRestricted back-to-back in the SAME
    // batch transaction, so even a creating arm that WROTE `standing` would be immediately
    // corrected by the restriction fact that follows it in the same drain — the mutant this test
    // exists to catch would be invisible. Batch size 1 forces two SEPARATE batches so the
    // intermediate state (creation replayed, restriction not yet re-applied) is observable
    // in between — exactly the live-read window the ADR is protecting.
    sqlx::query("UPDATE projection_checkpoint SET position = 0 WHERE projector = 'Rider'")
        .execute(&pool)
        .await
        .expect("rewind the Rider checkpoint");
    let worker = ProjectionWorker::new(pool.clone()).with_batch_size(1);
    worker.run_once().await.expect("run_once (replay, batch 1: RiderRegistered only)");
    assert_eq!(
        standing_of(&pool, rider_id).await,
        "RESTRICTED",
        "RiderRegistered replaying ALONE, before RiderRestricted catches up, must never re-grant \
         the rider — the creating arm's own write must never move `standing`"
    );
    worker.run_once().await.expect("run_once (replay, batch 2: RiderRestricted)");
    assert_eq!(
        standing_of(&pool, rider_id).await,
        "RESTRICTED",
        "a from-zero replay by checkpoint reset must re-grant nobody"
    );
}

/// (7) An unknown ground folds to RESTRICTED (the grant test is unaffected by an unrecognised
/// value) and the stream still LOADS — the catch-all's whole point: strict decoding would fail
/// `EventStore::load("Rider-{id}")` and block `ReinstateRider` forever.
#[tokio::test]
async fn an_unknown_ground_folds_to_restricted_and_the_stream_still_loads() {
    let Some(db) = crate::common::TestDb::acquire("rider_standing_unknown_ground").await else { return };
    let pool = db.pool();
    let rider_id = uuid::Uuid::new_v4();
    registered(&pool, rider_id, "auth-supabase-unknown-ground-1").await;
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        2,
        "RiderRestricted",
        serde_json::json!({
            "riderId": rider_id,
            "ground": "COUNSEL_ADDED_THIS_LATER",
            "decidedAt": "2026-01-06T12:00:00Z",
            "effectiveAt": "2026-01-06T12:00:00Z"
        }),
    )
    .await;
    let advanced = ProjectionWorker::new(pool.clone()).run_once().await;
    assert!(advanced.is_ok(), "run_once must advance past an unrecognised ground: {advanced:?}");
    assert_eq!(standing_of(&pool, rider_id).await, "RESTRICTED");

    let store = PgEventStore::new(pool.clone());
    let (events, version) = store.load(&format!("Rider-{rider_id}")).await.expect("the stream still loads");
    assert_eq!(version, 2, "both facts are on the stream");
    assert_eq!(events.len(), 2);

    let row = sqlx::query("SELECT ground FROM rider_restriction WHERE rider_id = $1")
        .bind(rider_id)
        .fetch_one(&pool)
        .await
        .expect("the RiderRestriction row decodes");
    // `EnumText::from_text` tolerates the unknown value into `UNRECOGNISED`, never a decode
    // failure — the raw text stays in the immutable domain_events.payload for counsel.
    assert_eq!(row.get::<Option<String>, _>("ground"), Some("UNRECOGNISED".to_string()));
    // NOTE: the `bam` projector (business_metrics.yaml's own `RiderRestriction` fold) has no
    // runtime yet (business_metrics.yaml's GENERATION STATUS note, #484) — the card's "the bam
    // fold both decode" clause is not exercisable until that machinery exists; recorded as a
    // card/precondition gap in the hand-back rather than silently dropped.
}

/// (8) The peak test: a `rider` row that predates this migration (no `standing` value was ever
/// written by an app upsert) reads ACTIVE from the column's own SQL `DEFAULT 'ACTIVE'` — the
/// backfill grants the fleet, never denies it.
#[tokio::test]
async fn a_rider_row_that_predates_the_migration_reads_active() {
    let Some(db) = crate::common::TestDb::acquire("rider_standing_predates_migration").await else { return };
    let pool = db.pool();
    let rider_id = uuid::Uuid::new_v4();
    // A row inserted the way a PRE-migration app upsert would have (no `standing` column named at
    // all) — the DEFAULT is what a real ALTER TABLE ... ADD COLUMN ... DEFAULT backfills onto it.
    sqlx::query(
        "INSERT INTO rider (rider_id, auth_ref, display_name, phone, status, created_at, updated_at) \
         VALUES ($1, 'auth-supabase-predates-1', 'Léa', '+33611223344', 'OFFLINE', now(), now())",
    )
    .bind(rider_id)
    .execute(&pool)
    .await
    .expect("insert a pre-migration-shaped row");
    assert_eq!(
        standing_of(&pool, rider_id).await,
        "ACTIVE",
        "the fleet is granted, not denied, by the migration's own DEFAULT"
    );
}
