-- #639 part C step 6-iv round 2 (ADR-20260905-101349 §2 amendment, PROP-20260831-180622 §6.4/§6.5):
-- the roster and the invitation list -- the source of `restaurantRoster`/`restaurantInvitations`.
-- DDL MIRRORS specs/generated/schema.generated.sql
-- (specs/database/tables/projection_tables.yaml#/RestaurantRoster,
-- #/RestaurantInvitationList) -- generated first, copied here; the index NAMES below are
-- hand-computed for these migrations' LOWERCASE table names (the generated schema spells the
-- tables `RestaurantRoster`/`RestaurantInvitationList`, which Postgres's own unquoted-identifier
-- fold turns into `restaurantroster`/`restaurantinvitationlist` with no underscore -- the SAME
-- pre-existing, corpus-wide quirk `rider_roster.sql`'s own header names, not this migration's to
-- fix). Both indexes carry `IF NOT EXISTS` and an EXPLICIT name matching exactly the default name
-- Postgres already auto-assigns (`tools/codegen-rs/src/emit/sql.rs`'s `pg_index_name`:
-- `<table>_<col1>_<col2>_..._idx`, lowercased) -- the round-3 `rider_roster.sql` lesson applied
-- from the start here, never re-learned.
--
-- BOTH tables are NEW, their OWN `ProjectorGroup` checkpoints
-- ("RestaurantRoster"/"RestaurantInvitationList", crates/infrastructure/src/projection/worker.rs)
-- starting at position 0 -- no `projection_checkpoint` row for either yet, so the WHOLE
-- `RestaurantMembership-`/`RestaurantInvitation-` stream prefixes replay through these projectors
-- alone the first time they drain (the `RiderRoster`/`Member` #424 precedent, generalised twice
-- more). No backfill copy from `member`/`domain_events` here, on purpose: a migration-time
-- INSERT ... SELECT would be a SECOND source of truth for a fact the event log already owns.
-- NO CHECK constraint on either (`DbFaultPolicy::Skip` semantics, the `RiderRoster`/`Member`
-- reasoning): a stray value fails the ONE row's projection, never the whole group.
CREATE TABLE IF NOT EXISTS restaurantroster (
  membership_id UUID PRIMARY KEY,
  scope_id UUID NOT NULL,
  member_id UUID NOT NULL,
  authority TEXT NOT NULL,
  since TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS restaurantroster_scope_id_member_id_idx ON restaurantroster (scope_id, member_id);

CREATE TABLE IF NOT EXISTS restaurantinvitationlist (
  invitation_id UUID PRIMARY KEY,
  scope_id UUID NOT NULL,
  invited_email TEXT NOT NULL,
  authority TEXT NOT NULL,
  status TEXT NOT NULL,
  expires_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS restaurantinvitationlist_scope_id_status_created_at_idx ON restaurantinvitationlist (scope_id, status, created_at);
