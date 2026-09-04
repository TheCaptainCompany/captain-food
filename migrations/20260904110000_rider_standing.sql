-- #639 part C step 4-i (ADR-20260904-081527 §2): the platform's GRANT on the rider identity row,
-- and the restriction attribution read model. DDL MIRRORS specs/generated/schema.generated.sql
-- (specs/database/tables/projection_tables.yaml#/Rider, #/RiderRestriction) -- generated first,
-- copied here, never hand-shaped, plus the DEFAULT the generator has no grammar for yet.
--
-- `standing` is metadata-only: a NEW event type (RiderRestricted/RiderReinstated) with ZERO stored
-- occurrences at this point in the log, unlike the 3-ii precedent (a column over events that
-- ALREADY existed) -- so this migration does NOT reset the Rider projection checkpoint. Every
-- existing row backfills to the DEFAULT 'ACTIVE' -- the fleet is granted, not denied, by this
-- migration (production stays suspended, ADR-20260817-105844, with a rider population of zero at
-- the time this lands). NO CHECK constraint (a constraint fault on this path is skipped by
-- `DbFaultPolicy::Skip` and the rider silently stays granted -- worse than the column simply being
-- wrong) and NO covering index (the seam's one read is already the primary-key lookup by rider_id;
-- see the Rider table's `rules:` for the `auth_ref` index reasoning, unaffected here).
ALTER TABLE rider ADD COLUMN IF NOT EXISTS standing TEXT NOT NULL DEFAULT 'ACTIVE';

-- The attribution behind `Rider.standing`: ground/decidedAt/effectiveAt/reinstatedAt -- the source
-- of `myStanding` and of 4-iii's admin surface. No `auth_ref`, no `phone` (see the table's `rules:`
-- in the spec). Round 2 item 7 (dba, young): "no backfill is owed" was WRONG as first written --
-- a brand-new table born under the ALREADY-ADVANCED `Rider` checkpoint would never replay a single
-- row for a rider registered before this migration, so a LATER RiderRestricted on that stream would
-- be silently dropped. The backfill IS owed, and it is REPLAY: `RiderRestriction` gets its OWN
-- `ProjectorGroup` (checkpoint `"RiderRestriction"`, `crates/infrastructure/src/projection/
-- worker.rs`) starting at position 0 -- no `projection_checkpoint` row for it yet, so the whole
-- `Rider-` stream prefix replays through this table alone the first time it drains. No copy between
-- derived tables here, on purpose: a migration-time INSERT ... SELECT FROM rider would be a SECOND
-- source of truth for a fact the event log already owns.
CREATE TABLE IF NOT EXISTS rider_restriction (
  rider_id UUID PRIMARY KEY,
  standing TEXT NOT NULL,
  ground TEXT,
  decided_at TIMESTAMPTZ,
  effective_at TIMESTAMPTZ,
  reinstated_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
