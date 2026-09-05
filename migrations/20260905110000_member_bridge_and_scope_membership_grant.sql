-- #639 part C step 6-i (ADR-20260905-101349 SS2/SS4/SS5): the staff-authentication bridge and the
-- grant. DDL MIRRORS specs/generated/schema.generated.sql
-- (specs/database/tables/projection_tables.yaml#/Member) -- generated first, copied here, never
-- hand-shaped.
--
-- NEW table, its OWN `ProjectorGroup` checkpoint ("Member",
-- crates/infrastructure/src/projection/worker.rs) starting at position 0 -- no
-- `projection_checkpoint` row is seeded here, so the whole `RestaurantMembership-` stream prefix
-- replays through this table alone the first time it drains (the `Rider`/`RiderRestriction`/
-- `RiderRoster` #424-lesson precedent). Rebuild by resetting the `Member` checkpoint, NEVER
-- TRUNCATE (the table's own `rules:`): upsert keyed on `member_id` with one creating arm
-- (`RestaurantAccessGranted`), so a from-zero replay rewrites every row in place and no member is
-- denied mid-rebuild.
--
-- auth_subject is NOT NULL UNIQUE, not a plain index -- the same security property as
-- `Rider.auth_ref`: the seam's lookup is `fetch_optional`, which on multiplicity returns an
-- ARBITRARY row. It does NOT create the invariant: the write-side reservation that does is
-- `auth_subject_reservations`, keyed `(principal_kind, auth_subject)` -- `grant_restaurant_access`
-- reserves BEFORE the append. No CREATE INDEX lines beyond the implicit ones: the primary key and
-- the unique constraint provide both indexes this table needs (declaring `index: true` beside
-- `unique: true` would emit a redundant second btree, separate emitter passes).
CREATE TABLE IF NOT EXISTS member (
  member_id UUID PRIMARY KEY,
  auth_subject TEXT NOT NULL UNIQUE,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);

-- The ScopeMembership ACL index (#144) gains a FOURTH stream category
-- (`RestaurantMembership-%`, the RestaurantAccessGranted/Revoked grant and targeted-revoke arms,
-- worker.rs's ScopeMembership ProjectorGroup). Per the group's own comment ("ADDING a prefix to
-- this group later requires deleting the ScopeMembership checkpoint row in the same migration --
-- a prefix joined below an advanced checkpoint is never folded, the #424 lesson"): rewind the
-- checkpoint to 0 so the WHOLE totally-ordered fold (Order-/DeliveryJob-/Restaurant-/
-- RestaurantMembership-, one checkpoint) replays from the start, exactly as
-- 20260720020500_ordertracking_payment_intent_id.sql rewound Order/Cart for the same reason.
-- Production is suspended (ADR-20260817-105844) and the log is throwaway smoke data at this
-- point, so this is a cheap, safe rewind rather than a real backfill cost.
UPDATE projection_checkpoint SET position = 0 WHERE projector = 'ScopeMembership';
