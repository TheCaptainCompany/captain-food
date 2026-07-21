-- Delivery dispatch strategy foundation (#60) — the CATALOG of delivery channels (referential) plus
-- the per-scope USAGE config the DeliveryDispatchProcess saga resolves at runtime (city ranked walk +
-- restaurant dispatch mode). Tables mirror specs/generated/schema.generated.sql (City,
-- DeliveryChannelCatalog, CityDeliveryRanking, RestaurantDispatchConfig); the PM state table gains the
-- walk cursor (current_rank/current_channel) + the offer-timeout sweep index.
--
-- These are seeded/managed CONFIG rows (ADR-0037; later API-writable via partner self-registration,
-- #61), NOT projections — no worker, no domain events. Idempotent: re-applying upserts the seed rows.

-- --- Channel catalog + city routing config (referential) -----------------------------------------

CREATE TABLE IF NOT EXISTS City (
  city_id UUID PRIMARY KEY,
  name    TEXT NOT NULL,
  country TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS DeliveryChannelCatalog (
  channel                   TEXT PRIMARY KEY,
  kind                      INTEGER NOT NULL,      -- DeliveryChannelKind ordinal: POOL=0, PARTNER=1
  default_offer_ttl_seconds INTEGER NOT NULL,
  enabled                   BOOLEAN NOT NULL,
  effective_from            TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS CityDeliveryRanking (
  id                   TEXT PRIMARY KEY,
  city_id              UUID NULL,                  -- NULL = platform-default fallback
  rank                 INTEGER NOT NULL,
  channel              TEXT NOT NULL,
  ttl_override_seconds INTEGER NULL,
  effective_from       TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS citydeliveryranking_city_id_idx ON CityDeliveryRanking (city_id);

CREATE TABLE IF NOT EXISTS RestaurantDispatchConfig (
  restaurant_id             UUID PRIMARY KEY,
  city_id                   UUID NOT NULL,
  mode                      INTEGER NOT NULL,      -- RestaurantDispatchMode ordinal: CAPTAIN=0, RESTAURANT=1
  self_dispatch_ttl_seconds INTEGER NULL,
  effective_from            TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS restaurantdispatchconfig_city_id_idx ON RestaurantDispatchConfig (city_id);

-- --- PM state table: the ranked-walk cursor + the offer-timeout sweep index -----------------------

ALTER TABLE delivery_dispatch_process_manager
  ADD COLUMN IF NOT EXISTS current_rank    INTEGER NULL,
  ADD COLUMN IF NOT EXISTS current_channel TEXT NULL;

-- The DeliveryOfferTimeoutWorker scans OFFERED runs by age (process_status, last_update_utc).
CREATE INDEX IF NOT EXISTS delivery_dispatch_pm_offer_sweep_idx
  ON delivery_dispatch_process_manager (process_status, last_update_utc);

-- --- V0 Tours seed (idempotent) ------------------------------------------------------------------

-- The Tours city (V0 operating scope).
INSERT INTO City (city_id, name, country)
VALUES ('00000000-0000-4000-8000-000000000037', 'Tours', 'FR')
ON CONFLICT (city_id) DO UPDATE SET name = EXCLUDED.name, country = EXCLUDED.country;

-- Channel catalog: the independent-rider POOL + the three known PARTNER channels (adapters land with
-- their own issues: avelo37 #28, uber_direct #57, coopcycle #58).
INSERT INTO DeliveryChannelCatalog (channel, kind, default_offer_ttl_seconds, enabled, effective_from)
VALUES
  ('independent', 0, 600, TRUE, TIMESTAMPTZ '2026-01-01 00:00:00+00'),
  ('avelo37',     1, 300, TRUE, TIMESTAMPTZ '2026-01-01 00:00:00+00'),
  ('uber_direct', 1, 300, TRUE, TIMESTAMPTZ '2026-01-01 00:00:00+00'),
  ('coopcycle',   1, 300, TRUE, TIMESTAMPTZ '2026-01-01 00:00:00+00')
ON CONFLICT (channel) DO UPDATE SET
  kind = EXCLUDED.kind,
  default_offer_ttl_seconds = EXCLUDED.default_offer_ttl_seconds,
  enabled = EXCLUDED.enabled,
  effective_from = EXCLUDED.effective_from;

-- Tours ranked walk (#60): INDEPENDENT then UBER_DIRECT. Seeded BOTH under the Tours city_id and as
-- the platform default (city_id IS NULL) so an as-yet-unconfigured restaurant (no dispatch-config
-- row ⇒ CAPTAIN, no city) still resolves the V0 ranking via the platform-default fallback.
INSERT INTO CityDeliveryRanking (id, city_id, rank, channel, ttl_override_seconds, effective_from)
VALUES
  ('tours-1',   '00000000-0000-4000-8000-000000000037', 1, 'independent', NULL, TIMESTAMPTZ '2026-01-01 00:00:00+00'),
  ('tours-2',   '00000000-0000-4000-8000-000000000037', 2, 'uber_direct', NULL, TIMESTAMPTZ '2026-01-01 00:00:00+00'),
  ('default-1', NULL,                                    1, 'independent', NULL, TIMESTAMPTZ '2026-01-01 00:00:00+00'),
  ('default-2', NULL,                                    2, 'uber_direct', NULL, TIMESTAMPTZ '2026-01-01 00:00:00+00')
ON CONFLICT (id) DO UPDATE SET
  city_id = EXCLUDED.city_id,
  rank = EXCLUDED.rank,
  channel = EXCLUDED.channel,
  ttl_override_seconds = EXCLUDED.ttl_override_seconds,
  effective_from = EXCLUDED.effective_from;

-- No RestaurantDispatchConfig rows by default: absence ⇒ CAPTAIN (today's behaviour).
