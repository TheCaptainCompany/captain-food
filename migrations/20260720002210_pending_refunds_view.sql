-- 20260720002210 pending_refunds_view — the refund queue read model (ADR-0039): the
-- View_PendingRefunds fold view over domain_events, copied from the generated
-- specs/generated/views.generated.sql (source: specs/database/projection_views.yaml#/View_PendingRefunds),
-- plus the ref_refund_status lookup table for the new RefundStatus enum scalar (ADR-0037 — enum
-- columns store the declaration-order INTEGER ordinal). Fed by RefundOpened (delivered by
-- RefundProcess to the Payment aggregate when a refundable fact hits a CAPTURED payment) and the
-- decision/settlement facts on the same Payment stream; backs the `pendingRefunds` GraphQL query.

-- RefundStatus
CREATE TABLE ref_refund_status(sort_order INT PRIMARY KEY, value TEXT NOT NULL UNIQUE);
INSERT INTO ref_refund_status (value, sort_order) VALUES ('REQUESTED',0),('APPROVED',1),('DENIED',2),('REFUNDED',3);

CREATE OR REPLACE VIEW View_PendingRefunds AS
SELECT
  (c.payload->>'orderId')::uuid AS order_id,
  (c.payload->>'restaurantId')::uuid AS restaurant_id,
  (SELECT CASE e.event_type WHEN 'RefundOpened' THEN 0 WHEN 'RefundApproved' THEN 1 WHEN 'RefundDenied' THEN 2 WHEN 'PaymentRefunded' THEN 3 END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('RefundOpened', 'RefundApproved', 'RefundDenied', 'PaymentRefunded')
     ORDER BY e.position DESC LIMIT 1) AS status,
  (c.payload->'amount'->>'amountCents')::bigint AS amount_cents,
  c.payload->'amount'->>'currency' AS currency,
  (SELECT (e.payload->'amount'->>'amountCents')::bigint FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('RefundApproved') AND e.payload ? 'amount'
     ORDER BY e.position DESC LIMIT 1) AS approved_amount_cents,
  (SELECT e.payload->>'reason' FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('RefundOpened', 'RefundApproved', 'RefundDenied') AND e.payload ? 'reason'
     ORDER BY e.position DESC LIMIT 1) AS reason,
  (SELECT e.payload->>'refundId' FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('PaymentRefunded') AND e.payload ? 'refundId'
     ORDER BY e.position DESC LIMIT 1) AS refund_id,
  c.occurred_at AS requested_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('RefundApproved', 'RefundDenied')) AS decided_at,
  c.occurred_at AS created_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('RefundOpened', 'RefundApproved', 'RefundDenied', 'PaymentRefunded')) AS updated_at
FROM domain_events c
WHERE c.event_type = 'RefundOpened';
