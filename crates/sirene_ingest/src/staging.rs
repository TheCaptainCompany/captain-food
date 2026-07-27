//! Raw UPSERT into the `external_sirene_restaurants` staging table (ADR-0045,
//! `specs/database/tables/integration_staging.yaml` / `migrations/20260718100000_…`).
//!
//! One row per SIRET, verbatim payload. The ingestion NEVER touches `processed_at` (the worker's
//! high-water mark) or `first_seen_at`; it bumps `last_seen_at`/`sync_run_id` and refreshes
//! `payload`/`etat`/`naf`/`department`, which makes the row pending again
//! (`processed_at < last_seen_at ⇒ pending`) for the on-app `sync_sirene_worker`.

use crate::client::{SireneError, SireneRecord};

/// Chunk size for the batched staging UPSERT. Each fetched INSEE page (up to `MAX_PAGE_SIZE` = 1000
/// rows) is written in chunks of this many rows in ONE multi-row `INSERT … SELECT … UNNEST` round-trip
/// instead of one round-trip per row — the single change that turns a ~40-hour France sweep into one
/// that fits the 90-minute CI budget (issue #215: at ~260-280 ms per single-row round-trip the DB write
/// was ~99% of wall-clock). 500 keeps each statement's bind arrays and parameter payload bounded.
pub const STAGING_BATCH_SIZE: usize = 500;

/// Counts from one staging write (a batch or its row-by-row fallback): rows successfully UPSERTed vs
/// rows that failed even individually — same meaning as the runner's `upserted`/`failed_rows` tallies.
#[derive(Debug, Default, Clone, Copy)]
pub struct StagingCounts {
    pub upserted: usize,
    pub failed_rows: usize,
}

/// UPSERT a chunk of fetched établissements in ONE multi-row round-trip, preserving
/// [`upsert_staging_row`]'s semantics EXACTLY: the INSERT arm stamps `first_seen_at = now()`,
/// `last_seen_at = now()`, `processed_at = NULL`; the `ON CONFLICT (siret) DO UPDATE` refreshes
/// `payload`/`etat`/`naf`/`department`/`last_seen_at`/`sync_run_id` and NEVER touches `first_seen_at`
/// or `processed_at` (the worker's `processed_at < last_seen_at ⇒ pending` high-water mark).
///
/// Resilience: a single multi-row statement fails as a whole on one bad row (or an intra-page duplicate
/// SIRET — `ON CONFLICT DO UPDATE cannot affect a row twice`), which would lose today's per-row
/// isolation. So on ANY batch error we FALL BACK to row-by-row via [`upsert_staging_row`], counting
/// exactly as the runner did before — the batch is a fast path, never a correctness change.
///
/// The verbatim JSON payload is passed as `text[]` and cast to `jsonb` in the statement (identical
/// stored value, and text arrays bind unambiguously) rather than as a `jsonb[]` bind array.
pub async fn upsert_staging_batch(
    pool: &sqlx::PgPool,
    records: &[SireneRecord],
    department: &str,
    sync_run_id: uuid::Uuid,
) -> StagingCounts {
    if records.is_empty() {
        return StagingCounts::default();
    }

    let mut sirets: Vec<String> = Vec::with_capacity(records.len());
    let mut payloads: Vec<String> = Vec::with_capacity(records.len());
    let mut etats: Vec<String> = Vec::with_capacity(records.len());
    let mut nafs: Vec<String> = Vec::with_capacity(records.len());
    let mut departments: Vec<String> = Vec::with_capacity(records.len());
    for record in records {
        // Same field extraction and defaulting as `upsert_staging_row`.
        sirets.push(record.etablissement.siret.trim().to_string());
        payloads.push(record.raw.to_string());
        etats.push(record.etablissement.etat().unwrap_or("A").to_string());
        nafs.push(record.etablissement.naf().unwrap_or("").to_string());
        departments.push(department.to_string());
    }

    let result = sqlx::query(
        "INSERT INTO external_sirene_restaurants \
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, processed_at) \
         SELECT t.siret, t.payload::jsonb, t.etat, t.naf, t.department, now(), now(), $6, NULL \
         FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[], $5::text[]) \
              AS t(siret, payload, etat, naf, department) \
         ON CONFLICT (siret) DO UPDATE SET \
           payload = EXCLUDED.payload, \
           etat = EXCLUDED.etat, \
           naf = EXCLUDED.naf, \
           department = EXCLUDED.department, \
           last_seen_at = EXCLUDED.last_seen_at, \
           sync_run_id = EXCLUDED.sync_run_id",
    )
    .bind(&sirets)
    .bind(&payloads)
    .bind(&etats)
    .bind(&nafs)
    .bind(&departments)
    .bind(sync_run_id)
    .execute(pool)
    .await;

    match result {
        Ok(_) => StagingCounts { upserted: records.len(), failed_rows: 0 },
        Err(e) => {
            // One bad row (or a duplicate SIRET within the chunk) must not sink the whole batch: fall
            // back to today's row-by-row path so partial-failure resilience is preserved exactly.
            eprintln!(
                "sirene_ingest: batch upsert of {} rows failed ({e}); falling back to row-by-row",
                records.len()
            );
            let mut counts = StagingCounts::default();
            for record in records {
                match upsert_staging_row(pool, record, department, sync_run_id).await {
                    Ok(()) => counts.upserted += 1,
                    Err(e) => {
                        counts.failed_rows += 1;
                        eprintln!("sirene_ingest: {e}");
                    }
                }
            }
            counts
        }
    }
}

/// UPSERT one fetched établissement into the staging table. `department` is the partition the sweep
/// queried (commune codes are prefixed by it), stamped for worker batching and re-partitioned sweeps;
/// `sync_run_id` correlates every row one ingestion run touched. Retained as the row-by-row fallback
/// path for [`upsert_staging_batch`].
pub async fn upsert_staging_row(
    pool: &sqlx::PgPool,
    record: &SireneRecord,
    department: &str,
    sync_run_id: uuid::Uuid,
) -> Result<(), SireneError> {
    let siret = record.etablissement.siret.trim();
    // The query is active-only, so a missing periode état defaults to 'A'; NAF has no meaningful
    // default (the column is NOT NULL) — an empty string marks "not stated".
    let etat = record.etablissement.etat().unwrap_or("A");
    let naf = record.etablissement.naf().unwrap_or("");
    sqlx::query(
        "INSERT INTO external_sirene_restaurants \
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, processed_at) \
         VALUES ($1, $2, $3, $4, $5, now(), now(), $6, NULL) \
         ON CONFLICT (siret) DO UPDATE SET \
           payload = EXCLUDED.payload, \
           etat = EXCLUDED.etat, \
           naf = EXCLUDED.naf, \
           department = EXCLUDED.department, \
           last_seen_at = EXCLUDED.last_seen_at, \
           sync_run_id = EXCLUDED.sync_run_id",
    )
    .bind(siret)
    .bind(&record.raw)
    .bind(etat)
    .bind(naf)
    .bind(department)
    .bind(sync_run_id)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| SireneError(format!("staging upsert for siret {siret}: {e}")))
}
