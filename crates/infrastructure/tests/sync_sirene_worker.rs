//! Integration test for the ADR-0045 staging→worker slice: a raw INSEE row in
//! `external_sirene_restaurants` → `SireneSyncWorker::run_once` → ACL → `register_restaurant` →
//! a `RestaurantRegistered` row in `domain_events` + `processed_at` set → a re-run is a no-op →
//! an explicit `etat=F` refresh closes the NON_PARTNER prospect via `MarkRestaurantClosed`.
//! Needs a real Postgres: set `DATABASE_URL` (see restaurant_write_path.rs for a throwaway docker
//! one-liner). Without it the test SKIPS so `cargo test` stays green offline.

use infrastructure::integrations::sirene::restaurant_id_for_siret;
use infrastructure::SireneSyncWorker;
use sqlx::PgPool;

/// The tests in this file share one DATABASE_URL and reset the same tables — serialize them.
static DB_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
fn db_lock() -> &'static tokio::sync::Mutex<()> {
    DB_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// Fresh copies of the tables the slice touches: the staging table (mirrors
/// migrations/20260718100000) + the write path's `domain_events` + the `restaurant` projection table
/// backing register_restaurant's SlugAlreadyTaken check (empty is fine — the worker does not project)
/// + `command_journal` (mirrors migrations/20260720030000), which every worker send writes on the
/// WORKER channel since #15.
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS external_sirene_restaurants, domain_events, restaurant, command_journal, inbound_events CASCADE;
        CREATE TABLE command_journal (
          message_id UUID PRIMARY KEY,
          correlation_id UUID NOT NULL,
          cause_id UUID NULL,
          session_id UUID NULL,
          trace_id TEXT NULL,
          user_id UUID NULL,
          user_type INTEGER NOT NULL,
          channel INTEGER NOT NULL,
          command_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          payload_hash TEXT NOT NULL,
          status INTEGER NOT NULL,
          error JSONB NULL,
          received_at TIMESTAMPTZ NOT NULL,
          completed_at TIMESTAMPTZ NULL
        );
        -- The worker stages registrations here since ADR-20260728-011344 (#227) — INSEE cannot be
        -- told "no", so a registry record is an inbound FACT, not a command. Missing from this fixture
        -- until #231; every drain assertion failed on `relation "inbound_events" does not exist`, and
        -- nothing caught it because CI has no DATABASE_URL and these tests skip.
        CREATE TABLE inbound_events (
          inbound_event_id UUID PRIMARY KEY,
          source TEXT NOT NULL,
          external_id TEXT NOT NULL,
          correlation_id UUID NOT NULL,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          status INTEGER NOT NULL,
          error JSONB NULL,
          received_at TIMESTAMPTZ NOT NULL,
          delivered_at TIMESTAMPTZ NULL,
          UNIQUE (source, external_id)
        );
        CREATE TABLE external_sirene_restaurants (
          siret TEXT PRIMARY KEY,
          -- NULLable since #231: the payload is TRANSIENT, present only while the row is pending
          -- (or when the record could not be mapped and it is kept as evidence).
          payload JSONB NULL,
          etat TEXT NOT NULL,
          naf TEXT NOT NULL,
          department TEXT NOT NULL,
          first_seen_at TIMESTAMPTZ NOT NULL,
          last_seen_at TIMESTAMPTZ NOT NULL,
          sync_run_id UUID NOT NULL,
          payload_hash TEXT NOT NULL DEFAULT 'unhashed-pre-20260728',
          processed_at TIMESTAMPTZ NULL,
          status TEXT NOT NULL DEFAULT 'PENDING',
          synced_at TIMESTAMPTZ NULL,
          last_attempt_sync_at TIMESTAMPTZ NULL,
          attempt_sync_retry_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE domain_events (
          position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
          id UUID NOT NULL UNIQUE,
          stream_name TEXT NOT NULL,
          version INTEGER NOT NULL,
          user_id UUID NOT NULL,
          user_type INTEGER NOT NULL,
          correlation_id UUID NOT NULL,
          cause_id UUID NULL,
          event_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          metadata JSONB NULL,
          occurred_at TIMESTAMPTZ NOT NULL,
          expired_at TIMESTAMPTZ NULL,
          UNIQUE (stream_name, version)
        );
        CREATE TABLE restaurant (
          restaurant_id UUID PRIMARY KEY,
          restaurant_account_id UUID,
          listing_status INTEGER NOT NULL,
          external_identifiers JSONB,
          google_place_id TEXT,
          slug TEXT NOT NULL UNIQUE,
          display_name TEXT NOT NULL,
          description TEXT,
          tags JSONB,
          margin_rate TEXT,
          cuisine_category INTEGER,
          uber_prices_opt_in BOOLEAN,
          website TEXT,
          rating TEXT,
          reviews_count INTEGER,
          gbp_order_url TEXT,
          gbp_link_status INTEGER,
          address JSONB NOT NULL,
          location JSONB,
          opening_hours JSONB NOT NULL,
          status INTEGER NOT NULL,
          order_acceptance INTEGER NOT NULL,
          default_currency TEXT NOT NULL,
          timezone TEXT,
          preparation_time_minutes INTEGER,
          created_at TIMESTAMPTZ NOT NULL,
          updated_at TIMESTAMPTZ NOT NULL
        );
        "#,
    )
    .execute(pool)
    .await
    .expect("reset schema");
}

