# ADR-20260817-105845 — A dispatch card may not state a derived number without its antecedents; the roster reversion is struck

**Status**: Accepted · **Date**: 2026-08-17 ·
**Decider**: the **FOUNDER / Tech CEO**, ruling on the replacement consequence put to him in
[DECISIONS §44 MOB-COST-1a](../proposals/DECISIONS.md) ·
**Amends**: [ADR-20260816-134352 "The mob's checkpoint goes to declared concerns, and review is priced by reversibility"](ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)
§4 — a **founder ruling amending a founder ruling**, which itself amends the founder directive
[ADR-20260809-013142](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md) ·
**Register**: [DECISIONS §44](../proposals/DECISIONS.md) **MOB-COST-1a**, now answered ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## Status

Accepted.

## Enforced by

**Half of it is executable today.** The spec-side half landed with
[PR #610 "Detect an authorized payment with no order birth"](https://github.com/TheCaptainCompany/captain-food/pull/610):
`*.metrics[*].thresholds[*].derived_from[*]` is a **`REF_CONTRACT` site** resolving to a `ConfigKey`,
so a threshold whose antecedent key is renamed reds `make validate` instead of leaving a stale
bound. The **dispatch-side half is prose until its own check exists** —
[#619 "Make the antecedent rule executable: a dispatch card may not state a derived number without naming its antecedents"](https://github.com/TheCaptainCompany/captain-food/issues/619)
carries it, and until that lands this ADR is enforced by the coordinator writing the card and any
lens reading it.

No `rules.yaml` entry: this records a guarantee about **dispatch cards**, which are records, not
runtime behaviour.

## Context

[ADR-20260816-134352](ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)
§4 shipped with a revert trigger — holub's verification condition — whose consequence clause read:

> **A MISS reverts that reversibility class to (a)** — whole roster at briefing *and* checkpoint for
> that class — with the evidence, recorded.

It fired on the first chunk run under the ruling, and the register recorded HIGH-CONSEQUENCE as
reverted. On 2026-08-17 a records-correction run checked the artifacts and found the **causal claim
wrong**. The founder was then given the corrected evidence and a recommended replacement.

### The evidence, corrected — n=2, and neither miss is a roster-width miss

**(i) [#608](https://github.com/TheCaptainCompany/captain-food/issues/608) — the briefing was never
narrowed.** The committed claim-time card (`6d00cb3`,
`docs/dispatch/608-authorized-payment-no-birth-detection.md`) states **`Briefing roster: WHOLE
ROSTER`**, and the claim commit message says *"Reversibility class HIGH-CONSEQUENCE => whole-roster
briefing"*. Only the **checkpoint** was narrowed. The wrong arithmetic — a birth-gap threshold
derived as `max_delivery_attempts × retry_spacing_seconds` ≈ **50 s**, when the mailbox backoff is
**exponential** (`base · 2^(N−1)` = 310 s; the landed value is 600 s) — was therefore in front of
**every lens including `dba`**, and none challenged it. To blame the narrowing you would have to
assert that a lens would have caught at the checkpoint what it had already been handed at briefing.

*(A second imprecision, recorded because the register must not rest on it: the card's
self-attribution "originated in THIS CARD" is not supported by the committed artifact — the card at
`6d00cb3` contains no 50 s figure, only "a threshold justified against the ~7-day Stripe hold
expiry". The number entered through the briefing message or the issue body, neither of which is
committed. That strengthens the finding rather than weakening it: the number had **no locatable
antecedent at all**.)*

**(ii) [#609](https://github.com/TheCaptainCompany/captain-food/issues/609) — the second miss, and
the lens rejected its own alibi.** The #609 checkpoint banked a MISS: converted assertion sites were
**incidentally pinning their actors' declared lane widths**, a contract over stored rows. It first
read as a roster-width failure (`young` absent), and **`vernon` rejected that clean attribution** —
his own briefing finding named those exact literals and observed they were coupled to the
declaration; he was on the surface, with the fact in hand, and read the coupling as a *liability*
without taking the one further step to reading it as a *pin*. Banked **SHARED, weighted to
`vernon`**, with only the **escalation** (that removing the pin is a gate weakening, hence a stop
rather than a follow-up) attributed to the absent `young`. The roster width cost the **severity**;
an **invited** lens missed the **fact**.

### What the two data points share

Not a roster defect: **a coordinator-authored derived number is consumed by every lens as
established fact, and nothing verifies it.** Widening the roster puts more readers in front of the
same unverified number.

## Decision

### 1. The roster reversion is STRUCK

The reversion of the **HIGH-CONSEQUENCE** class to whole-roster-at-briefing-*and*-checkpoint is
withdrawn. **(b)+(c) stand exactly as originally ruled** for every class: the whole roster at the
**briefing**, the **checkpoint** to lenses that declared a concern there (any lens may opt back in),
and the chunk's **reversibility class** sizing the briefing roster with the `HOLD: human` axis
winning where the two disagree. Nothing else in
[ADR-20260816-134352](ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)
moves — §1, §2, §3 and the third look are untouched.

### 2. In its place: the antecedent rule

> **A dispatch card may not state a derived number without naming its antecedents, and any bare
> number it does state is marked `UNVERIFIED input`.**

Concretely, on the card:

- A **derived** number carries its derivation inline — the inputs, each named as what it is (a
  config key, a spec value, a measured reading), and the arithmetic. `600 s` is legal written as
  `MAILBOX_HEARTBEAT_SECONDS × (2^MAILBOX_MAX_DELIVERY_ATTEMPTS − 1) = 310 s → rounded to 600 s`.
  It is not legal written as `600 s`.
- A number the card cannot derive is still allowed, and is marked **`UNVERIFIED input`**. That
  marking is the whole mechanism: it converts *"the coordinator says 50 s"* into *"someone must
  check 50 s"*, which is a question a briefed lens can answer cheaply and will not think to ask of
  an unmarked figure.
- **A COUNT or an ORDINAL is a derived number** (clarified 2026-08-19, after the rule was read
  past). *"Round 1's **two** card defects are recorded in ADR-20260818-210000"* — which records
  **four**, one of them a lens's self-report rather than a coordinator defect — is exactly the
  failure this rule names, but it reads as bookkeeping rather than as arithmetic, so neither the
  card's author nor its readers applied the rule to it. "How many X" and "which round/version/
  attempt" are derived from a source the card is not showing, and they drift the moment that source
  grows. The remedy is stronger than an antecedent: **cite the section and state no count at all**,
  because a cross-reference stays true while a transcribed number needs maintaining.
- The rule binds the **card**, so it applies at the briefing — before the executor has built
  anything against the number.

**Why a gate and not a roster**: it costs nothing per chunk, it addresses the cause both data
points share, and half of it is already executable in the spec (the `derived_from` → `ConfigKey`
`REF_CONTRACT` site from PR #610), where a renamed antecedent now reds the validator.

### 3. What is UNCHANGED — banking survives, only its consequence changes

holub's verification condition is **not** withdrawn. Every chunk still banks a
`Checkpoint verification:` line in the card's `## Findings` block **and** a sentence in the change's
record, either way, and the architect's run report still surfaces it — an unanswered verification
line is still a reportable defect of the run. A clean run is still banked, because an unrecorded
clean run is indistinguishable from a run nobody checked.

**What changes is only what a MISS triggers**: it no longer reverts a class automatically; it
triggers the cause analysis this ADR is the result of.

### 4. The residue this creates, named rather than smoothed

With the automatic reversion struck, **a genuine roster-width miss now has no automatic
consequence.** n=2 says neither miss so far was one; it does not say none will be. The honest shape
of the standing rule is therefore:

- a MISS is banked, with an explicit attribution — **card defect / invited-lens depth miss /
  roster width**;
- a MISS attributed to **roster width** goes back to the **founder** with its evidence, because
  reverting a class is amending his ruling, and this ADR removed the team's standing licence to do
  it automatically.

That is a slower loop than the one it replaces. It is also the loop that would have produced the
right answer on both existing data points, where the automatic one produced a wrong one.

## Alternatives considered

- **Keep the automatic reversion** (status quo). Rejected on the corrected evidence: it was applied
  once and, on the artifacts, for a cause that did not exist. A trigger that has already misfired
  at n=1 is not a control, it is noise with a cost — a whole-roster checkpoint on every
  HIGH-CONSEQUENCE chunk, paid indefinitely, for a defect the widening does not touch.
- **Revert AND add the antecedent rule** (belt and braces). Rejected by the founder: the two
  address different causes, and only one of them is the cause the evidence names. Paying for both
  buys the cheaper mistake twice.
- **A validator rule over dispatch cards** (make the card itself gate-checked). Not taken **now**,
  and not foreclosed — it is the compiler-first shape (ADR-20260803-234035) and is what the owed
  issue should evaluate: cards are markdown in `docs/dispatch/`, so a check is reachable, but the
  hard part is recognising "a number" in prose without a false-positive rate that trains people to
  ignore it. Prose plus the existing spec-side `REF_CONTRACT` is the floor, not the ceiling.

## Consequences

### Positive

- The cheap half of the mob's detection policy stays cheap: HIGH-CONSEQUENCE chunks go back to a
  concern-declared checkpoint, ending an indefinite whole-roster tax imposed on a false attribution.
- The control now sits on the **cause both data points share**, and half of it is already
  executable rather than promised.
- The register stops carrying a "recommended and PENDING FOUNDER" row against the founder's own
  ruling.

### Negative

- **The experiment loses its automatic teeth** (§4). Banking continues, but the loop from a genuine
  roster-width MISS back to a roster change now runs through the founder.
- **The dispatch-side half is prose today.** Until the owed check exists, the rule is enforced by
  the coordinator writing the card — which is precisely the role that authored both wrong numbers.
  That is the acknowledged weak point of shipping the prose half first.
- **CLAUDE.md's mob bullet needs no change**: it already describes (b)+(c) and the MISS-reverts
  clause in the same sentence. The clause is now false as written and the bullet is amended in this
  same change.

### Follow-up actions

- **[#619](https://github.com/TheCaptainCompany/captain-food/issues/619) — filed with this ADR**: a
  check (or a card template with a required field) making the antecedent rule executable rather than
  remembered, evaluating the validator-over-cards option above.
- The two cards written before ADR-20260816-134352 (`docs/dispatch/588-*.md`,
  `docs/dispatch/598-*.md`) stay **historical and un-retrofitted**, as that ADR already records.
  Cards written under this ADR carry the rule from their first line.

## Consulted (ADR-20260812-143619 — one line per lens)

The §44 consultation is recorded in
[ADR-20260816-134352](ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md);
what follows names only the lenses whose positions bear on **this amendment**, with provenance. The
consultation was run by the coordinator; this record was written by the executor from the register
and the committed artifacts, so a line that says "no fresh text relayed" means exactly that.

- **holub** — author of the verification condition, and of the standard this amendment is judged by:
  *"without it the change is untestable and should not be made."* The condition survives intact;
  only its consequence clause is replaced, which is the narrowest edit that keeps his test alive.
- **vernon** — the lens that **rejected the tidy attribution of its own miss** on #609, and the
  reason n=2 points at cards rather than rosters. This amendment rests on that refusal more than on
  any other single input.
- **architect** — established that narrowing the roster amends a founder directive and so is a
  register row, not a technique change; the same reasoning makes *un-narrowing* it a founder row,
  which is why this ADR exists rather than a register edit.
- **beck** — mutation-red is paid once; a narrowed checkpoint must not become the reason a test is
  not seen red. Unaffected: this amendment does not touch what the executor must prove.
- **dba** — the lens present at the #608 briefing with the exponential-backoff fact in reach and the
  wrong arithmetic in front of it. No fresh text relayed; the datum stands on the committed card.
- **business-specialist** — author of (c), reversibility pricing, which this ADR restores to full
  effect for the HIGH-CONSEQUENCE class.
- **farley** — gate economics: a narrowed human fan-out is safe only where the executable gates stay
  unweakened. This amendment adds a gate (half executable) and weakens none.

**Not asked** (named, per the rule): `young`, `evans`, `graphql-architect`, `legal-specialist`,
`observability`, `reviewer`, `ux-designer`, `generator`. **Worth naming plainly**: `young` is the
lens whose absence was blamed for the #609 miss and whose escalation the register credits — he was
not asked whether the corrected attribution is fair to him, and `observability` is the lens that
would own an instrument making a card's numbers checkable rather than argued.
