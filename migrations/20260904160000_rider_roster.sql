-- #639 part C step 4-iii-A (ADR-20260904-152807 §1/§3): the admin's rider roster — the source of
-- the `riders`/`rider` GraphQL queries (`/system/riders`, `/system/riders/:riderId`). DDL MIRRORS
-- specs/generated/schema.generated.sql (specs/database/tables/projection_tables.yaml#/RiderRoster)
-- -- generated first, copied here, never hand-shaped. Round 2 item 9 (dba): `CREATE TABLE`
-- carries `IF NOT EXISTS` (the standard idempotent-migration convention); the two `CREATE INDEX`
-- statements below do NOT (the `indexes:` emitter in tools/codegen-rs/src/emit/sql.rs never adds
-- it, corpus-wide, not just here) -- re-running this migration against a partially-applied schema
-- would fail on a duplicate index name, not silently no-op.
--
-- NEW table, its OWN `ProjectorGroup` checkpoint ("RiderRoster",
-- crates/infrastructure/src/projection/worker.rs) starting at position 0 -- no `projection_checkpoint`
-- row for it yet, so the whole `Rider-` stream prefix replays through this table alone the first
-- time it drains (the `RiderRestriction` precedent, #639 part C step 4-i round 2 item 7,
-- generalised one table further). No backfill copy from `rider`/`rider_restriction` here, on
-- purpose: a migration-time INSERT ... SELECT would be a SECOND source of truth for a fact the
-- event log already owns. NO CHECK constraint (`DbFaultPolicy::Skip` semantics, the 4-i
-- reasoning): a stray value fails the ONE row's projection, never the whole group.
--
-- `(display_name, rider_id)` serves the roster's declared page order; `standing` is a PLAIN index
-- (never partial: the `indexes:` grammar in tools/codegen-rs/src/emit/sql.rs emits only whole-
-- column btree indexes today, no `WHERE` clause is expressible — see the table's own `rules:` in
-- projection_tables.yaml).
CREATE TABLE IF NOT EXISTS rider_roster (
  rider_id UUID PRIMARY KEY,
  display_name TEXT NOT NULL,
  phone TEXT NOT NULL,
  status TEXT NOT NULL,
  standing TEXT NOT NULL,
  ground TEXT,
  decided_at TIMESTAMPTZ,
  effective_at TIMESTAMPTZ,
  reinstated_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX ON rider_roster (display_name, rider_id);
CREATE INDEX ON rider_roster (standing);
