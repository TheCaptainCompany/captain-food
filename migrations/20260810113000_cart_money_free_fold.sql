-- Cart becomes a MONEY-FREE pure fold (#451, PROP-20260810-231500 Option B, ADR-20260810-112836):
-- the price a customer sees is computed AT READ TIME by application::pricing::price_cart against
-- the live catalog — never materialized in the projection. The dropped columns were the impure-fold
-- declaration (and were 0/NULL stubs in practice: the projector never priced them, see
-- crates/application/src/projectors/cart.rs pre-#451). `lines` (kept, jsonb) changes SHAPE to the
-- repricing inputs only: [{ cart_line_id, offer_id, quantity, selected_option_ids }].
--
-- OPS NOTE (#264 disk lesson): schedule OFF-PEAK (never Fri/Sat 19:00-21:30 Europe/Paris). The
-- DELETE + checkpoint rewind below replays the Cart projection; Cart streams are small and
-- retention-trimmed, but the replay still churns WAL — off-peak costs nothing, peak does.

ALTER TABLE Cart DROP COLUMN total_amount_cents;
ALTER TABLE Cart DROP COLUMN currency;
ALTER TABLE Cart DROP COLUMN estimated_breakdown;
ALTER TABLE Cart DROP COLUMN uber_comparison;

-- The claim-resolved "most-recently-updated OPEN cart" lookup (queries/current): without this the
-- per-customer newest-first read is a seq scan. Status is filtered app-side (two-value enum).
CREATE INDEX ON Cart (customer_id, updated_at);

-- Restore-by-replay (the 20260720020500 pattern): existing rows carry the OLD priced `lines` shape;
-- the fold is pure, so a rewind rebuilds every row in the new shape. DELETE (not TRUNCATE) keeps the
-- statement transactional with the checkpoint rewind.
DELETE FROM Cart;
UPDATE projection_checkpoint SET position = 0 WHERE projector = 'Cart';
