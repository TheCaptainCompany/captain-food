-- Enum storage: INTEGER declaration-order ordinals -> the enum's TEXT value, verbatim
-- (ADR-20260728-170000). Replaces the retired single-file 20260728170000_enum_text_storage.sql,
-- whose one-transaction rewrite of every table at once blew the 2 GB disk on production
-- ("could not extend file: no space left on device") and rolled back cleanly.
-- Split: one transaction per table group, conversion folded into ALTER ... USING (a single
-- table rewrite, no separate UPDATE pass), biggest tables alone, views recreated last.

-- domain_events is the forever log and the largest single rewrite: last and alone, after every
-- earlier transaction has committed and released its old relation files.
ALTER TABLE domain_events
  ALTER COLUMN user_type TYPE TEXT USING (CASE user_type WHEN 0 THEN 'PUBLIC' WHEN 1 THEN 'CUSTOMER' WHEN 2 THEN 'RESTAURANT_ACCOUNT' WHEN 3 THEN 'RESTAURANT' WHEN 4 THEN 'RIDER' WHEN 5 THEN 'ADMIN' WHEN 6 THEN 'EXTERNAL' ELSE user_type::text END);
