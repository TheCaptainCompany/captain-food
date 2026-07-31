-- The actor mailbox and its partition registry (ADR-20260730-231500, PROP-20260728-152752 §3.2,
-- specs/database/tables/journals.yaml `inbound_messages` / `mailbox_partitions`).
--
-- Pulled FORWARD from the worker slice (#242 Runtime C) so the ADMIN supervision surface
-- (`mailboxLanes`, Runtime B) is DB-testable now: the tables exist, empty, alongside the still-live
-- command_journal / inbound_events. NOTHING writes them until the typed-client insert path lands;
-- the backfill (command_journal + inbound_events -> inbound_messages in received_at order) and the
-- legacy-table drop are Runtime C's migration, not this one. Forward-only, always.

-- One sequence for ALL actor types: `position` is the single consumption/checkpoint axis.
-- Owned by the column via OWNED BY so a future drop of the table drops the sequence with it.
CREATE SEQUENCE inbound_messages_position_seq AS BIGINT;

CREATE TABLE inbound_messages (
    -- Idempotency identity: the client's acceptance handle (commands), UUIDv5(source, external_id)
    -- (adapted facts), or UUIDv5(actor_id, purpose) (reminders).
    message_id      UUID        PRIMARY KEY,
    -- NULL while SCHEDULED — stamped at insert for immediate rows, at PROMOTION when due (§3.4).
    -- UNIQUE keeps the checkpoint axis unambiguous.
    position        BIGINT      UNIQUE DEFAULT nextval('inbound_messages_position_seq'),
    kind            TEXT        NOT NULL,            -- COMMAND | EVENT | MESSAGE (InboundMessageKind)
    actor_type      TEXT        NOT NULL,            -- actors.yaml actor (aggregate or process manager)
    actor_id        UUID        NOT NULL,            -- addressed instance = stream id
    partition       SMALLINT    NOT NULL,            -- stable_hash(actor_id) mod mailbox.partitions, stamped at insert
    message_type    TEXT        NOT NULL,            -- commands.yaml / events.yaml / messages.yaml key
    payload         JSONB       NOT NULL,            -- BUSINESS payload only — the envelope is the columns (ADR-0041)
    payload_hash    TEXT        NOT NULL,            -- sha256 hex over the canonical payload (duplicate-vs-conflict)
    channel         TEXT        NOT NULL,            -- GRAPHQL | WORKER | EXTERNAL (InboundMessageChannel)
    user_id         UUID            NULL,            -- acting user's auth subject; envelope (ADR-0041)
    user_type       TEXT        NOT NULL,            -- UserType TEXT value verbatim (ADR-20260728)
    correlation_id  UUID        NOT NULL,
    cause_id        UUID            NULL,            -- parent message/event id; NULL for a root command
    session_id      UUID            NULL,            -- X-SESSION-ID — anonymous operationStatus scope
    trace_id        TEXT            NULL,
    source          TEXT            NULL,            -- owning adapter for kind EVENT ('stripe', ...)
    external_id     TEXT            NULL,            -- provider event id for kind EVENT
    scheduled_at    TIMESTAMPTZ     NULL,            -- reminders: eligible only once due (promotion pass)
    -- SCHEDULED -> (CANCELLED | RECEIVED); RECEIVED -> SUCCEEDED | REJECTED | FAILED | IGNORED | DUPLICATE
    status          TEXT        NOT NULL DEFAULT 'RECEIVED',
    error           JSONB           NULL,            -- { code, context } on REJECTED / FAILED
    received_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at    TIMESTAMPTZ     NULL
);

ALTER SEQUENCE inbound_messages_position_seq OWNED BY inbound_messages.position;

-- The drain index: exactly what a worker pass scans (head-of-line within a partition).
CREATE INDEX idx_inbound_messages_drain
    ON inbound_messages (actor_type, partition, position)
    WHERE status = 'RECEIVED';

-- Per-instance history (support/debug) + the Reminders-companion load shares the prefix.
CREATE INDEX idx_inbound_messages_actor
    ON inbound_messages (actor_id, position);

-- The scheduler index: due reminders awaiting promotion (§3.4).
CREATE INDEX idx_inbound_messages_due
    ON inbound_messages (scheduled_at)
    WHERE status = 'SCHEDULED';

-- The Reminders-companion load: one actor's pending reminders, read before every handle (§3.4).
CREATE INDEX idx_inbound_messages_pending_reminders
    ON inbound_messages (actor_id)
    WHERE status = 'SCHEDULED';

-- Delivery-level dedupe for adapted facts: one row per provider event. Partial — commands and
-- reminders have no source and must not collide on (NULL, NULL).
CREATE UNIQUE INDEX idx_inbound_messages_source_external
    ON inbound_messages (source, external_id)
    WHERE source IS NOT NULL;

-- operationStatus reads kind = COMMAND rows by message_id (pk) — no extra index needed.

-- The partition registry: one row per (actor_type, partition) — checkpoint + lease + fencing
-- counter in ONE row, the single authority on a partition (PROP-20260728-152752 §3/§3.1).
-- Rows are seeded BY THE WORKER on startup from each actor's declared mailbox.partitions width
-- (idempotent INSERT .. ON CONFLICT DO NOTHING), not by this migration: the spec-declared widths
-- live in actors.yaml and the migration must not duplicate them.
CREATE TABLE mailbox_partitions (
    actor_type         TEXT        NOT NULL,          -- actors.yaml actor
    partition          SMALLINT    NOT NULL,          -- 0 .. mailbox.partitions-1
    ownership_version  BIGINT      NOT NULL DEFAULT 0, -- fencing counter — NOT a date (§3.1)
    claimed_by         TEXT            NULL,          -- worker instance id; NULL = unowned
    lease_until        TIMESTAMPTZ     NULL,          -- past or NULL = claimable; heartbeat-renewed
    checkpoint         BIGINT      NOT NULL DEFAULT 0, -- everything at or below it is terminal
    PRIMARY KEY (actor_type, partition)
);
