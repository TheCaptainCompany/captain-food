-- Cart becomes a MONEY-FREE pure fold (#451, PROP-20260810-231500 Option B, ADR-20260810-112836):
-- the price a customer sees is computed AT READ TIME by application::pricing::price_cart against
-- the live catalog — never materialized in the projection. `lines` (kept, jsonb) changes SHAPE to
-- the repricing inputs only: [{ cart_line_id, offer_id, quantity, selected_option_ids }].
--
-- EXPAND HALF ONLY (#451 Phase 2b). The four now-unread priced columns —
-- total_amount_cents, currency, estimated_breakdown, uber_comparison — are deliberately NOT dropped
-- here. They were 0/NULL stubs in practice (the projector never priced them) and the new binary no
-- longer selects them (`cart_store::COLUMNS`), so leaving them costs a few dead bytes per row and
-- nothing else.
--
-- What dropping them here would cost: render.yaml is `plan: free`, ONE instance, so a deploy that
-- fails after db-migrate rolls back to the PREVIOUS binary — which SELECTs those columns. The
-- health gate (`applied >= REQUIRED_SCHEMA_VERSION`) would still answer 200, because the schema is
-- ahead, not behind. Green health, every cart read 500ing, and no way back except rolling the
-- schema forward under pressure. Keeping the columns makes that rollback boring.
--
-- The CONTRACT half (the four DROP COLUMNs) is issue #470 "Contract migration: drop the four Cart
-- money columns once the money-free binary is stable" — a SEPARATE, LATER change, deliberately not
-- a second file in this one: `db-migrate` applies all pending migrations in a single deploy, so a
-- contract file added now would drop the columns in the very deploy this note exists to make
-- reversible, and would also force REQUIRED_SCHEMA_VERSION forward (the codegen guard pins it to
-- the newest migration). It ships once this binary has been smoked in production.
--
-- OPS NOTE (#264 disk lesson): schedule OFF-PEAK (never Fri/Sat 19:00-21:30 Europe/Paris). The
-- DELETE + checkpoint rewind below replays the Cart projection; Cart streams are small and
-- retention-trimmed, but the replay still churns WAL — off-peak costs nothing, peak does.

-- Restore-by-replay (the 20260720020500 pattern): existing rows carry the OLD priced `lines` shape;
-- the fold is pure, so a rewind rebuilds every row in the new shape. DELETE (not TRUNCATE) keeps the
-- statement transactional with the checkpoint rewind.
DELETE FROM Cart;
UPDATE projection_checkpoint SET position = 0 WHERE projector = 'Cart';

-- The claim-resolved "most-recently-updated OPEN cart" lookup (queries/current) and the
-- `by_customer` list: without this the per-customer newest-first read is a seq scan. Status is
-- filtered in SQL (a two-value enum — leading columns stay the selective ones).
-- Created AFTER the DELETE above, so it builds against an empty table instead of indexing rows the
-- same transaction is about to discard.
CREATE INDEX ON Cart (customer_id, updated_at);
