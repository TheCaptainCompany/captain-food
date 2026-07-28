-- The SIRENE mirror's `payload` becomes TRANSIENT (ADR-20260728-143000, issue #231).
-- DDL from specs/generated/schema.generated.sql (specs/database/tables/integration_staging.yaml).
--
-- `external_sirene_restaurants` kept the verbatim INSEE record FOREVER in order to read five fields out
-- of it: measured 2026-07-28 on production, 655 MB for 339k rows -- 77% of the whole database -- at
-- department 37 of 101, on a 2 GB disk with ~580 MB free. Full France projects to ~2 GB for this table
-- alone, so this is what actually gates national coverage (#218 paced the sweep; pacing does not create
-- disk).
--
-- The payload and the hash have DIFFERENT LIFETIMES. The payload is an input to TRANSLATION -- needed
-- from the moment INSEE reports a change until the worker has turned it into a domain fact, and never
-- again. The hash is the change-detection key and is needed forever. So the payload is present exactly
-- while a row is pending, and the worker NULLs it on successful translation. That is what this
-- migration enables; the nulling itself is done by the code, and the one-shot compaction of rows
-- already in the table is a separate paced pass (see `sirene_ingest --compact`).
--
-- This is deliberately a METADATA-ONLY change. Dropping NOT NULL does not rewrite the table, which
-- matters: there is not enough free disk to rewrite it (the earlier VACUUM FULL failed with
-- "No space left on device"). Everything expensive is done in bounded batches by the compaction pass.

ALTER TABLE external_sirene_restaurants ALTER COLUMN payload DROP NOT NULL;

-- `status` answers "has this row been SYNCED, or not?" directly. It is needed BECAUSE the payload became
-- transient: once `payload` is nullable, a row that has one is either awaiting translation or was kept
-- as evidence of an unmappable record, and nothing distinguishes the two. PENDING / SYNCED / UNMAPPABLE
-- / FAILED, written by the worker (the ingestion writes PENDING). It does NOT replace `processed_at` --
-- that stays the concurrency-safe checkpoint; this is the outcome, and `GROUP BY status` is the
-- operator's per-sweep report.
--
-- Every existing row starts PENDING, which is both true (none has been through the new path) and free:
-- a constant DEFAULT on ADD COLUMN is metadata-only in PG11+, no table rewrite -- and a rewrite is
-- exactly what there is no disk for. The compaction pass then stamps the accurate status as it visits
-- each row, so no separate backfill UPDATE is needed.
ALTER TABLE external_sirene_restaurants ADD COLUMN status TEXT NOT NULL DEFAULT 'PENDING';

CREATE INDEX ON external_sirene_restaurants (status);

-- NOTE for the follow-up (PROP-20260728-120931 D2, approved): `payload_hash` should become `bytea`
-- rather than 64-byte hex text (~11 MB at current row counts). It is NOT done here on purpose --
-- `ALTER COLUMN … TYPE bytea USING decode(payload_hash,'hex')` REWRITES the whole table and needs free
-- space equal to its current size (~655 MB against ~580 MB free), so it would fail exactly as the
-- VACUUM FULL did. Once the compaction has run and live data is ~90 MB, the same statement is cheap.
-- Sequencing, not a reversal of the decision.
