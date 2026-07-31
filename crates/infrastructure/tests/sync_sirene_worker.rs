//! Integration test for the ADR-0045 staging→worker slice, post-#227 (ADR-20260728-011344 D4): a raw
//! INSEE row in `external_sirene_restaurants` → `SireneSyncWorker::run_once` → ACL → an inbound FACT in
//! `inbound_events` (INSEE cannot be told "no", so registrations are recorded, not requested) →
//! `InboundEventsDrainWorker` delivers it through the normal write path and the AGGREGATE decides →
//! the verdict is reconciled back onto the mirror (`STAGED` → `SYNCED`/`FAILED`, #231). The explicit
//! `etat=F` closure stays a COMMAND (`MarkRestaurantClosed` — our inference, refusable), so that half
//! still journals through `command_journal`.
//! Needs a real Postgres: set `DATABASE_URL` (see restaurant_write_path.rs for a throwaway docker
//! one-liner). Without it the test SKIPS so `cargo test` stays green offline.

use std::sync::Arc;

use application::ports::{Actor, EventStore};
use domain::generated::entities::Address;
use domain::generated::events::{DomainEvent, RestaurantRegistered};
use domain::generated::scalars::{
    AddressLine, CityName, CountryCode, PostalCode, RestaurantDisplayName, RestaurantId,
    RestaurantListingStatus,
};
use infrastructure::integrations::sirene::restaurant_id_for_siret;
use infrastructure::{
    InboundEventsDrainWorker, PgCommandJournal, PgEventStore, PgInboundEvents, SireneSyncWorker,
};
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
          user_type TEXT NOT NULL,
          channel TEXT NOT NULL,
          command_type TEXT NOT NULL,
          payload JSONB NOT NULL,
          payload_hash TEXT NOT NULL,
          status TEXT NOT NULL,
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
          status TEXT NOT NULL,
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
          user_type TEXT NOT NULL,
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
          listing_status TEXT NOT NULL,
          external_identifiers JSONB,
          google_place_id TEXT,
          -- NULLABLE since migrations/20260728020000: a prospect has no slug until one is configured.
          slug TEXT UNIQUE,
          display_name TEXT NOT NULL,
          description TEXT,
          tags JSONB,
          margin_rate TEXT,
          cuisine_category TEXT,
          uber_prices_opt_in BOOLEAN,
          website TEXT,
          rating TEXT,
          reviews_count INTEGER,
          gbp_order_url TEXT,
          gbp_link_status TEXT,
          address JSONB NOT NULL,
          location JSONB,
          opening_hours JSONB NOT NULL,
          status TEXT NOT NULL,
          order_acceptance TEXT NOT NULL,
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
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
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

    // The snapshot `/sirene` serves (#244). A ping-triggered pass records itself too — the endpoint
    // describes the WORKER, not just the poll loop — while `running` stays false here because no loop
    // was started. That pair is the distinction the department-37 pilot (#238) had no way to make.
    {
        let handle = worker.status();
        let snapshot = handle.lock().expect("sirene status");
        assert!(!snapshot.running, "this test never started the poll loop");
        assert!(snapshot.last_tick_at.is_some(), "the pass stamped itself");
        assert!(snapshot.last_error.is_none(), "a successful pass leaves no error");
        assert_eq!(
            snapshot.last_summary.as_ref().map(|s| s.registered),
            Some(1),
            "the snapshot carries the pass's own counters"
        );
    }

    let (payload, hash, status): (Option<serde_json::Value>, String, String) = sqlx::query_as(
        "SELECT payload, payload_hash, status FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("translated row");
    // STAGED, not SYNCED: the fact has been handed to the inbox but the AGGREGATE has not decided yet.
    // Claiming SYNCED here would assert a success this worker never observed — and the payload must
    // survive with it, because an unaccepted translation is exactly when the raw record is still needed.
    assert_eq!(status, "STAGED", "handed over — the verdict is not in yet");
    assert!(payload.is_some(), "nothing is confirmed, so the only original is not thrown away");
    assert!(!hash.is_empty(), "the hash persists: it is what stops the next sweep re-pending the row");

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

/// The real delivery half (ADR-20260720-015400), wired over the same pool — so these tests assert what
/// production does with a staged fact, not a hand-simulated verdict.
fn drain_worker(pool: &PgPool) -> Arc<InboundEventsDrainWorker> {
    Arc::new(InboundEventsDrainWorker::new(
        Arc::new(PgInboundEvents::new(pool.clone())),
        Arc::new(PgCommandJournal::new(pool.clone())),
        Arc::new(PgEventStore::new(pool.clone())),
    ))
}

#[tokio::test]
async fn worker_drains_staging_rows_through_the_write_path_idempotently_and_closes_prospects() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
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

    // 1) A pending staged row is handed over as ONE inbound registry FACT — and nothing more. No
    //    command (INSEE cannot be told "no", ADR-20260728-011344 D4), and no domain event yet: the
    //    hand-over is not the delivery, and the aggregate has not decided anything.
    stage_row(&pool, "A").await;
    let summary = worker.run_once().await.expect("first drain");
    assert_eq!(summary.processed, 1);
    assert_eq!(summary.registered, 1, "one registry fact handed to the inbox");
    assert_eq!(summary.failed, 0);

    let (inbound_event_id, source, external_id, event_type, inbound_status): (
        uuid::Uuid,
        String,
        String,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT inbound_event_id, source, external_id, event_type, status FROM inbound_events",
    )
    .fetch_one(&pool)
    .await
    .expect("one inbound fact");
    assert_eq!(source, "sirene");
    // The stable dedupe key: the staged version is identified by WHAT the record says
    // (`{siret}:{payload_hash}`), not by WHEN it was seen — which is what lets a later sweep
    // re-see the same content without re-staging it.
    let staged_hash: String = sqlx::query_scalar(
        "SELECT payload_hash FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("staged hash");
    assert_eq!(external_id, format!("85242109900021:{staged_hash}"));
    assert_eq!(event_type, "RestaurantRegistered");
    assert_eq!(inbound_status, "RECEIVED", "RECEIVED — awaiting the drain");

    let register_journal: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM command_journal WHERE command_type = 'RegisterRestaurant'",
    )
    .fetch_one(&pool)
    .await
    .expect("count register journal rows");
    assert_eq!(register_journal, 0, "a fact is recorded, not requested — no command is journaled");
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert_eq!(events, 0, "hand-over is not delivery — nothing on the log yet");

    let pending: bool = sqlx::query_scalar(
        "SELECT processed_at IS NULL OR processed_at < last_seen_at \
         FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("pending flag");
    assert!(!pending, "a drained row must carry its processed_at checkpoint");

    // 2) The REAL drain worker delivers the fact through the normal write path: ONE
    //    RestaurantRegistered on the deterministic UUIDv5(SIRET) stream, stamped EXTERNAL, and
    //    caused by the exact inbound record that carried it.
    let drain = drain_worker(&pool);
    let delivered = drain.run_once().await.expect("single-flight");
    assert_eq!(delivered.delivered, 1);
    assert_eq!(delivered.failed, 0);

    let (stream, event_type, user_type, cause_id, payload): (
        String,
        String,
        String,
        Option<uuid::Uuid>,
        serde_json::Value,
    ) = sqlx::query_as(
        "SELECT stream_name, event_type, user_type, cause_id, payload FROM domain_events",
    )
    .fetch_one(&pool)
    .await
    .expect("one event row");
    assert_eq!(stream, format!("Restaurant-{restaurant_id}"));
    assert_eq!(event_type, "RestaurantRegistered");
    assert_eq!(user_type, "EXTERNAL"); // EXTERNAL envelope stamp (ADR-0041)
    assert_eq!(cause_id, Some(inbound_event_id), "the fact chains to the inbound record");
    assert_eq!(payload["ref"], serde_json::json!("85242109900021"));
    assert_eq!(payload["listingStatus"], serde_json::json!("NON_PARTNER"));

    // 3) The aggregate's verdict is reconciled onto the mirror by the next pass — which is otherwise
    //    a complete no-op — and only THERE does the row become SYNCED and lose its payload (#231).
    let replay = worker.run_once().await.expect("reconcile pass");
    assert_eq!(replay.processed, 0, "nothing pending — an idempotent re-run drains nothing");
    assert_eq!(replay.resolved, 1, "the DELIVERED verdict lands on the mirror");
    let (mirror_status, synced_at, mirror_payload): (
        String,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<serde_json::Value>,
    ) = sqlx::query_as(
        "SELECT status, synced_at, payload FROM external_sirene_restaurants \
          WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("reconciled row");
    assert_eq!(mirror_status, "SYNCED");
    assert!(synced_at.is_some(), "the sync's completion is timestamped");
    assert!(mirror_payload.is_none(), "confirmed — the payload is finally released");

    // 4) A re-ingested row (same SIRET, same content, refreshed last_seen_at) is pending again, but
    //    the stable (source, external_id) key absorbs it: nothing new staged, nothing appended, and
    //    the already-recorded verdict re-confirms the row within the same pass.
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    stage_row(&pool, "A").await;
    let refresh = worker.run_once().await.expect("refresh drain");
    assert_eq!(refresh.processed, 1);
    assert_eq!(refresh.registered, 0, "the same fact is not re-staged");
    assert_eq!(refresh.skipped, 1, "it deduplicates on the inbox instead");
    assert_eq!(refresh.resolved, 1, "and the known verdict re-confirms the row in the same pass");
    let inbound_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbound_events")
        .fetch_one(&pool)
        .await
        .expect("count inbound rows");
    assert_eq!(inbound_rows, 1, "one fact, however many times the sweep re-sees it");
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events after replay");
    assert_eq!(events, 1);

    // 5) Deletion reconciliation (ADR-0045): an explicit etat=F refresh closes the NON_PARTNER
    //    prospect via the ordinary MarkRestaurantClosed handler. Closure IS still a command — the
    //    closure is our inference and can be refused — so this half journals (WORKER channel, #15)…
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
             WHERE command_type = 'MarkRestaurantClosed' AND channel = 'WORKER' AND status = 'SUCCEEDED'",
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
         VALUES ($1, 'NON_PARTNER', $2, $3, 'CHEZ MARCO', '{}'::jsonb, '[]'::jsonb, 'DRAFT', 'NORMAL', 'EUR', now(), now())",
    )
    .bind(id)
    .bind(identifiers)
    .bind(slug)
    .execute(pool)
    .await
    .expect("seed projection row");
}

/// Production predates the UUIDv5(SIRET) derivation, so legacy listings live under other aggregate
/// ids and the projection row carrying the SIRET is the only thing that NAMES them. Since #227 the
/// adoption survives on the CLOSE path only: the register path derives its id deterministically and
/// never asks the read model, but a closure aimed at the derived id would close a sibling that does
/// not exist — leaving the real, legacy-id listing open and orderable, which is the worse failure.
#[tokio::test]
async fn worker_adopts_the_legacy_aggregate_id_the_projection_names_for_a_known_siret() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP worker_adopts_the_legacy_aggregate_id...: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let legacy_id = uuid::Uuid::new_v4();
    assert_ne!(legacy_id, restaurant_id_for_siret("85242109900021").0);

    // The legacy aggregate EXISTS in the log under its pre-derivation id, and the projection row
    // carrying the SIRET names it — exactly the shape production inherited.
    let store = PgEventStore::new(pool.clone());
    let registered = DomainEvent::RestaurantRegistered(RestaurantRegistered {
        mode: None,
        restaurant_id: RestaurantId(legacy_id),
        account_id: None,
        listing_status: RestaurantListingStatus::NON_PARTNER,
        r#ref: None,
        external_identifiers: vec![],
        display_name: RestaurantDisplayName("CHEZ MARCO".into()),
        contact: None,
        website: None,
        tags: vec![],
        margin_rate: None,
        cuisine_category: None,
        uber_prices_opt_in: None,
        address: Address {
            line1: AddressLine("12 RUE NATIONALE".into()),
            line2: None,
            postal_code: PostalCode("37000".into()),
            city: CityName("TOURS".into()),
            country: CountryCode("FR".into()),
        },
        location: None,
        timezone: None,
        preparation_time_minutes: None,
        opening_hours: vec![],
    });
    let actor = Actor {
        user_id: uuid::Uuid::nil(),
        user_type: "EXTERNAL".to_string(),
        domain_id: None,
        correlation_id: uuid::Uuid::new_v4(),
        cause_id: None,
    };
    store
        .append(&format!("Restaurant-{legacy_id}"), 0, std::slice::from_ref(&registered), &actor)
        .await
        .expect("seed the legacy aggregate");
    seed_projection_row(
        &pool,
        legacy_id,
        "chez-marco-00021",
        serde_json::json!([{ "key": "siret", "value": "85242109900021" }]),
    )
    .await;
    let worker = SireneSyncWorker::new(pool.clone());

    // An explicit closure signal for that SIRET must close the aggregate the projection NAMES,
    // not a derived sibling.
    stage_row(&pool, "F").await;
    let closing = worker.run_once().await.expect("closing drain");
    assert_eq!(closing.closed, 1);
    assert_eq!(closing.failed, 0);
    let (stream, event_type): (String, String) = sqlx::query_as(
        "SELECT stream_name, event_type FROM domain_events ORDER BY position DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("close event");
    assert_eq!(
        (stream.as_str(), event_type.as_str()),
        (format!("Restaurant-{legacy_id}").as_str(), "RestaurantMarkedClosed")
    );
    // And the register-path contrast (#227): a closure needs no inbound fact — nothing was staged.
    let inbound_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inbound_events")
        .fetch_one(&pool)
        .await
        .expect("count inbound rows");
    assert_eq!(inbound_rows, 0, "closing is a command, not an inbound fact");
}

/// The modern shape of the retired "deterministically rejected row" test. Since #227 the register
/// path has no rejections — a slug is not even validated (the worker stages a FACT; a prospect has no
/// slug until one is configured). What remains deterministic is a DELIVERY failure: a staged fact the
/// write path cannot apply. That verdict must (a) leave a durable, queryable trace instead of an
/// eprintln nobody reads, (b) reconcile onto the mirror as FAILED with the payload KEPT (it is the
/// only original if re-translation is needed), and (c) not be retried every pass — the production
/// 605-row SlugAlreadyTaken log storm was exactly the retry-forever shape this guards against.
/// Recovery needs no operator action: a changed record from INSEE re-pends the row.
#[tokio::test]
async fn a_failed_delivery_leaves_a_durable_trace_and_is_not_retried_forever() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP a_failed_delivery_leaves_a_durable_trace...: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let worker = SireneSyncWorker::new(pool.clone());

    stage_row(&pool, "A").await;
    let summary = worker.run_once().await.expect("stage the fact");
    assert_eq!(summary.registered, 1);

    // Break the staged fact the way an ACL/schema drift would: the payload no longer parses as a
    // DomainEvent. The REAL drain worker then records the verdict — this is not simulated.
    sqlx::query(
        "UPDATE inbound_events SET payload = '{\"eventType\":\"NotAnEvent\"}'::jsonb \
          WHERE source = 'sirene'",
    )
    .execute(&pool)
    .await
    .expect("corrupt the staged fact");
    let drain = drain_worker(&pool);
    let delivered = drain.run_once().await.expect("single-flight");
    assert_eq!(delivered.failed, 1);
    assert_eq!(delivered.delivered, 0);

    // (a) The verdict is durable and queryable — support can answer "what happened to this row".
    let (inbound_status, error): (String, Option<serde_json::Value>) =
        sqlx::query_as("SELECT status, error FROM inbound_events WHERE source = 'sirene'")
            .fetch_one(&pool)
            .await
            .expect("inbound verdict");
    assert_eq!(inbound_status, "FAILED");
    let error = error.expect("the failure reason is recorded on the row");
    assert!(
        error["detail"].as_str().is_some_and(|d| !d.is_empty()),
        "the trace says WHY, not just that it failed"
    );

    // (b) Reconciled onto the mirror as FAILED: nothing claims a sync, and the payload survives.
    let reconcile = worker.run_once().await.expect("reconcile pass");
    assert_eq!(reconcile.resolved, 1);
    let (mirror_status, payload, synced_at): (
        String,
        Option<serde_json::Value>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT status, payload, synced_at FROM external_sirene_restaurants \
          WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("reconciled row");
    assert_eq!(mirror_status, "FAILED");
    assert!(payload.is_some(), "a failed delivery is precisely when the raw record is still needed");
    assert!(synced_at.is_none(), "and nothing may claim it reached the domain");

    // (c) The row is checkpointed: the next pass has nothing to do — the churn is gone.
    let replay = worker.run_once().await.expect("no-op drain");
    assert_eq!(replay.processed, 0);
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM domain_events")
        .fetch_one(&pool)
        .await
        .expect("count events");
    assert_eq!(events, 0, "nothing reached the domain");
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
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
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
    let payload: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("staged row payload");
    assert!(payload.is_some(), "the payload survives hand-over — the verdict is not in yet");

    // The drain worker delivers it and the aggregate decides. Simulated here by writing the verdict the
    // real InboundEventsDrainWorker writes — DELIVERED (ordinal 1).
    let updated = sqlx::query(
        "UPDATE inbound_events SET status = 'DELIVERED', delivered_at = now() WHERE source = 'sirene'",
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
    // Only NOW is the payload spent — dropped in the same statement that recorded the certainty, never
    // on the strength of an assumption made earlier.
    let payload: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("resolved row payload");
    assert!(payload.is_none(), "confirmed synced — the payload is finally released");
}

/// A no-change verdict is a SUCCESS, not a failure. The aggregate folding "nothing moved" (IGNORED) means
/// the record reached the domain and the domain is now correct about it — conflating that with a failure
/// is what once made a sweep unable to tell 200,000 registrations from 200,000 no-ops.
#[tokio::test]
async fn an_ignored_verdict_still_counts_as_synced() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
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
    sqlx::query("UPDATE inbound_events SET status = 'IGNORED', delivered_at = now() WHERE source = 'sirene'")
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
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
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

/// A FAILED verdict must not cost the payload. The record still has to be re-translated, and the raw
/// staging copy is the only original we hold — the inbound row carries the TRANSLATED form, which is
/// exactly what is suspect when a delivery fails.
#[tokio::test]
async fn a_failed_verdict_keeps_the_payload() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        assert!(
            std::env::var("DB_TESTS_REQUIRED").is_err(),
            "DB_TESTS_REQUIRED is set but DATABASE_URL is not — a DB-gated test may not skip here (#230)"
        );
        eprintln!("SKIP a_failed_verdict_keeps_the_payload: DATABASE_URL not set");
        return;
    };
    let _guard = db_lock().lock().await;
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;
    let worker = SireneSyncWorker::new(pool.clone());

    stage_row(&pool, "A").await;
    worker.run_once().await.expect("stage the fact");
    // FAILED = 2 (declaration-order ordinal).
    sqlx::query("UPDATE inbound_events SET status = 'FAILED' WHERE source = 'sirene'")
        .execute(&pool)
        .await
        .expect("simulate a failed delivery");

    worker.run_once().await.expect("reconcile");
    let (status, payload, synced_at): (String, Option<serde_json::Value>, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as(
            "SELECT status, payload, synced_at FROM external_sirene_restaurants \
              WHERE siret = '85242109900021'",
        )
        .fetch_one(&pool)
        .await
        .expect("row");
    assert_eq!(status, "FAILED");
    assert!(payload.is_some(), "a failed delivery is precisely when the raw record is still needed");
    assert!(synced_at.is_none(), "and nothing may claim it reached the domain");
}
