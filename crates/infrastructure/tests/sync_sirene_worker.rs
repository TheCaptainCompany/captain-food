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
/// backing register_restaurant's SlugAlreadyTaken check (empty is fine — the worker does not project).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS external_sirene_restaurants, domain_events, restaurant CASCADE;
        CREATE TABLE external_sirene_restaurants (
          siret TEXT PRIMARY KEY,
          payload JSONB NOT NULL,
          etat TEXT NOT NULL,
          naf TEXT NOT NULL,
          department TEXT NOT NULL,
          first_seen_at TIMESTAMPTZ NOT NULL,
          last_seen_at TIMESTAMPTZ NOT NULL,
          sync_run_id UUID NOT NULL,
          processed_at TIMESTAMPTZ NULL
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

    // The row is checkpointed: the next pass has nothing to do — the churn is gone.
    let replay = worker.run_once().await.expect("no-op drain");
    assert_eq!(replay.processed, 0);
}
