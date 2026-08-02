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

-- Attempt pacing (review #314 MAJOR-3): push-driven retries have no natural cadence — a nudge
-- storm at peak could burn the whole cap in seconds on a transient blip. The worker refuses to
-- re-attempt a row before `last_attempt_at + retry spacing`, restoring the approved
-- cap x cadence arithmetic (>= ~50 s to poison at the defaults).
ALTER TABLE inbound_messages ADD COLUMN last_attempt_at TIMESTAMPTZ NULL;

-- The supervision surface (`mailboxLanes.poisoned`) counts terminally-FAILED-by-cap rows per
-- lane; this partial index keeps that count an indexed probe (poison rows are rare by design).
CREATE INDEX idx_inbound_messages_poisoned
    ON inbound_messages (actor_type, partition)
    WHERE status = 'FAILED' AND (error->>'code') = 'DeliveryInfrastructureError';
