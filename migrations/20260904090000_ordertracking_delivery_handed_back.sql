-- Review round 2 on #870 (#639 part C step 3-ii, ADR-20260904-015903 §7): the customer tracking
-- banner used to key on `order.status == 'OUT_FOR_DELIVERY'`, a token no OrderStatus producer ever
-- emits (`projectors/order_tracking.rs` yields PLACED/ACCEPTED/PREPARING/READY/DELIVERED/REJECTED/
-- CANCELLED_* only) AND read a SEPARATE `delivery.byOrder` query refreshed only in
-- `TrackingState::load` — the push path (`apply`, the primary transport) never touched it, so the
-- banner could neither render nor refresh live.
--
-- The fix puts the custody fact ON the order mirror itself: `delivery_handed_back` (boolean, default
-- false) is set true by DeliveryHandedBackByRider and reset false by the next
-- DeliveryAcceptedByRider/DeliveryAcceptedByPartner (a re-offer accepted) — folded by
-- `OrderTrackingCompute::delivery_handed_back` alongside the existing `delivery_status`/`courier`
-- mirror arms, so it rides the SAME row the pushed `Order` frame already carries on every
-- `orderStatusChanged` event. The banner predicate becomes `order.deliveryHandedBack == true` — no
-- order-status term, which also correctly covers the from-ASSIGNED NOT_COLLECTED case (the order is
-- only READY there, never OUT_FOR_DELIVERY).
ALTER TABLE OrderTracking ADD COLUMN IF NOT EXISTS delivery_handed_back BOOLEAN NOT NULL DEFAULT false;

-- Refold the history: an order that was ALREADY handed back before this migration landed has its
-- fact recorded in `domain_events` but not yet reflected in the new column (the append-only log is
-- the truth; the projected row is disposable and safe to rebuild, ADR-0040). Same precedent as
-- 20260720020500 (a projector column catch-up rewinds the checkpoint rather than leaving the row
-- stale until the order's NEXT event).
UPDATE projection_checkpoint SET position = 0 WHERE projector = 'Order';
