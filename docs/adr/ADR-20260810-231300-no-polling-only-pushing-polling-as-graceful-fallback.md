# ADR-20260810-231300 — No polling, only pushing; polling as a graceful fallback until pushing works again

## Status

Accepted — product-owner directive, 2026-08-10.

## Context

The product owner stated, verbatim:

> *"Principle: no polling only pushing, polling as graceful fallback until pushing works again"*

Direction now takes the form of principles rather than decisions about individual mechanisms
(the fourth delegation in three days — prioritisation [ADR-20260810-215503](ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md),
self-starting sessions [ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md),
product ownership [ADR-20260808-144738](ADR-20260808-144738-product-ownership-lives-in-the-team-no-pm-agent.md),
and the lifted `specs/**` freeze [ADR-20260810-221840](ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)).

**This is not new to the repo, which strengthens it rather than weakens it.** The crash-test verdict
already recorded *"stop polling — use notify instead"*
([ADR-20260808-212741](ADR-20260808-212741-solida-studio-strategic-frame.md) §3); PROP-20260726-170500
D5 chose Postgres `LISTEN`/`NOTIFY` over polling for subscription fan-out;
[ADR-20260802-200416](20260802-200416-push-driven-drain-loops.md) and
[ADR-20260802-224532](20260802-224532-push-driven-mailbox-approved.md) built it for `domain_events`
and for the actor mailbox. What is new is the **generality**: a principle that applies to every
state-change propagation in the system, not a decision about one mechanism.

The **second clause is the interesting half**. "No polling" alone is unusable — it is contradicted by
every reconnect story in the tree. *"Polling as graceful fallback until pushing works again"* is a
usable rule, because it makes polling a **degraded mode with an exit**, and a degraded mode with an
exit is measurable. The failure this ADR exists to prevent is not polling; it is **a poll with an
excuse**: a fallback that was never declared, cannot be observed, and has no path back — which is
just polling wearing the word "fallback".

## Decision

**Push is the primary transport for every state change one part of the system must learn from
another.** Polling is legitimate ONLY as a fallback, and only when all three of the following hold.
A fallback missing any one of them is a violation, not a fallback:

1. **Declared.** The degraded mode is a named, deliberate state in the design — recorded in the
   proposal/ADR that introduced it and visible in the code as an explicit branch. "It happens to
   retry on a timer" is not a declaration.
2. **Observably degraded.** An operator can tell, from telemetry, that the system is polling rather
   than pushing — a counter or gauge under a `specs/observability.yaml` contract, with the reason as
   an attribute. `mailbox_push_down_total{reason}` (`specs/observability.yaml:96`) is the reference
   shape. **A silent fallback is the worst outcome of all**, because it converts a loud outage into a
   permanent, invisible latency tax that nobody is paid to notice.
3. **Has a path back, and the path is exercised.** Something must actively detect that pushing works
   again and return the system to push — automatically, not by a deploy.

### What "pushing works again" is detected by

A fallback nobody can exit is permanent. Detection therefore may **never** rest on the absence of an
error, because the load-bearing failure mode produces no error: a `LISTEN` through a transaction-mode
pooler is accepted and then silently delivers nothing, so `recv()` never fails and a
connection-error-driven liveness flag stays `true` forever while every wake is lost.

The required shape is a **positive liveness proof on the push path itself**: the consumer periodically
causes a notification it must itself receive, and requires the echo before the next tick. Missing the
echo means push is down regardless of what the connection reports.
`crates/infrastructure/src/persistence/mailbox_wake.rs:131-204` is the reference implementation —
a 30 s self-`pg_notify` with a `__canary__` payload, echo required before the next tick, a missed echo
tearing down the connection and counting `mailbox_push_down_total{reason="canary_timeout"}`. Detection
is bounded by two canary intervals; any process's canary satisfies any listener.

Two corollaries follow, and both are load-bearing:

- **The transport must also surface a silent heal.** sqlx's `recv()` reconnects internally and drops
  whatever was notified in the gap; `try_recv()` surfaces that as `Ok(None)`, which is the cue to
  catch up unconditionally. A push path that cannot see its own heal loses events without noticing.
- **Reconnect must re-sync, and a fallback must engage even when reconnect never succeeds.** A client
  whose catch-up hangs off a successful handshake has no degraded mode at all when the handshake is
  refused — it is not polling, it is frozen. A refused push channel must fall back to reads, not to
  silence.

### Scope: what the principle governs, and what it does not

