-- Retention sweep for the write-path journals and adapter webhook mirrors
-- (ADR-20260721-025159; issue #18). The ONE place the retention windows live — schedule it from
-- the in-process RetentionSweepWorker (default) or a pg_cron job; either way the policy is here.
--
-- Scope, per table (aged rows only — the guard columns are the tables' own high-water marks):
--   command_journal            terminal rows (SUCCEEDED/REJECTED/FAILED)  90 days from completed_at
--   inbound_events             DELIVERED rows                             30 days from delivered_at
--   external_stripe_events     processed rows (processed_at set)          90 days from processed_at
--   external_hubrise_callbacks processed rows (processed_at set)          90 days from processed_at
--   external_avelo37_events    processed rows (processed_at set)          90 days from processed_at
--   external_uber_direct_events processed rows (processed_at set)         90 days from processed_at
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
  -- Status predicates compare the enum's TEXT value directly (ADR-20260728: enum columns store
  -- the scalars.yaml value verbatim; the ref_* ordinal lookups are gone).
  DELETE FROM command_journal
   WHERE status IN ('SUCCEEDED', 'REJECTED', 'FAILED')
     AND completed_at IS NOT NULL
     AND completed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'command_journal'; deleted := n; RETURN NEXT;

  DELETE FROM inbound_events
   WHERE status = 'DELIVERED'
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
