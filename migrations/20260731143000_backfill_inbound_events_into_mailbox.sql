-- C3b-5 (ADR-20260731-122500 "the mailbox is the only door"): backfill `inbound_events` into
-- `inbound_messages`, then DROP it. Nothing writes inbound_events anymore (every adapter ACL
-- enqueues kind-EVENT mailbox rows since the C3b-4 code); this migration converts the HISTORY,
-- retargets the retention sweep, and removes the table.
--
-- `command_journal` is deliberately NOT touched here: it remains the door for the three
-- process-manager mutations (placeOrder / approveRefund / denyRefund) and the pre-flip
-- operationStatus history — it backfills and drops when the PM mailboxes land (#242 Runtime D).
--
-- Identity mapping (uuid-ossp v5, deterministic — matches the Rust side):
--   message_id = uuid_generate_v5(inbound_ns, source || ':' || external_id)
--                (the SAME id a future redelivery of the same provider event computes, so the
--                 pk dedupe keeps holding across the flip);
--   user_id    = uuid_generate_v5(inbound_ns, 'system:' || source)  — the per-source principal;
--   Payment lanes = uuid_generate_v5(inbound_ns, 'Payment:' || paymentIntentId) — the FROZEN
--                surrogate (the aggregate id is the intent STRING, not a uuid).
-- inbound_ns = uuid_generate_v5(uuid_ns_url(), 'https://captain.food/integrations/inbound'),
-- mirroring infrastructure::mailbox::inbound_namespace().
--
-- Status vocabulary: DELIVERED -> SUCCEEDED, IGNORED/DUPLICATE/FAILED carry over;
-- completed_at = delivered_at. RECEIVED rows are REFUSED (guard below): a straggler cannot be
-- re-stamped onto its FROZEN FNV lane in SQL, and parking it on an arbitrary lane would let it
-- race the same aggregate's post-flip facts across two independently-leased lanes — the deploy
-- runbook drains the old pipeline to empty BEFORE applying this migration, and the guard makes
-- skipping that step a loud failure instead of a silent reorder.
--
-- Partition: the frozen FNV-1a hash is not reproducible in SQL; backfilled rows are ALL terminal
-- (guard above), so abs(hashtext(actor_id::text)) % 100 keeps the column honest for history reads
-- without ever being drained.

-- WRITE-FENCE: this migration snapshots inbound_events and then drops it. An old-code instance
-- still ACKing webhooks during the deploy window would otherwise write rows AFTER the snapshot
-- and have them silently destroyed by the DROP — after the provider was ACKed, i.e. a lost
-- payment fact. Taking ACCESS EXCLUSIVE first makes any such writer block and then fail loudly
-- (its transaction errors, the webhook answers 5xx, the provider redelivers to the new code).
LOCK TABLE inbound_events IN ACCESS EXCLUSIVE MODE;

DO $$
DECLARE
  stragglers BIGINT;
BEGIN
  SELECT count(*) INTO stragglers FROM inbound_events WHERE status = 'RECEIVED';
  IF stragglers > 0 THEN
    RAISE EXCEPTION
      'inbound_events still holds % RECEIVED row(s) — drain the legacy pipeline to empty before '
      'applying the mailbox backfill (a re-parked straggler would race its aggregate''s live lane)',
      stragglers;
  END IF;
END $$;

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

WITH ns AS (
    SELECT uuid_generate_v5(uuid_ns_url(), 'https://captain.food/integrations/inbound') AS inbound
)
INSERT INTO inbound_messages
    (message_id, kind, actor_type, actor_id, partition, message_type, payload, payload_hash,
     channel, user_id, user_type, correlation_id, cause_id, source, external_id,
     status, error, received_at, completed_at)
SELECT
    uuid_generate_v5(ns.inbound, i.source || ':' || i.external_id),
    'EVENT',
    CASE
        WHEN i.event_type IN ('PaymentCaptured', 'PaymentFailed', 'PaymentRefunded') THEN 'Payment'
        WHEN i.event_type IN ('DeliveryAcceptedByPartner', 'DeliveryRejectedByPartner', 'DeliveryStatusUpdated') THEN 'DeliveryJob'
        ELSE 'Restaurant'
    END,
    -- NULL-safe: a historical payload missing/malforming its id falls back to the old event id
    -- (these rows are terminal history — the lane is never drained; aborting the whole deploy on
    -- one malformed legacy payload would be the worse trade).
    CASE
        WHEN i.event_type IN ('PaymentCaptured', 'PaymentFailed', 'PaymentRefunded')
            THEN uuid_generate_v5(ns.inbound, 'Payment:' || COALESCE(i.payload -> 'payload' ->> 'paymentIntentId', i.external_id))
        WHEN i.event_type IN ('DeliveryAcceptedByPartner', 'DeliveryRejectedByPartner', 'DeliveryStatusUpdated')
            THEN CASE
                WHEN (i.payload -> 'payload' ->> 'deliveryJobId') ~ '^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$'
                    THEN (i.payload -> 'payload' ->> 'deliveryJobId')::uuid
                ELSE i.inbound_event_id
            END
        ELSE CASE
            WHEN (i.payload -> 'payload' ->> 'restaurantId') ~ '^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$'
                THEN (i.payload -> 'payload' ->> 'restaurantId')::uuid
            ELSE i.inbound_event_id
        END
    END,
    0,  -- placeholder; re-stamped to the stable history partition in the SET below
    i.event_type,
    i.payload,
    md5(i.payload::text),  -- documentary for history rows: the Rust payload_hash (sha256 over the
                           -- canonical serde form) is not reproducible in SQL, and the hash only
                           -- discriminates FUTURE duplicate-vs-conflict on the same message_id —
                           -- which the pk collision path reads from this column; a false conflict
                           -- on a post-backfill redelivery is logged and skipped, never applied
                           -- twice (the safe direction).
    'EXTERNAL',
    uuid_generate_v5(ns.inbound, 'system:' || i.source),
    'EXTERNAL',
    i.correlation_id,
    i.inbound_event_id,  -- the old causality handle, preserved as the parent
    i.source,
    i.external_id,
    CASE i.status
        WHEN 'DELIVERED' THEN 'SUCCEEDED'
        ELSE i.status  -- IGNORED / DUPLICATE / FAILED carry over verbatim (RECEIVED refused above)
    END,
    i.error,
    i.received_at,
    i.delivered_at
FROM inbound_events i, ns
ORDER BY i.received_at, i.inbound_event_id
ON CONFLICT (message_id) DO NOTHING;  -- an already-enqueued redelivery of the same provider event wins

-- Stable (if arbitrary) history partition — every backfilled row is terminal, no lane drains it.
-- `status NOT IN (RECEIVED, SCHEDULED)` keeps the re-stamp off any LIVE post-flip row that
-- legitimately FNV-hashed to partition 0 and shares a provider identity with a legacy row —
-- undelivered work must never be moved off its frozen lane; terminal rows are history either way.
UPDATE inbound_messages m
   SET partition = abs(hashtext(m.actor_id::text)) % 100
 WHERE m.kind = 'EVENT'
   AND m.partition = 0
   AND m.source IS NOT NULL
   AND m.status NOT IN ('RECEIVED', 'SCHEDULED')
   AND EXISTS (SELECT 1 FROM inbound_events i WHERE i.source = m.source AND i.external_id = m.external_id);

DROP TABLE inbound_events;

-- RETARGET THE RETENTION SWEEP (would otherwise abort on the dropped table): PL/pgSQL resolves
-- table names lazily, so the deployed sweep_retention() would raise undefined_table on its first
-- post-drop tick and take the command_journal + webhook-mirror retention windows (a GDPR posture)
-- down with it. Same body as specs/database/functions/sweep_retention.sql — the spec is the
-- source; this is its redeployment.
CREATE OR REPLACE FUNCTION sweep_retention()
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
END;
$$;
