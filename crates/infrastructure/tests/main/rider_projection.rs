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

    // Round-2 item 4 (beck, farley): the real Postgres resolver
    // (`PgRiderRepository::rider_id_by_auth_subject`) is the one untested link on this slice's own
    // read side — the walk injects `ReadScope` directly and the seam suite scripts `resolve`, so
    // neither exercises the actual `SELECT rider_id, standing` decode against a RESTRICTED row.
    let repo = infrastructure::PgRiderRepository::new(pool.clone());
    let resolved = <infrastructure::PgRiderRepository as application::queries::RiderIdentityRepository>::rider_id_by_auth_subject(
        &repo,
        domain::generated::scalars::AuthSubject("auth-supabase-restrict-1".to_string()),
    )
    .await
    .expect("rider_id_by_auth_subject (restricted)");
    assert_eq!(
        resolved,
        Some((domain::generated::scalars::RiderId(rider_id), domain::generated::scalars::RiderStanding::RESTRICTED)),
        "the real Postgres resolver must read RESTRICTED off the actual row, not just the fold's own table"
    );

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

    let resolved = <infrastructure::PgRiderRepository as application::queries::RiderIdentityRepository>::rider_id_by_auth_subject(
        &repo,
        domain::generated::scalars::AuthSubject("auth-supabase-restrict-1".to_string()),
    )
    .await
    .expect("rider_id_by_auth_subject (reinstated)");
    assert_eq!(
        resolved,
        Some((domain::generated::scalars::RiderId(rider_id), domain::generated::scalars::RiderStanding::ACTIVE)),
        "the real Postgres resolver must read ACTIVE again off the actual row after reinstatement"
    );
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

/// (6) Round 2 item 9 (farley, reviewer, beck — rewritten to what this test actually proves):
/// an end-to-end checkpoint-reset replay over an EXISTING restricted row must re-grant nobody.
/// This is NOT an arm-level proof that the creating arm's own write never moves `standing` — that
/// proof is the `RiderCompute::standing` hook's own unit test (`crates/application/src/
/// projectors/rider.rs`), pinned directly against M5 on the 4-i card. `run_once()`'s inner loop
/// drains every pending event to exhaustion regardless of `with_batch_size`, so the two calls
/// below never actually observe an intermediate state between "creation replayed" and "restriction
/// re-applied" — the first call alone already drains both events to the SAME end state the second
/// call's assertion repeats. Kept as the end-to-end guarantee (a real checkpoint-reset replay,
/// through the real router/projector/store, re-grants nobody) rather than removed, since that
/// property is real and worth its own DB-gated proof — it is simply a DIFFERENT, weaker claim than
/// the arm-level one the original comment implied.
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
    // drain again. `with_batch_size(1)` bounds each individual BATCH TRANSACTION to one event, but
    // `run_once()`'s own loop keeps calling batches until nothing is pending — so the FIRST
    // `run_once()` below already drains both RiderRegistered and RiderRestricted to the same end
    // state the second call's assertion repeats; neither call observes a genuine intermediate
    // state (round 2 item 9). This proves the end-to-end replay guarantee only — the arm-level
    // proof (the creating arm's own write never moves `standing`) is the `RiderCompute::standing`
    // hook's own unit test.
    sqlx::query("UPDATE projection_checkpoint SET position = 0 WHERE projector = 'Rider'")
        .execute(&pool)
        .await
        .expect("rewind the Rider checkpoint");
    let worker = ProjectionWorker::new(pool.clone()).with_batch_size(1);
    worker.run_once().await.expect("run_once (replay, drains to exhaustion regardless of batch_size)");
    assert_eq!(
        standing_of(&pool, rider_id).await,
        "RESTRICTED",
        "a from-zero replay by checkpoint reset must re-grant nobody"
    );
    // A second call is a no-op (nothing pending) — kept to document that this drained to
    // completion rather than stopping partway, not to observe a second state.
    worker.run_once().await.expect("run_once (replay, second call is a no-op)");
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

