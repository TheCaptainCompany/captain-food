# ADR-20260720-015300 — Command sourcing: a durable `command_journal` written before handling

## Status

Accepted — landed with the acceptance-first GraphQL surface (ADR-20260720-015500) and the inbound
journal (ADR-20260720-015400). Amends ADR-0046 (dispatch becomes journal-then-spawn); extends
ADR-0041 (the journal row IS the technical envelope, persisted); realizes the P-02 identifier
contract (`message_id`) on the write path.

## Context

Until now a mutation was accepted at the GraphQL boundary, validated, handled and appended to
`domain_events` inside one synchronous request (ADR-0046). A request could therefore disappear
between validation, dispatch and append with no durable trace: no receipt of the caller's intent,
no deterministic idempotent-retry key, no record of **rejected** commands (a rejection leaves zero
rows anywhere), and no stable handle a caller can use to observe an asynchronous outcome. The
platform principles already demanded durable `message_id`/`correlation_id`/`cause_id` propagation
(P-02) and the distinction between transport success and business success (P-10).

Two constraints were pre-agreed for this work (STATUS.md): journals NEVER write `domain_events`
(aggregates own the log via the write-side `Repository`, ADR-20260719-031136), and the event log
stays the single source of truth (a journal records requests — it never replays as state).

## Decision

Every accepted write request is **persisted as a command record before domain handling starts** —
command sourcing for inbound write intent, complementing (not replacing) the domain event store.

- **Table `command_journal`** (declared in `specs/database/tables/journals.yaml`, DDL generated):
  pk **`message_id`** (client-suppliable, else server-generated **UUIDv7**), envelope columns
  (`correlation_id`, `cause_id`, `session_id`, `trace_id`, `user_id`, `user_type`, `channel`),
  `command_type` (commands.yaml key), the **business payload only** as `payload` jsonb (the
  envelope is columns — ADR-0041), a `payload_hash` (sha256 over the canonical payload), a
  lifecycle `status`, an `error` jsonb (`{code, context}`, errors.yaml code) and
  `received_at`/`completed_at`.
- **Lifecycle** (`CommandJournalStatus` scalar): `RECEIVED → SUCCEEDED | REJECTED | FAILED`.
  Deliberately minimal: no `IN_PROGRESS` (the handler runs immediately after insert; a
  progress state would only widen the crash window semantics), no `DUPLICATE` status (a duplicate
  is an **acceptance-response** attribute — the journal row keeps the original's real status).
- **Idempotency**: re-submitting a `message_id` with the **same** `payload_hash` acknowledges
  against the original row (its current status, `duplicate: true`); the **same id with a different
  payload is rejected** (`Conflict`) — that is a client bug, not a retry.
- **Causality**: events appended by the handler carry `cause_id = message_id` in `domain_events`
  (the existing envelope column — no event-store schema change), so
  request → command record → domain events is one traceable chain.
- **Channels** (`CommandChannel`): `GRAPHQL` (the BFF dispatch), `WORKER` (on-app drain/enrichment
  workers issuing commands, e.g. HubRise import), `INTERNAL` (internal triggers). All command
  submissions converge on the same journal regardless of origin.
- The journal **never writes `domain_events`** and is **never replayed as state**; recovery from a
  crash between insert and completion is a stale-`RECEIVED` sweep (marked `FAILED` after a
  timeout), not a re-execution.

## Alternatives considered

- **No journal (status quo)** — rejected: no durable receipt, weak retries, invisible rejections,
  and no honest asynchronous GraphQL contract (ADR-20260720-015500 depends on this journal).
- **Store commands in `domain_events`** — rejected: commands are *requests*, domain events are
  *accepted facts*; mixing them makes replay semantics ambiguous and violates the pre-agreed
  single-source-of-truth constraint.
- **Doc-style rich lifecycle (RECEIVED/ACCEPTED/IN_PROGRESS/…/DUPLICATE)** — simplified as above;
  states that neither the runtime nor the callers can distinguish are liability, not observability.

## Consequences

### Positive
- Durable acceptance before execution; rejected commands finally leave a trace (support/audit).
- Deterministic duplicate resolution keyed by `message_id` + `payload_hash`.
- A stable caller handle (`messageId`) for `operationStatus` polling and subscriptions.
- Envelope ids propagate end-to-end (journal → `Actor` → `domain_events.correlation_id/cause_id`).

### Negative
- A second persistent log to operate (retention/indexing policy separate from `domain_events`).
- Spawned handling outlives the request → the stale-`RECEIVED` sweep is mandatory hygiene.
- Enum ordinals (`ref_command_journal_status`, `ref_command_channel`) freeze at first migration —
  append-only forever (ADR-0037 ordinal convention).

### Follow-up actions
- Retention policy for `command_journal` (usefulness window ≠ event store's forever).
- ~~Route the HubRise enricher's command sends through the journaling dispatch (`channel: WORKER`).~~
  Done (#15): the HubRise enricher AND the SIRENE sync worker dispatch via
  `application::dispatch::dispatch_journaled`, with deterministic UUIDv5 `message_id`s for
  idempotent redelivery.
