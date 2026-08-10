//! Integration test for the SIRENE ingestion UPSERT (ADR-0045): one row per SIRET in the
//! `external_sirene_restaurants` staging table, idempotent across runs (a re-run refreshes
//! `last_seen_at`/`payload`/`etat`/`sync_run_id` and makes the row pending again, without touching
//! `first_seen_at`/`processed_at`). Needs a real Postgres: set `DATABASE_URL` (e.g. a throwaway
//! `docker run -e POSTGRES_PASSWORD=postgres -p 5433:5432 postgres:16-alpine`, then
//! `DATABASE_URL=postgres://postgres:postgres@localhost:5433/postgres?sslmode=disable`).
//! Without it the test SKIPS so `cargo test` stays green offline.

use chrono::{DateTime, Utc};
use sirene_ingest::{upsert_staging_batch, upsert_staging_row, Etablissement, SireneRecord};
use sqlx::PgPool;

/// Every test here DROP+CREATEs the SAME staging table and works the SAME SIRET, and the test
/// harness runs a file's tests on concurrent threads — unserialized, each test yanks the table
/// out from under its siblings and the suite passes or fails on interleaving luck (observed:
/// 3 vs 4 nondeterministic failures per run, 2026-08-01). One suite-wide lock, held for each
/// test's whole body, restores determinism; a poisoned lock (a failed sibling) is fine to reuse.
static SUITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serialize_suite() -> std::sync::MutexGuard<'static, ()> {
    SUITE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Fresh copy of the staging table (mirrors migrations/20260718100000_external_sirene_restaurants.sql).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS external_sirene_restaurants CASCADE;
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
        "#,
    )
    .execute(pool)
    .await
    .expect("reset staging schema");
}

/// The same realistic Sirene 3.11 shape the client/ACL tests use.
fn sample_record() -> SireneRecord {
    let raw: serde_json::Value = serde_json::from_str(
        r#"{
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
                "etatAdministratifEtablissement": "A",
                "enseigne1Etablissement": "CHEZ MARCO",
                "activitePrincipaleEtablissement": "56.10A"
            } ]
        }"#,
    )
    .expect("parse sample établissement JSON");
    let etablissement: Etablissement =
        serde_json::from_value(raw.clone()).expect("typed subset parses");
    SireneRecord { raw, etablissement }
}

/// Same realistic shape as [`sample_record`], with a caller-chosen SIRET so a batch can carry several
/// distinct rows.
fn record_with_siret(siret: &str) -> SireneRecord {
    let raw: serde_json::Value = serde_json::from_str(&format!(
        r#"{{
            "siren": "852421099",
            "nic": "00021",
            "siret": "{siret}",
            "uniteLegale": {{ "denominationUniteLegale": "SARL CHEZ MARCO",
                             "activitePrincipaleUniteLegale": "56.10A" }},
            "adresseEtablissement": {{
                "codeCommuneEtablissement": "37261"
            }},
            "periodesEtablissement": [ {{
                "dateFin": null,
                "etatAdministratifEtablissement": "A",
                "activitePrincipaleEtablissement": "56.10A"
            }} ]
        }}"#
    ))
    .expect("parse sample établissement JSON");
    let etablissement: Etablissement =
        serde_json::from_value(raw.clone()).expect("typed subset parses");
    SireneRecord { raw, etablissement }
}

