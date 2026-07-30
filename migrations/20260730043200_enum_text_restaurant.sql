-- Enum storage: INTEGER declaration-order ordinals -> the enum's TEXT value, verbatim
-- (ADR-20260728-170000). Replaces the retired single-file 20260728170000_enum_text_storage.sql,
-- whose one-transaction rewrite of every table at once blew the 2 GB disk on production
-- ("could not extend file: no space left on device") and rolled back cleanly.
-- Split: one transaction per table group, conversion folded into ALTER ... USING (a single
-- table rewrite, no separate UPDATE pass), biggest tables alone, views recreated last.

-- restaurant carries every SIRENE prospect (~hundreds of thousands of rows): its own transaction.
ALTER TABLE restaurant
  ALTER COLUMN listing_status TYPE TEXT USING (CASE listing_status WHEN 0 THEN 'NON_PARTNER' WHEN 1 THEN 'PASSIVE_PARTNER' WHEN 2 THEN 'ACTIVE_PARTNER' ELSE listing_status::text END),
  ALTER COLUMN cuisine_category TYPE TEXT USING (CASE cuisine_category WHEN 0 THEN 'FAST_FOOD' WHEN 1 THEN 'PIZZA' WHEN 2 THEN 'TRADITIONAL' WHEN 3 THEN 'BISTRONOMIC' WHEN 4 THEN 'FOOD_TRUCK' ELSE cuisine_category::text END),
  ALTER COLUMN gbp_link_status TYPE TEXT USING (CASE gbp_link_status WHEN 0 THEN 'UNSET' WHEN 1 THEN 'CONFIGURED' WHEN 2 THEN 'VERIFIED' WHEN 3 THEN 'BROKEN' ELSE gbp_link_status::text END),
  ALTER COLUMN status TYPE TEXT USING (CASE status WHEN 0 THEN 'DRAFT' WHEN 1 THEN 'ACTIVE' WHEN 2 THEN 'INACTIVE' ELSE status::text END),
  ALTER COLUMN order_acceptance TYPE TEXT USING (CASE order_acceptance WHEN 0 THEN 'NORMAL' WHEN 1 THEN 'BUSY' WHEN 2 THEN 'PAUSED' ELSE order_acceptance::text END);