/// (8) Round 2 item 7 (dba, young — a migration-class defect, #639 part C step 4-i): `RiderRestriction`
/// must ride its OWN `ProjectorGroup` checkpoint, starting at 0 — never a prefix bolted onto the
/// ALREADY-ADVANCED `"Rider"` checkpoint. Reproduced by pre-seeding ONLY the shared `"Rider"`
/// checkpoint past this rider's own `RiderRegistered` position BEFORE any drain ever runs — the
/// exact starting condition a real fleet is in the moment `RiderRestriction`'s code ships onto an
/// already-running platform whose `"Rider"` checkpoint has long since passed this rider. Under the
/// bundled (pre-fix) arrangement the shared checkpoint would skip straight to the NEW
/// `RiderRestricted` fact without ever having folded the `RiderRegistered` birth into
/// `rider_restriction`, so `let mut row = state?;` drops it silently. Under the fix,
/// `RiderRestriction`'s OWN checkpoint has no row yet (starts at 0), so this ONE drain replays the
/// WHOLE `Rider-` stream for THIS group — `RiderRegistered` then `RiderRestricted`, in order — and
/// the row is created and updated in the same pass regardless of where the SHARED `"Rider"`
/// checkpoint sits.
#[tokio::test]
async fn a_rider_predating_the_restriction_group_still_gets_backfilled_by_its_own_replay() {
    let Some(db) = crate::common::TestDb::acquire("rider_restriction_own_checkpoint").await else { return };
    let pool = db.pool();
    let rider_id = uuid::Uuid::new_v4();
    registered(&pool, rider_id, "auth-supabase-own-checkpoint-1").await;

    let registered_position: i64 =
        sqlx::query_scalar("SELECT position FROM domain_events WHERE stream_name = $1 AND version = 1")
            .bind(format!("Rider-{rider_id}"))
            .fetch_one(&pool)
            .await
            .expect("the RiderRegistered position");

    // Pre-seed ONLY the SHARED `"Rider"` checkpoint past this rider's registration -- simulating a
    // fleet that already existed before `RiderRestriction` shipped. Never touch a
    // `"RiderRestriction"` row: its absence IS the fix (starts at 0 on its own first drain).
    sqlx::query(
        "INSERT INTO projection_checkpoint (projector, position, updated_at) VALUES ('Rider', $1, now())",
    )
    .bind(registered_position)
    .execute(&pool)
    .await
    .expect("pre-seed the shared Rider checkpoint");

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
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (restricted, own-checkpoint replay)");

    let ground: Option<String> = sqlx::query_scalar("SELECT ground FROM rider_restriction WHERE rider_id = $1")
        .bind(rider_id)
        .fetch_optional(&pool)
        .await
        .expect("query rider_restriction");
    assert_eq!(
        ground,
        Some("IDENTITY_MISMATCH".to_string()),
        "RiderRestriction's own checkpoint must replay this rider's birth on its first-ever drain, \
         regardless of where the shared Rider checkpoint already sits -- a row bundled under an \
         already-advanced checkpoint would be silently dropped here"
    );
}

// ─── #639 part C step 4-iii-A (ADR-20260904-152807 §1/§3) — the admin roster fold ────────────────
// Own checkpoint group `"RiderRoster"`, `derive:`-mechanical `standing` (never a hand-written
// hook, unlike `Rider`/`RiderRestriction` above — the table's own `rules:` explain why the
// simplification is safe here: this group's rebuild is TRUNCATE-together, never checkpoint-only).

async fn roster_row(pool: &PgPool, rider_id: uuid::Uuid) -> Option<(String, String, Option<String>)> {
    sqlx::query("SELECT display_name, standing, ground FROM rider_roster WHERE rider_id = $1")
        .bind(rider_id)
        .fetch_optional(pool)
        .await
        .expect("query rider_roster")
        .map(|row| {
            (
                row.get::<String, _>("display_name"),
                row.get::<String, _>("standing"),
                row.get::<Option<String>, _>("ground"),
            )
        })
}

/// (9) The row is born by `RiderRegistered` and carries NO `auth_ref` column at all — a rule line
/// in the spec, pinned here as a STRUCTURE-sensitive assertion: `SELECT auth_ref FROM rider_roster`
/// must ERROR (undefined column), never merely return NULL/empty, because this table answers WHO to
/// show an admin and never WHO a session resolves to (that stays exclusively on `Rider`).
#[tokio::test]
async fn the_roster_row_is_born_by_registration_and_has_no_auth_ref_column() {
    let Some(db) = crate::common::TestDb::acquire("rider_roster_no_auth_ref").await else { return };
    let pool = db.pool();
    let rider_id = uuid::Uuid::new_v4();
    registered(&pool, rider_id, "auth-supabase-roster-1").await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (roster birth)");

    let (display_name, standing, ground) =
        roster_row(&pool, rider_id).await.expect("the roster row exists after RiderRegistered");
    assert_eq!(display_name, "Léa");
    assert_eq!(standing, "ACTIVE");
    assert_eq!(ground, None);

    let err = sqlx::query("SELECT auth_ref FROM rider_roster WHERE rider_id = $1")
        .bind(rider_id)
        .fetch_optional(&pool)
        .await
        .expect_err("rider_roster must declare no auth_ref column at all — a structural guarantee, not a value check");
    assert!(
        format!("{err}").to_lowercase().contains("auth_ref") || format!("{err}").to_lowercase().contains("column"),
        "expected an undefined-column error naming auth_ref, got: {err}"
    );
}

