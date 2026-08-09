# ADR-20260809-013142 — Mob programming: every agent is in the dev, so issues are found DURING it

**Status**: Accepted · **Date**: 2026-08-09 · **Decider**: the customer (product owner), in
session, as a standing principle. Composes with — and materially sharpens — the ensemble-consent
decision model (ADR-20260808-144738/155656) and the lens-involvement failure the focus-coach audit
recorded the same night.

## The directive (verbatim)

> "Principle: Mob programming or ensemble programming means everyone is involved in the dev so
> ensure that every agent is involved so everyone will be able to detect issues during the dev"

## Why it was issued (the evidence, same night)

The customer asked whether every lens was involved in decisions. It was not: six of eleven agents
never spoke during the 2026-08-08 night run, and the two lenses whose absence cost the most were
consulted only AFTER code existed:

- The **ux-designer** reviewed [#424](https://github.com/TheCaptainCompany/captain-food/issues/424)
  post-implementation and found the built `payment_failed_state` **cannot render at all** in
  production (the flag is hardcoded false; `sdui: false` screens never hydrate), that a card
  refused after checkout lands on a screen reading *"Commande introuvable"*, and that the approved
  French copy (*"Paiement refusé"*) is untrue when the failure is technical rather than a decline.
  Every one of those findings would have changed the WORK, not just the review — at zero cost if
  known before the first line.
- The **legal** and **business** lenses were absent from
  [ADR-20260808-235545](ADR-20260808-235545-riders-first-uber-direct-is-the-fallback.md), which
  made independent riders the launch-critical path — a French regulatory surface and a per-order
  economics question, decided by engineering lenses alone.

Review-after-build makes a lens an auditor. This directive makes it a participant.

## Decision

**Every agent is in the dev.** A work dispatch is a MOB, not a solo executor followed by reviewers:

1. **Mob briefing (before any code)** — the dispatch brief goes to the whole roster in parallel,
   not to a coordinator-chosen subset. Each lens answers one question: *what will you catch in this
   work, and what must the executor know before starting?* Silence is a valid answer and costs one
   line ("nothing in my lens"). The executor's brief then carries every lens's constraints.
   **This is the load-bearing half**: it is where a "this can never render" finding is free.
2. **Mob checkpoints (during)** — the executor stops at declared phase boundaries and the mob reads
   the ACTUAL diff so far. A lens may stop the work at a checkpoint; that is the point of being
   there.
3. **Mob review (after)** — the existing independent full-diff pass stays. It is now the third
   look, not the first.

**Selection by coordinator taste ends.** The audit named that discretion as where a product-manager
quietly re-emerges (ADR-20260808-144738 decision 4). The roster is invited by default; a lens
excuses itself, the coordinator does not excuse it.

## Consequences

- **Cost is real and accepted**: every dispatch now pays N briefings plus checkpoint reads. The
  customer issued the principle knowing the run operates under a weekly budget
  (ADR-20260808-223000). If the cost proves unsustainable the answer is FEWER, BIGGER dispatches —
  never a quietly smaller mob.
- **Silence must stay cheap**, or the mob degrades into ceremony: a lens with nothing to say says
  so in one line, and that is a complete contribution.
- **The dispatch template changes** (docs/claude/autonomous-run.md): brief → mob briefing → work
  with checkpoints → mob review. The executor prompt must name its phase boundaries up front so
  checkpoints are schedulable.
- **A lens's briefing answer is part of the record**: when a finding later arrives that a lens
  could have named at briefing time, that is a process defect worth a line in
  docs/claude/sessions.md — the same way a missing gate is.
- **Open tuning question, deliberately not pre-decided**: whether the mob briefing is truly all
  eleven lenses on every dispatch, or all-by-default with an explicit, recorded excusal (e.g. no
  money path → business-specialist writes "nothing in my lens" without reading the diff). Starts at
  ALL, with the first three mobbed dispatches measured; a reduction is its own decision, recorded,
  never a drift.
