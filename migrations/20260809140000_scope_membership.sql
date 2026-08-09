-- Read-side per-instance authorization (#144, PROP-20260725-185140): the ScopeMembership ACL index.
-- WHO may see WHICH protected instance — one row per (scope, principal), pk derived (UUIDv5 over the
-- natural key) so a replayed grant upserts onto itself and a revoke needs no lookup.
-- DDL mirrors specs/generated/schema.generated.sql exactly (enum columns TEXT per ADR-20260728).
CREATE TABLE ScopeMembership (
  membership_id UUID PRIMARY KEY,
  scope_type TEXT NOT NULL,
  scope_id UUID NOT NULL,
  principal_type TEXT NOT NULL,
  principal_id UUID NOT NULL,
  granted_at TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
-- "everything this principal may see" -> list-query filtering
CREATE INDEX ON ScopeMembership (principal_type, principal_id, scope_type);
-- "who may see this instance" -> audit + revoke sweeps
CREATE INDEX ON ScopeMembership (scope_type, scope_id);
