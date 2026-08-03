# ADR-20260803-002712 — The four #313 open questions decided (requeue, backoff, alerting, adapter fleets)

## Status

Accepted (product owner, in-session, 2026-08-03)

## Context

[PROP-20260802-223522 "Push-driven mailbox"](../proposals/PROP-20260802-223522-push-driven-mailbox.md)
shipped ([PR #314 "feat(#313): push-driven mailbox — pg_notify at the door, idle lane gate, poison cap"](https://github.com/TheCaptainCompany/captain-food/pull/314))
with four questions deliberately left open on
[#313](https://github.com/TheCaptainCompany/captain-food/issues/313)'s checklist. The product
owner answered all four in-session, after scenario walk-throughs and sequence diagrams for the
adapter-fleet trade-off.

## Decisions

1. **Requeue tooling — a first-class ADMIN mutation** (against the SQL-runbook recommendation):
   operator recovery of a poisoned row gets the full ADR-0032 completeness train (command, test,
   story step, supervision-surface wiring). Rationale: operators never touch SQL on the money
   path. Tracked: [#315 "Admin requeue mutation for poisoned mailbox rows"](https://github.com/TheCaptainCompany/captain-food/issues/315).
2. **Backoff — exponential** (against the fixed-spacing recommendation): `next_attempt_at`
   column, doubling delays from the heartbeat base (10 s → 20 s → 40 s → 80 s → 160 s ≈ ~5 min
   to poison at cap 5). A struggling dependency gets more room before terminal FAILED; the
   head-of-line wait semantics are unchanged. The #314 fixed spacing stays in force until this
   lands. Tracked: [#316 "Exponential backoff for mailbox delivery retries"](https://github.com/TheCaptainCompany/captain-food/issues/316).
3. **Alerting — every poison FAILED pages** (broader than the money-lanes-only recommendation):
   a Honeycomb (EU) trigger on `mailbox_poison_failed_total` across ALL actor types. At V0
   volume a poison anywhere is rare enough to always be worth a look; revisit the scope if it
   ever gets noisy. Tracked: [#317 "Honeycomb trigger: page on any mailbox_poison_failed_total"](https://github.com/TheCaptainCompany/captain-food/issues/317).
4. **Adapter worker fleets — default-off until the PM posture is DB-persisted** (as
   recommended): `RUN_MAILBOX_WORKERS` guidance stays opt-in; the deploy-window delivery delay
   (durable, never lost) is accepted until `PM_MAILBOX_DELIVERY` lives in one database row all
   processes read — which makes the silent paid-order stall (a drifted env value on one of five
   adapter deploys delivering Payment facts without the PM chain hop) structurally impossible.
   Flipping the guidance to on afterwards is its own one-line ADR. Tracked:
   [#318 "DB-persisted PM_MAILBOX_DELIVERY posture"](https://github.com/TheCaptainCompany/captain-food/issues/318).

## Consequences

- #313's unresolved-questions checklist is closed; the four successor issues carry the work and
  enter the prioritised backlog for ordering by the product owner.
- Until #316 lands, poison time-to-terminal stays ≥ ~50 s (cap 5 × 10 s fixed spacing); until
  #317 lands, poison visibility is the supervision screen + the metric without a page; until
  #318 lands, monolith deploys imply a short delivery delay on adapter-recorded facts.
