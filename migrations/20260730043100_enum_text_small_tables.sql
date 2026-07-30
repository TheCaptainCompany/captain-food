-- Enum storage: INTEGER declaration-order ordinals -> the enum's TEXT value, verbatim
-- (ADR-20260728-170000). Replaces the retired single-file 20260728170000_enum_text_storage.sql,
-- whose one-transaction rewrite of every table at once blew the 2 GB disk on production
-- ("could not extend file: no space left on device") and rolled back cleanly.
-- Split: one transaction per table group, conversion folded into ALTER ... USING (a single
-- table rewrite, no separate UPDATE pass), biggest tables alone, views recreated last.

-- 1) The fold views read domain_events and are recreated (with text CASEs) at the end of the split;
-- out of the way first so no ALTER can trip on a dependent view.
DROP VIEW IF EXISTS View_RestaurantAccount;
DROP VIEW IF EXISTS View_DeliveryJob;
DROP VIEW IF EXISTS View_DeliverySatisfaction;
DROP VIEW IF EXISTS View_DeliveryPartnerAvailability;
DROP VIEW IF EXISTS View_Reclamation;
DROP VIEW IF EXISTS View_PendingRefunds;

-- 2) Retention predicates become text (the ref_* lookups the old body resolved through are retired
-- below; between this migration and the journal conversions the sweep would error benignly and
-- retry -- it runs every 6 h and tolerates failure).
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

-- 3) The ordinal lookups are retired.
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

-- 4) Small-table conversions, one rewrite each.
ALTER TABLE payment_process_manager
  ALTER COLUMN process_status TYPE TEXT USING (CASE process_status WHEN 0 THEN 'AWAITING_PAYMENT_RESULT' WHEN 1 THEN 'ORDER_PLACED' WHEN 2 THEN 'FAILED' ELSE process_status::text END),
  ALTER COLUMN payment_status TYPE TEXT USING (CASE payment_status WHEN 0 THEN 'PENDING' WHEN 1 THEN 'CAPTURED' WHEN 2 THEN 'FAILED' WHEN 3 THEN 'REFUNDED' ELSE payment_status::text END);

ALTER TABLE refund_process_manager
  ALTER COLUMN process_status TYPE TEXT USING (CASE process_status WHEN 0 THEN 'PENDING_APPROVAL' WHEN 1 THEN 'APPROVED_AWAITING_SETTLEMENT' WHEN 2 THEN 'DENIED' WHEN 3 THEN 'REFUNDED' ELSE process_status::text END);

ALTER TABLE delivery_dispatch_process_manager
  ALTER COLUMN process_status TYPE TEXT USING (CASE process_status WHEN 0 THEN 'OFFERED' WHEN 1 THEN 'ACCEPTED' WHEN 2 THEN 'FAILED' WHEN 3 THEN 'COMPLETED' WHEN 4 THEN 'SELF_DISPATCHED' ELSE process_status::text END);

ALTER TABLE uberestimationpolicy
  ALTER COLUMN cuisine_category TYPE TEXT USING (CASE cuisine_category WHEN 0 THEN 'FAST_FOOD' WHEN 1 THEN 'PIZZA' WHEN 2 THEN 'TRADITIONAL' WHEN 3 THEN 'BISTRONOMIC' WHEN 4 THEN 'FOOD_TRUCK' ELSE cuisine_category::text END);

ALTER TABLE deliverychannelcatalog
  ALTER COLUMN kind TYPE TEXT USING (CASE kind WHEN 0 THEN 'POOL' WHEN 1 THEN 'PARTNER' ELSE kind::text END);

ALTER TABLE restaurantdispatchconfig
  ALTER COLUMN mode TYPE TEXT USING (CASE mode WHEN 0 THEN 'CAPTAIN' WHEN 1 THEN 'RESTAURANT' ELSE mode::text END);

ALTER TABLE prospectionpipeline
  ALTER COLUMN pipeline_status TYPE TEXT USING (CASE pipeline_status WHEN 0 THEN 'NEW' WHEN 1 THEN 'CONTACTED' WHEN 2 THEN 'COLD' WHEN 3 THEN 'REPLIED' WHEN 4 THEN 'CONVERTED' ELSE pipeline_status::text END);

ALTER TABLE cart
  ALTER COLUMN status TYPE TEXT USING (CASE status WHEN 0 THEN 'OPEN' WHEN 1 THEN 'CHECKED_OUT' ELSE status::text END);

ALTER TABLE orderconversation
  ALTER COLUMN status TYPE TEXT USING (CASE status WHEN 0 THEN 'PLACED' WHEN 1 THEN 'ACCEPTED' WHEN 2 THEN 'REJECTED' WHEN 3 THEN 'PREPARING' WHEN 4 THEN 'READY' WHEN 5 THEN 'OUT_FOR_DELIVERY' WHEN 6 THEN 'DELIVERED' WHEN 7 THEN 'CANCELLED_BY_CUSTOMER' WHEN 8 THEN 'CANCELLED_BY_RESTAURANT' ELSE status::text END);

ALTER TABLE ordertracking
  ALTER COLUMN status TYPE TEXT USING (CASE status WHEN 0 THEN 'PLACED' WHEN 1 THEN 'ACCEPTED' WHEN 2 THEN 'REJECTED' WHEN 3 THEN 'PREPARING' WHEN 4 THEN 'READY' WHEN 5 THEN 'OUT_FOR_DELIVERY' WHEN 6 THEN 'DELIVERED' WHEN 7 THEN 'CANCELLED_BY_CUSTOMER' WHEN 8 THEN 'CANCELLED_BY_RESTAURANT' ELSE status::text END),
  ALTER COLUMN service_type TYPE TEXT USING (CASE service_type WHEN 0 THEN 'DELIVERY' WHEN 1 THEN 'COLLECTION' ELSE service_type::text END),
  ALTER COLUMN uber_basis TYPE TEXT USING (CASE uber_basis WHEN 0 THEN 'ESTIMATED' WHEN 1 THEN 'REAL' ELSE uber_basis::text END),
  ALTER COLUMN rider_thumb TYPE TEXT USING (CASE rider_thumb WHEN 0 THEN 'UP' WHEN 1 THEN 'DOWN' ELSE rider_thumb::text END),
  ALTER COLUMN delivery_timeliness TYPE TEXT USING (CASE delivery_timeliness WHEN 0 THEN 'ON_TIME' WHEN 1 THEN 'ACCEPTABLE_DELAY' WHEN 2 THEN 'TOO_LATE' ELSE delivery_timeliness::text END),
  ALTER COLUMN delivery_status TYPE TEXT USING (CASE delivery_status WHEN 0 THEN 'PENDING' WHEN 1 THEN 'ASSIGNED' WHEN 2 THEN 'PICKED_UP' WHEN 3 THEN 'OUT_FOR_DELIVERY' WHEN 4 THEN 'DELIVERED' WHEN 5 THEN 'FAILED' WHEN 6 THEN 'CANCELLED' ELSE delivery_status::text END);
