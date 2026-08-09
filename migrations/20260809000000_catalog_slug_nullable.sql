-- Catalog.slug becomes NULLABLE — the migration 20260728020000 (Restaurant.slug) forgot its twin.
--
-- WHY: `specs/generated/schema.generated.sql` (specs/database/tables/) has declared `Catalog.slug`
-- nullable since the slug-configuration split: creation never derives a slug, the catalog projector
-- folds `slug` from `CatalogSlugConfigured` and writes NULL on `CatalogCreated`. Against this
-- column's original NOT NULL (from 20260717120000_domain_schema.sql) that first fold FAILS, so a
-- freshly created catalog never materializes in the read model. The drift was invisible while the
-- integration suites hand-rolled their own `catalog` DDL (already nullable); it surfaced the moment
-- the suites started running against the real migrated schema (#335 consolidation).
--
-- Same posture as 20260728020000: no UNIQUE on this column to preserve, and no backfill — a NULL
-- slug is the first-class "owner has not configured a route yet" state.

ALTER TABLE catalog ALTER COLUMN slug DROP NOT NULL;
