-- claude_ro: SELECT-only diagnosis role for agent sessions
-- (ADR-20260807-002705 D7; PROP-20260806-223656 s2b practice 5; #360).
--
-- Split of responsibilities, on purpose:
--   - ROLE LIFECYCLE (login, password) belongs to the platform: CNPG `managed.roles` creates
--     the LOGIN role from the sealed `claude-ro-credentials` secret
--     (deploy/platform/cnpg/cluster.yaml). This migration only ensures the role EXISTS so the
--     grants below always have a target -- as NOLOGIN if the platform has not created it,
--     which is exactly right for local dev / CI where nobody should log in as claude_ro.
--   - WHAT IT MAY READ travels with the schema it reads: this migration, in the ordinary
--     chain, so a fresh CNPG bootstrap (D6 clean start), local dev and CI all converge on the
--     same grants without a console step.
--
-- Runs everywhere the chain runs (local dev superuser, CI throwaway postgres, today's Supabase,
-- tomorrow's CNPG `app` owner). If the migration user may not CREATE ROLE (some managed
-- providers), the role is skipped WITH A LOUD NOTICE and the grants are skipped too -- rerun
-- after creating the role out-of-band; SELECT grants themselves only require object ownership,
-- which the migration user has (it created every table in this schema).

DO $$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = 'claude_ro') THEN
    BEGIN
      CREATE ROLE claude_ro NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT;
    EXCEPTION WHEN insufficient_privilege THEN
      RAISE NOTICE 'claude_ro not created (migration user lacks CREATEROLE); grants skipped -- create the role and re-run';
      RETURN;
    END;
  END IF;

  -- Diagnosis = read the public schema, nothing else: no sequences (nextval is a WRITE), no
  -- functions beyond PUBLIC defaults, no other schemas.
  GRANT USAGE ON SCHEMA public TO claude_ro;
  GRANT SELECT ON ALL TABLES IN SCHEMA public TO claude_ro;

  -- Future tables/views created by the SAME migration user (every later migration in this
  -- chain) are readable without a follow-up grant. A table created by a DIFFERENT role would
  -- not be covered -- in this schema that does not happen: migrations are the only DDL path
  -- (specs/database DSL discipline).
  ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO claude_ro;
END $$;
