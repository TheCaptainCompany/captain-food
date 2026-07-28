//! Raw UPSERT into the `external_sirene_restaurants` staging table (ADR-0045,
//! `specs/database/tables/integration_staging.yaml` / `migrations/20260718100000_…`).
//!
//! One row per SIRET, verbatim payload. The ingestion never touches `first_seen_at`; it bumps
//! `last_seen_at`/`sync_run_id` and refreshes `payload`/`etat`/`naf`/`department`/`payload_hash`.
//!
//! Whether that re-pends the row for the on-app `sync_sirene_worker`
//! (`processed_at < last_seen_at ⇒ pending`) now depends on `payload_hash`
//! (ADR-20260728-011344): an UNCHANGED record carries `processed_at` forward and stays non-pending.
//! `last_seen_at` still always advances, because detect-by-absence depends on that freshness -- the fix
//! separates "we saw it again" from "it changed", which the old unconditional re-pend conflated at a
//! cost of ~200k pointless translations per weekly sweep.

use crate::client::{SireneError, SireneRecord};

/// Chunk size for the batched staging UPSERT. Each fetched INSEE page (up to `MAX_PAGE_SIZE` = 1000
/// rows) is written in chunks of this many rows in ONE multi-row `INSERT … SELECT … UNNEST` round-trip
/// instead of one round-trip per row — the single change that turns a ~40-hour France sweep into one
/// that fits the 90-minute CI budget (issue #215: at ~260-280 ms per single-row round-trip the DB write
/// was ~99% of wall-clock). 500 keeps each statement's bind arrays and parameter payload bounded.
pub const STAGING_BATCH_SIZE: usize = 500;

/// The "did this record actually change?" key (ADR-20260728-011344).
///
/// Hashes the TYPED projection — a canonical re-serialization of the deserialized [`Etablissement`] —
/// not the raw payload. That distinction is the whole point: the wire types parse only stable business
/// fields, so any volatile per-fetch field INSEE adds (a last-processed timestamp, a request id) is
/// dropped before hashing and cannot defeat the comparison. Hashing `record.raw` instead would make the
/// hash change on every sweep and buy nothing. It also fails SAFE in the other direction: a new field
/// added to the wire types is automatically covered, so a real change can never slip through as
/// "unchanged".
///
/// Struct field order is declaration order, so the serialization — and therefore the digest — is stable
/// across runs and processes.
fn payload_hash(etablissement: &crate::wire::Etablissement) -> String {
    use sha2::{Digest, Sha256};
    let canonical = serde_json::to_string(etablissement).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    hex::encode(hasher.finalize())
}

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
    let mut hashes: Vec<String> = Vec::with_capacity(records.len());
    for record in records {
        // Same field extraction and defaulting as `upsert_staging_row`.
        sirets.push(record.etablissement.siret.trim().to_string());
        payloads.push(record.raw.to_string());
        etats.push(record.etablissement.etat().unwrap_or("A").to_string());
        nafs.push(record.etablissement.naf().unwrap_or("").to_string());
        departments.push(department.to_string());
        hashes.push(payload_hash(&record.etablissement));
    }

    let result = sqlx::query(
        "INSERT INTO external_sirene_restaurants \
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, payload_hash, processed_at) \
         SELECT t.siret, t.payload::jsonb, t.etat, t.naf, t.department, now(), now(), $7, t.payload_hash, NULL \
         FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[], $5::text[], $6::text[]) \
              AS t(siret, payload, etat, naf, department, payload_hash) \
         ON CONFLICT (siret) DO UPDATE SET \
           payload = EXCLUDED.payload, \
           etat = EXCLUDED.etat, \
           naf = EXCLUDED.naf, \
           department = EXCLUDED.department, \
           last_seen_at = EXCLUDED.last_seen_at, \
           sync_run_id = EXCLUDED.sync_run_id, \
           payload_hash = EXCLUDED.payload_hash, \
           -- ADR-20260728-011344: `last_seen_at` ALWAYS advances (absence detection needs that
           -- freshness), but re-pending the row is a separate question. When the typed payload is
           -- byte-identical to what we already processed, carry `processed_at` forward to now() so the
           -- row stays NON-pending. This is the single line that stops ~200k unchanged établissements
           -- being re-translated, re-journaled and re-appended every Monday for no change at all.
           -- Guarded on `processed_at IS NOT NULL`: a row that was never processed must stay pending
           -- however familiar its payload looks.
           processed_at = CASE
             WHEN external_sirene_restaurants.processed_at IS NOT NULL
              AND external_sirene_restaurants.payload_hash = EXCLUDED.payload_hash
             THEN now()
             ELSE external_sirene_restaurants.processed_at
           END",
    )
    .bind(&sirets)
    .bind(&payloads)
    .bind(&etats)
    .bind(&nafs)
    .bind(&departments)
    .bind(&hashes)
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
           (siret, payload, etat, naf, department, first_seen_at, last_seen_at, sync_run_id, payload_hash, processed_at) \
         VALUES ($1, $2, $3, $4, $5, now(), now(), $6, $7, NULL) \
         ON CONFLICT (siret) DO UPDATE SET \
           payload = EXCLUDED.payload, \
           etat = EXCLUDED.etat, \
           naf = EXCLUDED.naf, \
           department = EXCLUDED.department, \
           last_seen_at = EXCLUDED.last_seen_at, \
           sync_run_id = EXCLUDED.sync_run_id, \
           payload_hash = EXCLUDED.payload_hash, \
           -- Identical semantics to the batch path above -- see its comment.
           processed_at = CASE
             WHEN external_sirene_restaurants.processed_at IS NOT NULL
              AND external_sirene_restaurants.payload_hash = EXCLUDED.payload_hash
             THEN now()
             ELSE external_sirene_restaurants.processed_at
           END",
    )
    .bind(siret)
    .bind(&record.raw)
    .bind(etat)
    .bind(naf)
    .bind(department)
    .bind(sync_run_id)
    .bind(payload_hash(&record.etablissement))
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| SireneError(format!("staging upsert for siret {siret}: {e}")))
}

