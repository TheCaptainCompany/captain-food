-- Enum storage: INTEGER declaration-order ordinals -> the enum's TEXT value, verbatim
-- (ADR-20260728-170000, supersedes the ADR-0037 ordinal + ref_<enum> lookup scheme).
--
-- 1. Drop the fold views (their enum columns change type, CREATE OR REPLACE cannot do that).
-- 2. For every enum column: ALTER to TEXT (ordinal digits survive as text), then rewrite each
--    ordinal to its value with a CASE frozen from the ref_* seeds this migration retires.
-- 3. Replace sweep_retention() with the text-predicate version.
-- 4. Drop every ref_<enum> lookup table.
-- 5. Recreate the fold views from the regenerated views.generated.sql.

-- 1. Fold views out of the way.
DROP VIEW IF EXISTS View_RestaurantAccount;
DROP VIEW IF EXISTS View_DeliveryJob;
DROP VIEW IF EXISTS View_DeliverySatisfaction;
DROP VIEW IF EXISTS View_DeliveryPartnerAvailability;
DROP VIEW IF EXISTS View_Reclamation;
DROP VIEW IF EXISTS View_PendingRefunds;

-- 2. Ordinal -> TEXT value, per enum column.

-- domain_events
ALTER TABLE domain_events ALTER COLUMN user_type TYPE TEXT USING user_type::text;
UPDATE domain_events SET user_type = CASE user_type WHEN '0' THEN 'PUBLIC' WHEN '1' THEN 'CUSTOMER' WHEN '2' THEN 'RESTAURANT_ACCOUNT' WHEN '3' THEN 'RESTAURANT' WHEN '4' THEN 'RIDER' WHEN '5' THEN 'ADMIN' WHEN '6' THEN 'EXTERNAL' ELSE user_type END; -- UserType

-- command_journal
ALTER TABLE command_journal ALTER COLUMN user_type TYPE TEXT USING user_type::text;
UPDATE command_journal SET user_type = CASE user_type WHEN '0' THEN 'PUBLIC' WHEN '1' THEN 'CUSTOMER' WHEN '2' THEN 'RESTAURANT_ACCOUNT' WHEN '3' THEN 'RESTAURANT' WHEN '4' THEN 'RIDER' WHEN '5' THEN 'ADMIN' WHEN '6' THEN 'EXTERNAL' ELSE user_type END; -- UserType
ALTER TABLE command_journal ALTER COLUMN channel TYPE TEXT USING channel::text;
UPDATE command_journal SET channel = CASE channel WHEN '0' THEN 'GRAPHQL' WHEN '1' THEN 'WORKER' WHEN '2' THEN 'INTERNAL' ELSE channel END; -- CommandChannel
ALTER TABLE command_journal ALTER COLUMN status TYPE TEXT USING status::text;
UPDATE command_journal SET status = CASE status WHEN '0' THEN 'RECEIVED' WHEN '1' THEN 'SUCCEEDED' WHEN '2' THEN 'REJECTED' WHEN '3' THEN 'FAILED' ELSE status END; -- CommandJournalStatus

-- inbound_events
ALTER TABLE inbound_events ALTER COLUMN status TYPE TEXT USING status::text;
UPDATE inbound_events SET status = CASE status WHEN '0' THEN 'RECEIVED' WHEN '1' THEN 'DELIVERED' WHEN '2' THEN 'FAILED' WHEN '3' THEN 'IGNORED' WHEN '4' THEN 'DUPLICATE' ELSE status END; -- InboundEventStatus

-- payment_process_manager
ALTER TABLE payment_process_manager ALTER COLUMN process_status TYPE TEXT USING process_status::text;
UPDATE payment_process_manager SET process_status = CASE process_status WHEN '0' THEN 'AWAITING_PAYMENT_RESULT' WHEN '1' THEN 'ORDER_PLACED' WHEN '2' THEN 'FAILED' ELSE process_status END; -- PaymentProcessStatus
ALTER TABLE payment_process_manager ALTER COLUMN payment_status TYPE TEXT USING payment_status::text;
UPDATE payment_process_manager SET payment_status = CASE payment_status WHEN '0' THEN 'PENDING' WHEN '1' THEN 'CAPTURED' WHEN '2' THEN 'FAILED' WHEN '3' THEN 'REFUNDED' ELSE payment_status END; -- PaymentStatus

