-- no-transaction
-- Reclaim the SIRENE mirror's dead space BEFORE the enum-text table rewrites (ADR-20260728-170000
-- follow-up). The payload-transient change (ADR-20260728-143000, #231/#240) NULLed the bulk of
-- external_sirene_restaurants (once ~655 MB = 77% of a 2 GB disk), but plain vacuum only marks the
-- pages reusable inside the file -- the filesystem space stays taken, and it is exactly what the
-- 20260728170000 one-shot conversion died on. VACUUM FULL rewrites the relation compact and returns
-- the space to the OS, giving the table rewrites that follow their headroom.
-- (-- no-transaction: VACUUM cannot run inside a transaction block; on an empty/fresh database this
-- is instant and harmless.)
VACUUM FULL external_sirene_restaurants;
