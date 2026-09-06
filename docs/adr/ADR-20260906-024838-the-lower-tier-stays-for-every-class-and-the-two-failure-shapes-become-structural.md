# ADR-20260906-024838 — The lower tier stays for every class, and the two failure shapes become structural

<!-- Filename: docs/adr/ADR-20260906-024838-the-lower-tier-stays-for-every-class-and-the-two-failure-shapes-become-structural.md -->

## Status

Accepted — a **founder decision**, 2026-09-06, recorded under `/decision` (the founder decided; the team is the
scribe). Answers register row [LOWER-TIER-TRIP](../decisions/LOWER-TIER-TRIP.yaml) (opened 2026-09-04, owner founder),
which now reads `decided`. **Amends in place**
[ADR-20260904-013450](ADR-20260904-013450-the-executor-runs-on-the-lower-model-tier-and-lenses-and-reviewers-keep-the-bigger-one.md)
§3 (the `HOLD: human` card contract gains the red-first clause) and §5 (the tier-exit trip fired five times and is
answered; it is no longer a decision-queue trigger). It reverses nothing: the tier ruling stands as recorded, the
review ceiling ([ADR-20260826-084500](ADR-20260826-084500-one-review-pass-per-presentation-and-findings-are-triaged-not-chased.md))
is untouched, the derived-number rule ([ADR-20260817-105845](ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md))
is untouched, and the lifted specs freeze
([ADR-20260810-221840](ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)) is **not narrowed** —
the hand-back rule below binds an executor's mid-run invention, never the team's right to change the DSL.
Reversal check run against `docs/decisions/`, `docs/proposals/DECISIONS.md` and `docs/adr/` on the terms *tier*,
*executor model*, *red-first*, *DSL grammar*, *hand-back*, *trip*: the two amended sections above are the only
records touched. Tracking issue for the executable half:
[#910 "Lower tier stays: make the two failure shapes executable"](https://github.com/TheCaptainCompany/captain-food/issues/910).

## Enforced by

n/a — no behavioral guarantee (an operating-model decision; its executable form is a dispatch-surface hook and a
hand-back template, tracked in #910, not a `rules.yaml` entry).

## Context

ADR-20260904-013450 §5 set an exit condition on the lower executor tier: a `HOLD: human`-class lower-tier PR hitting
the three-round review ceiling opens a decision-queue row for the founder. It fired on #875 (2026-09-04), then on
#885, #899, #901 and #907 — five trips by 2026-09-06, all recorded on the row with attribution. The row's evidence,
across the five: round-1 misses were card-shaped (a stop clause satisfiable after the fact, "render unconditionally"
without checking each control had a renderer arm, a decided record cited as either/or, an artifact invariant never
stated as a test) at least as often as executor-shaped, and the executor-shaped ones were **inventions made honestly**
— an exemption scoped by filename under a fence read too broadly (#885), a wrong read model in a fail-closed arm
(#895), a deploy gate widened by treating an absent field as an explicit one (#907). First-round PASS on the lower
tier: 2 of 12 (#864, #892) — the same rate the bigger tier recorded before the ruling (1 of 5), antecedents on the row.

The founder's status page put the row to him as one question with three options (A keep + structural fixes, B narrow
to GREEN work, C revert), and a second question surfaced by holub on the step order. He answered both on 2026-09-06.

## Decision

The founder's answers, verbatim:

> **1.** *"A — keep the lower tier for every class; make the two failure shapes structural (every ADR line naming a
> test becomes a red-first card step; any new DSL grammar an executor invents is a mandatory hand-back item)"*
>
> **2.** *"A — keep 3, 4, 5, 6, 7 as answered; step 6 follows step 5"*

What the team records from them, consulted for completeness (block below), never relitigated:

1. **The lower tier stays for every class.** ADR-20260904-013450 §1–§4 stand unchanged; no card is re-tiered on
   judgment; the reviewer and lens tier stays load-bearing.
2. **Rule 1 — the red-first card step.** For every dispatch card, every line of the card's **cited** record(s) that
   names a test, a belt or a mutant becomes a **red-first card step**: an entry
   `<test path>::<name> — <record>:<line> — mutant: <planted change> — expected red: <message fragment>`, the test path
   existing or marked `NEW`. Executable form: Lane D of `.claude/hooks/register-check.sh` (the surface that already
   gates `Register check:` lines on write-capable dispatches) refuses a card whose cited record names a test and
   which carries no `Red-first:` section, or an entry without its mutant and expected-red fields; the explicit negative
   (`Red-first: none — <record> names no test`) is admissible. The hook checks **presence, resolution and shape**; it
   cannot check that a test is real, that it was ever red, or that the extraction is complete — those stay a lens read
   and a reviewer read, and the hook's header says so (beck, farley, holub). It is proven red-first in
   `register-check-selftest.sh` before it counts (beck). The term *red-first card step* is declared ONCE, in the card
   template of [`docs/claude/sessions/workflow.md`](../claude/sessions/workflow.md#the-red-first-card-step--canonical-format-declared-once-here),
   and cited from the hook and from this ADR (evans). Named instances
   the lenses owe to cards in their areas: the rebuild-recipe TWIN — TRUNCATE-replay **and** mid-drain resolution, since
   the first passes under either recipe (dba); per-control renderer-arm coverage where a dead control is the expected
   red (ux); `obs-metric-no-emitter` and the gauge-liveness emission-order test (observability); a per-role generated-SDL
   additivity baseline — removal, nullability flip, narrowing, `roles:` widening fail without a declared `@deprecated`
   path — as a validator rule where it can reach, the card step as the fallback (graphql-architect, negative trail: no
   such rule exists today).
3. **Rule 2 — the mandatory hand-back item.** Every executor hand-back carries a **mandatory** line
   `New grammar / invented exemption:` (`none` allowed; absence fails the hand-back). Its scope, as the consult widened
   the founder's words to the failures they were written for: a new spec key **or generated-artifact semantics** (#907's
   invention was emitter semantics, not a spec key — evans); **any invented exemption, fence boundary or gate scope**
   (#885, #895, #907 were carve-out inventions — vernon); a self-authored `rules:` recipe string (dba); a new SDUI
   component or action name, or a control bound to a declared `gap` (ux); counsel-reviewable copy or a grade-(a)
   obligation item — allergens under FIC 1169/2011, the withdrawal right under C. consom. L221-28, GDPR Art. 17/21
   erasure and objection wording — because a fresh-written notice is a published misstatement, not a review round
   (legal, never clearance). The item is **unconditional** on `events`, `fold:` and snapshot keys whatever the tier or
   class: stored shapes are never rewritten, and a key the loader accepts silently becomes history no upcaster knows
   (young). A wire-reply field stays additive-only with a tolerant reader — no hand-back (young). The item sits inside
   the protocol-mandated mechanical hand-backs of
   [ADR-20260821-010543](ADR-20260821-010543-agents-never-ask-an-answered-question-the-register-check-binds-every-agent.md) and needs no
   register trail (architect). Compiler-first companion, owed in #910 or its own row: a GENERATED inventory of the
   loader's declared key set under the existing drift gate, so an invented spec key is unspellable without a
   regenerate diff the reviewer sees (farley, young).
4. **The tier-exit trip is retired as a decision-queue trigger.** §5's trip fired and was answered; a sixth ceiling
   hit is **measured, not escalated** (holub: a trip condition with no consequence is ceremony — it is retired). What
   remains of §5: the per-window journal line, which speaks on a non-trip window too (observability: a tally that
   speaks only on trip goes quiet exactly when nothing runs), and the measure itself as a **wide row per PR** — id,
   tier, class, rounds, blocker attribution (card defect / lens depth miss / roster width) — from which any fraction is
   re-derivable, never a standing ratio in a record (ADR-20260817-105845). The word for §5's event is **tier-exit
   trip**, distinct from a gate or validator rule *tripping* on a diff (evans).
5. **The cost clause is `UNVERIFIED`.** ADR-20260904-013450 recorded "cheaper per run"; the unit that matters is
   fully-loaded **cost per merged PR by tier** = executor run + Σ(review rounds × lenses engaged), and no meter for it
   exists — the antecedents (tier, rounds, lenses per round) are in the journal per PR, the loop-budget meter is not
   attributed per PR (business-specialist). Option A is unaffected either way: the five trips track card quality.
6. **The step order is reaffirmed, not re-decided.** *"keep 3, 4, 5, 6, 7 as answered; step 6 follows step 5"*
   restates the founder's 2026-09-04 answer; step 5 is merged (#895), step 6 is in its last slice (6-iii, #906). No
   register row exists for the order and none is created; the reaffirmation is a journal line. Holub's ask stands on
   the record: name the date a Tours human first touches running software — production is suspended
   ([ADR-20260817-105844](ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)) and every step-6 PR has shipped dark.

## Alternatives considered

- **B — narrow the lower tier to GREEN/reversible work**, returning `HOLD: human` slices to the bigger tier. Not
  chosen by the founder; the row's evidence said the rounds tracked card quality and fresh-written shapes, which B
  relocates rather than fixes.
- **C — revert ADR-20260904-013450.** Not chosen; forgoes the saving on twelve merged PRs and stops the tally.
- **Step 6 before step 5 / step 6 re-ordered** (holub's objection). Not chosen; the order stands as answered.

## Consequences

### Positive
- The two failure shapes the five trips exposed get a gate and a template instead of a memory (#910).
- The founder's queue loses a recurring interruption that had stopped carrying information.
- The vocabulary is fixed once: *red-first card step*, *tier-exit trip*, *New grammar / invented exemption*.

### Negative
- A fail-closed Lane D addition blocks every dispatch the moment it lands — #910 lands the selftest red first and
  treats the next card as the smoke (farley).
- Rule 2 has no dispatch-time seam; until the key inventory exists it is a template line and a reviewer read.
- Non-additive GraphQL changes stay on the lower tier with no additivity validator yet (graphql-architect).

### Follow-up actions
- [x] [#910](https://github.com/TheCaptainCompany/captain-food/issues/910): Lane D `Red-first:` rule + selftest cases seen red first; the
      card-template declaration in `docs/claude/sessions/workflow.md`; the mandatory hand-back line in
      `.claude/agents/executor.md` and the reviewer checklist ([PR #913](https://github.com/TheCaptainCompany/captain-food/pull/913)).
- [ ] The loader key inventory under drift (farley/young), NOT part of #910: a GENERATED inventory of
      the loader's declared key set under the existing drift gate, so an invented spec key is unspellable
      without a regenerate diff the reviewer sees — left open, own issue or a later card.
- [ ] Architect rows: the per-role generated-SDL additivity rule (graphql-architect); per-control renderer-arm
      coverage as a validator or renderer test (ux); cost per merged PR by tier, meter and antecedents (business).
- [ ] The journal keeps the per-PR wide row from #907 onward; the next window line names the fraction with its PR list.

## Consulted (ADR-20260812-143619 — one line per lens)

Consulted for the completeness of the record, never to relitigate; **no lens output is legal advice or clearance**.

- **holub** — supports A; asked that the red-first rule be a hook, that the hand-back be checked by grammar-file path, and that §5's now-consequence-free trip condition be retired or restated; the record should name the date a Tours human first touches running software.
- **beck** — the red-first card step is checkable in shape only (section present, ADR line resolves and names a test, mutant + expected-red message fields required); completeness stays a lens read; `New DSL grammar:` is a MANDATORY hand-back line (`none` allowed, absence fails closed); the new gate is unverified until seen red in the selftest.
- **farley** — put both rules on the existing Lane D + `register-check-selftest.sh` + always-run `gate-scripts` path (grep the CITED record for test/belt lines, require one `Red-first:` step per hit or an explicit negative); make the grammar rule a drift-detected generated inventory of the loader's declared key set rather than a remembered hand-back; land the selftest red first, the next card is the smoke; no release-path objection.
- **architect** — reversal check: amends ADR-20260904-013450 §3 as well as §5, and must state that the DSL hand-back does not narrow the lifted freeze (ADR-20260810-221840); the item sits inside ADR-20260821-010543's protocol-mandated mechanical hand-backs; order answer contradicts nothing (PROP-20260831-180622 §11, step 5 merged); rule 1 is gatable in register-check.sh Lane D; rule 2 has no hook seam and rides executor.md + the reviewer checklist; the hook is a separate GREEN PR with a tracking issue named in the ADR.
- **young** — the mandatory hand-back should name `specs/**` grammar and bind unconditionally on `events`/`fold:`/snapshot keys (stored shapes, never rewritten); wire-reply fields stay additive-only, no hand-back; #907's absent-vs-explicit conflation is the tolerant-reader defect a red-first step must pin.
- **vernon** — concurs with A; asks the card rule read "any invented exemption, fence boundary or gate scope", since the runtime trips (#885, #895, #907) were carve-out inventions, not DSL grammar.
- **evans** — "trip" collides with the gate/validator sense (name it *tier-exit trip*), "red-first card step" has no declaration (declare it once, cite it), "DSL grammar" is narrower than the failure it was written for (say "spec key or generated-artifact semantics"); declare the terms once, cite that declaration from skill, hook and ADR.
- **dba** — A keeps migrations and rebuild recipes on the lower tier safely only if the rebuild-recipe TWIN (TRUNCATE-replay + mid-drain resolution) is a red-first card step, not ADR prose; a self-authored `rules:` recipe string is a mandatory hand-back.
- **graphql-architect** — no objection to A or the step order; flagged that non-additive schema changes stay lower-tier with no additivity/deprecation validator in existence (negative trail); asked that the red-first guard for that class be a per-role generated-SDL baseline rule (removal / nullability flip / narrowing / `roles:` widening fail without a declared `@deprecated` path), not a card sentence.
- **observability-agent** — keep the per-PR row (id, tier, class, rounds, attribution), not the ratio; §5's non-trip journal line is the dead-man and stays; `obs-metric-no-emitter` and the §8 gauge-liveness test become red-first card steps.
- **legal-specialist** — no objection to A or the step order; asked that the record make counsel-reviewable copy and grade-(a) obligations (FIC 1169/2011 allergens; C. consom. L221-28; GDPR Art. 17/21) mandatory hand-back STOP items on any tier, since a fresh-written notice is a published misstatement, not a review round. No lens output is legal advice or clearance.
- **business-specialist** — agrees with A; the record's "cheaper per run" is an unpriced claim — marked UNVERIFIED; measure fully-loaded cost per merged PR by tier (executor run + Σ rounds × lenses); no meter for it exists today.
- **ux-designer** — concurs; asks that the red-first step be per-control renderer-arm coverage (dead control = expected red) and that new SDUI component/action grammar or a gap-bound control be a named hand-back item.