/// (10) A restriction fact writes `standing` + `ground`; a reinstatement fact returns `standing` to
/// ACTIVE and leaves `ground` untouched (the Art. 11 history stays for admin/counsel — the SAME
/// `RiderRestriction` discipline, mirrored one table further).
#[tokio::test]
async fn a_restricted_fact_writes_standing_and_ground_and_reinstate_returns_active() {
    let Some(db) = crate::common::TestDb::acquire("rider_roster_restrict_reinstate").await else { return };
    let pool = db.pool();
    let rider_id = uuid::Uuid::new_v4();
    registered(&pool, rider_id, "auth-supabase-roster-2").await;
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        2,
        "RiderRestricted",
        serde_json::json!({
            "riderId": rider_id, "ground": "ELIGIBILITY_DOCUMENT_LAPSED",
            "decidedAt": "2026-01-06T12:00:00Z", "effectiveAt": "2026-01-06T12:00:00Z"
        }),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (roster restricted)");
    let (_, standing, ground) = roster_row(&pool, rider_id).await.expect("roster row");
    assert_eq!(standing, "RESTRICTED");
    assert_eq!(ground, Some("ELIGIBILITY_DOCUMENT_LAPSED".to_string()));

    append_event(&pool, &format!("Rider-{rider_id}"), 3, "RiderReinstated", serde_json::json!({ "riderId": rider_id }))
        .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (roster reinstated)");
    let (_, standing, ground) = roster_row(&pool, rider_id).await.expect("roster row");
    assert_eq!(standing, "ACTIVE");
    assert_eq!(
        ground,
        Some("ELIGIBILITY_DOCUMENT_LAPSED".to_string()),
        "the attribution stays for admin/counsel history until the NEXT RiderRestricted overwrites it"
    );
}

/// (11) A from-zero replay (TRUNCATE + checkpoint reset, this group's OWN rebuild discipline —
/// see the table's `rules:`) reproduces the byte-identical row: register, restrict, reinstate,
/// replayed from position 0, yields the SAME (display_name, standing, ground) tuple the live drain
/// already produced.
#[tokio::test]
async fn a_from_zero_replay_yields_the_same_roster_rows() {
    let Some(db) = crate::common::TestDb::acquire("rider_roster_from_zero_replay").await else { return };
    let pool = db.pool();
    let rider_id = uuid::Uuid::new_v4();
    registered(&pool, rider_id, "auth-supabase-roster-3").await;
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        2,
        "RiderRestricted",
        serde_json::json!({
            "riderId": rider_id, "ground": "ACCOUNT_COMPROMISE",
            "decidedAt": "2026-01-06T12:00:00Z", "effectiveAt": "2026-01-06T12:00:00Z"
        }),
    )
    .await;
    append_event(&pool, &format!("Rider-{rider_id}"), 3, "RiderReinstated", serde_json::json!({ "riderId": rider_id }))
        .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (live drain)");
    let live = roster_row(&pool, rider_id).await.expect("roster row (live)");

    // TRUNCATE + checkpoint reset — this group's OWN rebuild discipline (unlike Rider/RiderRestriction,
    // which forbid TRUNCATE): the mechanical `derive:` standing replays correctly ONLY because the
    // table is empty again, not merely because the checkpoint rewound.
    sqlx::query("TRUNCATE rider_roster").execute(&pool).await.expect("truncate rider_roster");
    sqlx::query("DELETE FROM projection_checkpoint WHERE projector = 'RiderRoster'")
        .execute(&pool)
        .await
        .expect("reset the RiderRoster checkpoint to 0 (no row = starts at 0)");
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (from-zero replay)");
    let replayed = roster_row(&pool, rider_id).await.expect("roster row (replayed)");

    assert_eq!(live, replayed, "a from-zero replay must reproduce the byte-identical roster row");
    assert_eq!(replayed.1, "ACTIVE");
    assert_eq!(replayed.2, Some("ACCOUNT_COMPROMISE".to_string()));
}

/// (12) A legacy `RiderStatusChanged { SUSPENDED }` folds `status` (availability) to SUSPENDED but
/// `standing` (the platform's grant) stays ACTIVE — the SAME two-vocabulary guarantee `Rider`/
/// `RiderRestriction` already give, mirrored on the admin's OWN read model (never conflate the
/// legacy availability value with an access restriction, ADR-20260904-014136 §4/§6).
#[tokio::test]
async fn a_legacy_suspended_row_folds_to_availability_suspended_and_standing_active() {
    let Some(db) = crate::common::TestDb::acquire("rider_roster_legacy_suspended").await else { return };
    let pool = db.pool();
    let rider_id = uuid::Uuid::new_v4();
    registered(&pool, rider_id, "auth-supabase-roster-4").await;
    append_event(
        &pool,
        &format!("Rider-{rider_id}"),
        2,
        "RiderStatusChanged",
        serde_json::json!({ "riderId": rider_id, "status": "SUSPENDED" }),
    )
    .await;
    ProjectionWorker::new(pool.clone()).run_once().await.expect("run_once (roster legacy suspended)");

    let status: String = sqlx::query_scalar("SELECT status FROM rider_roster WHERE rider_id = $1")
        .bind(rider_id)
        .fetch_one(&pool)
        .await
        .expect("roster row (status)");
    assert_eq!(status, "SUSPENDED", "the legacy availability value folds through unchanged");
    let (_, standing, _) = roster_row(&pool, rider_id).await.expect("roster row");
    assert_eq!(standing, "ACTIVE", "a legacy SUSPENDED status must never be read as a grant on the roster either");
}
