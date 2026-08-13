-- SMS-OTP send quota (#516): the SHARED, cross-replica counter behind the OTP send guards.
--
-- WHY A TABLE AT ALL. `requestPhoneVerification` is anonymous by design and every accepted send
-- spends real money on our own OVHcloud account. A per-pod in-memory limiter multiplies the
-- allowance by the replica count and resets on every deploy, so for the GLOBAL DAILY CEILING --
-- the only guard that bounds the total bill, because an attacker rotates numbers -- it would be
-- the difference between a ceiling and a suggestion. Every pod must count into the same row.
--
-- WHY THE PRIMARY KEY IS THE WHOLE INVARIANT. `RequestPhoneVerification` has NO per-phone actor
-- lane (the GraphQL door mints a fresh actor id per request), so nothing serialises two concurrent
-- requests for the same number except a single-statement atomic claim against this key. The claim
-- is `INSERT ... ON CONFLICT DO UPDATE ... WHERE`, where the WHERE clause IS the limit: a losing
-- claim updates no row and returns none, so a refusal never burns budget.
--
-- Keys: 'phone:<E.164>:hour' | 'phone:<E.164>:day' | 'global:day' -- always the CANONICAL number,
-- so '0612345678', '00612345678' and '612345678' under '+33' share one bucket, and a phone-CHANGE
-- send draws on the same budget as a verification send.
--
-- Copied from specs/generated/schema.generated.sql
-- (specs/database/tables/integration_connections.yaml).
--
-- Also replaces sweep_retention() (specs/database/functions/sweep_retention.sql) so quota rows join
-- the sweep: the key embeds a phone number, which is personal data, and a bucket nobody is still
-- asking about has no reason to exist. Body copied VERBATIM from the generated source
-- (CREATE -> CREATE OR REPLACE).

CREATE TABLE sms_send_quota (
  quota_key TEXT PRIMARY KEY,
  window_start TIMESTAMPTZ NOT NULL,
  sent_count INTEGER NOT NULL,
  last_granted_at TIMESTAMPTZ NOT NULL
);

CREATE OR REPLACE FUNCTION sweep_retention()
RETURNS TABLE (swept_table TEXT, deleted BIGINT)
LANGUAGE plpgsql
AS $$
DECLARE
  n BIGINT;
BEGIN
  -- Status predicates compare the enum's TEXT value directly (ADR-20260728: enum columns store
  -- the scalars.yaml value verbatim; the ref_* ordinal lookups are gone).
  --
  -- The mailbox (journals.yaml `inbound_messages.retention`): terminal rows only — RECEIVED is
  -- pending work, SCHEDULED is future work; neither is ever age-swept.
  DELETE FROM inbound_messages
   WHERE status IN ('SUCCEEDED', 'REJECTED', 'FAILED', 'IGNORED', 'DUPLICATE', 'CANCELLED')
     AND completed_at IS NOT NULL
     AND completed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'inbound_messages'; deleted := n; RETURN NEXT;

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

  -- sms_send_quota (#516): OTP send-guard counters whose last activity is 2 days old. The key embeds
  -- a phone number, which is personal data -- a quota row for a number nobody is still asking about
  -- has no reason to exist. Live windows (hour, day) are never touched; the 'global:day' row is swept
  -- by the same rule and simply recreated by the next send.
  DELETE FROM sms_send_quota
   WHERE last_granted_at < now() - INTERVAL '2 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'sms_send_quota'; deleted := n; RETURN NEXT;
END;
$$;
