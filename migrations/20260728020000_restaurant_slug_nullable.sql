-- Restaurant.slug becomes NULLABLE, and the derived open-data slugs are released (ADR-20260728-011344).
--
-- WHY: the slug is the STOREFRONT HOST ({slug}.captain.food) — an identity the owner chooses during
-- onboarding, not something derivable from a registration. `RestaurantRegistered` no longer carries one,
-- so the projector writes NULL for every listing without a configured storefront. Against the current
-- NOT NULL column that would fail on the very first projected registration, which is why this migration
-- must land with (or before) the deploy that carries the new projector.
--
-- The UNIQUE constraint STAYS. Postgres permits any number of NULLs in a unique index, so the ~200k
-- open-data listings coexist happily while the DATABASE keeps enforcing uniqueness over exactly the
-- configured set — the invariant lands in the right scope with no application-level check at all.
--
-- Releasing the derived slugs (D5): every NON_PARTNER listing was given a machine-built label
-- (slugify(name)-NIC, e.g. `chez-marco-00021`) that no merchant chose and that collided systematically
-- for generic names on the common 00019/00021 establishment numbers. Nothing was ever published at those
-- addresses, so nulling them breaks no live link and frees the tenant namespace. Claimed listings
-- (restaurant_account_id IS NOT NULL) KEEP their slug: those may already be in use, and a storefront
-- address must never move without a redirect.
--
-- Reversible: re-deriving the labels is not, but the column type change is (see the DOWN note below).

ALTER TABLE Restaurant ALTER COLUMN slug DROP NOT NULL;

-- Release the addresses reserved on behalf of listings that never opted in.
UPDATE Restaurant
   SET slug = NULL
 WHERE restaurant_account_id IS NULL
   AND slug IS NOT NULL;

-- DOWN (manual, not run): re-adding NOT NULL requires backfilling a slug for every unconfigured
-- listing, which is precisely the mistake this change removes. Roll back the application instead.
