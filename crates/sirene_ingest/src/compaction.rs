//! One-shot compaction of the SIRENE mirror (ADR-20260728-143000, issue #231).
//!
//! [`crate::staging`] stops WRITING payloads for unchanged records, and the on-app worker NULLs them on
//! successful translation — but neither touches the 339k rows (655 MB) already sitting in the table.
//! This pass does, in bounded batches.
//!
//! # Why the hash must be recomputed BEFORE the payload is dropped
//!
//! Every existing row carries `payload_hash = 'unhashed-pre-20260728'`, the deliberate matches-nothing
//! sentinel from the migration that added the column. Nulling payloads while that sentinel stands would
//! make the next sweep see a hash mismatch on ALL 339k rows, re-pend every one of them and re-write all
//! 655 MB — the exact opposite of the intent. So the order is, per row:
//!
//! ```text
//! parse payload -> compute the real hash -> store the hash -> null the payload
//! ```
//!
//! After that, the next sweep hashes the same record, matches, and writes a ~200-byte row version.
//!
//! # Why it keeps unparsable payloads, and what that does NOT cover
//!
//! A payload that does not deserialize into [`crate::wire::Etablissement`] KEEPS its payload and is left
//! entirely alone (it has no computable hash anyway). That payload is the only evidence of why the
//! record was unusable, and discarding it is how a systematic mapping bug hides.
//!
//! Be precise about the limit, though. This crate deliberately contains no domain code (ADR-0045), so it
//! runs the wire types but NOT the ACL. It therefore cannot recognise a row that parses fine yet is
//! **ACL-unmappable** (no usable name, no postal code) — those lose their payload here. Going forward
//! the on-app worker keeps them, because the worker HAS the ACL; it is only this historical pass that
//! cannot. Roughly 5.4k rows historically. Running the compaction server-side instead would close that
//! gap, and was the recommendation; running it in CI was the product owner's call (#231).
//!
//! # Disk, honestly
//!
//! A plain `VACUUM` makes the freed space reusable by this table — it does NOT return it to the OS, so
//! `pg_total_relation_size` stays ~655 MB. That is still the win that matters: departments 38–101 fill
//! that free space instead of extending the file, so full France fits in what department 1–37 already
//! costs. Returning the disk needs a `VACUUM FULL`, which becomes affordable only AFTER this pass —
//! it rewrites into free space equal to the LIVE data (~90 MB once compacted, against the ~620 MB that
//! made the earlier attempt fail with `No space left on device`).

use crate::staging::{payload_hash, status};
use crate::wire::Etablissement;

/// Rows examined per batch. Each batch is one SELECT + up to two multi-row UPDATEs, so the round-trip
/// count is bounded by `rows / BATCH` rather than by rows — the same lesson as #215/#216, where
/// per-row round-trips were ~99% of wall-clock.
pub const COMPACTION_BATCH_SIZE: i64 = 2_000;

/// `VACUUM` after this many batches, not after every one (D4 asks for interleaved vacuuming, not for
/// maximal vacuuming). Each `VACUUM` scans the whole table, so running it per batch would make the pass
/// quadratic in table size for no benefit; every ~50k rows keeps dead tuples reusable while the pass is
/// still running, which is the point — the batches that follow reuse the space the batches before freed.
pub const VACUUM_EVERY_BATCHES: usize = 25;

/// What compaction decided to do with ONE row. Extracted as a pure function ([`classify`]) because it
/// carries the entire correctness argument of this pass — hash before drop, keep the evidence — and the
/// database integration tests do not run without a live Postgres. A rule this load-bearing must be
/// asserted somewhere that always executes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowAction {
    /// Translated already: store the real hash AND drop the payload. The disk win.
    Compact { hash: String },
    /// Still pending: store the real hash, KEEP the payload — the worker has yet to translate it.
    RehashOnly { hash: String },
    /// Does not deserialize: touch nothing. No computable hash, and the payload is the evidence.
    LeaveAsEvidence,
}

/// Decide what a row earns, from its payload and whether the worker is done with it.
///
/// `done` is `processed_at IS NOT NULL AND processed_at >= last_seen_at` — the negation of the worker's
/// own pending predicate, so the two can never disagree about which rows still need their payload.
pub fn classify(payload: &serde_json::Value, done: bool) -> RowAction {
    let Ok(etablissement) = serde_json::from_value::<Etablissement>(payload.clone()) else {
        return RowAction::LeaveAsEvidence;
    };
    let hash = payload_hash(&etablissement);
    if done {
        RowAction::Compact { hash }
    } else {
        RowAction::RehashOnly { hash }
    }
}