/// The same realistic Sirene 3.11 shape the ACL/ingestion tests use, with a parameterizable état.
fn sample_payload(etat: &str) -> serde_json::Value {
    serde_json::json!({
        "siren": "852421099",
        "nic": "00021",
        "siret": "85242109900021",
        "uniteLegale": { "denominationUniteLegale": "SARL CHEZ MARCO",
                         "activitePrincipaleUniteLegale": "56.10A" },
        "adresseEtablissement": {
            "numeroVoieEtablissement": "12",
            "typeVoieEtablissement": "RUE",
            "libelleVoieEtablissement": "NATIONALE",
            "codePostalEtablissement": "37000",
            "libelleCommuneEtablissement": "TOURS",
            "codeCommuneEtablissement": "37261"
        },
        "periodesEtablissement": [ {
            "dateFin": null,
            "etatAdministratifEtablissement": etat,
            "enseigne1Etablissement": "CHEZ MARCO",
            "activitePrincipaleEtablissement": "56.10A"
        } ]
    })
}

/// Stage one row the way the ingestion does (fresh `last_seen_at`, untouched `processed_at`).
async fn stage_row(pool: &PgPool, etat: &str) {
    sqlx::query(
        "INSERT INTO external_sirene_restaurants \
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, processed_at) \
         VALUES ('85242109900021', $1, $2, '56.10A', '37', now(), now(), $3, NULL) \
         ON CONFLICT (siret) DO UPDATE SET \
           payload = EXCLUDED.payload, etat = EXCLUDED.etat, last_seen_at = EXCLUDED.last_seen_at, \
           sync_run_id = EXCLUDED.sync_run_id",
    )
    .bind(sample_payload(etat))
    .bind(etat)
    .bind(uuid::Uuid::new_v4())
    .execute(pool)
    .await
    .expect("stage row");
}

/// Stage a row carrying an arbitrary payload (the `stage_row` helper always stages a mappable one).
async fn stage_row_with_payload(pool: &PgPool, siret: &str, payload: serde_json::Value) {
    sqlx::query(
        "INSERT INTO external_sirene_restaurants \
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, processed_at) \
         VALUES ($1, $2, 'A', '56.10A', '37', now(), now(), $3, NULL) \
         ON CONFLICT (siret) DO UPDATE SET \
           payload = EXCLUDED.payload, last_seen_at = EXCLUDED.last_seen_at, \
           sync_run_id = EXCLUDED.sync_run_id",
    )
    .bind(siret)
    .bind(payload)
    .bind(uuid::Uuid::new_v4())
    .execute(pool)
    .await
    .expect("stage row with payload");
}