#[tokio::test]
async fn staging_upsert_is_idempotent_per_siret_and_bumps_last_seen_at() {
    let _suite = serialize_suite();
    let Some(url) = db_test_gate::database_url("staging_upsert_is_idempotent_per_siret_and_bumps_last_seen_at") else { return };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    // Run 1: the SIRET lands with etat/naf/department extracted and processed_at NULL (pending).
    let run_1 = uuid::Uuid::new_v4();
    upsert_staging_row(&pool, &sample_record(), "37", run_1).await.expect("first upsert");

    let (etat, naf, department, first_seen, last_seen, run_id, processed): (
        String,
        String,
        String,
        DateTime<Utc>,
        DateTime<Utc>,
        uuid::Uuid,
        Option<DateTime<Utc>>,
    ) = sqlx::query_as(
        "SELECT etat, naf, department, first_seen_at, last_seen_at, sync_run_id, processed_at \
         FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("staged row");
    assert_eq!(etat, "A");
    assert_eq!(naf, "56.10A");
    assert_eq!(department, "37");
    assert_eq!(first_seen, last_seen);
    assert_eq!(run_id, run_1);
    assert_eq!(processed, None);

    // The worker marks the row processed (high-water mark = the last_seen_at it drained).
    sqlx::query("UPDATE external_sirene_restaurants SET processed_at = last_seen_at")
        .execute(&pool)
        .await
        .expect("simulate the worker mark");

    // Run 2 (later): still ONE row per SIRET; last_seen_at/sync_run_id move, first_seen_at and
    // processed_at do not — so the row is pending again (processed_at < last_seen_at).
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    let run_2 = uuid::Uuid::new_v4();
    upsert_staging_row(&pool, &sample_record(), "37", run_2).await.expect("second upsert");

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM external_sirene_restaurants")
        .fetch_one(&pool)
        .await
        .expect("count rows");
    assert_eq!(rows, 1, "re-running the ingestion must not duplicate the SIRET");

    let (first_seen_2, last_seen_2, run_id_2, processed_2, pending): (
        DateTime<Utc>,
        DateTime<Utc>,
        uuid::Uuid,
        Option<DateTime<Utc>>,
        bool,
    ) = sqlx::query_as(
        "SELECT first_seen_at, last_seen_at, sync_run_id, processed_at, \
                (processed_at IS NULL OR processed_at < last_seen_at) AS pending \
         FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("re-staged row");
    assert_eq!(first_seen_2, first_seen, "first_seen_at is set once");
    assert!(last_seen_2 > last_seen, "a re-run bumps last_seen_at");
    assert_eq!(run_id_2, run_2, "the latest run stamps sync_run_id");
    // Since ADR-20260728-011344 (#226) re-seeing a record is NOT the same as it changing. The typed
    // payload is byte-identical, so the conflict arm carries `processed_at` forward and the row stays
    // NON-pending — that is the line that stopped ~200k unchanged établissements being re-translated
    // every Monday. `last_seen_at` still advances, because detect-by-absence depends on that freshness.
    assert!(processed_2.is_some(), "an unchanged record keeps its processed checkpoint");
    assert!(!pending, "re-seeing an UNCHANGED record must not re-pend it");
}

#[tokio::test]
async fn staging_batch_upsert_matches_row_by_row_semantics_and_is_idempotent() {
    let _suite = serialize_suite();
    let Some(url) = db_test_gate::database_url("staging_batch_upsert_matches_row_by_row_semantics_and_is_idempotent") else { return };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    // Run 1: a batch of TWO distinct SIRETs lands in one call — each row fresh (first_seen == last_seen,
    // processed_at NULL/pending), exactly as the single-row path stamps them.
    let batch = vec![record_with_siret("85242109900021"), record_with_siret("85242109900039")];
    let run_1 = uuid::Uuid::new_v4();
    let counts = upsert_staging_batch(&pool, &batch, "37", run_1).await;
    assert_eq!(counts.upserted, 2, "both rows upserted");
    assert_eq!(counts.failed_rows, 0, "no row failed");

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM external_sirene_restaurants")
        .fetch_one(&pool)
        .await
        .expect("count rows");
    assert_eq!(rows, 2, "a batch of two distinct SIRETs yields two rows");

    let (first_seen, last_seen, run_id, processed): (
        DateTime<Utc>,
        DateTime<Utc>,
        uuid::Uuid,
        Option<DateTime<Utc>>,
    ) = sqlx::query_as(
        "SELECT first_seen_at, last_seen_at, sync_run_id, processed_at \
         FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("staged row");
    assert_eq!(first_seen, last_seen, "a fresh batched row sets first_seen == last_seen");
    assert_eq!(run_id, run_1);
    assert_eq!(processed, None, "a fresh batched row is pending (processed_at NULL)");

    // Worker marks both rows processed (high-water mark = last_seen_at).
    sqlx::query("UPDATE external_sirene_restaurants SET processed_at = last_seen_at")
        .execute(&pool)
        .await
        .expect("simulate the worker mark");

    // Run 2 (later): re-batch the SAME two SIRETs — still two rows, first_seen frozen, last_seen and
    // sync_run_id refreshed, and the rows pending again — mirroring the single-row idempotency test.
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    let run_2 = uuid::Uuid::new_v4();
    let counts_2 = upsert_staging_batch(&pool, &batch, "37", run_2).await;
    assert_eq!(counts_2.upserted, 2);
    assert_eq!(counts_2.failed_rows, 0);

    let rows_2: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM external_sirene_restaurants")
        .fetch_one(&pool)
        .await
        .expect("count rows");
    assert_eq!(rows_2, 2, "re-batching must not duplicate SIRETs");

    let (first_seen_2, last_seen_2, run_id_2, processed_2, pending): (
        DateTime<Utc>,
        DateTime<Utc>,
        uuid::Uuid,
        Option<DateTime<Utc>>,
        bool,
    ) = sqlx::query_as(
        "SELECT first_seen_at, last_seen_at, sync_run_id, processed_at, \
                (processed_at IS NULL OR processed_at < last_seen_at) AS pending \
         FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("re-staged row");
    assert_eq!(first_seen_2, first_seen, "first_seen_at is set once, never bumped by a re-batch");
    assert!(last_seen_2 > last_seen, "a re-batch bumps last_seen_at");
    assert_eq!(run_id_2, run_2, "the latest run stamps sync_run_id");
    // Same contract as the row-by-row path above (#226): unchanged means non-pending.
    assert!(processed_2.is_some(), "an unchanged record keeps its processed checkpoint");
    assert!(!pending, "re-batching an UNCHANGED record must not re-pend it");
}

#[tokio::test]
async fn staging_batch_upsert_falls_back_to_row_by_row_on_batch_error() {
    let _suite = serialize_suite();
    let Some(url) = db_test_gate::database_url("staging_batch_upsert_falls_back_to_row_by_row_on_batch_error") else { return };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    // A duplicate SIRET within one chunk makes the multi-row statement fail (`ON CONFLICT DO UPDATE
    // cannot affect a row a second time`). The fallback retries the chunk row-by-row, where the second
    // occurrence simply updates the first — so both count as upserted and exactly one row lands. This is
    // the same resilience the single-row loop had (one bad record never sinks its neighbours).
    let batch = vec![record_with_siret("85242109900021"), record_with_siret("85242109900021")];
    let run = uuid::Uuid::new_v4();
    let counts = upsert_staging_batch(&pool, &batch, "37", run).await;
    assert_eq!(counts.upserted, 2, "the fallback processes both rows row-by-row");
    assert_eq!(counts.failed_rows, 0, "neither row fails individually");

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM external_sirene_restaurants")
        .fetch_one(&pool)
        .await
        .expect("count rows");
    assert_eq!(rows, 1, "the duplicate SIRET collapses to a single row via the fallback");
}

/// The steady-state property #231 exists for: once the worker has translated a row and dropped its
/// payload, re-seeing the SAME record must NOT write the payload back.
///
/// Without this, every weekly sweep would re-inflate the whole mirror to ~1.8 kB a row and the
/// compaction would be undone within days — the change would look done and buy nothing, which is the
/// failure mode worth a test rather than a comment.
#[tokio::test]
async fn an_unchanged_processed_row_does_not_get_its_payload_written_back() {
    let _suite = serialize_suite();
    let Some(url) = db_test_gate::database_url("an_unchanged_processed_row_does_not_get_its_payload_written_back") else { return };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    // Sweep 1 stages the record with its payload; the row is pending.
    upsert_staging_row(&pool, &sample_record(), "37", uuid::Uuid::new_v4()).await.expect("first upsert");
    let payload: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT payload FROM external_sirene_restaurants WHERE siret = '85242109900021'")
            .fetch_one(&pool)
            .await
            .expect("staged row");
    assert!(payload.is_some(), "a pending row carries the payload the worker will translate");

    // The worker translates it and drops the payload (what `mark_processed(.., true)` does).
    sqlx::query("UPDATE external_sirene_restaurants SET processed_at = last_seen_at, payload = NULL")
        .execute(&pool)
        .await
        .expect("simulate the worker's transient-payload mark");

    // Sweep 2 sees the identical record again.
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    upsert_staging_row(&pool, &sample_record(), "37", uuid::Uuid::new_v4()).await.expect("second upsert");

    let (payload_2, pending): (Option<serde_json::Value>, bool) = sqlx::query_as(
        "SELECT payload, (processed_at IS NULL OR processed_at < last_seen_at) AS pending \
         FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("re-staged row");
    assert!(payload_2.is_none(), "an unchanged, already-processed record must NOT get its payload back");
    assert!(!pending, "and it must not re-pend — the hash matched");
}

/// The other half: a record that ACTUALLY changed must get its payload back, or the worker would pend
/// a row with nothing to translate.
#[tokio::test]
async fn a_changed_record_gets_its_payload_written_again() {
    let _suite = serialize_suite();
    let Some(url) = db_test_gate::database_url("a_changed_record_gets_its_payload_written_again") else { return };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    upsert_staging_row(&pool, &sample_record(), "37", uuid::Uuid::new_v4()).await.expect("first upsert");
    sqlx::query("UPDATE external_sirene_restaurants SET processed_at = last_seen_at, payload = NULL")
        .execute(&pool)
        .await
        .expect("simulate the worker's transient-payload mark");

    // `record_with_siret` carries the same SIRET but a different record (no enseigne, no address
    // lines) — a real change to fields the ACL reads, so a different hash.
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    upsert_staging_row(&pool, &record_with_siret("85242109900021"), "37", uuid::Uuid::new_v4())
        .await
        .expect("changed upsert");

    let (payload, pending): (Option<serde_json::Value>, bool) = sqlx::query_as(
        "SELECT payload, (processed_at IS NULL OR processed_at < last_seen_at) AS pending \
         FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("re-staged row");
    assert!(payload.is_some(), "a CHANGED record must carry its payload — the worker has to translate it");
    assert!(pending, "and it must pend");
}

/// Quarantine must be self-healing, or a POISON row is a permanent leak that needs an operator.
///
/// A row is quarantined for failing to sync the record it currently holds. When INSEE sends a DIFFERENT
/// record, that verdict no longer applies — so the ordinary conflict arm, which already re-pends changed
/// rows, must also clear the quarantine. This is the whole recovery path, and it costs nothing extra:
/// `status` follows exactly the same predicate as `payload` and `processed_at`.
#[tokio::test]
async fn a_changed_record_releases_a_poisoned_row() {
    let _suite = serialize_suite();
    let Some(url) = db_test_gate::database_url("a_changed_record_releases_a_poisoned_row") else { return };
    let pool = PgPool::connect(&url).await.expect("connect Postgres");
    reset_schema(&pool).await;

    upsert_staging_row(&pool, &sample_record(), "37", uuid::Uuid::new_v4()).await.expect("first upsert");
    sqlx::query(
        "UPDATE external_sirene_restaurants \
            SET status = 'POISON', attempt_sync_retry_count = 10, processed_at = last_seen_at",
    )
    .execute(&pool)
    .await
    .expect("quarantine the row");

    // Re-seeing the SAME record must NOT release it — nothing has changed, so the failure still applies.
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    upsert_staging_row(&pool, &sample_record(), "37", uuid::Uuid::new_v4()).await.expect("unchanged upsert");
    let status: String =
        sqlx::query_scalar("SELECT status FROM external_sirene_restaurants WHERE siret = '85242109900021'")
            .fetch_one(&pool)
            .await
            .expect("row");
    assert_eq!(status, "POISON", "an identical record is not new information");

    // A CHANGED record is: it re-pends the row, and the quarantine goes with it.
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    upsert_staging_row(&pool, &record_with_siret("85242109900021"), "37", uuid::Uuid::new_v4())
        .await
        .expect("changed upsert");
    let (status, payload, pending): (String, Option<serde_json::Value>, bool) = sqlx::query_as(
        "SELECT status, payload, (processed_at IS NULL OR processed_at < last_seen_at) AS pending \
         FROM external_sirene_restaurants WHERE siret = '85242109900021'",
    )
    .fetch_one(&pool)
    .await
    .expect("row");
    assert_eq!(status, "PENDING", "a changed record releases the quarantine — no operator needed");
    assert!(payload.is_some(), "and brings back the payload the retry will translate");
    assert!(pending, "and the row is pending again");
}
