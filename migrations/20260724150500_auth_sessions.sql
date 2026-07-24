-- Auth-session parking (#112, PROP-20260724-150500): the identity provider issues its session at
-- VERIFICATION time inside the async VerifyPhone/verify-email handler, but the client only holds
-- the acceptance messageId — this table bridges the two. Ciphertext (AES-256-GCM under
-- AUTH_SESSION_KEY), single-read at `POST /auth/session` pickup (row deleted), minutes-scale TTL,
-- ownership = the journaling X-SESSION-ID must match. Copied from
-- specs/generated/schema.generated.sql (specs/database/tables/integration_connections.yaml).
--
-- Also replaces sweep_retention() (specs/database/functions/sweep_retention.sql,
-- ADR-20260721-025159) so abandoned pickup rows join the sweep — body copied VERBATIM from the
-- source (CREATE → CREATE OR REPLACE), which resolves status predicates via the ref_* lookups.

CREATE TABLE auth_sessions (
  message_id UUID PRIMARY KEY,
  session_id UUID NULL,
  ciphertext BYTEA NOT NULL,
  nonce BYTEA NOT NULL,
  expires_at TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX ON auth_sessions (session_id);
CREATE INDEX ON auth_sessions (expires_at);

CREATE OR REPLACE FUNCTION sweep_retention()
RETURNS TABLE (swept_table TEXT, deleted BIGINT)
LANGUAGE plpgsql
AS $$
DECLARE
  n BIGINT;
BEGIN
  -- Status predicates resolve through the ref_* enum lookups (ADR-0037 ordinals) — never
  -- hard-coded integers.
  DELETE FROM command_journal
   WHERE status IN (SELECT sort_order FROM ref_command_journal_status
                     WHERE value IN ('SUCCEEDED', 'REJECTED', 'FAILED'))
     AND completed_at IS NOT NULL
     AND completed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'command_journal'; deleted := n; RETURN NEXT;

  DELETE FROM inbound_events
   WHERE status = (SELECT sort_order FROM ref_inbound_event_status WHERE value = 'DELIVERED')
     AND delivered_at IS NOT NULL
     AND delivered_at < now() - INTERVAL '30 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'inbound_events'; deleted := n; RETURN NEXT;

  DELETE FROM external_stripe_events
   WHERE processed_at IS NOT NULL
     AND processed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'external_stripe_events'; deleted := n; RETURN NEXT;

  DELETE FROM external_hubrise_callbacks
   WHERE processed_at IS NOT NULL
     AND processed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'external_hubrise_callbacks'; deleted := n; RETURN NEXT;

  DELETE FROM external_avelo37_events
   WHERE processed_at IS NOT NULL
     AND processed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'external_avelo37_events'; deleted := n; RETURN NEXT;

  DELETE FROM external_uber_direct_events
   WHERE processed_at IS NOT NULL
     AND processed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'external_uber_direct_events'; deleted := n; RETURN NEXT;

  -- auth_sessions (#112): unclaimed cookie-pickup rows past their minutes-scale deadline. Claimed
  -- rows are deleted at pickup (single-read); this sweeps only the abandoned ones.
  DELETE FROM auth_sessions
   WHERE expires_at < now();
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'auth_sessions'; deleted := n; RETURN NEXT;
END;
$$;
