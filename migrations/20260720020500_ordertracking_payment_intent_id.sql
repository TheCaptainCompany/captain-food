-- Catch-up: OrderTracking predates the payment_intent_id column (added for RefundProcess to open
-- pending refunds from the captured intent). The projector upserts every column, so production
-- skipped every Order-group event ("column payment_intent_id does not exist") — the smoke order
-- never materialized. Matches the generated DDL (nullable TEXT).
ALTER TABLE OrderTracking ADD COLUMN IF NOT EXISTS payment_intent_id TEXT;

-- Refold the history the two schema-drift incidents skipped (Cart.session_id, this column):
-- projectors are idempotent per-position upserts, so rewinding the checkpoints is safe and
-- backfills the rows those skipped events should have produced.
UPDATE projection_checkpoint SET position = 0 WHERE projector IN ('Order', 'Cart');
