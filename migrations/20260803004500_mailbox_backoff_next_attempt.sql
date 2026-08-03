-- Exponential backoff for mailbox delivery retries (#316, ADR-20260803-002712 Q2,
-- specs/database/tables/journals.yaml `inbound_messages.next_attempt_at`).
--
-- The #314 fixed spacing paced re-attempts one heartbeat apart; the product owner chose doubling
-- delays instead: attempt N schedules the next try `base * 2^(N-1)` seconds out (base = the
-- retry-spacing/heartbeat, so 10s -> 20s -> 40s -> 80s -> 160s; ~5 min to poison at cap 5).
-- The worker's drain gate holds a lane whose head row has `next_attempt_at` in the future —
-- head-of-line WAIT, never a skip. NULL = deliverable now (fresh rows, and the pacing-off
-- rollback lever). `last_attempt_at` stays as evidence; this column is the schedule.
ALTER TABLE inbound_messages ADD COLUMN next_attempt_at TIMESTAMPTZ NULL;
