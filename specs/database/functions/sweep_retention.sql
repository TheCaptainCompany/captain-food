-- Retention sweep for the write-path journals and adapter webhook mirrors
-- (ADR-20260721-025159; issue #18). The ONE place the retention windows live — schedule it from
-- the in-process RetentionSweepWorker (default) or a pg_cron job; either way the policy is here.
--
-- Scope, per table (aged rows only — the guard columns are the tables' own high-water marks):
--   command_journal            terminal rows (SUCCEEDED/REJECTED/FAILED)  90 days from completed_at
--   inbound_events             DELIVERED rows                             30 days from delivered_at
--   external_stripe_events     processed rows (processed_at set)          90 days from processed_at
--   external_hubrise_callbacks processed rows (processed_at set)          90 days from processed_at
--
-- NEVER swept, at any age: domain_events / domain_stream (the forever log — deliberately not
-- referenced here; its only trimming is the opt-in per-stream $maxAge/$maxCount machinery),
-- command_journal RECEIVED rows (the stale-RECEIVED sweep marks crashed runs FAILED first),
-- inbound_events FAILED rows (kept until resolved) and RECEIVED rows (pending work),
-- unprocessed mirror rows (processed_at IS NULL), and external_sirene_restaurants (a full
-- mirror — detect-by-absence needs the complete row set, ADR-0045).
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
END;
$$;