The principle governs **state-change propagation** — one component learning that another component's
state moved. It does **not** govern **time-triggered work**, where the trigger is that a deadline
passed and there is no producer to push: nobody can `NOTIFY` "time has elapsed". Reminder promotion,
offer-TTL expiry, retention and erasure sweeps are time-triggered and are outside this rule.

For those, the equivalent discipline is **sleep until the next due row, not scan on a fixed interval**
— ask the database when the earliest deadline is and wait exactly that long, with the interval only as
a floor. That is the same idea one level down: the data tells you when to wake, instead of you asking
repeatedly whether it is time yet.

The distinction is a boundary, not an escape hatch. "This is time-triggered" is only true when no
component knows the fact earlier than the clock does. A timer whose deadline is *set* by a state
change (an acceptance timeout armed at order placement) is time-triggered in its firing and
push-driven in its arming — and its arming must not be discovered by a scan.

### The same principle one level up: the team's own operating loop

A 5-minute status cron polled for agent completions. Agent completions **already arrive as push
notifications**, so the poll was pure redundancy — the highest-frequency loop in the operating model
existed to re-discover facts that had already been delivered. It is deleted and replaced by an
**hourly fallback whose prompt states explicitly that push is primary and that it must report only
what push failed to deliver** (cadence per [ADR-20260809-020859](ADR-20260809-020859-hourly-status-cadence-and-no-torres-lens.md)).
That is this ADR applied to the team itself: declared (the prompt says so), observably degraded (it
reports what push missed, so a non-empty report *is* the signal), and exiting by default (it reports
nothing when push works).

**Do not reintroduce a polling status loop.** If push delivery is unreliable, fix the delivery — a
shorter cron is the excuse, not the fix.

## Alternatives considered

- **"Never poll" (first clause only).** Rejected by the directive itself, and correctly: it is
  unimplementable over transports with no delivery guarantee. `NOTIFY` has no replay, so every push
  path in this repo needs a durable backstop (`domain_events` + `projection_checkpoint`,
  `inbound_messages` + `status='RECEIVED'`). Banning the backstop bans correctness.
- **"Prefer push, poll where convenient."** Rejected: this is the status quo the directive corrects.
  Without the three conditions, every poll is retroactively a "fallback" and the principle is
  unfalsifiable.
- **Polling with a tuned-down interval instead of push.** Rejected on evidence already in the repo:
  PROP-20260802-223522 D1 option C measured a 1 s heartbeat at ~290k queries/hour at mailbox width 5.
  The latency-vs-cost curve is a dead end, not a trade-off.
- **Push with a fixed short safety net, no liveness proof.** Rejected: it is exactly the shape the
  #314 review's MAJOR-1 rejected. A deaf-but-errorless `LISTEN` leaves the safety net *stretched*
  because the flag says push is live — so the failure makes the system slower and reports it as
  healthy.

## Consequences

### Positive
- A poll is now a **reviewable claim** with three checkable conditions, rather than an aesthetic
  preference. "Is this a violation?" has an answer a reviewer can reach from the code.
- Names the second-order failure the repo has already hit twice: a fallback that engages silently, and
  a fallback that cannot exit. Both are worse than the poll they replaced.
- Gives the missing-signal case a home: a degraded mode with no telemetry contract is a finding
  against `specs/observability.yaml`, which is already a blocking gate.
- Draws the honest boundary at time-triggered work, so the principle is not applied where it produces
  a worse system.

### Negative
- Condition 2 makes new work: each push path needs a `push_down`-shaped contract, and `domain_events`
  does not have one today.
- Condition 3's canary costs one notification per interval per listening process, and one connection
  slot. That is the price of not silently degrading; it is paid deliberately.
- The boundary invites litigation at its edge. The tiebreaker is the question, not the label: *does any
  component know this fact before the clock does?* If yes, it is propagation and must be pushed.

### Follow-up actions
- Audit findings against this principle are reported separately and filed as issues; this ADR does not
  itself schedule the fixes.
- `event_wake` (the `domain_events` listener) predates the canary and the `try_recv` heal and has no
  `push_down` contract — the reference implementation is `mailbox_wake`, and the gap is a finding.
- The cross-process subscription fan-out decided as PROP-20260726-170500 D5
  ([ADR-20260808-171056](ADR-20260808-171056-register-sweep-consent-decisions.md), veto open) is
  decided and unbuilt; the role gateway answers WebSocket upgrades `501`
  (`crates/gateway_runtime/src/lib.rs:316-322`). Under this principle the missing client-side fallback
  is as much of the work as the transport.
