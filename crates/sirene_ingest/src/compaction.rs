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
//! They carry `status = 'PENDING'` (the migration default) and no `synced_at`, so the rule above will
//! never touch them -- but for many of them the evidence the rule demands EXISTS, one table over.
//! Every sync is a `RegisterRestaurant`/`MarkRestaurantClosed` COMMAND handed to the write-path
//! journal, which since #242 Runtime D is `inbound_messages` and nothing else: one row per
//! submission, with a deterministic `message_id`
//! (UUIDv5 over the command type, the SIRET and the staged version's `last_seen_at` -- exactly the
//! version this row still carries, since a later refresh would have re-pended it), and a verdict
//! (`SUCCEEDED`/`REJECTED`/`FAILED` + `completed_at`) written when the delivery completed. The seed
//! is unchanged across the retirement -- `send_envelope` and [`journal_message_id`] derive the same
//! UUIDv5 from the same namespace, which the infrastructure parity test pins -- so the join below
//! simply retargets to the surviving table and cannot match a row it did not mean.
//! That is a RECORDED outcome for the exact staged version -- the same standard `status`/`synced_at`
//! meet -- so this pass transcribes it: `status = 'SYNCED'`, `synced_at = journal.completed_at`, payload
//! dropped, all in one statement. Rows whose journal verdict is a rejection or a failure, and rows with
//! no journal row at all (drained before journaling existed, or an `etat=F` signal that had nothing to
//! close), are left untouched: the sweep re-pends them once on resume (their hash sentinel guarantees
//! it, migration `20260728040000`) and the ordinary path confirms them.
//!
//! **Until #227 the journal was `command_journal`, and those verdicts DIED WITH IT.** Nothing carried
//! them over: migration `20260731143000` says in as many words that `command_journal` is deliberately
//! NOT touched, and `20260812000000` DROPs it with no backfill, by design -- acceptance history is
//! not a domain fact, and the log is empty by decision at the moment it lands. A pre-#227 row's
//! evidence is therefore not recoverable here, whatever id it once had; it falls into the
//! "no journal row at all" class above and reclaims by re-sync. That is the safe direction -- the
//! pass keeps the payload and INSEE re-supplies the record -- but do not read the paragraph above as
//! promising that pre-#227 history can still be transcribed. It cannot.
//!
//! Two bounds on the journal evidence, worth stating plainly:
//!
//! - **It expires.** The retention sweep (ADR-20260721-025159) deletes terminal `inbound_messages`
//!   rows 90 days after `completed_at`. Run this pass before the verdicts age out, or their rows
//!   fall back to reclamation-by-re-sync.
//! - **`SUCCEEDED` means the submission succeeded, not that the record's every field reached the
//!   domain**: the pre-#227 register path absorbed changed records as idempotent no-op replays. The
//!   sync HAPPENED -- the aggregate holds the record -- but a later INSEE change may not have been
//!   applied. That is not a reason to keep the payload: INSEE is the system of record, the row's hash
//!   sentinel makes the next sweep re-pend it regardless, and the aggregate then decides on fresh data.
//!
//! The ACL gap that came with running compaction in CI stays GONE: this pass still classifies nothing --
//! both arms only read verdicts someone else recorded.
//!
//! # Why the SYNCED arm exists
//!
//! As a backstop. The worker drops a confirmed payload in the SAME statement that records the
//! confirmation, so the two cannot normally drift -- but a row holding both `SYNCED` and a payload is
//! exactly what should be swept up, and doing that in bounded batches with `VACUUM` interleaved is the
//! only shape that fits the disk this table has left.

use chrono::{DateTime, Utc};

/// UUIDv5 in the SIRENE integration namespace. DUPLICATED from
/// `infrastructure::integrations::sirene::sirene_uuid` -- this CI crate cannot see the domain layers
/// (ADR-0045), and the namespace is a fixed published constant, not logic. A parity unit test in
/// `crates/infrastructure` (`journal_message_id_parity_with_the_compaction`) pins the two derivations
/// together so they cannot drift silently.
fn sirene_uuid(seed: &str) -> uuid::Uuid {
    let ns = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"https://captain.food/integrations/sirene");
    uuid::Uuid::new_v5(&ns, seed.as_bytes())
}

/// The deterministic journal `message_id` a SIRENE sync send carries for one staged version: UUIDv5
/// over (command type, SIRET, the row's `last_seen_at` as READ). Public so the infrastructure parity
/// test can compare it against the worker's own derivation.
pub fn journal_message_id(command_type: &str, siret: &str, last_seen_at: DateTime<Utc>) -> uuid::Uuid {
    sirene_uuid(&format!("command:{command_type}:{siret}:{}", last_seen_at.to_rfc3339()))
}

