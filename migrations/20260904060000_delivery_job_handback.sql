-- #639 part C step 3-ii (ADR-20260904-015903 §1-3): the handback is a NEW fact, and the read
-- models fold it in the SAME slice. `View_DeliveryJob` gains:
--   * `status`/`provider`/`rider_id` — RESET/re-derived custody-keyed on a handback: `status` moves
--     PENDING unless `foodLocation` is WITH_RIDER (then FAILED, never re-offered while a
--     restricted rider's bag still holds the food — an oversell); `provider`/`rider_id` reset to
--     NULL (`DeriveVal::Null`, the 3-i grammar) so the job reads unassigned again — the fold-reset
--     proof `TestDeliveryReofferedAfterHandBack` needs (a second rider can accept right after).
--   * `food_location`/`handed_back_at` (new, nullable) — the custody fact itself: set by a
--     handback, reset to NULL by the next acceptance (rider or partner) so a stale marker never
--     survives a re-offer. The board's pinned card headline and the customer tracking banner's
--     predicate both read `food_location`.
--
-- The view body below is COPIED VERBATIM from the regenerated specs/generated/views.generated.sql
-- (never hand-shaped): `views.generated.sql` is applied by nothing (farley, #861 — the drift gate),
-- so every view change ships as a hand-written migration, readers first.
--
-- `DROP VIEW IF EXISTS` first, on purpose: `CREATE OR REPLACE VIEW` may only APPEND columns, and
-- the generated body places `food_location`/`handed_back_at` after the 3-i `open_issue_kind` and
-- before the emitter's trailing `created_at`/`updated_at` — replacing in place would fail with
-- "cannot change name of view column". Nothing depends on this view (no other view or function
-- selects from it; the read repository is SQL at call time), so the drop is safe; precedent:
-- 20260730043100, mirrored again by 20260904021500.
--
-- EMITTER FIX RIDING THIS MIGRATION (found live, via the named `status.derive` mutant): a
-- `derive: { from: prop }` arm used to extract the payload as bare TEXT (`e.payload->>'prop'`),
-- unlike every other typed extraction in this file. For an all-TEXT column (`status`/`provider`/
-- `open_issue_kind`/`food_location`) that is invisible; for `rider_id` (UUID) it silently made the
-- WHOLE CASE ladder's inferred type TEXT — Postgres infers a CASE's type from its branches — which
-- compiled clean and only broke `for_rider`'s `rider_id = $1::uuid` comparison at query time
-- ("operator does not exist: text = uuid"). The emitter (`emit/sql.rs`) now casts a `derive:`
-- payload extraction the same way `payload_extract` already does for every other column mode.

DROP VIEW IF EXISTS View_DeliveryJob;

CREATE OR REPLACE VIEW View_DeliveryJob AS
SELECT
  (c.payload->>'deliveryJobId')::uuid AS delivery_job_id,
  (c.payload->>'orderId')::uuid AS order_id,
  (c.payload->>'restaurantId')::uuid AS restaurant_id,
  (SELECT CASE e.event_type WHEN 'DeliveryRequested' THEN 'PENDING' WHEN 'DeliveryAcceptedByRider' THEN 'ASSIGNED' WHEN 'DeliveryAcceptedByPartner' THEN 'ASSIGNED' WHEN 'DeliveryPickedUp' THEN 'PICKED_UP' WHEN 'DeliveryStatusUpdated' THEN e.payload->>'status' WHEN 'DeliveryCompleted' THEN 'DELIVERED' WHEN 'DeliveryCancelled' THEN 'CANCELLED' WHEN 'DeliveryDispatchFailed' THEN 'FAILED' WHEN 'DeliveryHandedBackByRider' THEN (CASE e.payload->>'foodLocation' WHEN 'NOT_COLLECTED' THEN 'PENDING' WHEN 'RETURNED_TO_RESTAURANT' THEN 'PENDING' WHEN 'WITH_RIDER' THEN 'FAILED' END) END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryRequested', 'DeliveryAcceptedByRider', 'DeliveryAcceptedByPartner', 'DeliveryPickedUp', 'DeliveryStatusUpdated', 'DeliveryCompleted', 'DeliveryCancelled', 'DeliveryDispatchFailed', 'DeliveryHandedBackByRider')
     ORDER BY e.position DESC LIMIT 1) AS status,
  (SELECT CASE e.event_type WHEN 'DeliveryAcceptedByRider' THEN 'INDEPENDENT' WHEN 'DeliveryAcceptedByPartner' THEN 'PARTNER' WHEN 'DeliveryHandedBackByRider' THEN NULL END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryAcceptedByRider', 'DeliveryAcceptedByPartner', 'DeliveryHandedBackByRider')
     ORDER BY e.position DESC LIMIT 1) AS provider,
  (SELECT CASE e.event_type WHEN 'DeliveryAcceptedByRider' THEN (e.payload->>'riderId')::uuid WHEN 'DeliveryHandedBackByRider' THEN NULL END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryAcceptedByRider', 'DeliveryHandedBackByRider')
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
  (SELECT CASE e.event_type WHEN 'DeliveryHandedBackByRider' THEN e.payload->>'foodLocation' WHEN 'DeliveryAcceptedByRider' THEN NULL WHEN 'DeliveryAcceptedByPartner' THEN NULL END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryHandedBackByRider', 'DeliveryAcceptedByRider', 'DeliveryAcceptedByPartner')
     ORDER BY e.position DESC LIMIT 1) AS food_location,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryHandedBackByRider')) AS handed_back_at,
  c.occurred_at AS created_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryRequested', 'DeliveryAcceptedByPartner', 'DeliveryRejectedByPartner', 'DeliveryStatusUpdated', 'DeliveryAcceptedByRider', 'DeliveryPickedUp', 'DeliveryCompleted', 'DeliveryCancelled', 'DeliveryDispatchFailed', 'DeliveryIssueReported', 'DeliveryIssueResolved', 'DeliveryHandedBackByRider')) AS updated_at
FROM domain_events c
WHERE c.event_type = 'DeliveryRequested';
