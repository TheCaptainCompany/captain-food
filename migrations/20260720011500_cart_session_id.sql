-- Catch-up: the Cart projection table predates the sessionId introduction (CartBindingProcess),
-- so the deployed schema lacks the column the projector INSERTs — every Cart event was being
-- skipped in production ("column session_id does not exist"). Matches the generated DDL
-- (specs/generated/schema.generated.sql): session_id UUID NOT NULL + index.
ALTER TABLE Cart ADD COLUMN IF NOT EXISTS session_id UUID;
-- Pre-existing rows (written before sessionId existed) get the nil session: no live session can
-- ever match it, so CartBindingProcess ignores them by construction.
UPDATE Cart SET session_id = '00000000-0000-0000-0000-000000000000' WHERE session_id IS NULL;
ALTER TABLE Cart ALTER COLUMN session_id SET NOT NULL;
CREATE INDEX IF NOT EXISTS cart_session_id_idx ON Cart (session_id);
