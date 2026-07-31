-- C3b-5 (ADR-20260731-122500 "the mailbox is the only door"): backfill `inbound_events` into
-- `inbound_messages`, then DROP it. Nothing writes inbound_events anymore (every adapter ACL
-- enqueues kind-EVENT mailbox rows since the C3b-4 code); this migration converts the HISTORY and
-- any not-yet-delivered stragglers, in received_at order, and removes the table.
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
-- Status vocabulary: RECEIVED -> RECEIVED (fresh position, the workers deliver it),
-- DELIVERED -> SUCCEEDED, IGNORED/DUPLICATE/FAILED carry over; completed_at = delivered_at.
-- Partition = the frozen FNV-1a hash is NOT reproducible in SQL, so backfilled rows use
-- abs(hashtext(actor_id::text)) % width for TERMINAL rows (history only — the lane never drains
-- them) and the workers' seed pass leaves them untouched. RECEIVED stragglers are the one case
-- that must land on a LIVE lane: they are re-stamped by the post-backfill UPDATE below onto
-- partition 0 of their actor type — a valid, always-seeded lane; head-of-line order against
-- future rows for the same aggregate is preserved by position (stragglers get the lowest fresh
-- positions), and cross-lane racing cannot arise because nothing else writes these aggregates'
-- old facts anymore.

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
    CASE
        WHEN i.event_type IN ('PaymentCaptured', 'PaymentFailed', 'PaymentRefunded')
            THEN uuid_generate_v5(ns.inbound, 'Payment:' || (i.payload -> 'payload' ->> 'paymentIntentId'))
        WHEN i.event_type IN ('DeliveryAcceptedByPartner', 'DeliveryRejectedByPartner', 'DeliveryStatusUpdated')
            THEN COALESCE((i.payload -> 'payload' ->> 'deliveryJobId')::uuid, i.inbound_event_id)
        ELSE COALESCE((i.payload -> 'payload' ->> 'restaurantId')::uuid, i.inbound_event_id)
    END,
    0,  -- re-derived below for the rows that matter; terminal rows are history
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
        ELSE i.status  -- RECEIVED / IGNORED / DUPLICATE / FAILED carry over verbatim
    END,
    i.error,
    i.received_at,
    i.delivered_at
FROM inbound_events i, ns
ORDER BY i.received_at, i.inbound_event_id
ON CONFLICT (message_id) DO NOTHING;  -- an already-enqueued redelivery of the same provider event wins

-- Terminal history rows get a stable (if arbitrary) partition so the column is honest; RECEIVED
-- stragglers go to partition 0 — an always-seeded live lane the workers WILL drain.
UPDATE inbound_messages m
   SET partition = CASE
        WHEN m.status = 'RECEIVED' THEN 0
        ELSE abs(hashtext(m.actor_id::text)) % 100
   END
 WHERE m.kind = 'EVENT'
   AND m.partition = 0
   AND m.source IS NOT NULL
   AND EXISTS (SELECT 1 FROM inbound_events i WHERE i.source = m.source AND i.external_id = m.external_id);

DROP TABLE inbound_events;
