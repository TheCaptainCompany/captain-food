# ADR-20260728-224500 — Every in-process background loop publishes readiness, and every `RUN_*` toggle parses the same way

- **Status**: Accepted
- **Date**: 2026-07-28
- **Issue**: [#244 "The SIRENE sync worker is the one background loop with no status endpoint — and a strict env gate that fails silently"](https://github.com/TheCaptainCompany/captain-food/issues/244)
- **Refines**: [ADR-0045](0045-sirene-staging-table-and-split-sync.md) (the split sync's on-app half), [ADR-0043](0043-schema-version-gate-and-out-of-band-migrations.md) (`/health` as the readiness contract)

## Context

The department-37 pilot ([#238](https://github.com/TheCaptainCompany/captain-food/issues/238)) swept
6,649 établissements into `external_sirene_restaurants`. Four hours later every row was still `PENDING`
with its payload, across what should have been four hourly poll passes.

Establishing *why* took a code read, three `curl`s and an elimination argument — and still ended at
"read the Render boot log", which an operator on a phone does not have. The loop could have been:
paused by its env gate, running and erroring every pass, or running and finding nothing. All three look
identical from outside the process.

Two causes, both structural rather than incidental:

1. **The worker published nothing.** `/projector` and `/saga` each expose
   `{running, checkpoint, head, lag, last_tick_at, last_error}`; `/health` covers the DB and schema
   version. The SIRENE sync worker — the loop driving a paused pipeline back to life, i.e. exactly the
   one under active operational scrutiny — exposed no state at all.
2. **Its env gate was an exact string match.** `RUN_SIRENE_WORKER` was read as `v == "true"`, so
   `TRUE`, `True`, a space-padded or dashboard-quoted value all silently meant *paused*. The
   neighbouring flags used `v != "false"`, under which `RUN_INBOUND_DRAIN=0` means *on*. Five gates,
   two incompatible conventions, no shared parser.

`CLAUDE.md` already requires observability of critical workflows; what was missing is the rule that
makes it apply uniformly rather than per-worker.

## Decision

**1. Every in-process background loop publishes a readiness snapshot on its own endpoint.**

A loop that can be paused, can crash, or can silently find nothing must be observable from outside the
process. `GET /sirene` joins `/projector` and `/saga`: `running`, `lastTickAt`, `lastError`, plus
`lastSummary` (the counters of the last pass). Same `Arc<Mutex<…>>` handle pattern, same camelCase wire
shape, same `200`-when-running / `503`-otherwise semantics.

**The distinguishing `503` is the deliverable, not the `200`.** A green check answers a question nobody
was asking; the pilot needed to tell *paused* from *broken*. So the body always names the state:

| state | code | body |
|---|---|---|
| no `DATABASE_URL` → no worker | `503` | `reason: sirene_worker_not_available` |
| worker built, poll loop never started | `503` | `reason: poll_loop_not_started` |
| loop running, last pass OK | `200` | `lastSummary`, `lastError: null` |
| loop running, last pass failed | `200` | `lastError: "<cause>"` — the loop turns; the pass failed |

The status handle is taken **unconditionally**, before the env gate, because the worker is constructed
either way (the ping endpoint drives it). That is what makes `poll_loop_not_started` distinguishable
from "no worker" rather than collapsing both into one opaque `503`. For the same reason a
ping-triggered pass updates the snapshot too: the endpoint describes the *worker*, not just the loop.

**2. Every `RUN_*` toggle goes through one lenient parser.**

`server::env_flag(name, default)` accepts `true/1/yes/on` and `false/0/no/off`, case-insensitive, with
surrounding whitespace and wrapping quotes trimmed. Anything unrecognised — including empty — falls
back to the **documented default and logs that it did**. Applied to `RUN_SIRENE_WORKER`,
`RUN_PROJECTOR`, `RUN_INBOUND_DRAIN`, `RUN_RETENTION_SWEEP`, `RUN_PROCESS_MANAGERS`.

A typo must never be silently *interpreted*. Falling back to the default is honest — the operator gets
the documented behaviour plus a log line — where guessing is not.

## Consequences

- **`RUN_INBOUND_DRAIN=0` now means OFF**, where the `!= "false"` shortcut read it as ON. The new
  reading is the intended one; the old was an artifact of the shortcut. Same for `no`/`off`.
- **A new background loop owes an endpoint.** Reviewers should treat "loop with no readiness endpoint"
  the way they treat a missing behaviour test. The three existing endpoints are the template.
- **This does not replace logs**, it replaces *needing* them for the first question. Boot lines still
  say what started; the endpoint says what is true now, to anyone with `curl`.
- **Deliberately not included**: a pending-row count on `/sirene`. It would need a query per request on
  an endpoint meant to be cheap and poll-able, and it answers a data question (how much work is left)
  rather than the readiness question (is the worker alive). `SELECT status, count(*)` already answers
  the former, and `lastSummary` gives the last pass's shape for free.
- The endpoints stay **unauthenticated ops routes**, like `/projector` and `/saga`: they expose worker
  liveness and counters, never record content.
