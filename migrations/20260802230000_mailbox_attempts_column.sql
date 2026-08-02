-- Poison bound for mailbox delivery (PROP-20260802-223522 D4, ADR-20260802-224532,
-- specs/database/tables/journals.yaml `inbound_messages.attempts`).
--
-- Counts delivery attempts whose completion TRANSACTION failed with an infrastructure error: the
-- status flip aborts with the transaction, so the row records nothing — this counter is the only
-- evidence, incremented in its own statement outside the failed transaction. At
-- MAILBOX_MAX_DELIVERY_ATTEMPTS the worker flips the row to terminal FAILED with the error
-- recorded, unblocking the lane's head-of-line. Handler verdicts (SUCCEEDED/REJECTED/...) are
-- terminal on their first attempt and never read or write it.
ALTER TABLE inbound_messages ADD COLUMN attempts SMALLINT NOT NULL DEFAULT 0;
