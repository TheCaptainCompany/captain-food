-- Delivery-delay satisfaction check (#62) — the post-delivery customer survey ("was the delivery on
-- time?") plus the restaurant-facing timeliness insight. Mirrors specs/generated/schema.generated.sql
-- + views.generated.sql. Additive and idempotent: a nullable column on ordertracking, the enum lookup
-- table, and the fold VIEW the restaurant query reads.

-- --- Enum lookup (DeliveryTimeliness) ------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ref_delivery_timeliness (
  sort_order INT PRIMARY KEY,
  value      TEXT NOT NULL UNIQUE
);
INSERT INTO ref_delivery_timeliness (value, sort_order)
VALUES ('ON_TIME', 0), ('ACCEPTABLE_DELAY', 1), ('TOO_LATE', 2)
ON CONFLICT (sort_order) DO NOTHING;

-- --- Order read model gains the customer's delay verdict (null until answered) -------------------
ALTER TABLE ordertracking ADD COLUMN IF NOT EXISTS delivery_timeliness INTEGER;

-- --- Restaurant-facing timeliness insight (fold over DeliverySatisfactionRecorded) ---------------
CREATE OR REPLACE VIEW View_DeliverySatisfaction AS
SELECT
  (c.payload->>'orderId')::uuid AS order_id,
  (c.payload->>'restaurantId')::uuid AS restaurant_id,
  (CASE c.payload->>'timeliness' WHEN 'ON_TIME' THEN 0 WHEN 'ACCEPTABLE_DELAY' THEN 1 WHEN 'TOO_LATE' THEN 2 END) AS timeliness,
  c.payload->>'reason' AS reason,
  c.occurred_at AS recorded_at,
  c.occurred_at AS created_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliverySatisfactionRecorded')) AS updated_at
FROM domain_events c
WHERE c.event_type = 'DeliverySatisfactionRecorded';
