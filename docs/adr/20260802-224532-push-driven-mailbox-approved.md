# ADR-20260802-224532 — Push-driven mailbox approved as proposed (D1–D5)

## Status

Accepted (product owner, in-session, 2026-08-02)

## Context

[PROP-20260802-223522 "Push-driven mailbox: NOTIFY everywhere, idle gate, poison policy"](../proposals/PROP-20260802-223522-push-driven-mailbox.md)
put five decisions on the table, extending
[#301 "feat(#300): push the drain loops from Postgres NOTIFY instead of polling every 1.5s"](https://github.com/TheCaptainCompany/captain-food/pull/301)
(ADR-20260802-200416) to the actor mailbox — the last polling surface — after the 2026-08-02
audit found it out-polled what #301 removed, left adapters unable to wake workers across
processes (up to 10 s on the money path), and retried infrastructure-failed deliveries forever
with no recorded evidence.

## Decision

All five as recommended:

- **D1 — transport**: `pg_notify('inbound_messages', actor_type)` inside the enqueue
  transaction, at the single `PgMailbox` door (and the PM fact-chain insert). Adapters get
  cross-process wake with zero adapter changes.
- **D2 — topology**: one channel, payload = actor type (per-type coalescing, one LISTEN
  connection per consuming process, no thundering herd).
- **D3 — idle gate**: one lanes-with-work query per pass
  (`SELECT DISTINCT partition … WHERE status='RECEIVED'`, served by `idx_inbound_messages_drain`);
  drain only those lanes. Lease renewal (`beat`) stays on the heartbeat timer unconditionally —
  push replaces the drain trigger, never the lease clock.
- **D4 — poison policy**: `attempts` column on `inbound_messages`; a delivery whose completion
  transaction fails increments it OUTSIDE the failed transaction; at the cap the row flips to
  terminal `FAILED` with the error recorded, unblocking the lane.
- **D5 — gating**: `RUN_MAILBOX_PUSH` (default `true`, degradation to the current poll whenever
  the listener is down) and `MAILBOX_MAX_DELIVERY_ATTEMPTS` (default 5; `0` = today's infinite
  retry — the rollback lever). Declared in `specs/configuration.yaml`.

Unresolved questions (requeue tooling, backoff shape, poison alerting, adapter-fleet posture)
stay open on [#313](https://github.com/TheCaptainCompany/captain-food/issues/313)'s checklist —
approval does not close them.

## Consequences

- Idle DB chatter drops to the safety-net floor (full pass every 60 s while push is confirmed
  live, heartbeat cadence otherwise); delivery — command or adapter fact — starts on commit.
- Two LISTEN channels per process (`domain_events`, `inbound_messages`) deepen the
  session-pooler constraint of ADR-20260802-200416; a transaction-mode pooler now silently
  degrades both push paths to polling.
- A `FAILED`-by-cap row is new operational surface: visible on the supervision lanes screen,
  with alerting policy still an open question on #313.
