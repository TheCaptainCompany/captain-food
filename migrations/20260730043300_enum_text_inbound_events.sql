-- Enum storage: INTEGER declaration-order ordinals -> the enum's TEXT value, verbatim
-- (ADR-20260728-170000). Replaces the retired single-file 20260728170000_enum_text_storage.sql,
-- whose one-transaction rewrite of every table at once blew the 2 GB disk on production
-- ("could not extend file: no space left on device") and rolled back cleanly.
-- Split: one transaction per table group, conversion folded into ALTER ... USING (a single
-- table rewrite, no separate UPDATE pass), biggest tables alone, views recreated last.

-- inbound_events holds the SIRENE delivery backlog: its own transaction.
ALTER TABLE inbound_events
  ALTER COLUMN status TYPE TEXT USING (CASE status WHEN 0 THEN 'RECEIVED' WHEN 1 THEN 'DELIVERED' WHEN 2 THEN 'FAILED' WHEN 3 THEN 'IGNORED' WHEN 4 THEN 'DUPLICATE' ELSE status::text END);