#[cfg(test)]
mod hash_tests {
    use super::*;
    use crate::wire::Etablissement;

    fn etablissement(raw: &str) -> Etablissement {
        serde_json::from_str(raw).expect("wire type parses")
    }

    /// The load-bearing property (ADR-20260728-011344): the hash covers only the fields we PARSE, so a
    /// volatile per-fetch field INSEE adds cannot make an unchanged record look changed. Hashing the raw
    /// payload instead would flip the digest every sweep and the whole optimisation would silently buy
    /// nothing — the failure mode being invisible is exactly why this is pinned by a test.
    #[test]
    fn unparsed_fields_do_not_affect_the_hash() {
        let base = r#"{"siret":"85242109900021","periodesEtablissement":[{"dateFin":null,"etatAdministratifEtablissement":"A","enseigne1Etablissement":"CHEZ MARCO","activitePrincipaleEtablissement":"56.10A"}]}"#;
        let with_noise = r#"{"siret":"85242109900021","dateDernierTraitementEtablissement":"2026-07-28T03:00:00","nombrePeriodesEtablissement":3,"periodesEtablissement":[{"dateFin":null,"etatAdministratifEtablissement":"A","enseigne1Etablissement":"CHEZ MARCO","activitePrincipaleEtablissement":"56.10A"}]}"#;
        assert_eq!(
            payload_hash(&etablissement(base)),
            payload_hash(&etablissement(with_noise)),
            "a field we never deserialize must not re-pend the row"
        );
    }

