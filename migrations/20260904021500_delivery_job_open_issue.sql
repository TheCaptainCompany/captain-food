-- #639 part C step 3-i (ADR-20260904-015903 §3/§4): the issue door tells the restaurant THROUGH
-- THE READ MODEL. `View_DeliveryJob` gains `open_issue_kind` — the closed DeliveryIssueKind of the
-- latest DeliveryIssueReported, cleared (NULL) by a DeliveryIssueResolved that follows it (the
-- `derive:` grammar's explicit `null` arm, DeriveVal::Null) — and folds both issue facts into
-- `updated_at`. Neither fact moves `status` or `rider_id`.
--
-- The view body below is COPIED VERBATIM from the regenerated specs/generated/views.generated.sql
-- (never hand-shaped): `views.generated.sql` is applied by nothing (farley, the 3-i briefing —
-- the drift gate is #861), so every view change ships as a hand-written migration, readers first:
-- this lands BEFORE anything writes `kind`, and a row appended before it reads NULL here.
--
-- `DROP VIEW IF EXISTS` first, on purpose: `CREATE OR REPLACE VIEW` may only APPEND columns, and
-- the generated body places `open_issue_kind` before the emitter's trailing `created_at` /
-- `updated_at` — replacing in place would fail with "cannot change name of view column". Nothing
-- depends on this view (no other view or function selects from it; the read repository is SQL at
-- call time), so the drop is safe; the 20260730043100 migration set the precedent.

DROP VIEW IF EXISTS View_DeliveryJob;

CREATE OR REPLACE VIEW View_DeliveryJob AS
SELECT
  (c.payload->>'deliveryJobId')::uuid AS delivery_job_id,
  (c.payload->>'orderId')::uuid AS order_id,
  (c.payload->>'restaurantId')::uuid AS restaurant_id,
  (SELECT CASE e.event_type WHEN 'DeliveryRequested' THEN 'PENDING' WHEN 'DeliveryAcceptedByRider' THEN 'ASSIGNED' WHEN 'DeliveryAcceptedByPartner' THEN 'ASSIGNED' WHEN 'DeliveryPickedUp' THEN 'PICKED_UP' WHEN 'DeliveryStatusUpdated' THEN e.payload->>'status' WHEN 'DeliveryCompleted' THEN 'DELIVERED' WHEN 'DeliveryCancelled' THEN 'CANCELLED' WHEN 'DeliveryDispatchFailed' THEN 'FAILED' END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryRequested', 'DeliveryAcceptedByRider', 'DeliveryAcceptedByPartner', 'DeliveryPickedUp', 'DeliveryStatusUpdated', 'DeliveryCompleted', 'DeliveryCancelled', 'DeliveryDispatchFailed')
     ORDER BY e.position DESC LIMIT 1) AS status,
  (SELECT CASE e.event_type WHEN 'DeliveryAcceptedByRider' THEN 'INDEPENDENT' WHEN 'DeliveryAcceptedByPartner' THEN 'PARTNER' END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryAcceptedByRider', 'DeliveryAcceptedByPartner')
     ORDER BY e.position DESC LIMIT 1) AS provider,
  (SELECT (e.payload->>'riderId')::uuid FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryAcceptedByRider') AND e.payload ? 'riderId'
     ORDER BY e.position DESC LIMIT 1) AS rider_id,
  (SELECT e.payload->'courier' FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryAcceptedByPartner') AND e.payload ? 'courier'
     ORDER BY e.position DESC LIMIT 1) AS courier,
  (SELECT e.payload->>'partnerRef' FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryAcceptedByPartner') AND e.payload ? 'partnerRef'
     ORDER BY e.position DESC LIMIT 1) AS partner_ref,
  c.payload->'pickup' AS pickup_address,
  c.payload->'dropoff' AS dropoff_address,
  (SELECT (e.payload->>'estimatedPickupAt')::timestamptz FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryAcceptedByPartner') AND e.payload ? 'estimatedPickupAt'
     ORDER BY e.position DESC LIMIT 1) AS estimated_pickup_at,
  (SELECT (e.payload->>'estimatedDropoffAt')::timestamptz FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryAcceptedByPartner') AND e.payload ? 'estimatedDropoffAt'
     ORDER BY e.position DESC LIMIT 1) AS estimated_dropoff_at,
  c.occurred_at AS requested_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryPickedUp')) AS picked_up_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND (e.event_type = 'DeliveryCompleted' OR (e.event_type = 'DeliveryStatusUpdated' AND e.payload->>'status' = 'DELIVERED'))) AS delivered_at,
  (SELECT e.payload->>'reason' FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryRejectedByPartner') AND e.payload ? 'reason'
     ORDER BY e.position DESC LIMIT 1) AS last_partner_rejection,
  (SELECT CASE e.event_type WHEN 'DeliveryIssueReported' THEN e.payload->>'kind' WHEN 'DeliveryIssueResolved' THEN NULL END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryIssueReported', 'DeliveryIssueResolved')
     ORDER BY e.position DESC LIMIT 1) AS open_issue_kind,
  c.occurred_at AS created_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryRequested', 'DeliveryAcceptedByPartner', 'DeliveryRejectedByPartner', 'DeliveryStatusUpdated', 'DeliveryAcceptedByRider', 'DeliveryPickedUp', 'DeliveryCompleted', 'DeliveryCancelled', 'DeliveryDispatchFailed', 'DeliveryIssueReported', 'DeliveryIssueResolved')) AS updated_at
FROM domain_events c
WHERE c.event_type = 'DeliveryRequested';