/// Rows updated per batch. One round-trip each rather than one per row -- the #215/#216 lesson.
pub const COMPACTION_BATCH_SIZE: i64 = 2_000;

/// `VACUUM` after this many batches. Each one scans the whole table, so per-batch vacuuming would make
/// the pass quadratic in table size for no benefit; this keeps dead tuples reusable by the batches that
/// follow, which is the point.
pub const VACUUM_EVERY_BATCHES: usize = 25;

/// What one compaction run did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CompactionCounts {
    /// Payloads dropped from rows already confirmed SYNCED. The backstop arm.
    pub compacted: u64,
    /// Pre-#227 rows whose sync was confirmed FROM THE COMMAND JOURNAL's recorded verdict (see the
    /// module doc) -- marked `SYNCED`, `synced_at` transcribed, payload dropped, in one statement.
    /// On a mirror full of historical rows this is the disk win.
    pub journal_confirmed: u64,
    /// Rows still holding a payload that this pass REFUSED to touch, because their sync is not confirmed
    /// by either arm. Reported rather than silently ignored: a run that reclaimed nothing because nothing
    /// was confirmed must not look like a run that found nothing to do. Those two need opposite
    /// responses -- resume the sweep, versus do nothing -- and silence would make them identical.
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

    // Arm 1 -- transcribe the command journal's recorded verdicts onto pre-#227 rows (module doc).
    //
    // The candidate set is NOT its own progress marker (a row with a REJECTED verdict, or none at all,
    // stays a candidate forever), so unlike arm 2 this loop carries a keyset cursor. The expected
    // message_id is derived per row from the SAME `last_seen_at` the retired command path read when it
    // drained the row -- a version refreshed since then no longer matches its journal row and is left
    // alone, which errs on the conservative side.
    let mut cursor = String::new();
    loop {
        if started.elapsed() >= budget {
            println!(
                "sirene_ingest: compaction budget of {}s reached in the journal arm -- stopping \
                 cleanly; re-run to continue",
                budget.as_secs()
            );
            break;
        }

        let candidates: Vec<(String, chrono::DateTime<chrono::Utc>, String)> = sqlx::query_as(
            "SELECT siret, last_seen_at, etat FROM external_sirene_restaurants \
              WHERE siret > $1 \
                AND payload IS NOT NULL \
                AND status = 'PENDING' \
                AND synced_at IS NULL \
                AND processed_at IS NOT NULL \
                AND processed_at >= last_seen_at \
              ORDER BY siret LIMIT $2",
        )
        .bind(&cursor)
        .bind(COMPACTION_BATCH_SIZE)
        .fetch_all(pool)
        .await?;
        let Some(last) = candidates.last() else { break };
        cursor = last.0.clone();

        let sirets: Vec<String> = candidates.iter().map(|(siret, _, _)| siret.clone()).collect();
        let expected: Vec<uuid::Uuid> = candidates
            .iter()
            .map(|(siret, last_seen_at, etat)| {
                // The path the retired worker took for this row decided WHICH command it journaled:
                // an explicit closure signal went through MarkRestaurantClosed, everything else
                // through RegisterRestaurant.
                let command = if etat == "F" { "MarkRestaurantClosed" } else { "RegisterRestaurant" };
                journal_message_id(command, siret, *last_seen_at)
            })
            .collect();

        // `SUCCEEDED` resolves through the ref_* lookup (ADR-0037 ordinals) -- never a hardcoded
        // integer, same idiom as the retention sweep function. A REJECTED or FAILED verdict simply
        // never matches, so those rows keep their payload and fall to reclamation-by-re-sync.
        let confirmed = sqlx::query(
            "UPDATE external_sirene_restaurants s \
                SET status = 'SYNCED', synced_at = j.completed_at, payload = NULL \
               FROM unnest($1::text[], $2::uuid[]) AS v(siret, message_id) \
               JOIN inbound_messages j ON j.message_id = v.message_id \
              WHERE s.siret = v.siret \
                AND s.payload IS NOT NULL \
                AND s.status = 'PENDING' \
                AND j.status = 'SUCCEEDED' \
                AND j.completed_at IS NOT NULL",
        )
        .bind(&sirets)
        .bind(&expected)
        .execute(pool)
        .await?
        .rows_affected();
        counts.journal_confirmed += confirmed;

        batches += 1;
        if batches % VACUUM_EVERY_BATCHES == 0 {
            if let Err(e) = sqlx::query("VACUUM external_sirene_restaurants").execute(pool).await {
                eprintln!("sirene_ingest: compaction VACUUM skipped ({e})");
            }
            println!(
                "sirene_ingest: compaction progress -- {} journal-confirmed payload(s) dropped",
                counts.journal_confirmed
            );
        }
    }

    // Arm 2 -- the backstop: rows already confirmed SYNCED that somehow still hold a payload.
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
