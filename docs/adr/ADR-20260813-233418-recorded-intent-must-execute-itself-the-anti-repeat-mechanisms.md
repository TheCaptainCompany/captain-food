# ADR-20260813-233418 — Recorded intent must execute itself: the anti-repeat mechanisms

- **Status**: Accepted (founder directive, 2026-08-13)
- **Date**: 2026-08-13
- **Refines**: ADR-0032 (completeness — rules↔tests both ways) · ADR-20260730-034635 (every recurring
  failure becomes a rule/test/ADR) · ADR-20260810-215503 (the team sets the backlog) ·
  ADR-20260812-143619 (every founder message goes to the whole team)

## Context

The founder, verbatim, 2026-08-13: **"These things have been already said in the past I'm repeating
myself."** He is right, and it is a process defect, not a mood. Two concrete instances triggered it,
both in the same session:

1. **Dropped directive.** The Uber Eats catalog/order-sync directive was recorded on 2026-07-30 in
   [Epic #260](https://github.com/TheCaptainCompany/captain-food/issues/260) +
   [PROP-20260730-032306](../proposals/PROP-20260730-032306-uber-eats-marketplace-and-per-surface-direct-credentials.md),
   D1/D3/D4/D7 were **answered** on 2026-08-08, and the approved slices then **sat undone for two
   weeks** until the founder re-stated the directive today. Nothing pulled the recorded, answered
   intent into execution on its own.

2. **Recorded but unenforced.** The capture-on-delivered posture was recorded in prose in
   [ADR-20260808-195315](ADR-20260808-195315-customer-brief-answers.md) with **no `rules.yaml` entry
   pinning it and therefore no test**. The code was free to disagree, and did — the capture leg did
   not implement the recorded timing — and no gate caught it, because there was nothing to check
   against. It surfaced only when the founder re-raised it.

Both are the same meta-failure in two forms: **a recorded decision does not execute or enforce itself;
it waits for a human to repeat it.** The fix must *reduce* the times the founder repeats himself, not
add ceremony he must track (proportionality — founder directive, 2026-07-31). Two mechanisms were
proposed; both are kept only in their **light** form after judging them against noise and automability.

## Decision

### Mechanism 1 — the unrealized-directive sweep is a STANDING step of the architect run (not a raw validator rule)

Every architect run (`.claude/skills/architecture-review`) surfaces **dropped directives** to the top
of its report, ranked by the value method — so the next session executes recorded intent without the
founder re-stating it. A **dropped directive** is:

> a decision marked **✅ DECIDED / Approved** (in `docs/proposals/DECISIONS.md` or a proposal `Status`,
> or an `Accepted` ADR) whose realizing work is **neither merged nor in-progress** (no open
> `status/in-progress` issue, no live PR, no merged realizing PR/ADR).

**Why this is a review-run step and not a validator warning — a judged rejection of the heavier form.**
The tempting executable version — "a proposal `Approved` with an empty `Realized by` header for >N
days is a warning" — was measured against the repo and **rejected as noise**: `grep "Realized by"
docs/proposals/*.md` shows **~30 proposals** carrying `_(filled at completion)_` / `(pending)`, most of
them **already shipped with an un-maintained header** (e.g. the #191 observability work, the #144
read-authz work whose header *is* filled, versus a dozen that are not). A raw rule would fire mostly
false positives on day one, and the offline validator (`tools/codegen-rs`) **cannot see GitHub PR or
issue state** — the one signal that separates "dropped" from "in flight". The clean signal lives at the
intersection of a **repo marker** (a ✅/Approved decision) and **live GitHub state** (no realizing
work), and only the architect run holds both. A noisy gate is worse than a sharp review step here.

The **conditional** validator hardening is named, not adopted: *if* a one-off hygiene pass first fills
every stale `Realized by` header (making the signal clean), *then* a ratcheted warning — "an
`Approved`/`Accepted` proposal whose `Realized by` stays empty and whose tracking issue is closed
un-realized" — becomes worth its noise. That pass is backlog work, not a blocker on this ADR, and it is
strictly lower-leverage than the standing sweep, which ships today.

### Mechanism 2 — a recorded BEHAVIORAL guarantee carries its enforcing rule+test, or is flagged

ADR-0032 already forces rules↔tests **both ways** for every new command/event/error. The capture drift
escaped it because the guarantee was recorded in an **ADR, in prose**, and ADR-0032 does not reach ADR
text. The extension:

- **The ADR template gains an `Enforced by:` field.** An ADR that records a behavioral guarantee names
  the `rules.yaml` entry (or entries) that pin it — which ADR-0032 then forces to carry a test. An ADR
  with no behavioral guarantee writes `Enforced by: n/a — no behavioral guarantee`. The field is
  **never blank**, so the rules-question cannot be silently skipped.
- **A cheap presence/existence check is the only gate.** A hook/validator can assert that when
  `Enforced by:` names rule ids, those ids **exist** in `rules.yaml`. It **cannot** decide whether an
  ADR *is* behavioral — that is the reviewer's judgment (the mob/review lens), not a machine's.

**Compiler-first and the heavy gate are judged and rejected.** The compiler-first version — make
"a decision without an enforcing rule" *unspellable* — has no purchase: an ADR is prose, not a type,
so there is no constructor to seal. The heavy gate — auto-classify every ADR as behavioral-or-not and
block the behavioral ones lacking a rule — requires reading prose intent (NLP-hard) and would
mis-fire on the majority of ADRs that are structural, not behavioral. That is over-engineering for a
frustrated-by-overhead founder. The proportionate mechanism is a **template field + an existence check
+ the review lens**, plus the one concrete remediation below.

### The concrete remediation that closes the specific hole

The capture-timing guarantee gets a `rules.yaml` entry — *"an authorized order's payment is captured on
delivery, never before; an order with no Captain authorization is never captured"* — with its test,
and it lands **as part of [#544](https://github.com/TheCaptainCompany/captain-food/issues/544) (capture
on delivered)**. That rule also carries the external-order boundary (PROP-20260730-032306 §9): the
capture leg keys on the presence of a Captain authorization/PaymentIntent, so a marketplace
`ExternalOrderReceived` order reaching `MarkOrderDelivered` triggers **no** capture. This is the
reviewer check for [PR #545] and the first failing test to write when Slice D lands.

## Consequences

### Positive
- Recorded, answered intent surfaces for execution **every run**, without the founder repeating it —
  the direct cure for "I'm repeating myself".
- A behavioral decision cannot be recorded as prose that code is free to contradict: it either names
  the rule that pins it or explicitly declares it has none, and the reviewer sees the choice.
- Both mechanisms reuse existing machinery (the DECISIONS ✅ markers, the architect standing run,
  ADR-0032, the ADR template) — no new subsystem, no tracked ceremony.

### Negative
- The sweep depends on the architect run actually happening each cycle; it is a review-discipline
  backstop, not a compiler guarantee. Acceptable — the signal it needs (live GitHub state) is not
  available to the offline gate anyway.
- `Enforced by: n/a` can be written thoughtlessly. The review lens, not the check, is what catches a
  behavioral guarantee mislabeled `n/a` — recorded here so that gap is known, not hidden.

### Follow-up
- Skill edit (this change): add the unrealized-directive sweep to
  `.claude/skills/architecture-review/SKILL.md` §4 and `references/checklist.md`.
- ADR template edit (this change): add the `Enforced by:` field to `docs/adr/_template.md`.
- Backlog (named, not filed as blocking): the `Realized by` header-hygiene pass that would make the
  conditional validator warning clean; and the capture-timing rule+test inside #544.

## Consulted (ADR-20260812-143619 — one line per lens)

- **architect**: The sweep is the cure and it is cheap; the validator form is noise until headers are
  clean — recorded that judgment rather than shipping a spammy gate.
- **process/operating-model (holub-style)**: Executable beats prose, but a *sharp* review step beats a
  *noisy* gate — the exception to "always prefer the gate" is when the gate cannot see the
  distinguishing signal (live PR state). Named it so the next session does not "fix" it into a rule.
- **payments**: The capture-timing rule+test is the real closure of the specific drift; it belongs in
  #544 and carries the external-order boundary. Nothing else in my lens.
- **legal-specialist**: Nothing in my lens — this is a workflow decision, no obligation attaches.
- **beck/farley**: Shortest first slice = the skill edit + the template field, both shipping now; the
  validator hardening is correctly deferred behind the hygiene pass. Nothing further.
- **graphql-architect / ux-designer / dba / observability**: Nothing in my lens.
