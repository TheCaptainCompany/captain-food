-- 0001 baseline (ADR-0043): establishes schema version 1.
--
-- The domain schema (event store `domain_events`/`domain_stream`, `ref_*` enum lookups, projection
-- tables, and the generated `View_*` read models) is bootstrapped from specs/generated/*.sql in a later
-- migration, once it has been test-applied against the database. This baseline intentionally makes no
-- schema change — it only marks the starting version so the app's readiness gate has something to check.
SELECT 1;
