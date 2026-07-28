-- Sync attempt tracking on the SIRENE mirror (ADR-20260728-143000 follow-up, issue #231).
-- DDL from specs/generated/schema.generated.sql (specs/database/tables/integration_staging.yaml).
--
-- A SEPARATE migration from 20260728050000 on purpose: that one is merged and may already be applied,
-- and an applied migration's checksum is recorded in `_sqlx_migrations` — editing it in place would
-- either fail the checksum or simply never re-run. Forward-only, always.
--
-- Three columns, answering three questions the table could not answer before. All three are
-- metadata-only DDL (nullable, or a constant DEFAULT — no table rewrite in PG11+), which matters:
-- there is no disk here for a rewrite, which is the entire subject of #231.

-- 1. WHEN did this row last become a domain fact?
--
-- NOT derivable from `processed_at`, and the difference is easy to miss. The worker sets `processed_at`
-- to the `last_seen_at` it READ — deliberately not now(), so a concurrent ingestion bump re-pends the
-- row instead of being swallowed — and the ingestion then advances `processed_at` to now() on every
-- UNCHANGED row it re-sees (the hash-match carry-forward). So `processed_at` moves for rows nothing
-- happened to, and it is a sweep timestamp, not a sync timestamp. This moves only when a fact is
-- actually produced, and it SURVIVES a re-pend: "last synced 3 weeks ago, PENDING again since
-- yesterday" is the useful reading. NULL = never synced.
ALTER TABLE external_sirene_restaurants ADD COLUMN synced_at TIMESTAMPTZ NULL;

-- 2. WHEN did the worker last TRY? Moves on every attempt, successful or not.
ALTER TABLE external_sirene_restaurants ADD COLUMN last_attempt_sync_at TIMESTAMPTZ NULL;

-- 3. How many CONSECUTIVE attempts have failed?
--
-- This is what makes a stuck row findable. A failed sync deliberately leaves the row pending WITH its
-- payload — the retry needs something to translate — so a permanently-failing row is otherwise
-- indistinguishable from one simply not reached yet, and retries silently forever. (Not theoretical:
-- the 605-row SlugAlreadyTaken log storm was exactly this shape.)
--
-- It RESETS to 0 on any checkpointed outcome (SYNCED or UNMAPPABLE). Resetting is what gives the number
-- meaning: a monotonic lifetime tally says nothing about whether a row is stuck NOW. At 10 consecutive
-- failures the worker sets `status = 'POISON'` and the drain stops selecting the row; a CHANGED record
-- from INSEE re-pends it through the ordinary conflict arm (which writes PENDING) and so releases the
-- quarantine without any operator action.
ALTER TABLE external_sirene_restaurants ADD COLUMN attempt_sync_retry_count INTEGER NOT NULL DEFAULT 0;

-- Backfill note: every existing row gets synced_at/last_attempt_sync_at NULL and a count of 0, which is
-- honest — nothing recorded any of this before now. The compaction pass (`sirene_ingest --compact`)
-- backfills `synced_at` from `processed_at` for rows it can tell were already synced: for those,
-- `processed_at` IS the last_seen_at at which the worker drained them, and the carry-forward cannot have
-- moved it because their hash is still the `unhashed-pre-20260728` sentinel.
