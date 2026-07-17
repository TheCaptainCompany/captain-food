-- ADR-0045: raw-integration STAGING table for the SIRENE sync.
--
-- The thin CI ingestion job UPSERTs one row per SIRET with the verbatim INSEE payload; the on-app
-- sync_sirene_worker drains rows past its checkpoint (processed_at < last_seen_at ⇒ pending), runs the
-- SIRENE ACL and registers/closes restaurants via the normal write path. NOT projected from
-- domain_events, NOT a GraphQL reads target. Mirrors specs/database/tables/integration_staging.yaml
-- (kept in sync with specs/generated/schema.generated.sql).
--
-- REQUIRED_SCHEMA_VERSION is intentionally NOT bumped for this migration: it is data-plane the deployed
-- app does not depend on to boot. Bump it to 20260718100000 when the sync_sirene_worker that reads this
-- table ships (ADR-0045 follow-up).
CREATE TABLE external_sirene_restaurants (
  siret TEXT PRIMARY KEY,
  payload JSONB NOT NULL,
  etat TEXT NOT NULL,
  naf TEXT NOT NULL,
  department TEXT NOT NULL,
  first_seen_at TIMESTAMPTZ NOT NULL,
  last_seen_at TIMESTAMPTZ NOT NULL,
  sync_run_id UUID NOT NULL,
  processed_at TIMESTAMPTZ NULL
);
CREATE INDEX ON external_sirene_restaurants (etat);
CREATE INDEX ON external_sirene_restaurants (naf);
CREATE INDEX ON external_sirene_restaurants (department);
CREATE INDEX ON external_sirene_restaurants (last_seen_at);
