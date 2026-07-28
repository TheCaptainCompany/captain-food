-- `payload_hash` on the SIRENE mirror: separate "we saw it again" from "it changed"
-- (ADR-20260728-011344, slice 5). DDL from specs/generated/schema.generated.sql
-- (specs/database/tables/integration_staging.yaml).
--
-- The ingestion bumps `last_seen_at` on EVERY row it sees, and must: detect-by-absence depends on that
-- freshness. But the worker's pending predicate is `processed_at IS NULL OR processed_at < last_seen_at`,
-- so bumping it also re-pended the row — and ~200k identical établissements were re-translated,
-- re-journaled and re-appended every Monday for no change at all. With the hash, the UPSERT's conflict
-- arm carries `processed_at` forward when the typed payload is byte-identical, and the row stays
-- non-pending.
--
-- Backfill note: existing rows get a sentinel that matches NOTHING, so the first sweep after this
-- migration re-processes every row exactly once (it cannot know whether they changed while unhashed) and
-- from the second sweep onward only genuine changes pend. Defaulting to a value that could collide with
-- a real hash would be worse: it would silently skip rows that HAD changed.

ALTER TABLE external_sirene_restaurants
  ADD COLUMN payload_hash TEXT NOT NULL DEFAULT 'unhashed-pre-20260728';

CREATE INDEX ON external_sirene_restaurants (payload_hash);

-- Drop the default: from here on the ingestion always supplies a real hash, and leaving the default in
-- place would let a future insert path quietly create an unhashable row.
ALTER TABLE external_sirene_restaurants ALTER COLUMN payload_hash DROP DEFAULT;
