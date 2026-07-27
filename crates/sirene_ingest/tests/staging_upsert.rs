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

/// Fresh copy of the staging table (mirrors migrations/20260718100000_external_sirene_restaurants.sql).
async fn reset_schema(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        DROP TABLE IF EXISTS external_sirene_restaurants CASCADE;
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
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP staging_upsert_is_idempotent_per_siret_and_bumps_last_seen_at: DATABASE_URL not set");
        return;
    };
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
    assert!(processed_2.is_some(), "the ingestion never touches the worker's processed_at");
    assert!(pending, "a refreshed row is pending again for the worker");
}

#[tokio::test]
async fn staging_batch_upsert_matches_row_by_row_semantics_and_is_idempotent() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP staging_batch_upsert_matches_row_by_row_semantics_and_is_idempotent: DATABASE_URL not set");
        return;
    };
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
    assert!(processed_2.is_some(), "the batched ingestion never touches the worker's processed_at");
    assert!(pending, "a re-batched row is pending again for the worker");
}

#[tokio::test]
async fn staging_batch_upsert_falls_back_to_row_by_row_on_batch_error() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("SKIP staging_batch_upsert_falls_back_to_row_by_row_on_batch_error: DATABASE_URL not set");
        return;
    };
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
