-- HubRise connect flow (issue #20, ADR-20260721-100601): adapter-owned OAuth connection state.
-- Mirrors specs/database/tables/integration_connections.yaml (see the generated schema).
-- Credentials at rest: never event-sourced, never referenced by api.yaml, never projected.

CREATE TABLE IF NOT EXISTS hubrise_connections (
  restaurant_account_id UUID PRIMARY KEY,
  hubrise_account_id TEXT NOT NULL UNIQUE,
  access_token TEXT NOT NULL,
  account_name TEXT NULL,
  connected_at TIMESTAMPTZ NOT NULL,
  last_connected_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS hubrise_connection_locations (
  hubrise_location_id TEXT PRIMARY KEY,
  restaurant_account_id UUID NOT NULL,
  restaurant_id UUID NOT NULL,
  last_connected_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS hubrise_connection_locations_restaurant_account_id_idx
  ON hubrise_connection_locations (restaurant_account_id);
