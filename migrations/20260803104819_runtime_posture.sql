-- DB-persisted PM_MAILBOX_DELIVERY posture (#318, ADR-20260803-104819; decided by
-- ADR-20260803-002712 Q4) -- process-wide runtime postures, ONE row per posture, read at startup
-- by every process the posture governs: the monolith composition root and every standalone
-- adapter worker fleet. Replaces the per-process env read: per-process env is per-deploy state,
-- and a drifted gate value on one adapter fleet delivering Payment facts without the PM chain
-- hop is the silent paid-order stall. Mirrors specs/generated/schema.generated.sql
-- (RuntimePosture, from specs/database/tables/referential.yaml).
--
-- Seeded/managed CONFIG rows (ADR-0037), NOT projections. Idempotent: re-applying keeps any
-- operator-flipped value (ON CONFLICT DO NOTHING -- the seed must never overwrite a flip).

CREATE TABLE IF NOT EXISTS RuntimePosture (
  posture    TEXT PRIMARY KEY,
  enabled    BOOLEAN NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The Runtime D1 money gate (ADR-20260801-023000), default OFF (gate-then-stabilize): the flip
-- is an operator UPDATE on this row + a rolling restart, recorded by its own one-line ADR.
INSERT INTO RuntimePosture (posture, enabled)
VALUES ('PM_MAILBOX_DELIVERY', FALSE)
ON CONFLICT (posture) DO NOTHING;