/// The payload lifetime (#231, ADR-20260728-143000): the worker DROPS the payload it has translated and
/// KEEPS the one it could not map.
///
/// Both halves matter. Dropping the translated payload is the 655 MB — the mirror was 77% of the
/// database because it kept verbatim records forever to read five fields out of them. Keeping the
/// unmappable one is the diagnostic: that payload is the only evidence of WHY INSEE's record was
/// unusable, and a silent unmappable row with no evidence is how a systematic mapping bug hides.
#[tokio::test]
async fn worker_drops_the_payload_it_translated_and_keeps_what_it_could_not_map() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP worker_drops_the_payload_it_translated_and_keeps_what_it_could_not_map: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let worker = SireneSyncWorker::new(pool.clone());

    // A mappable record, and one that parses as an établissement but that the ACL rejects (no
    // enseigne/denomination anywhere, so there is no usable name).
    stage_row(&pool, "A").await;
    stage_row_with_payload(
        &pool,
        "85242109900039",
        serde_json::json!({
            "siret": "85242109900039",
            "adresseEtablissement": { "codePostalEtablissement": "37000",
                                      "libelleCommuneEtablissement": "TOURS" },
            "periodesEtablissement": [ { "dateFin": null,
                                         "etatAdministratifEtablissement": "A",
                                         "activitePrincipaleEtablissement": "56.10A" } ]
        }),
    )
    .await;

    let summary = worker.run_once().await.expect("drain");
    assert_eq!(summary.processed, 2, "both rows drained");
    assert_eq!(summary.registered, 1, "one staged as an inbound registry fact");
    assert_eq!(summary.skipped, 1, "one could not be mapped");

    let (payload, hash, status): (Option<serde_json::Value>, String, String) = sqlx::query_as(
        "SELECT payload, payload_hash, status FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("translated row");
    assert!(payload.is_none(), "a translated payload is spent — it is never read again");
    assert!(!hash.is_empty(), "the hash persists: it is what stops the next sweep re-pending the row");
    // STAGED, not SYNCED: the fact has been handed to the inbox but the AGGREGATE has not decided yet.
    // Claiming SYNCED here would assert a success this worker never observed.
    assert_eq!(status, "STAGED", "handed over — the verdict is not in yet");

    let (unmappable, unmappable_status): (Option<serde_json::Value>, String) = sqlx::query_as(
        "SELECT payload, status FROM external_sirene_restaurants WHERE siret = '85242109900039'",
    )
    .fetch_one(&pool)
    .await
    .expect("unmappable row");
    assert!(unmappable.is_some(), "the evidence of an unusable INSEE record is kept");
    // Both rows now hold different things and say why. Without `status` these two would be
    // "payload NULL" vs "payload present" with no way to tell evidence from a pending translation.
    assert_eq!(unmappable_status, "UNMAPPABLE", "and it is NOT reported as synced");

    // Both rows are checkpointed either way — an unmappable row must not be retried forever.
    let still_pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM external_sirene_restaurants \
          WHERE processed_at IS NULL OR processed_at < last_seen_at",
    )
    .fetch_one(&pool)
    .await
    .expect("pending count");
    assert_eq!(still_pending, 0, "keeping a payload is not the same as leaving the row pending");
}

