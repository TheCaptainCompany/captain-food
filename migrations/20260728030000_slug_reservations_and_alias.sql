-- The storefront-address machinery: a write-side reservation and the superseded-label alias
-- (ADR-20260728-011344, slice 3). DDL copied from specs/generated/schema.generated.sql
-- (specs/database/tables/reservations.yaml + projection_tables.yaml#/SlugAlias).
--
-- slug_reservations — the ARBITER of storefront-slug uniqueness.
--   The primary key IS the invariant. `configure_restaurant_slug` does a single
--   `INSERT … ON CONFLICT DO NOTHING` before appending its event: exactly one of two concurrent claims
--   inserts, and the loser is told SlugAlreadyTaken. There is no read-then-write window, so the outcome
--   cannot be raced — unlike a lookup against the eventually-consistent Restaurant projection, where both
--   claimants would pass and only diverge once the projector caught up, having each been told "yes".
--   RELEASED IS NOT FREE: a rename stamps `released_at` and KEEPS the row, so a label a restaurant moved
--   away from stays un-claimable. Freeing it would let someone else take the old address and inherit the
--   301 below — along with its printed menus, QR codes and search results. There is deliberately no
--   retention policy: those artefacts have no expiry we control.
--
-- SlugAlias — so a rename does not 404 the world.
--   One row per superseded label, keyed by that label (NOT by an aggregate id — a restaurant renamed N
--   times leaves N rows on the same Restaurant stream). `hosts.rs` resolves an unknown host here and
--   301s to the restaurant's CURRENT address, looked up through `restaurant_id` so one hop always lands
--   on the live label however many renames have happened. `current_slug` records the address that
--   superseded this one at the time — historical, not authoritative.
--
-- Both tables start empty: no slug has ever been owner-configured (the previous migration released the
-- machine-derived ones), so there is nothing to backfill.

CREATE TABLE slug_reservations (
  slug TEXT PRIMARY KEY,
  restaurant_id UUID NOT NULL,
  reserved_at TIMESTAMPTZ NOT NULL,
  released_at TIMESTAMPTZ NULL
);
CREATE INDEX ON slug_reservations (restaurant_id);

CREATE TABLE SlugAlias (
  previous_slug TEXT PRIMARY KEY,
  restaurant_id UUID NOT NULL,
  current_slug TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX ON SlugAlias (restaurant_id);

-- Any slug a claimed restaurant already holds must be reserved, or its owner could lose it to someone
-- else's ConfigureRestaurantSlug. The previous migration nulled only the unclaimed (NON_PARTNER) ones,
-- so whatever remains is live and belongs to its holder.
INSERT INTO slug_reservations (slug, restaurant_id, reserved_at, released_at)
SELECT slug, restaurant_id, now(), NULL
  FROM Restaurant
 WHERE slug IS NOT NULL
ON CONFLICT (slug) DO NOTHING;
