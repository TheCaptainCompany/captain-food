//! Drop the payloads of rows whose sync is CONFIRMED (ADR-20260728-143000, issues #231/#238).
//!
//! # The rule: evidence before removal
//!
//! A payload is removed only from a row that positively records having reached the domain --
//! `status = 'SYNCED'` AND `synced_at IS NOT NULL`. Nothing else is touched, and this pass never decides
//! for itself that a row was synced.
//!
//! That rule replaces an earlier version of this pass, which inferred "already translated" from
//! `processed_at >= last_seen_at`, wrote `SYNCED` on the strength of that inference, and then dropped the
//! payload. The inference was wrong: `processed_at` is a CHECKPOINT, not a verdict. The worker advances
//! it for an unmappable row, for one whose write failed, and for one it has merely handed to the inbox --
//! and the ingestion advances it again on every unchanged row it re-sees. So the pass derived certainty
//! from a column that never carried any, and destroyed the only copy of the record on the strength of it.
//! `status` and `synced_at` exist precisely so that certainty is RECORDED rather than guessed.
//!
//! # What this means for rows that predate the status column
//!
//! They are NOT compacted here, deliberately. They carry `status = 'PENDING'` (the migration default) and
//! the `unhashed-pre-20260728` hash sentinel, so the next sweep re-pends each of them exactly once --
//! which is what migration `20260728040000` already documented as its cost -- the worker translates them,
//! the aggregate's verdict comes back, and the payload is dropped THERE, with the sync confirmed.
//! Reclamation runs through the ordinary path rather than around it.
//!
//! Two consequences worth stating plainly, because they change WHEN disk comes back:
//!
//! - Historical reclamation now depends on the sweep resuming. This pass alone will not shrink a mirror
//!   full of pre-#231 rows, and it says so in its summary rather than reporting a quiet success.
//! - The ACL gap that came with running compaction in CI is GONE. This pass no longer classifies
//!   anything, so it no longer needs the ACL it does not have, and no unmappable row can lose its
//!   evidence to a crate that cannot recognise one.
//!
//! # Why the pass still exists
//!
//! As a backstop. The worker drops a confirmed payload in the SAME statement that records the
//! confirmation, so the two cannot normally drift -- but a row holding both `SYNCED` and a payload is
//! exactly what should be swept up, and doing that in bounded batches with `VACUUM` interleaved is the
//! only shape that fits the disk this table has left.

/// Rows updated per batch. One round-trip each rather than one per row -- the #215/#216 lesson.
pub const COMPACTION_BATCH_SIZE: i64 = 2_000;

/// `VACUUM` after this many batches. Each one scans the whole table, so per-batch vacuuming would make
/// the pass quadratic in table size for no benefit; this keeps dead tuples reusable by the batches that
/// follow, which is the point.
pub const VACUUM_EVERY_BATCHES: usize = 25;

/// What one compaction run did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CompactionCounts {
    /// Payloads dropped from rows confirmed SYNCED. The disk win.
    pub compacted: u64,
    /// Rows still holding a payload that this pass REFUSED to touch, because their sync is not confirmed.
    /// Reported rather than silently ignored: a run that reclaimed nothing because nothing was confirmed
    /// must not look like a run that found nothing to do. Those two need opposite responses -- resume the
    /// sweep, versus do nothing -- and silence would make them identical.
    pub left_unconfirmed: u64,
}

/// Drop payloads from rows whose sync is confirmed, in batches, until done or `budget` is spent.
///
/// Resumable by construction: the predicate is its own progress marker -- a compacted row stops matching
/// `payload IS NOT NULL`, so a killed run costs only the batch it was in.
pub async fn compact_payloads(
    pool: &sqlx::PgPool,
    budget: std::time::Duration,
) -> Result<CompactionCounts, sqlx::Error> {
    let started = std::time::Instant::now();
    let mut counts = CompactionCounts::default();
    let mut batches = 0usize;

    loop {
        if started.elapsed() >= budget {
            println!(
                "sirene_ingest: compaction budget of {}s reached -- stopping cleanly; re-run to continue",
                budget.as_secs()
            );
            break;
        }

        // The entire safety argument is this WHERE clause. `status = 'SYNCED'` is written only by the
        // worker, and only once the aggregate's verdict has come back (or, on the explicit-closure path,
        // once the command has actually executed). `synced_at IS NOT NULL` is a second, independent
        // witness of the same fact -- belt and braces on an irreversible delete, since recovering a
        // payload costs a ~4h re-fetch from INSEE.
        //
        // No cursor is carried: rows leave the predicate as they are compacted, so the next batch is
        // simply the next 2000 that still match.
        let updated = sqlx::query(
            "WITH batch AS ( \
                 SELECT siret FROM external_sirene_restaurants \
                  WHERE payload IS NOT NULL \
                    AND status = 'SYNCED' \
                    AND synced_at IS NOT NULL \
                  ORDER BY siret LIMIT $1 \
             ) \
             UPDATE external_sirene_restaurants s SET payload = NULL \
               FROM batch b WHERE s.siret = b.siret",
        )
        .bind(COMPACTION_BATCH_SIZE)
        .execute(pool)
        .await?
        .rows_affected();

        if updated == 0 {
            break;
        }
        counts.compacted += updated;

        batches += 1;
        if batches % VACUUM_EVERY_BATCHES == 0 {
            // Autocommit (no surrounding transaction), which VACUUM requires. Best-effort: a role
            // without permission must not fail the pass -- autovacuum catches up, just later.
            if let Err(e) = sqlx::query("VACUUM external_sirene_restaurants").execute(pool).await {
                eprintln!("sirene_ingest: compaction VACUUM skipped ({e})");
            }
            println!("sirene_ingest: compaction progress -- {} payload(s) dropped", counts.compacted);
        }
    }

    counts.left_unconfirmed = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM external_sirene_restaurants \
          WHERE payload IS NOT NULL AND NOT (status = 'SYNCED' AND synced_at IS NOT NULL)",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0) as u64;

    if let Err(e) = sqlx::query("VACUUM external_sirene_restaurants").execute(pool).await {
        eprintln!("sirene_ingest: final compaction VACUUM skipped ({e})");
    }
    Ok(counts)
}