#[tokio::test]
async fn worker_drains_staging_rows_through_the_write_path_idempotently_and_closes_prospects() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!(
            "SKIP worker_drains_staging_rows_through_the_write_path_idempotently_and_closes_prospects: DATABASE_URL not set"
        );
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let restaurant_id = restaurant_id_for_siret("85242109900021").0;
    let worker = SireneSyncWorker::new(pool.clone());

    // 1) A pending staged row drains into ONE RestaurantRegistered on the aggregate stream and the
    //    row's processed_at checkpoint is set (no longer pending).
    stage_row(&pool, "A").await;
    let summary = worker.run_once().await.expect("first drain");
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.registered, 1);
    assert_eq!(summary.failed, 0);

    let (stream, event_type, user_type, payload): (String, String, i32, serde_json::Value) =
        sqlx::query_as("SELECT stream_name, event_type, user_type, payload FROM domain_events")
            .fetch_one(&pool)
            .await
            .expect("one event row");
    assert_eq!(stream, format!("Restaurant-{restaurant_id}"));
    assert_eq!(event_type, "RestaurantRegistered");
    assert_eq!(user_type, 6); // EXTERNAL envelope stamp (ADR-0041)
    assert_eq!(payload["ref"], serde_json::json!("85242109900021"));
    assert_eq!(payload["listingStatus"], serde_json::json!("NON_PARTNER"));

    // The send converged on command_journal (channel WORKER=1, status SUCCEEDED=1, #15), and the
    // appended event's cause_id is the journal row's message_id — the full causal chain holds.
    let (message_id, channel, status): (uuid::Uuid, i32, i32) = sqlx::query_as(
        "SELECT message_id, channel, status FROM command_journal WHERE command_type = 'RegisterRestaurant'",
    )
    .fetch_one(&pool)
    .await
    .expect("one RegisterRestaurant journal row");
    assert_eq!((channel, status), (1, 1));
    let (event_cause,): (Option<uuid::Uuid>,) =
        sqlx::query_as("SELECT cause_id FROM domain_events WHERE event_type = 'RestaurantRegistered'")
            .fetch_one(&pool)
            .await
            .expect("registered event cause");
    assert_eq!(event_cause, Some(message_id));

    let pending: bool = sqlx::query_scalar(
        "SELECT processed_at IS NULL OR processed_at < last_seen_at \
         FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("pending flag");
    assert!(!pending, "a drained row must carry its processed_at checkpoint");

    // 2) Re-running the worker with nothing new staged is a complete no-op.
    let replay = worker.run_once().await.expect("no-op drain");
    assert_eq!(replay.processed, 0);
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert_eq!(events, 1, "an idempotent re-run must not append events");

    // 3) A re-ingested row (same SIRET, refreshed last_seen_at) is pending again but the deterministic
    //    UUIDv5 id makes the registration replay a no-op.
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    stage_row(&pool, "A").await;
    let refresh = worker.run_once().await.expect("refresh drain");
    assert_eq!(refresh.processed, 1);
    assert_eq!(refresh.registered, 1); // Ok covers the idempotent replay of a known SIRET
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events after replay");
    assert_eq!(events, 1);
    // The refreshed staged version (bumped last_seen_at) journals as a NEW send — the aggregate
    // no-op is still a SUCCEEDED submission, visible per delivery.
    let register_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM command_journal WHERE command_type = 'RegisterRestaurant'",
    )
    .fetch_one(&pool)
    .await
    .expect("count register journal rows");
    assert_eq!(register_rows, 2);

    // 4) Deletion reconciliation (ADR-0045): an explicit etat=F refresh closes the NON_PARTNER
    //    prospect via the ordinary MarkRestaurantClosed handler…
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    stage_row(&pool, "F").await;
    let closing = worker.run_once().await.expect("closing drain");
    assert_eq!(closing.processed, 1);
    assert_eq!(closing.closed, 1);
    let (last_type,): (String,) = sqlx::query_as(
        "SELECT event_type FROM domain_events ORDER BY position DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("latest event");
    assert_eq!(last_type, "RestaurantMarkedClosed");

    // …and repeating the signal is absorbed (the aggregate already folds to INACTIVE).
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    stage_row(&pool, "F").await;
    let closed_again = worker.run_once().await.expect("idempotent closing drain");
    assert_eq!(closed_again.closed, 0);
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("final event count");
    assert_eq!(events, 2, "register + one close, no matter how often the signal repeats");
    // The repeated signal never reached the dispatch (the aggregate already folds INACTIVE), so the
    // journal holds exactly ONE MarkRestaurantClosed submission — journaled, WORKER, SUCCEEDED.
    let (close_rows, close_channel_status): (i64, i64) = (
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM command_journal WHERE command_type = 'MarkRestaurantClosed'",
        )
        .fetch_one(&pool)
        .await
        .expect("count close journal rows"),
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM command_journal \
             WHERE command_type = 'MarkRestaurantClosed' AND channel = 1 AND status = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("count close journal rows by channel/status"),
    );
    assert_eq!((close_rows, close_channel_status), (1, 1));
}

/// Seed the `restaurant` projection the way production looks for pre-derivation listings: a row
/// owning the slug under an arbitrary legacy aggregate id, carrying (or not) the SIRET identifier.
async fn seed_projection_row(pool: &PgPool, id: uuid::Uuid, slug: &str, identifiers: serde_json::Value) {
    sqlx::query(
        "INSERT INTO restaurant (restaurant_id, listing_status, external_identifiers, slug, \
           display_name, address, opening_hours, status, order_acceptance, default_currency, \
           created_at, updated_at) \
         VALUES ($1, 0, $2, $3, 'CHEZ MARCO', '{}'::jsonb, '[]'::jsonb, 0, 0, 'EUR', now(), now())",
    )
    .bind(id)
    .bind(identifiers)
    .bind(slug)
    .execute(pool)
    .await
    .expect("seed projection row");
}