-- refund_process_manager
ALTER TABLE refund_process_manager ALTER COLUMN process_status TYPE TEXT USING process_status::text;
UPDATE refund_process_manager SET process_status = CASE process_status WHEN '0' THEN 'PENDING_APPROVAL' WHEN '1' THEN 'APPROVED_AWAITING_SETTLEMENT' WHEN '2' THEN 'DENIED' WHEN '3' THEN 'REFUNDED' ELSE process_status END; -- RefundProcessStatus

-- delivery_dispatch_process_manager
ALTER TABLE delivery_dispatch_process_manager ALTER COLUMN process_status TYPE TEXT USING process_status::text;
UPDATE delivery_dispatch_process_manager SET process_status = CASE process_status WHEN '0' THEN 'OFFERED' WHEN '1' THEN 'ACCEPTED' WHEN '2' THEN 'FAILED' WHEN '3' THEN 'COMPLETED' WHEN '4' THEN 'SELF_DISPATCHED' ELSE process_status END; -- DeliveryDispatchProcessStatus

-- uberestimationpolicy
ALTER TABLE uberestimationpolicy ALTER COLUMN cuisine_category TYPE TEXT USING cuisine_category::text;
UPDATE uberestimationpolicy SET cuisine_category = CASE cuisine_category WHEN '0' THEN 'FAST_FOOD' WHEN '1' THEN 'PIZZA' WHEN '2' THEN 'TRADITIONAL' WHEN '3' THEN 'BISTRONOMIC' WHEN '4' THEN 'FOOD_TRUCK' ELSE cuisine_category END; -- CuisineCategory

-- deliverychannelcatalog
ALTER TABLE deliverychannelcatalog ALTER COLUMN kind TYPE TEXT USING kind::text;
UPDATE deliverychannelcatalog SET kind = CASE kind WHEN '0' THEN 'POOL' WHEN '1' THEN 'PARTNER' ELSE kind END; -- DeliveryChannelKind

-- restaurantdispatchconfig
ALTER TABLE restaurantdispatchconfig ALTER COLUMN mode TYPE TEXT USING mode::text;
UPDATE restaurantdispatchconfig SET mode = CASE mode WHEN '0' THEN 'CAPTAIN' WHEN '1' THEN 'RESTAURANT' ELSE mode END; -- RestaurantDispatchMode

-- restaurant
ALTER TABLE restaurant ALTER COLUMN listing_status TYPE TEXT USING listing_status::text;
UPDATE restaurant SET listing_status = CASE listing_status WHEN '0' THEN 'NON_PARTNER' WHEN '1' THEN 'PASSIVE_PARTNER' WHEN '2' THEN 'ACTIVE_PARTNER' ELSE listing_status END; -- RestaurantListingStatus
ALTER TABLE restaurant ALTER COLUMN cuisine_category TYPE TEXT USING cuisine_category::text;
UPDATE restaurant SET cuisine_category = CASE cuisine_category WHEN '0' THEN 'FAST_FOOD' WHEN '1' THEN 'PIZZA' WHEN '2' THEN 'TRADITIONAL' WHEN '3' THEN 'BISTRONOMIC' WHEN '4' THEN 'FOOD_TRUCK' ELSE cuisine_category END; -- CuisineCategory
ALTER TABLE restaurant ALTER COLUMN gbp_link_status TYPE TEXT USING gbp_link_status::text;
UPDATE restaurant SET gbp_link_status = CASE gbp_link_status WHEN '0' THEN 'UNSET' WHEN '1' THEN 'CONFIGURED' WHEN '2' THEN 'VERIFIED' WHEN '3' THEN 'BROKEN' ELSE gbp_link_status END; -- GbpLinkStatus
ALTER TABLE restaurant ALTER COLUMN status TYPE TEXT USING status::text;
UPDATE restaurant SET status = CASE status WHEN '0' THEN 'DRAFT' WHEN '1' THEN 'ACTIVE' WHEN '2' THEN 'INACTIVE' ELSE status END; -- RestaurantStatus
ALTER TABLE restaurant ALTER COLUMN order_acceptance TYPE TEXT USING order_acceptance::text;
UPDATE restaurant SET order_acceptance = CASE order_acceptance WHEN '0' THEN 'NORMAL' WHEN '1' THEN 'BUSY' WHEN '2' THEN 'PAUSED' ELSE order_acceptance END; -- OrderAcceptanceMode

