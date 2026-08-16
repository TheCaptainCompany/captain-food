# ADR-20260816-134352 — The mob's checkpoint goes to declared concerns, and review is priced by reversibility

**Status**: Accepted · **Date**: 2026-08-16 ·
**Decider**: the **FOUNDER / Tech CEO**, ruling on the register row he was asked to own ·
**Amends**: [ADR-20260809-013142 "Mob programming: every agent is in the dev, so issues are found DURING it"](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md)
— itself a founder directive, so this is **a founder directive amending a founder directive** ·
**Register**: [DECISIONS](../proposals/DECISIONS.md) §44 (**MOB-COST-1**), now decided ·
**Context**: [ADR-20260816-020752 "The loop's context budget: a dispatch card, snapshot semantics, and phase commits"](ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
(the six technique changes; this is the seventh item, the one that was not the team's) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## The ruling (founder / Tech CEO, 2026-08-16, verbatim)

> "Go for the Recommendation: (b)+(c), with holub's verification condition."

## What it amends (the 2026-08-09 directive, verbatim)

> "Principle: Mob programming or ensemble programming means everyone is involved in the dev so
> ensure that every agent is involved so everyone will be able to detect issues during the dev"

That directive's own words for the roster rule were: *"The roster is invited by default; a lens
excuses itself, the coordinator does not excuse it."* **That sentence still holds at the BRIEFING.**
The 2026-08-09 record also left this exact tuning question open on purpose (*"Open tuning question,
deliberately not pre-decided… a reduction is its own decision, recorded, never a drift"*). This is
that recorded reduction. It is a reduction of **one phase only**.

## Decision

**(b) + (c) compose: (c) sizes the briefing roster, (b) decides who returns for the checkpoint.**

### 1. The briefing is UNTOUCHED — the whole roster, before any code

Every lens named by the chunk's reversibility class is briefed **before any code**, in parallel, and
answers *what will you catch, and what must the executor know before starting?*. Silence stays cheap
and complete (*"nothing in my lens"*). A lens excuses itself; the coordinator still does not excuse
it. **This is the load-bearing half** — it is where a "this can never render" finding is free — and
nothing below reduces it. Only the **checkpoint fan-out** and the **roster-sizing rule** change.

### 2. (b) holub — the checkpoint goes only to lenses that DECLARED a concern at briefing

A lens that answered *"nothing in my lens"* at the briefing is **not** invited back to read the diff
at the checkpoint. A lens that declared a concern is, and keeps its power to **stop the work** — that
is unchanged and is the point of being there. A lens may also **opt back in** at any checkpoint on
its own initiative; the narrowing is a coordinator's *invitation* rule, never a *permission* rule,
and no lens is ever barred from the diff.

### 3. (c) business-specialist — review is priced by REVERSIBILITY, not by chunk

Every chunk is assigned a **reversibility class** before the briefing:

- **IRREVERSIBLE / expensive to undo → full mob briefing.** Money movement, **stored event shapes**
  (a shape written to `domain_events` outlives every deploy), **legal surfaces** (allergens, VAT and
  receipts, GDPR erasure, P2B terms), and **anything Tours-facing** — anything a real customer,
  courier or restaurant in Tours can see or be billed by.
- **REVERSIBLE → 2–3 lenses.** Internal refactors behind a stable interface, generated artifacts,
  documentation sweeps.

This is the **same axis the `HOLD: human` merge class already uses**
([ADR-20260815-115220](ADR-20260815-115220-auto-merge-on-green-by-default-hold-human-for-the-named-class.md),
amended by [ADR-20260815-134655](ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md)):
**one vocabulary, two uses** — it prices review fan-out *and* merge posture. Where the two would
disagree, the wider one wins: **a chunk in the `HOLD: human` class is irreversible for briefing
purposes**, whatever else it looks like. The call is made **before the diff exists**, which is the
option's acknowledged cost; the class is written on the dispatch card so the call is inspectable and
can be challenged by any lens in its briefing answer.

**The third look does not move.** The independent full-diff reviewer pass before ready-for-review
(founder directive 2026-08-01) is untouched, and the multi-lens fan-out it already carries for
payments, migrations and erasure stays.

### 4. holub's verification condition — a concrete obligation, with a home

Attached to the ruling, and the reason the change is legal at n=1:

- **Every chunk run under this ADR records, at the checkpoint, whether the narrowed checkpoint
  missed anything the full roster would have caught.** The concrete question the coordinator answers
  is: *did the independent full-diff review, or anything downstream (CI, the merge, a later chunk),
  surface a finding that a lens excluded from the checkpoint would have named?*
- **Home**: the **dispatch card's `## Findings` block**
  ([ADR-20260816-020752](ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
  decision 2), as a line labelled **`Checkpoint verification:`**. The **architect's run report
  surfaces it** — an unanswered verification line is a reportable defect of the run, like a missing
  gate.
- **"Banked" means, either way**: (1) a line in the card, and (2) a sentence in the change's record
  — `STATUS.md` or the chunk's ADR. **A clean run is banked too**: it turns n=1 into n=2, and an
  unrecorded clean run is indistinguishable from a run nobody checked.
- **A MISS reverts that reversibility class to (a)** — whole roster at briefing *and* checkpoint for
  that class — with the evidence, recorded. A miss is a result, not a failure of the experiment.

## The empirical support, and its limit

Both checkpoint **STOPs** on
[#167 "No order-acceptance timeout: a paid, unaccepted order sits forever with no alert, cancel or refund"](https://github.com/TheCaptainCompany/captain-food/issues/167)
— **ux**'s false banner (a state the built checkout could show but never render) and **legal**'s
tense on the GDPR clock — came from lenses that **had declared a concern at the briefing**. A
narrowed checkpoint would have lost neither.

**That is n=1.** The honest counter-argument stands and is not dismissed: *the diff is the first time
a lens sees what was actually built*, so a lens silent at briefing can still catch something no
briefing could have anticipated. The verification condition exists precisely because n=1 is not a
law, and it is what makes this ruling testable rather than merely cheaper.

## Composition with the dispatch card — this is detection policy, not the bill

The card ([ADR-20260816-020752](ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
decision 2) already cuts per-lens cost roughly **10×** *whichever way this row went*, and the
measured evidence for that is now on the record:

| Chunk | Lens context per invocation |
|---|---|
| [#167](https://github.com/TheCaptainCompany/captain-food/issues/167) — lenses read the repo | **50–85k each** |
| [#588](https://github.com/TheCaptainCompany/captain-food/issues/588) — lenses read the card | **26–44k each** |

So this ruling must not be read as a cost-cutting measure that happened to touch policy: **the bill
was already addressed by the card**. What changes here is *who looks at the diff and when* — a
**detection policy**, judged by what it catches, which is why it ships with a verification condition
and a revert trigger rather than a savings target.

## Consequences

- **CLAUDE.md's mob bullet is amended in this same change.** Its closing sentence (*"The roster is
  invited by default and a lens excuses itself; coordinator-chosen subsets are over"*) became **half
  true** on this ruling: it holds for the briefing, not for the checkpoint. Leaving it unamended
  would be the most likely way this decision is silently ignored. The rule stays **resident** in
  CLAUDE.md under young's test — forgetting it produces state a rebuild cannot undo (an unreviewed
  money-path change ships).
- **Every dispatch card must state its reversibility class and the briefing roster derived from
  it**, and must carry the `Checkpoint verification:` line at the checkpoint. Recorded in
  [docs/claude/sessions.md](../claude/sessions.md) with the rest of the card's shape. The two cards
  written before this ruling (`docs/dispatch/588-*.md`, `docs/dispatch/598-*.md`) are **historical
  and are not retrofitted**.
- **The coordinator now makes a classification call it can get wrong.** That is the cost of (c),
  accepted knowingly: the mitigation is that the class is written down before the briefing, every
  briefed lens sees it, and any lens may contest it in its briefing answer — a misclassification is
  visible, not silent.
- **This does not license coordinator taste to return.** The excluded set at a checkpoint is
  **derived mechanically** from the briefing answers, not chosen. If a coordinator has to *decide*
  who returns, the rule is being broken.
- No `specs/**` change, no regeneration, no SPEC-LOG row: this is a records-and-process change.

## Consulted (ADR-20260812-143619 — one line per lens)

The §44 consultation ran on the founder question *"Do you have recommendations to optimise tokens
consumption?"* ([ADR-20260816-020752](ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)),
and this row is the one item it escalated rather than decided.

- **holub**: option (b) — whole roster at the briefing, checkpoint only to lenses that declared a
  concern; and the **verification condition** that makes it testable, *"without it the change is
  untestable and should not be made"*.
- **business-specialist**: option (c) — price review by **reversibility**, not by chunk; full mob for
  money movement, stored event shapes, legal surfaces and anything Tours-facing.
- **architect**: the dispatch card, whose ~10× per-lens cut is what makes this a detection-policy
  question rather than a budget one; and that narrowing the roster amends a founder directive, so it
  is a register row and not a technique change.
- **young**: snapshot semantics for the card — the artifact the narrowed checkpoint reads
  (card@SHA + `git diff`) is a disposable cached fold with a fall-through right, never a second
  source of truth.
- **beck**: mutation-red is paid once — adjacent economics; nothing in this lens on who is invited,
  beyond that a narrowed checkpoint must not become the reason a test is not seen red.
- **farley**: gate economics and the pre-flight; a narrowed human fan-out is only safe where the
  executable gates stay unweakened, which this ruling does not touch.
- **observability**: the `tokens`/`agent` fields on the loop ledger and the dead-man's-switch
  framing — the instrument that will let the next such decision be a reading rather than a
  reconstruction.

**Not asked** (named, per the rule that an unasked lens must not be indistinguishable from a silent
one): `dba`, `evans`, `generator`, `graphql-architect`, `legal-specialist`, `reviewer`,
`ux-designer`, `vernon`. The consultation was framed as token consumption and reached the lenses with
standing on the loop's mechanics. **Worth naming plainly**: `ux-designer` and `legal-specialist` are
exactly the two lenses whose #167 checkpoint STOPs form this ruling's entire empirical base, and
neither was asked whether a narrowed checkpoint would have cost them their finding. The verification
condition is the compensating control, and their answer at the first checkpoint under this ADR is
worth soliciting explicitly.
