# ADR-20260904-013450 — The executor runs on the lower model tier; lenses and reviewers keep the bigger one

<!-- Filename: docs/adr/ADR-20260904-013450-the-executor-runs-on-the-lower-model-tier-and-lenses-and-reviewers-keep-the-bigger-one.md -->

## Status

Accepted (founder `/decision` 2026-09-03, scope answer 2026-09-04). Records a founder directive:
the `Consulted:` block below is required ([ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)).

**Amends** [docs/claude/sessions/workflow.md](../claude/sessions/workflow.md) §"Delegate execution
to a cheaper model tier (founder, 2026-08-28)", which carries a banner pointing here. That section
keeps its text as history; **this record is the authority**, and where the two differ this one wins
(the difference is stated in §Decision, item 3).

## Enforced by

n/a — no behavioral guarantee. This is an operating-model rule. Its executable half is the
`model:` key in the executor agent's frontmatter (`.claude/agents/executor.md`), landed by its own
PR because `.claude/**` is a code path; the coordinator's `Agent` call for an executor carries no
`model` override unless this ADR is amended.

## Context

The founder's standing instruction of 2026-08-28 (workflow.md, verbatim: *"execution goes to
subagents on a lower model tier; the coordinating session keeps judgment, mob mechanics and
founder-facing surfaces"*) said `sonnet` by default for `executor`/`generator`/sweep agents and
kept the coordinator model for triage, review verdicts, records, *"and anything on the `HOLD:
human` class"*. It was prose, it had no ADR, and **it was never applied**: every executor run on
#639 part C (steps 1, 2a, 2b, 2c-i, 2c-ii) ran on the session model, because the executor agent
file has no `model:` key and no dispatch passed one. A rule that lives only in prose is exactly the
kind the register exists to stop being optional.

On 2026-09-03 the founder tagged a message `/decision`, verbatim:

> **"use lower model for to the executor if it's possible / Keep bigger model for the mob and the
> reviewers"**

Three lenses were consulted for the completeness of the record (below). Two of them read *"if it's
possible"* as room for the coordinator to keep the bigger tier on a `HOLD: human` card. That
reading was put to the founder as a question on 2026-09-04 with three options, and he chose the
literal one:

> **"B — lower tier always for the executor; big tier only for lenses and reviewers"**

The register check found no contradiction: the 2026-08-28 section is the only record on the
subject, and this decision **amends** it (item 3 below) rather than reversing a register row.

Evidence this decision was priced against, n from ONE session on ONE issue (#639 part C,
2026-09-01 → 2026-09-03), so marked: the surviving executor runs each took 25–140 minutes of
wall-clock (three gates against Postgres, not model latency); four of five PRs needed a round-2
fix after the independent review, with the bigger model as executor (round-1 PASS rate **1 of 5**
— #835, #846, #849, #852 each FAIL then PASS; #854 PASS on its first pass, 2026-09-04); five runs
were killed by container restarts (×3) and API overloads (×2), which are orthogonal to the tier.

## Decision

1. **The executor runs on the lower model tier, always.** `.claude/agents/executor.md` declares
   `model: sonnet`; the `generator` and any sweep-style agent follow the same rule (`haiku` stays
   permitted for purely mechanical sweeps, as the 2026-08-28 text already said).
2. **Lenses and reviewers keep the bigger tier.** Every `.claude/agents/*.md` that is a lens
   (`architect`, `beck`, `business-specialist`, `dba`, `evans`, `farley`, `graphql-architect`,
   `holub`, `legal-specialist`, `observability-agent`, `ux-designer`, `vernon`, `young`) and the
   `reviewer` inherit the session model and declare no `model:` key. **The reviewer tier is
   load-bearing**, not a default that cost pressure may creep into next (holub): with a cheaper
   author, the reviewer is the only net for the two defect classes gates cannot catch — record
   accuracy and fail-closed defaults.
3. **The `HOLD: human` carve-out of 2026-08-28 is withdrawn.** The old text kept the coordinator
   model for "anything on the `HOLD: human` class"; the founder chose the literal reading, so a
   `HOLD: human` executor card runs on the lower tier too. What changes on such a card is the
   **card**, not the model (beck, below): the mutant named, the expected-red list pre-classified,
   no commit mixing runtime code and an existing assertion, red and green SHAs with
   `git diff --stat` between them, the negative cases spelled out, `DB_TESTS_REQUIRED=1` named.
4. **The coordinator keeps the bigger tier** for triage, review verdicts, records (ADRs, journal,
   dispatch cards) and founder-facing surfaces — unchanged from 2026-08-28.
5. **Exit condition, measured, never re-litigated per dispatch** (holub). Precondition: **the
   dispatch card and the PR body state the executor tier**, or the count cannot be made — today no
   PR says which tier ran it, so no per-tier baseline exists. Numerator: lower-tier-executed PRs
   whose FIRST independent reviewer pass (the presentation pass of
   [ADR-20260826-084500](ADR-20260826-084500-one-review-pass-per-presentation-and-findings-are-triaged-not-chased.md))
   returns **no BLOCKING finding**. Denominator: lower-tier-executed PRs that reached a reviewer
   pass (abandoned drafts excluded). Window: the first **10** such PRs or **14 days** from this
   record's date, whichever closes first, then rolling per 10. **Trip**, either: first-round PASS
   is **0/10** at window close, or **any `HOLD: human`-class lower-tier PR hits the three-round
   ceiling** before the window closes. The coordinator counts it in the run report; at window close
   **one journal line** with the fraction and the PR list (the antecedents,
   [ADR-20260817-105845](ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md));
   a **trip is a decision-queue row** for the founder naming the work class, because reverting this
   ruling amends a founder decision — a non-trip window is a journal line and nothing else. Nobody
   re-escalates a single dispatch to the bigger tier on judgment: the only route back is that row.
   Baseline, **UNVERIFIED as to tier**: on #639 part C, 1 of 5 passed its first round (above);
   since 2026-08-28 at least five other PRs carried a round-2 commit (#708, #771, #797, #802, #807 —
   antecedent: `git log --since=2026-08-28` titles matching "round 2"), all codegen-emitter /
   DSL-surface changes, which is the ONE class expected to trip first: emitter + regenerate +
   three record surfaces in one diff under `HOLD: human`.
6. **Push-first is made structural, not remembered** (farley): the claim commit and the draft PR
   are the coordinator's permitted pushes ([ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md))
   and MAY be made before the executor is spawned, so a lower-tier executor cannot start on an
   unpushed branch. This is already the environment.md §17 rule; it is restated here because a
   weaker model skips procedure more readily.

## Alternatives considered

- **A — lower tier by default, the coordinator may keep the bigger tier for one run on a
  `HOLD: human` card, saying why on the card.** Holub's and farley's reading of *"if it's
  possible"*; the recommended option on the 2026-09-04 form. Rejected by the founder: a per-card
  judgment that drifts back to "always big" unless the reason is written every time.
- **C — by class, no discretion: lower tier on GREEN and reversible cards, bigger tier on every
  `HOLD: human` card.** Farley's first-batch shape. Rejected: most of part C is `HOLD: human`, so
  the saving would arrive late, and the founder's second clause names lenses and reviewers, not a
  class of executor work.
- **Keep the prose as it was.** Rejected: it was never applied, and a rule that is not in an
  agent file or a record is a preference.

## Consequences

### Positive
- Premium tokens stop being spent on tool-echo (the founder's stated reason: a long diff-authoring
  run is mostly tool output, priced the same on every tier and carrying no judgment).
- The rule becomes executable at the one point it can be — the agent file — instead of a sentence
  the coordinator has to remember at spawn time.
- The exit condition is a number with antecedents, so reversing this is a measurement, not an
  argument.

### Negative
- Holub expects round-1 review failures to rise on stored-shape, legal-surface and record-bearing
  work, and the reviewer (bigger tier) to author more fixes by comment. The three-round ceiling
  ([ADR-20260826-084500](ADR-20260826-084500-one-review-pass-per-presentation-and-findings-are-triaged-not-chased.md))
  becomes likelier to trip, and tripping it is a founder interruption.
- Beck: red-first did not catch the 2b regression (the executor saw red and greened it by editing
  the assertion); a cheaper executor widens that gap, so **the card now carries design work** —
  mutant, expected-red list, negative cases — that used to be inferred by the executor. More
  card-writing per dispatch, on the coordinator's tier.
- Farley: gate wall-clock is unchanged by tier; what shortens runs is smaller dispatches, and no
  per-run gate timing is recorded anywhere to evaluate this decision against. Capturing it is
  owed (below).

### Follow-up actions
- [x] `.claude/agents/executor.md` and `generator.md` gain `model: sonnet` —
      [PR #859 "Executor and generator run on the lower model tier (`model: sonnet`)"](https://github.com/TheCaptainCompany/captain-food/pull/859).
- [ ] Every executor dispatch card from #639 part C step 3 onward carries the beck items of
      §Decision 3 and records the run's gate wall-clock in the hand-back (farley), so the exit
      condition has antecedents.
- [ ] The first-round PASS count starts at PR #854's successor; the journal carries the running
      tally.

## Consulted (ADR-20260812-143619 — one line per lens)

Consulted for the completeness of the record, never to relitigate; **no lens output is legal
advice or clearance**.

- **holub** — cost lever, not a flow lever; the decision is right only while round 1 passes often
  enough that the cheaper executor is cheaper *in total*; demanded the measured exit condition
  (§Decision 5) and named the reviewer tier as load-bearing; read *"if it's possible"* as room —
  the founder chose the literal reading instead.
- **beck** — red-first is not more load-bearing, it was already satisfied and caught nothing; the
  gap is diff-shaped (which side of the equation was edited to go green), so the card names the
  mutant, pre-classifies expected reds, forbids runtime+assertion in one commit, demands red/green
  SHAs and the negative cases spelled out (§Decision 3).
- **farley** — the pipeline's verdict must be independent of the author, so a cheaper executor is
  a *test* of the pipeline, not a threat; run length is gate wall-clock not model latency; push-first
  must be structural (§Decision 6); would have kept the bigger tier for changes to the pipeline
  itself (CI workflows, gate scripts, deploy emitters) — the founder's literal reading covers those
  too, and the reviewer pass is where that class is now caught.
- **architect, business-specialist, dba, evans, graphql-architect, legal-specialist,
  observability-agent, ux-designer, vernon, young** — not asked: the subject is the operating model
  of the loop, in no domain lens; recorded so a lens never asked is distinguishable from one with
  nothing to say.
