# ADR-20260809-013142 — Mob programming: every agent is in the dev, so issues are found DURING it

**Status**: Accepted · **Date**: 2026-08-09 · **Decider**: the customer (product owner), in
session, as a standing principle. Composes with — and materially sharpens — the ensemble-consent
decision model (ADR-20260808-144738/155656) and the lens-involvement failure the focus-coach audit
recorded the same night.

> **Amended 2026-08-16 by [ADR-20260816-134352 "The mob's checkpoint goes to declared concerns, and review is priced by reversibility"](ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)**
> — a founder directive amending a founder directive, closing the *"Open tuning question"* below.
> **The briefing half of this ADR is untouched**: whole roster, before any code, cheap silence, no
> excusal by coordinator taste. What changes: the **checkpoint** (§2) goes only to lenses that
> DECLARED a concern at the briefing, and the chunk's **reversibility class** sizes the briefing
> roster. A verification condition rides on it — the narrowed checkpoint's misses are banked either
> way, and ~~a miss reverts that class to the whole roster~~ — **struck 2026-08-17 by
> [ADR-20260817-105845](ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)**:
> banking survives, the automatic reversion does not. A MISS is banked **with an attribution**, and
> only one attributed to roster width returns to the founder.

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

## Measurement log — the instrument this ADR asked for

Appended as the first three mobbed dispatches happen, per the open tuning question above. This log
records what the mob COST and what it CAUGHT; it does not change the decision, which stays ALL.

### Dispatch 1 — [#410 "Epic: public try-before-committing demo"](https://github.com/TheCaptainCompany/captain-food/issues/410) briefing, 2026-08-09

**Deviation, recorded honestly: the coordinator invited 4 of 11 lenses by its own taste** — farley
(lead), ux-designer, beck, dba. That is exactly the discretion §"Selection by coordinator taste
ends" abolished, on the very first dispatch after the ADR landed. Not a reduction decision, not a
lens excusing itself: a drift.

**What the four caught** (each independently, none sent to look for it): the customer path is inert
on `main` — `hydrate()` returns early for every `sdui: false` screen, so checkout mounts no Stripe
element and tracking renders the not-found hero for every order; and no notification port exists, so
a paid order tells nobody. Twenty-two web tests pass in ten milliseconds over all of it. The
briefing paid for itself several times over before any code was written, which is the ADR's claim
holding.

**The correction, same night**: the remaining lenses with plausible standing were invited on the
committed proposal — legal-specialist, business-specialist, graphql-architect, holub,
observability-agent, architect. Two were deferred with a stated reason rather than by taste:
`reviewer`'s standing is on a finished diff (it is the third look on the #420 PR), and `generator`'s
is on the emitter change that dispatch may or may not make. **Excusal by timing is legitimate;
excusal by the coordinator's guess about relevance is not** — the difference is that the first can
be named in one sentence and checked.

### Dispatch 2 — [#420 "Customer delivery reassurance"](https://github.com/TheCaptainCompany/captain-food/issues/420) code-only hydration, 2026-08-09

**No fresh briefing was run**, deliberately: the four-lens #410 briefing WAS the briefing for this
work — the same files, the same defect, and beck's two named failing tests went into the dispatch
prompt verbatim. Recorded because "we already briefed this" is the most plausible way the ritual
decays: it is legitimate only when the earlier briefing covered *this* diff's surface, and the
coordinator must say which briefing it is reusing. Here: PROP-20260809-021351 §2 and §6 move 1.
