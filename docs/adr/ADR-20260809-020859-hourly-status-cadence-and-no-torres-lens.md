# ADR-20260809-020859 — Status cadence drops to hourly; the Teresa Torres discovery lens is declined

**Status**: Accepted · **Date**: 2026-08-09 · **Decider**: the customer (product owner), in
session, two directives in one message. Supersedes the 5-minute cadence of
[ADR-20260808-223000](ADR-20260808-223000-all-day-autonomous-operation.md)'s companion rule in
docs/claude/autonomous-run.md; closes the roster question raised the same night.

## 1. Status cadence: hourly, not every five minutes

> "Every hour don't need more often"

The 5-minute cadence was itself a customer directive (2026-08-08, *"inform me every 5 minutes"*)
and was honoured; the cost warning recorded with it — that it converts supervision into narration
and spends the weekly budget markedly faster — proved accurate over ~4 hours of heartbeats, most
of which reported "no change" while runner queues and long dispatches did the waiting. The focus
audit the same night independently recommended reverting it during execution blocks.

**Now**: post a status **at every meaningful transition** (dispatched · PR opened · merged ·
blocked · question queued · a finding the customer would act on) **plus an hourly heartbeat while
work is in flight**. Silence between transitions is correct, not neglect. While the customer is
asleep or away, drop the heartbeat further — work, and keep the morning summary current instead.

## 2. The Teresa Torres discovery lens is declined

> "No need for Torres I get it thanks guys"

The customer asked whether to add a standing agent channelling Teresa Torres (*Continuous
Discovery Habits*: opportunity solution trees, weekly touchpoints, assumption mapping) and asked
the team. Two lenses answered before the customer closed it; both said **NOT YET**, and the
customer took the point without waiting for the third (the architect's artifact-fit analysis was
stood down mid-flight rather than finish answering a closed question).

**The reasoning, kept because it will be re-proposed one day:**

- **Zero of the 15 open register decisions** would have been better or cheaper with an opportunity
  tree (both lenses audited the register row by row, independently). The §23 step-DSL rows are
  internal design; the §22 rows are legal, external or engineering. One row — the avelo37
  threshold — resolves with **a supplier phone call**, not discovery.
- **Wrong order**: `specs/observability.yaml` carries business metrics for prospection only — no
  basket, funnel conversion, rider decline rate, repeat rate or contribution take-rate.
  Continuous discovery without signal is opinion with a diagram; epic
  [#400 "Epic: reality-sensing infrastructure — agents closer to customers, mission metrics as contracts"](https://github.com/TheCaptainCompany/captain-food/issues/400)
  is the real prerequisite.
- **Wrong shape for the bottleneck**: the team's constraint is finishing, not choosing. Torres's
  method is a choosing machine, and under the mob rule every lens is a recurring cost on every
  dispatch.
- **PM-drift risk**: a lens that OWNS an opportunity tree and derives the next slice from it is a
  product manager with a nicer vocabulary — precisely what ADR-20260808-144738 bans.
- **Torres's own diagnosis would agree**: she is explicit that discovery without delivery is
  research. With production down and no order ever having flowed, the dead URL is the finding.

## Consequences — what survives the decline

The method's cheap half is worth having without the agent, and is NOT lost here:

1. **A four-row assumption register**, owned by the existing `business-specialist` lens (every row
   is a viability claim, already its charter): the voluntary-contribution take-rate
   (ADR-20260808-203443 — the largest unvalidated economic assumption in the company); "restaurants
   need market parity to switch" (the premise under #410); postal-code zones giving an honest Tours
   ETA; riders-first being operable at Tours density. Each with its cheapest test and the cost if
   wrong.
2. **Three human activities needing no agent at all**, ranked by evidence-per-hour: a Tours
   competitor teardown (~5 h — validates or kills the PROP-032306 D5 price-uplift assumption); the
   avelo37 + Uber Direct Tours pricing calls (~2 h — puts a number on a launch-critical fallback
   rung); rider conversations at the 19:15 pickup clusters (~7 h — the post-ADR-20260808-235545
   critical path currently has **zero** evidence).
3. **A validator rule instead of a coach**: `specs/stories.yaml` declares `goals:`/`frustrations:`
   per persona with nothing hanging off them. A rule that every activity serves ≥1 declared persona
   goal delivers the *pruning* half of an opportunity tree in this repo's native idiom
   (checkable, blocking, no prose). Recorded as a candidate, not yet a decision.
4. **The trigger for revisiting**, if ever: business-signal contracts emitting (#400) AND the first
   ~20 real paid orders — and even then as a bounded workshop ritual attached to #400, never a
   standing lens.
