-- The Rider identity read model (#639 STAFF-AUTH part A; ADR-20260818-094500 ruling A,
-- ADR-20260818-004646). The auth subject -> riderId mapping the authenticated request path resolves
-- against, plus the rider's profile and availability status. DDL MIRRORS
-- specs/generated/schema.generated.sql (specs/database/tables/projection_tables.yaml#/Rider) --
-- generated first, copied here, never hand-shaped. Enum columns are TEXT (ADR-20260728).
--
-- `RiderRegistered` has carried `authRef` as required since it was written; nothing projected it,
-- so `auth_ref` occurred exactly once in the entire projection set (on Customer) and the RIDER role
-- had no resolvable identity at all.
--
-- auth_ref is NOT NULL UNIQUE, not a plain index, and that is a security property rather than a
-- performance one: the repository lookup is `fetch_optional`, which on multiplicity returns an
-- ARBITRARY row -- plan-dependent, no error -- and ScopeMembership keys its grants on
-- `member_id = rider_id`, so a duplicate would hand one rider another rider's order scope. The
-- constraint turns a silent breach into a visible denial. It does NOT create the invariant:
-- nothing on the write side prevents two RiderRegistered with the same authRef and different
-- riderIds (`register_rider` guards riderId existence only), and the write-side reservation that
-- would -- the `slug_reservations` shape -- is designed and unbuilt, tracked with the rider sign-in
-- door (#639 part C). Uniqueness over a POPULATION is not an aggregate's to enforce.
--
-- NOT NULL on display_name and phone is load-bearing, not tidiness: the projector emitter branches
-- on the COLUMN's nullability, so a nullable column gets a blind assignment and a NOT NULL one gets
-- an `if let Some(v)` guard. RiderInfoUpdated is a PARTIAL update, so a nullable display_name would
-- mean a rider who changes only their phone silently loses their name -- live AND on every replay.
--
-- phone carries NO constraint and NO index, deliberately: authRef is the lookup key precisely so
-- the phone never becomes a domain key, and French mobile numbers are recycled -- a unique phone
-- here is a scheduled future projector fault on a number's second owner.
--
-- No CREATE INDEX lines: the primary key and the unique constraint provide both indexes this table
-- needs. Declaring `index: true` beside `unique: true` in the spec would have emitted a redundant
-- second btree on auth_ref (separate emitter passes), which a later reader would read as intent.
--
-- Starts EMPTY and backfills for free: no RiderRegistered has ever been appended, and the Rider
-- projector group has no `projection_checkpoint` row, so its first tick starts at position 0. No
-- checkpoint row is seeded here -- seeding one is what would break that.

CREATE TABLE Rider (
  rider_id UUID PRIMARY KEY,
  auth_ref TEXT NOT NULL UNIQUE,
  display_name TEXT NOT NULL,
  phone TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