/// Production predates the UUIDv5(SIRET) derivation: the projection row carrying the SIRET names the
/// real aggregate, so the worker must adopt ITS id (register replay + close both target it) instead
/// of deriving a slug-colliding sibling and retrying forever.
#[tokio::test]
async fn worker_adopts_the_legacy_aggregate_id_the_projection_names_for_a_known_siret() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP worker_adopts_the_legacy_aggregate_id...: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let legacy_id = uuid::Uuid::new_v4();
    assert_ne!(legacy_id, restaurant_id_for_siret("85242109900021").0);
    seed_projection_row(
        &pool,
        legacy_id,
        "chez-marco-00021",
        serde_json::json!([{ "key": "siret", "value": "85242109900021" }]),
    )
    .await;
    let worker = SireneSyncWorker::new(pool.clone());

    // The register replay adopts the legacy id — no SlugAlreadyTaken, no derived sibling.
    stage_row(&pool, "A").await;
    let summary = worker.run_once().await.expect("adoption drain");
    assert_eq!((summary.registered, summary.skipped, summary.failed), (1, 0, 0));
    let (stream,): (String,) =
        sqlx::query_as("SELECT stream_name FROM domain_events ORDER BY position LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("registered event");
    assert_eq!(stream, format!("Restaurant-{legacy_id}"));

    // The close path resolves the SAME id, so legacy listings are closable too.
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    stage_row(&pool, "F").await;
    let closing = worker.run_once().await.expect("closing drain");
    assert_eq!(closing.closed, 1);
    let (stream, event_type): (String, String) = sqlx::query_as(
        "SELECT stream_name, event_type FROM domain_events ORDER BY position DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("close event");
    assert_eq!((stream.as_str(), event_type.as_str()), (format!("Restaurant-{legacy_id}").as_str(), "RestaurantMarkedClosed"));
}

/// A catalogued rejection (here a REAL slug conflict — same slug, different establishment) is
/// deterministic: the worker must mark the row processed and move on, not retry it every pass
/// (the production 605-row SlugAlreadyTaken log storm).
#[tokio::test]
async fn worker_marks_a_deterministically_rejected_row_processed_instead_of_retrying_forever() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP worker_marks_a_deterministically_rejected_row...: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    // The slug is owned by a DIFFERENT establishment (different SIRET identifier): a true conflict
    // the sync can never resolve by itself.
    seed_projection_row(
        &pool,
        uuid::Uuid::new_v4(),
        "chez-marco-00021",
        serde_json::json!([{ "key": "siret", "value": "11111111100021" }]),
    )
    .await;
    let worker = SireneSyncWorker::new(pool.clone());

    stage_row(&pool, "A").await;
    let summary = worker.run_once().await.expect("rejected drain");
    assert_eq!((summary.registered, summary.skipped, summary.failed), (0, 1, 0));
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert_eq!(events, 0);

    // Since #15 the rejection leaves a durable trace: a REJECTED (=2) WORKER-channel journal row
    // carrying the errors.yaml code — support can finally answer "what happened to this row".
    let (status, error): (i32, serde_json::Value) = sqlx::query_as(
        "SELECT status, error FROM command_journal WHERE command_type = 'RegisterRestaurant'",
    )
    .fetch_one(&pool)
    .await
    .expect("rejected journal row");
    assert_eq!(status, 2);
    assert_eq!(error["code"], serde_json::json!("SlugAlreadyTaken"));

    // The row is checkpointed: the next pass has nothing to do — the churn is gone.
    let replay = worker.run_once().await.expect("no-op drain");
    assert_eq!(replay.processed, 0);
}