-- prospectionpipeline
ALTER TABLE prospectionpipeline ALTER COLUMN pipeline_status TYPE TEXT USING pipeline_status::text;
UPDATE prospectionpipeline SET pipeline_status = CASE pipeline_status WHEN '0' THEN 'NEW' WHEN '1' THEN 'CONTACTED' WHEN '2' THEN 'COLD' WHEN '3' THEN 'REPLIED' WHEN '4' THEN 'CONVERTED' ELSE pipeline_status END; -- ProspectPipelineStatus

-- cart
ALTER TABLE cart ALTER COLUMN status TYPE TEXT USING status::text;
UPDATE cart SET status = CASE status WHEN '0' THEN 'OPEN' WHEN '1' THEN 'CHECKED_OUT' ELSE status END; -- CartStatus

-- ordertracking
ALTER TABLE ordertracking ALTER COLUMN status TYPE TEXT USING status::text;
UPDATE ordertracking SET status = CASE status WHEN '0' THEN 'PLACED' WHEN '1' THEN 'ACCEPTED' WHEN '2' THEN 'REJECTED' WHEN '3' THEN 'PREPARING' WHEN '4' THEN 'READY' WHEN '5' THEN 'OUT_FOR_DELIVERY' WHEN '6' THEN 'DELIVERED' WHEN '7' THEN 'CANCELLED_BY_CUSTOMER' WHEN '8' THEN 'CANCELLED_BY_RESTAURANT' ELSE status END; -- OrderStatus
ALTER TABLE ordertracking ALTER COLUMN service_type TYPE TEXT USING service_type::text;
UPDATE ordertracking SET service_type = CASE service_type WHEN '0' THEN 'DELIVERY' WHEN '1' THEN 'COLLECTION' ELSE service_type END; -- ServiceType
ALTER TABLE ordertracking ALTER COLUMN uber_basis TYPE TEXT USING uber_basis::text;
UPDATE ordertracking SET uber_basis = CASE uber_basis WHEN '0' THEN 'ESTIMATED' WHEN '1' THEN 'REAL' ELSE uber_basis END; -- ComparisonBasis
ALTER TABLE ordertracking ALTER COLUMN rider_thumb TYPE TEXT USING rider_thumb::text;
UPDATE ordertracking SET rider_thumb = CASE rider_thumb WHEN '0' THEN 'UP' WHEN '1' THEN 'DOWN' ELSE rider_thumb END; -- ThumbRating
ALTER TABLE ordertracking ALTER COLUMN delivery_timeliness TYPE TEXT USING delivery_timeliness::text;
UPDATE ordertracking SET delivery_timeliness = CASE delivery_timeliness WHEN '0' THEN 'ON_TIME' WHEN '1' THEN 'ACCEPTABLE_DELAY' WHEN '2' THEN 'TOO_LATE' ELSE delivery_timeliness END; -- DeliveryTimeliness
ALTER TABLE ordertracking ALTER COLUMN delivery_status TYPE TEXT USING delivery_status::text;
UPDATE ordertracking SET delivery_status = CASE delivery_status WHEN '0' THEN 'PENDING' WHEN '1' THEN 'ASSIGNED' WHEN '2' THEN 'PICKED_UP' WHEN '3' THEN 'OUT_FOR_DELIVERY' WHEN '4' THEN 'DELIVERED' WHEN '5' THEN 'FAILED' WHEN '6' THEN 'CANCELLED' ELSE delivery_status END; -- DeliveryStatus

-- orderconversation
ALTER TABLE orderconversation ALTER COLUMN status TYPE TEXT USING status::text;
UPDATE orderconversation SET status = CASE status WHEN '0' THEN 'PLACED' WHEN '1' THEN 'ACCEPTED' WHEN '2' THEN 'REJECTED' WHEN '3' THEN 'PREPARING' WHEN '4' THEN 'READY' WHEN '5' THEN 'OUT_FOR_DELIVERY' WHEN '6' THEN 'DELIVERED' WHEN '7' THEN 'CANCELLED_BY_CUSTOMER' WHEN '8' THEN 'CANCELLED_BY_RESTAURANT' ELSE status END; -- OrderStatus

-- 3. Text-predicate retention sweep (specs/database/functions/sweep_retention.sql).
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

