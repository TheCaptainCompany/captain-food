-- Uber Direct delivery-partner webhook mirror (issue #57, ADR-20260721-172500).
-- Copied from specs/generated/schema.generated.sql (generated from
-- specs/database/tables/integration_staging.yaml): the adapter-owned raw staging table for verified
-- Uber Direct webhook deliveries — verify (X-Uber-Signature raw-body HMAC) → mirror verbatim →
-- translate into inbound_events → drain through the normal write path onto the DeliveryJob stream.
--
-- Also replaces sweep_retention() (specs/database/functions/sweep_retention.sql,
-- ADR-20260721-025159) so the new mirror joins the 90-day processed-rows sweep.

CREATE TABLE external_uber_direct_events (
  uber_event_id TEXT PRIMARY KEY,
  event_type TEXT NOT NULL,
  payload JSONB NOT NULL,
  received_at TIMESTAMPTZ NOT NULL,
  processed_at TIMESTAMPTZ NULL
);
CREATE INDEX ON external_uber_direct_events (event_type);
CREATE INDEX ON external_uber_direct_events (received_at);
CREATE INDEX ON external_uber_direct_events (processed_at);

DROP FUNCTION IF EXISTS sweep_retention();
CREATE FUNCTION sweep_retention()
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
END;
$$;