/// What one compaction run did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CompactionCounts {
    /// Rows examined (had a payload when selected).
    pub examined: u64,
    /// Processed rows: real hash stored, payload dropped. This is the disk win.
    pub compacted: u64,
    /// Pending rows: real hash stored, payload KEPT — the worker still has to translate them.
    pub rehashed_pending: u64,
    /// Payloads that do not deserialize: left completely untouched, payload retained as evidence.
    pub unparsable: u64,
}

/// Compact every row that still carries a payload, in batches, until the table is done or `budget` is
/// spent. Resumable by construction: the selection predicate is `payload IS NOT NULL`, so a compacted
/// row simply stops being selected and a killed run costs nothing but the batch it was in.
///
/// `budget` bounds wall-clock so this can share the ingestion job's CI window. It is checked BETWEEN
/// batches, never inside one, so the pass never leaves a batch half-applied.
pub async fn compact_payloads(
    pool: &sqlx::PgPool,
    budget: std::time::Duration,
) -> Result<CompactionCounts, sqlx::Error> {
    let started = std::time::Instant::now();
    let mut counts = CompactionCounts::default();
    // Keyset cursor. It only has to skip the rows this run KEEPS (unparsable + pending); compacted rows
    // drop out of the predicate on their own.
    let mut after = String::new();
    let mut batches = 0usize;

    loop {
        if started.elapsed() >= budget {
            println!(
                "sirene_ingest: compaction budget of {}s reached — stopping cleanly, the next run \
                 resumes where the predicate leaves off",
                budget.as_secs()
            );
            break;
        }

        let rows: Vec<(String, serde_json::Value, bool)> = sqlx::query_as(
            "SELECT siret, payload, (processed_at IS NOT NULL AND processed_at >= last_seen_at) AS done \
               FROM external_sirene_restaurants \
              WHERE payload IS NOT NULL AND siret > $1 \
              ORDER BY siret LIMIT $2",
        )
        .bind(&after)
        .bind(COMPACTION_BATCH_SIZE)
        .fetch_all(pool)
        .await?;
        if rows.is_empty() {
            break;
        }

        // Split the batch by what each row earns: a processed row loses its payload, a pending row only
        // gains a real hash, an unparsable row is not touched at all.
        let mut compact_sirets: Vec<String> = Vec::new();
        let mut compact_hashes: Vec<String> = Vec::new();
        let mut rehash_sirets: Vec<String> = Vec::new();
        let mut rehash_hashes: Vec<String> = Vec::new();
        let mut unmappable_sirets: Vec<String> = Vec::new();
        for (siret, payload, done) in &rows {
            counts.examined += 1;
            after = siret.clone();
            match classify(payload, *done) {
                RowAction::Compact { hash } => {
                    compact_sirets.push(siret.clone());
                    compact_hashes.push(hash);
                }
                RowAction::RehashOnly { hash } => {
                    rehash_sirets.push(siret.clone());
                    rehash_hashes.push(hash);
                }
                RowAction::LeaveAsEvidence => {
                    counts.unparsable += 1;
                    unmappable_sirets.push(siret.clone());
                }
            }
        }

        if !compact_sirets.is_empty() {
            // Hash and payload move in ONE statement so a row can never be left hashed-but-unpayloaded
            // or payloaded-but-unhashed by a crash between two writes.
            sqlx::query(
                // `synced_at` is backfilled from `processed_at` rather than stamped now(): these rows
                // were synced in the PAST and pretending otherwise would make every historical row look
                // freshly translated. For them `processed_at` IS the `last_seen_at` at which the worker
                // drained the row, and the hash-match carry-forward cannot have moved it, because their
                // hash is still the sentinel. COALESCE keeps an existing value if one is somehow there.
                "UPDATE external_sirene_restaurants s \
                    SET payload_hash = t.hash, payload = NULL, status = $3, \
                        synced_at = COALESCE(s.synced_at, s.processed_at) \
                   FROM UNNEST($1::text[], $2::text[]) AS t(siret, hash) \
                  WHERE s.siret = t.siret",
            )
            .bind(&compact_sirets)
            .bind(&compact_hashes)
            .bind(status::SYNCED)
            .execute(pool)
            .await?;
            counts.compacted += compact_sirets.len() as u64;
        }

        if !rehash_sirets.is_empty() {
            // Pending rows keep their payload (the worker has not translated them yet) but still get the
            // real hash: the worker seeds its inbound `external_id` with it, and a sentinel there would
            // make the dedupe key meaningless.
            sqlx::query(
                "UPDATE external_sirene_restaurants s \
                    SET payload_hash = t.hash \
                   FROM UNNEST($1::text[], $2::text[]) AS t(siret, hash) \
                  WHERE s.siret = t.siret",
            )
            .bind(&rehash_sirets)
            .bind(&rehash_hashes)
            .execute(pool)
            .await?;
            counts.rehashed_pending += rehash_sirets.len() as u64;
        }

        if !unmappable_sirets.is_empty() {
            // The payload stays (it is the evidence), but the status must say WHY it is still there —
            // otherwise it is indistinguishable from a row still waiting to be translated, which is the
            // exact ambiguity `status` exists to remove. The hash is deliberately NOT written: there
            // isn't one, and a sentinel left in place is honest about that.
            sqlx::query(
                "UPDATE external_sirene_restaurants SET status = $2 WHERE siret = ANY($1::text[])",
            )
            .bind(&unmappable_sirets)
            .bind(status::UNMAPPABLE)
            .execute(pool)
            .await?;
        }

        batches += 1;
        if batches % VACUUM_EVERY_BATCHES == 0 {
            // Autocommit (no surrounding transaction), which VACUUM requires. Best-effort: a role
            // without permission must not fail the pass — autovacuum will catch up, just later.
            if let Err(e) = sqlx::query("VACUUM external_sirene_restaurants").execute(pool).await {
                eprintln!("sirene_ingest: compaction VACUUM skipped ({e})");
            }
            println!("sirene_ingest: compaction progress — {counts:?}");
        }
    }

    // A final VACUUM so the last batches' dead tuples are reusable before the run ends.
    if let Err(e) = sqlx::query("VACUUM external_sirene_restaurants").execute(pool).await {
        eprintln!("sirene_ingest: final compaction VACUUM skipped ({e})");
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(raw: &str) -> serde_json::Value {
        serde_json::from_str(raw).expect("test payload parses as JSON")
    }

    const REAL: &str = r#"{"siret":"85242109900021","periodesEtablissement":[{"dateFin":null,"etatAdministratifEtablissement":"A","enseigne1Etablissement":"CHEZ MARCO","activitePrincipaleEtablissement":"56.10A"}]}"#;

    /// THE property of this whole pass (#231). Every existing row carries the sentinel
    /// `payload_hash = 'unhashed-pre-20260728'`. If a payload is dropped while that sentinel stands,
    /// the next sweep sees a mismatch on all 339k rows, re-pends every one and re-writes all 655 MB --
    /// the exact opposite of the intent. So a compacted row MUST come with a real hash to store.
    #[test]
    fn a_processed_row_yields_a_real_hash_to_store_before_its_payload_is_dropped() {
        let action = classify(&payload(REAL), true);
        let RowAction::Compact { hash } = action else {
            panic!("a processed, parsable row must be compacted, got {action:?}");
        };
        assert_ne!(hash, "unhashed-pre-20260728", "the sentinel must be replaced, never carried forward");
        assert_eq!(hash.len(), 64, "SHA-256 as hex");
        // And it must be the SAME digest the ingestion will compute next sweep, or the row re-pends
        // anyway and the compaction bought nothing.
        let etablissement = serde_json::from_value(payload(REAL)).expect("wire type parses");
        assert_eq!(hash, payload_hash(&etablissement));
    }

    /// A row the worker has NOT translated yet still needs its payload — dropping it would leave the
    /// worker with nothing to translate and the record would never reach the domain.
    #[test]
    fn a_pending_row_keeps_its_payload_and_only_gains_a_hash() {
        assert!(matches!(classify(&payload(REAL), false), RowAction::RehashOnly { .. }));
    }

    /// D3: an unparsable payload is the only evidence of why INSEE's record was unusable. It has no
    /// computable hash either, so the row is left exactly as it is rather than half-updated.
    #[test]
    fn an_unparsable_payload_is_left_untouched_as_evidence() {
        // `siret` is required by the wire type, so a record without it cannot be deserialized.
        let junk = payload(r#"{"periodesEtablissement":[],"note":"INSEE sent something unexpected"}"#);
        assert_eq!(classify(&junk, true), RowAction::LeaveAsEvidence);
        assert_eq!(classify(&junk, false), RowAction::LeaveAsEvidence);
    }

    /// The compaction and the ingestion must agree on what "unchanged" means, or compacted rows re-pend
    /// on the very next sweep. Both go through [`payload_hash`] over the TYPED projection, so a field
    /// INSEE adds that we never deserialize cannot make a compacted row look changed.
    #[test]
    fn compaction_agrees_with_the_ingestion_on_unchanged_records() {
        let with_noise = payload(
            r#"{"siret":"85242109900021","dateDernierTraitementEtablissement":"2026-07-28T03:00:00","periodesEtablissement":[{"dateFin":null,"etatAdministratifEtablissement":"A","enseigne1Etablissement":"CHEZ MARCO","activitePrincipaleEtablissement":"56.10A"}]}"#,
        );
        let (RowAction::Compact { hash: a }, RowAction::Compact { hash: b }) =
            (classify(&payload(REAL), true), classify(&with_noise, true))
        else {
            panic!("both records are processed and parsable");
        };
        assert_eq!(a, b, "an unparsed INSEE field must not make a compacted row re-pend");
    }
}
