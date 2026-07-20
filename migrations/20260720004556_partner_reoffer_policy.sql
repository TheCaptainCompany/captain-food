-- 20260720004556 partner_reoffer_policy (ADR-20260720-004556) — bounded partner re-offer for
-- DeliveryDispatchProcess (specs/processmanager.yaml, DeliveryRejectedByPartner leg).
-- 1) offer_attempts: TOTAL offers made per dispatch run (the birth offer = 1); cap = 3
--    (rules.yaml#/DispatchRetriesAreBounded). Existing rows backfill to 1.
-- 2) DeliveryDispatchProcessStatus: FAILED reuses retired REOFFER_REQUIRED's ordinal slot (2) —
--    both flag manual handling, so stored rows keep their meaning; COMPLETED stays 3 (ADR-0037
--    declaration-order INTEGER ordinals; no data remap needed).
-- 3) View_DeliveryJob learns the DeliveryDispatchFailed fold (status -> FAILED), copied verbatim
--    from specs/generated/views.generated.sql (same column set: CREATE OR REPLACE is safe).
ALTER TABLE delivery_dispatch_process_manager
  ADD COLUMN offer_attempts INTEGER NOT NULL DEFAULT 1;

CREATE OR REPLACE VIEW View_DeliveryJob AS
SELECT
  (c.payload->>'deliveryJobId')::uuid AS delivery_job_id,
  (c.payload->>'orderId')::uuid AS order_id,
  (c.payload->>'restaurantId')::uuid AS restaurant_id,
  (SELECT CASE e.event_type WHEN 'DeliveryRequested' THEN 0 WHEN 'DeliveryAcceptedByRider' THEN 1 WHEN 'DeliveryAcceptedByPartner' THEN 1 WHEN 'DeliveryPickedUp' THEN 2 WHEN 'DeliveryStatusUpdated' THEN (CASE e.payload->>'status' WHEN 'PENDING' THEN 0 WHEN 'ASSIGNED' THEN 1 WHEN 'PICKED_UP' THEN 2 WHEN 'OUT_FOR_DELIVERY' THEN 3 WHEN 'DELIVERED' THEN 4 WHEN 'FAILED' THEN 5 WHEN 'CANCELLED' THEN 6 END) WHEN 'DeliveryCompleted' THEN 4 WHEN 'DeliveryCancelled' THEN 6 WHEN 'DeliveryDispatchFailed' THEN 5 END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryRequested', 'DeliveryAcceptedByRider', 'DeliveryAcceptedByPartner', 'DeliveryPickedUp', 'DeliveryStatusUpdated', 'DeliveryCompleted', 'DeliveryCancelled', 'DeliveryDispatchFailed')
     ORDER BY e.position DESC LIMIT 1) AS status,
  (SELECT CASE e.event_type WHEN 'DeliveryAcceptedByRider' THEN 1 WHEN 'DeliveryAcceptedByPartner' THEN 0 END FROM domain_events e
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
  c.occurred_at AS created_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryRequested', 'DeliveryAcceptedByPartner', 'DeliveryRejectedByPartner', 'DeliveryStatusUpdated', 'DeliveryAcceptedByRider', 'DeliveryPickedUp', 'DeliveryCompleted', 'DeliveryCancelled', 'DeliveryDispatchFailed')) AS updated_at
FROM domain_events c
WHERE c.event_type = 'DeliveryRequested';