-- 4. The ordinal lookups are retired.
DROP TABLE IF EXISTS ref_cart_status;
DROP TABLE IF EXISTS ref_catalog_item_availability;
DROP TABLE IF EXISTS ref_city_availability_status;
DROP TABLE IF EXISTS ref_claim_timeline_event_kind;
DROP TABLE IF EXISTS ref_command_channel;
DROP TABLE IF EXISTS ref_command_journal_status;
DROP TABLE IF EXISTS ref_comparison_basis;
DROP TABLE IF EXISTS ref_conversation_author_role;
DROP TABLE IF EXISTS ref_cuisine_category;
DROP TABLE IF EXISTS ref_delivery_channel_kind;
DROP TABLE IF EXISTS ref_delivery_dispatch_process_status;
DROP TABLE IF EXISTS ref_delivery_provider;
DROP TABLE IF EXISTS ref_delivery_status;
DROP TABLE IF EXISTS ref_delivery_timeliness;
DROP TABLE IF EXISTS ref_gbp_link_status;
DROP TABLE IF EXISTS ref_inbound_event_status;
DROP TABLE IF EXISTS ref_message_visibility;
DROP TABLE IF EXISTS ref_mode;
DROP TABLE IF EXISTS ref_operation_status;
DROP TABLE IF EXISTS ref_order_acceptance_mode;
DROP TABLE IF EXISTS ref_order_status;
DROP TABLE IF EXISTS ref_outreach_channel;
DROP TABLE IF EXISTS ref_payment_process_status;
DROP TABLE IF EXISTS ref_payment_status;
DROP TABLE IF EXISTS ref_price_range;
DROP TABLE IF EXISTS ref_prospect_pipeline_status;
DROP TABLE IF EXISTS ref_reclamation_category;
DROP TABLE IF EXISTS ref_reclamation_resolution;
DROP TABLE IF EXISTS ref_reclamation_status;
DROP TABLE IF EXISTS ref_refund_process_status;
DROP TABLE IF EXISTS ref_refund_status;
DROP TABLE IF EXISTS ref_restaurant_dispatch_mode;
DROP TABLE IF EXISTS ref_restaurant_list_key;
DROP TABLE IF EXISTS ref_restaurant_listing_status;
DROP TABLE IF EXISTS ref_restaurant_status;
DROP TABLE IF EXISTS ref_rider_status;
DROP TABLE IF EXISTS ref_service_type;
DROP TABLE IF EXISTS ref_stock_status;
DROP TABLE IF EXISTS ref_thumb_rating;
DROP TABLE IF EXISTS ref_tip_recipient;
DROP TABLE IF EXISTS ref_tipper;
DROP TABLE IF EXISTS ref_user_type;
DROP TABLE IF EXISTS ref_weekday;

-- 5. Fold views, regenerated (specs/generated/views.generated.sql).
-- Read models realized as SQL VIEWS: a `CREATE OR REPLACE VIEW` state-fold over domain_events, generated
-- from each column's `from` lineage (ADR-0039). Read models whose columns are COMPUTED are materialized
-- tables in tables/projection_tables.yaml (emitted into schema.generated.sql) instead.

CREATE OR REPLACE VIEW View_RestaurantAccount AS
SELECT
  (c.payload->>'restaurantAccountId')::uuid AS restaurant_account_id,
  c.payload->>'ref' AS ref,
  (SELECT e.payload->>'legalName' FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('RestaurantAccountRegistered', 'RestaurantAccountUpdated') AND e.payload ? 'legalName'
     ORDER BY e.position DESC LIMIT 1) AS legal_name,
  c.payload->>'defaultCurrency' AS default_currency,
  (SELECT e.payload->>'timezone' FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('RestaurantAccountRegistered', 'RestaurantAccountUpdated') AND e.payload ? 'timezone'
     ORDER BY e.position DESC LIMIT 1) AS timezone,
  c.occurred_at AS created_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('RestaurantAccountRegistered', 'RestaurantAccountUpdated', 'RestaurantAccountDeleted')) AS updated_at
FROM domain_events c
WHERE c.event_type = 'RestaurantAccountRegistered'
  AND NOT EXISTS (SELECT 1 FROM domain_events d
                  WHERE d.stream_name = c.stream_name AND d.event_type = 'RestaurantAccountDeleted');

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
  c.occurred_at AS created_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryRequested', 'DeliveryAcceptedByPartner', 'DeliveryRejectedByPartner', 'DeliveryStatusUpdated', 'DeliveryAcceptedByRider', 'DeliveryPickedUp', 'DeliveryCompleted', 'DeliveryCancelled', 'DeliveryDispatchFailed')) AS updated_at
FROM domain_events c
WHERE c.event_type = 'DeliveryRequested';