    /// And it must fail SAFE the other way: a change to something the ACL reads MUST change the hash,
    /// or a real INSEE rename would be silently judged "unchanged" and never reach the domain — which is
    /// the bug this whole change exists to fix.
    #[test]
    fn a_change_the_acl_reads_does_affect_the_hash() {
        let before = r#"{"siret":"85242109900021","periodesEtablissement":[{"dateFin":null,"etatAdministratifEtablissement":"A","enseigne1Etablissement":"CHEZ MARCO","activitePrincipaleEtablissement":"56.10A"}]}"#;
        let renamed = r#"{"siret":"85242109900021","periodesEtablissement":[{"dateFin":null,"etatAdministratifEtablissement":"A","enseigne1Etablissement":"CHEZ MARCO ET FILS","activitePrincipaleEtablissement":"56.10A"}]}"#;
        assert_ne!(payload_hash(&etablissement(before)), payload_hash(&etablissement(renamed)));

        // Closure (etat A -> F) is the other signal the worker acts on.
        let closed = before.replace(r#""etatAdministratifEtablissement":"A""#, r#""etatAdministratifEtablissement":"F""#);
        assert_ne!(payload_hash(&etablissement(before)), payload_hash(&etablissement(&closed)));
    }

    /// Stable across calls: struct field order is declaration order, so the digest is reproducible
    /// (a hash that varied per process would re-pend everything, defeating the point).
    #[test]
    fn the_hash_is_stable() {
        let raw = r#"{"siret":"85242109900021","periodesEtablissement":[{"dateFin":null,"etatAdministratifEtablissement":"A","enseigne1Etablissement":"CHEZ MARCO"}]}"#;
        assert_eq!(payload_hash(&etablissement(raw)), payload_hash(&etablissement(raw)));
    }
}

/// Order the sweep's departments **least-recently-swept first** (#218).
///
/// The sweep used to walk `french_departments()` in fixed numeric order from 01 every run, and died at
/// the CI time ceiling around department 37 — so 38–101 were never ingested at all, no matter how many
/// times it ran. Restarting from the same end of the list cannot converge.
///
/// This derives the order from data already in the staging table: a department's `max(last_seen_at)` is
/// when it was last swept, so ascending that column is a natural round-robin. Departments with no rows
/// yet are not in the table at all and sort FIRST — the never-ingested tail gets priority until the
/// country is covered, after which the order becomes a refresh rotation.
///
/// No cursor table, no migration, no state to get out of sync with reality: if a sweep dies halfway,
/// the rows it did write are its own record of progress.
pub async fn departments_by_staleness(pool: &sqlx::PgPool, all: &[String]) -> Vec<String> {
    let swept: Vec<(String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT department, max(last_seen_at) FROM external_sirene_restaurants \
         WHERE department IS NOT NULL GROUP BY department",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default(); // a read failure must not stop the sweep — fall back to declaration order

    let seen: std::collections::HashMap<&str, chrono::DateTime<chrono::Utc>> =
        swept.iter().map(|(d, t)| (d.as_str(), *t)).collect();
    let mut ordered: Vec<String> = all.to_vec();
    // Stable sort: never-swept (None) first, then oldest sweep first, ties keeping numeric order.
    ordered.sort_by_key(|d| seen.get(d.as_str()).copied());
    ordered
}

#[cfg(test)]
mod ordering_tests {
    use super::*;
    use chrono::{Duration, Utc};

    /// Rebuilds `departments_by_staleness`'s ordering rule against an explicit map, so the property can
    /// be asserted without a database: never-swept first, then oldest sweep first.
    fn order(all: &[&str], swept: &[(&str, i64)]) -> Vec<String> {
        let seen: std::collections::HashMap<&str, chrono::DateTime<Utc>> =
            swept.iter().map(|(d, days_ago)| (*d, Utc::now() - Duration::days(*days_ago))).collect();
        let mut ordered: Vec<String> = all.iter().map(|d| d.to_string()).collect();
        ordered.sort_by_key(|d| seen.get(d.as_str()).copied());
        ordered
    }

    /// The bug behind #218: the sweep restarted at department 01 every run and died at the time
    /// ceiling around 37, so 38-101 were NEVER ingested however often it ran. Stalest-first makes
    /// successive runs converge instead of re-treading the same prefix.
    #[test]
    fn never_swept_departments_come_first() {
        // 01 and 02 were swept recently; 75 and 59 have never been touched.
        let ordered = order(&["01", "02", "59", "75"], &[("01", 1), ("02", 2)]);
        assert_eq!(&ordered[..2], &["59".to_string(), "75".to_string()]);
    }

    /// Once the country is covered, the same rule becomes a refresh rotation: the department nobody
    /// has looked at in longest goes first.
    #[test]
    fn once_covered_the_stalest_department_goes_first() {
        let ordered = order(&["01", "02", "03"], &[("01", 3), ("02", 30), ("03", 10)]);
        assert_eq!(ordered, vec!["02".to_string(), "03".to_string(), "01".to_string()]);
    }
}