/// The worker does NOT know, at hand-over, whether the aggregate accepted the record — since
/// ADR-20260728-011344 the register path stages an inbound FACT and `InboundEventsDrainWorker` delivers
/// it later. So `STAGED -> SYNCED` has to be resolved from the aggregate's verdict on a subsequent pass,
/// and this pins that it actually happens: without it the mirror would sit on STAGED forever, or (worse)
/// claim a success nobody observed.
///
/// The join needs no extra bookkeeping — the ACL already writes
/// `inbound_events.external_id = '{siret}:{payload_hash}'`, and both halves are columns on the row.
#[tokio::test]
async fn staged_rows_resolve_to_synced_from_the_aggregates_verdict() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP staged_rows_resolve_to_synced_from_the_aggregates_verdict: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let worker = SireneSyncWorker::new(pool.clone());

    stage_row(&pool, "A").await;
    worker.run_once().await.expect("first drain stages the fact");

    let (status, synced_at): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT status, synced_at FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("staged row");
    assert_eq!(status, "STAGED");
    assert!(synced_at.is_none(), "nothing has reached the domain yet, so nothing may claim a sync time");

    // The drain worker delivers it and the aggregate decides. Simulated here by writing the verdict the
    // real InboundEventsDrainWorker writes — DELIVERED (ordinal 1).
    let updated = sqlx::query(
        "UPDATE inbound_events SET status = 1, delivered_at = now() WHERE source = 'sirene'",
    )
    .execute(&pool)
    .await
    .expect("simulate delivery")
    .rows_affected();
    assert_eq!(updated, 1, "the register path staged exactly one inbound fact");

    let summary = worker.run_once().await.expect("second drain reconciles");
    assert_eq!(summary.resolved, 1, "the verdict is read back onto the mirror");

    let (status, synced_at): (String, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT status, synced_at FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("resolved row");
    assert_eq!(status, "SYNCED", "the aggregate accepted it, so now the mirror may say so");
    assert!(synced_at.is_some(), "and records WHEN it reached the domain");
}

/// A no-change verdict is a SUCCESS, not a failure. The aggregate folding "nothing moved" (IGNORED) means
/// the record reached the domain and the domain is now correct about it — conflating that with a failure
/// is what once made a sweep unable to tell 200,000 registrations from 200,000 no-ops.
#[tokio::test]
async fn an_ignored_verdict_still_counts_as_synced() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP an_ignored_verdict_still_counts_as_synced: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let worker = SireneSyncWorker::new(pool.clone());

    stage_row(&pool, "A").await;
    worker.run_once().await.expect("stage the fact");
    // IGNORED = 3 (declaration-order ordinal): the aggregate decided nothing had changed.
    sqlx::query("UPDATE inbound_events SET status = 3, delivered_at = now() WHERE source = 'sirene'")
        .execute(&pool)
        .await
        .expect("simulate a no-change verdict");

    worker.run_once().await.expect("reconcile");
    let status: String = sqlx::query_scalar(
        "SELECT status FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(status, "SYNCED", "'nothing changed' is a real answer, not a failure");
}

/// The quarantine has to actually STOP the retry, or it is just a label.
///
/// A failed sync deliberately leaves the row pending WITH its payload — the retry needs something to
/// translate — so nothing in the pending predicate ever excludes a permanently-broken row. It would be
/// re-attempted on every pass forever, burning the sweep's budget and emitting an error nobody acts on.
/// (Not hypothetical: the 605-row SlugAlreadyTaken log storm was exactly this shape.) So the drain
/// filters on `status <> 'POISON'`, and this pins it.
#[tokio::test]
async fn a_poisoned_row_is_skipped_by_the_drain_and_keeps_its_payload() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP a_poisoned_row_is_skipped_by_the_drain_and_keeps_its_payload: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let worker = SireneSyncWorker::new(pool.clone());

    // A pending row that has already exhausted its attempts.
    stage_row(&pool, "A").await;
    sqlx::query(
        "UPDATE external_sirene_restaurants SET status = 'POISON', attempt_sync_retry_count = 10",
    )
    .execute(&pool)
    .await
    .expect("quarantine the row");

    let summary = worker.run_once().await.expect("drain");
    assert_eq!(summary.processed, 0, "a quarantined row is not re-attempted");

    let (payload, status): (Option<serde_json::Value>, String) = sqlx::query_as(
        "SELECT payload, status FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("row");
    assert!(payload.is_some(), "its payload is the evidence needed to diagnose it — never dropped");
    assert_eq!(status, "POISON", "and it stays quarantined until INSEE sends something different");
}
