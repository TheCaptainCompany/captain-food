-- #639 part C step 6-v (ADR-20260905-223957 §1/§2): the platform grant bridge. DDL MIRRORS
-- specs/generated/schema.generated.sql (specs/database/tables/projection_tables.yaml#/PlatformMember)
-- -- generated first, copied here, never hand-shaped.
--
-- NEW table, its OWN `ProjectorGroup` checkpoint ("PlatformMember",
-- crates/infrastructure/src/projection/worker.rs) starting at position 0 -- no
-- `projection_checkpoint` row is seeded here, so the whole `PlatformMembership-` stream prefix
-- replays through this table alone the first time it drains (the `Member`/`Rider` #424-lesson
-- precedent). Rebuild by resetting the `PlatformMember` checkpoint, NEVER TRUNCATE (the table's
-- own `rules:`): upsert keyed on `platform_membership_id` with one creating arm
-- (`PlatformAccessGranted`), so a from-zero replay rewrites every row in place and no admin is
-- denied mid-rebuild.
--
-- auth_subject is NOT NULL UNIQUE, not a plain index -- the same security property as
-- `Member.auth_subject`/`Rider.auth_ref`: the seam's lookup is `fetch_optional`, which on
-- multiplicity returns an ARBITRARY row. It is the SOLE arbiter of the one-subject-one-platform-
-- membership invariant (ADMIN is not a `PrincipalKind`, PRINCIPALS-MEMBER, so this reuses no
-- reservation table) -- the write-side handler consults this SAME bridge, read-before-append, as
-- its own first line. No CREATE INDEX lines beyond the implicit ones: the primary key and the
-- unique constraint provide both indexes this table needs (declaring `index: true` beside
-- `unique: true` would emit a redundant second btree, separate emitter passes).
--
-- Entirely STANDALONE -- no `ScopeMembership` prefix, no checkpoint rewind (dba CATCH,
-- ADR-20260905-223957 fence): the platform grant relationship never touches that table or its
-- group.
CREATE TABLE IF NOT EXISTS platform_member (
  platform_membership_id UUID PRIMARY KEY,
  auth_subject TEXT NOT NULL UNIQUE,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