CREATE OR REPLACE VIEW View_DeliverySatisfaction AS
SELECT
  (c.payload->>'orderId')::uuid AS order_id,
  (c.payload->>'restaurantId')::uuid AS restaurant_id,
  c.payload->>'timeliness' AS timeliness,
  c.payload->>'reason' AS reason,
  c.occurred_at AS recorded_at,
  c.occurred_at AS created_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliverySatisfactionRecorded')) AS updated_at
FROM domain_events c
WHERE c.event_type = 'DeliverySatisfactionRecorded';

CREATE OR REPLACE VIEW View_DeliveryPartnerAvailability AS
SELECT
  (c.payload->>'registrationId')::uuid AS registration_id,
  c.payload->>'channel' AS channel,
  (c.payload->>'cityId')::uuid AS city_id,
  c.payload->>'partnerName' AS partner_name,
  c.payload->>'contactEmail' AS contact_email,
  (SELECT CASE e.event_type WHEN 'DeliveryPartnerAvailabilityRequested' THEN 'PENDING' WHEN 'DeliveryPartnerAvailabilityApproved' THEN 'APPROVED' WHEN 'DeliveryPartnerAvailabilityRevoked' THEN 'REVOKED' END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryPartnerAvailabilityRequested', 'DeliveryPartnerAvailabilityApproved', 'DeliveryPartnerAvailabilityRevoked')
     ORDER BY e.position DESC LIMIT 1) AS status,
  c.occurred_at AS requested_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryPartnerAvailabilityApproved', 'DeliveryPartnerAvailabilityRevoked')) AS decided_at,
  c.occurred_at AS created_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('DeliveryPartnerAvailabilityRequested', 'DeliveryPartnerAvailabilityApproved', 'DeliveryPartnerAvailabilityRevoked')) AS updated_at
FROM domain_events c
WHERE c.event_type = 'DeliveryPartnerAvailabilityRequested';

CREATE OR REPLACE VIEW View_Reclamation AS
SELECT
  (c.payload->>'reclamationId')::uuid AS reclamation_id,
  (c.payload->>'orderId')::uuid AS order_id,
  (c.payload->>'customerId')::uuid AS customer_id,
  (c.payload->>'restaurantId')::uuid AS restaurant_id,
  c.payload->>'category' AS category,
  c.payload->>'description' AS description,
  c.payload->>'requestedResolution' AS requested_resolution,
  (SELECT CASE e.event_type WHEN 'ReclamationOpened' THEN 'OPEN' WHEN 'ReclamationResolved' THEN 'RESOLVED' WHEN 'ReclamationRejected' THEN 'REJECTED' WHEN 'ReclamationReopened' THEN 'OPEN' END FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('ReclamationOpened', 'ReclamationResolved', 'ReclamationRejected', 'ReclamationReopened')
     ORDER BY e.position DESC LIMIT 1) AS status,
  (SELECT e.payload->>'resolution' FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('ReclamationResolved') AND e.payload ? 'resolution'
     ORDER BY e.position DESC LIMIT 1) AS resolution,
  (SELECT (e.payload->'refundAmount'->>'amountCents')::bigint FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('ReclamationResolved') AND e.payload ? 'refundAmount'
     ORDER BY e.position DESC LIMIT 1) AS refund_amount_cents,
  (SELECT e.payload->'refundAmount'->>'currency' FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('ReclamationResolved') AND e.payload ? 'refundAmount'
     ORDER BY e.position DESC LIMIT 1) AS currency,
  (SELECT e.payload->>'reason' FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('ReclamationRejected') AND e.payload ? 'reason'
     ORDER BY e.position DESC LIMIT 1) AS reject_reason,
  c.occurred_at AS opened_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('ReclamationResolved', 'ReclamationRejected')) AS decided_at,
  c.occurred_at AS created_at,
  (SELECT max(e.occurred_at) FROM domain_events e
     WHERE e.stream_name = c.stream_name AND e.event_type IN ('ReclamationOpened', 'ReclamationResolved', 'ReclamationRejected', 'ReclamationReopened')) AS updated_at
FROM domain_events c
WHERE c.event_type = 'ReclamationOpened';

CREATE OR REPLACE VIEW View_PendingRefunds AS
SELECT
  (c.payload->>'orderId')::uuid AS order_id,
  (c.payload->>'restaurantId')::uuid AS restaurant_id,
  (SELECT CASE e.event_type WHEN 'RefundOpened' THEN 'REQUESTED' WHEN 'RefundApproved' THEN 'APPROVED' WHEN 'RefundDenied' THEN 'DENIED' WHEN 'PaymentRefunded' THEN 'REFUNDED' END FROM domain_events e
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
