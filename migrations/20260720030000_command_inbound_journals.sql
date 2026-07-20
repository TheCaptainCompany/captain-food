-- Command sourcing + inbound event sourcing (ADR-20260720-015300 / -015400 / -015500).
-- Copied from specs/generated/schema.generated.sql (generated from specs/database/tables/*.yaml +
-- specs/scalars.yaml — ADR-0037/0043); the payment_process_manager ALTERs are the expand step for
-- the columns process_managers.yaml v3 added (the generated DDL is CREATE-only).
--
-- New tables:
--   command_journal            — one row per command submission, persisted BEFORE handling (pk
--                                message_id = the idempotency key; records rejections; the source
--                                of operationStatus/operationStatusChanged).
--   inbound_events             — adapted inbound BUSINESS events staged by adapter ACLs, drained
--                                through the normal write path (unique (source, external_id)).
--   external_stripe_events    /
--   external_hubrise_callbacks — adapter-owned VERBATIM webhook mirrors (verify → UPSERT → ACK).
-- New enum lookup seeds: ref_command_journal_status, ref_command_channel, ref_inbound_event_status
-- (declaration-order ordinals — append-only forever).

-- Enum lookups (scalars.yaml → ref_<enum>, ADR-0037)
CREATE TABLE ref_command_journal_status(sort_order INT PRIMARY KEY, value TEXT NOT NULL UNIQUE);
INSERT INTO ref_command_journal_status (value, sort_order) VALUES ('RECEIVED',0),('SUCCEEDED',1),('REJECTED',2),('FAILED',3);

CREATE TABLE ref_command_channel(sort_order INT PRIMARY KEY, value TEXT NOT NULL UNIQUE);
INSERT INTO ref_command_channel (value, sort_order) VALUES ('GRAPHQL',0),('WORKER',1),('INTERNAL',2);

CREATE TABLE ref_inbound_event_status(sort_order INT PRIMARY KEY, value TEXT NOT NULL UNIQUE);
INSERT INTO ref_inbound_event_status (value, sort_order) VALUES ('RECEIVED',0),('DELIVERED',1),('FAILED',2);

-- Adapter-owned raw webhook mirrors (integration_staging.yaml, staging: true)
CREATE TABLE external_stripe_events (
  stripe_event_id TEXT PRIMARY KEY,
  event_type TEXT NOT NULL,
  payload JSONB NOT NULL,
  received_at TIMESTAMPTZ NOT NULL,
  processed_at TIMESTAMPTZ NULL
);
CREATE INDEX ON external_stripe_events (event_type);
CREATE INDEX ON external_stripe_events (received_at);
CREATE INDEX ON external_stripe_events (processed_at);

CREATE TABLE external_hubrise_callbacks (
  callback_id TEXT PRIMARY KEY,
  resource_type TEXT NOT NULL,
  event_type TEXT NOT NULL,
  location_id TEXT NULL,
  payload JSONB NOT NULL,
  received_at TIMESTAMPTZ NOT NULL,
  processed_at TIMESTAMPTZ NULL
);
CREATE INDEX ON external_hubrise_callbacks (resource_type);
CREATE INDEX ON external_hubrise_callbacks (location_id);
CREATE INDEX ON external_hubrise_callbacks (received_at);
CREATE INDEX ON external_hubrise_callbacks (processed_at);

-- Write-path journals (journals.yaml)
CREATE TABLE command_journal (
  message_id UUID PRIMARY KEY,
  correlation_id UUID NOT NULL,
  cause_id UUID NULL,
  session_id UUID NULL,
  trace_id TEXT NULL,
  user_id UUID NULL,
  user_type INTEGER NOT NULL,
  channel INTEGER NOT NULL,
  command_type TEXT NOT NULL,
  payload JSONB NOT NULL,
  payload_hash TEXT NOT NULL,
  status INTEGER NOT NULL,
  error JSONB NULL,
  received_at TIMESTAMPTZ NOT NULL,
  completed_at TIMESTAMPTZ NULL
);
CREATE INDEX ON command_journal (correlation_id);
CREATE INDEX ON command_journal (session_id);
CREATE INDEX ON command_journal (user_id);
CREATE INDEX ON command_journal (command_type);
CREATE INDEX ON command_journal (status);
CREATE INDEX ON command_journal (received_at);

CREATE TABLE inbound_events (
  inbound_event_id UUID PRIMARY KEY,
  source TEXT NOT NULL,
  external_id TEXT NOT NULL,
  correlation_id UUID NOT NULL,
  event_type TEXT NOT NULL,
  payload JSONB NOT NULL,
  status INTEGER NOT NULL,
  error JSONB NULL,
  received_at TIMESTAMPTZ NOT NULL,
  delivered_at TIMESTAMPTZ NULL,
  UNIQUE (source, external_id)
);
CREATE INDEX ON inbound_events (source);
CREATE INDEX ON inbound_events (correlation_id);
CREATE INDEX ON inbound_events (event_type);
CREATE INDEX ON inbound_events (status);
CREATE INDEX ON inbound_events (received_at);

-- payment_process_manager expand (process_managers.yaml v3, ADR-20260720-015500): the
-- initiator-scoped paymentStatus read — ownership columns + the transient Stripe client secret
-- (NULLed when the run resolves; never event-sourced).
ALTER TABLE payment_process_manager
  ADD COLUMN customer_id UUID NULL,
  ADD COLUMN session_id UUID NULL,
  ADD COLUMN client_secret TEXT NULL;
