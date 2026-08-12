-- #242 Runtime D -- DROP `command_journal`: `inbound_messages` is the only write-path journal.
-- Product-owner direction, 2026-08-11: "Remove inbound events and command journal from the dsl,
-- the only tables that must remain is inbound messages". Companion to 20260731143000, which
-- backfilled and dropped `inbound_events`; this one closes the second half.
--
-- WHY NOTHING IS BACKFILLED HERE (unlike inbound_events)
-- -----------------------------------------------------
-- `command_journal` rows are ACCEPTANCE HISTORY, not domain facts: one row per command
-- submission with its terminal verdict. They are NOT the event log (aggregates own that via the
-- write-side Repository -- journals never write `domain_events`), they are never replayed as
-- state, and the retention sweep already deletes them 90 days after `completed_at`. What is lost
-- by dropping them is therefore: (a) the `operationStatus` answer for any messageId accepted on
-- the legacy arm, and (b) the PM-leg idempotency memory for those same ids.
--
-- Both are acceptable HERE and ONLY here, in this window:
--   * the event log is EMPTY BY DECISION (start-clean, ADR-20260807-002705 D6) and production is
--     down, so there are no historical acceptances to answer for and no in-flight client poll;
--   * the ids at risk are exactly those the legacy arm accepted, and the legacy arm is deleted in
--     the same change -- no future submission can land there;
--   * a straggler is refused rather than silently reordered: the guard below fails the migration
--     if any RECEIVED row still exists, exactly like 20260731143000's.
-- Deploying this against a database with live command history would need a backfill into
-- `inbound_messages` first (id-preserving, the way 20260731143000 did it) -- do not copy this
-- migration's shape into that situation.
--
-- ORDERING (expand/contract): the code that reads and writes this table is deleted in the SAME
-- change, and `sweep_retention()` is redeployed below without its `command_journal` section --
-- the function is CREATE OR REPLACE'd from the generated schema, so dropping the table first
-- would leave a stale function body referencing a missing relation.

BEGIN;

-- WRITE-FENCE, mirroring 20260731143000: an old-code instance still journaling would otherwise
-- insert between the guard and the DROP.
LOCK TABLE command_journal IN ACCESS EXCLUSIVE MODE;

DO $$
DECLARE
  stragglers BIGINT;
BEGIN
  SELECT count(*) INTO stragglers FROM command_journal WHERE status = 'RECEIVED';
  IF stragglers > 0 THEN
    RAISE EXCEPTION
      'command_journal still holds % RECEIVED row(s) -- drain the legacy arm to empty before '
      'applying this migration (a command accepted but never handled would be silently lost)',
      stragglers;
  END IF;
END;
$$;

-- The retention sweep loses its `command_journal` section. Kept in step with
-- specs/database/functions/sweep_retention.sql (the generated schema is the source of truth);
-- the windows themselves are unchanged.
CREATE OR REPLACE FUNCTION sweep_retention()
RETURNS TABLE (swept_table TEXT, deleted BIGINT)
LANGUAGE plpgsql
AS $$
DECLARE
  n BIGINT;
BEGIN
  DELETE FROM inbound_messages
   WHERE status IN ('SUCCEEDED', 'REJECTED', 'FAILED', 'IGNORED', 'DUPLICATE', 'CANCELLED')
     AND completed_at IS NOT NULL
     AND completed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'inbound_messages'; deleted := n; RETURN NEXT;

  DELETE FROM external_stripe_events
   WHERE processed_at IS NOT NULL
     AND processed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'external_stripe_events'; deleted := n; RETURN NEXT;

  DELETE FROM external_hubrise_callbacks
   WHERE processed_at IS NOT NULL
     AND processed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'external_hubrise_callbacks'; deleted := n; RETURN NEXT;

  DELETE FROM external_avelo37_events
   WHERE processed_at IS NOT NULL
     AND processed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'external_avelo37_events'; deleted := n; RETURN NEXT;

  DELETE FROM external_uber_direct_events
   WHERE processed_at IS NOT NULL
     AND processed_at < now() - INTERVAL '90 days';
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'external_uber_direct_events'; deleted := n; RETURN NEXT;

  DELETE FROM auth_sessions
   WHERE expires_at < now();
  GET DIAGNOSTICS n = ROW_COUNT;
  swept_table := 'auth_sessions'; deleted := n; RETURN NEXT;
END;
$$;

DROP TABLE command_journal;

-- The PM_MAILBOX_DELIVERY posture retires WITH the table it selected: its OFF arm WAS
-- `command_journal`, so with the table gone the row could only ever mean one thing -- and a
-- control that renders but does nothing is worse than no control (ADR-20260812-000000).
-- `RuntimePosture` itself STAYS: the mechanism (#318, ADR-20260803-104819) is right and the next
-- process-wide posture needs it; it simply has no tenant today.
DELETE FROM RuntimePosture WHERE posture = 'PM_MAILBOX_DELIVERY';

COMMIT;
