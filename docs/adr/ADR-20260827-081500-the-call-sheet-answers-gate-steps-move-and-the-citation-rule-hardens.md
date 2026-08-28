# ADR-20260827-081500 — The call-sheet answers: gate steps move to a sibling job, and the citation rule hardens to an error

**Status**: Accepted · **Date**: 2026-08-27 ·
**Decider**: the **FOUNDER / Tech CEO**, verbatim below ·
**Closes**: [`docs/decisions/GATE-STEP-LOCUS.yaml`](../decisions/GATE-STEP-LOCUS.yaml) and
[`docs/decisions/CITATION-RULE-LEVEL.yaml`](../decisions/CITATION-RULE-LEVEL.yaml) ·
**Issue**: [#689 "Execute the call-sheet decisions"](https://github.com/TheCaptainCompany/captain-food/issues/689) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted.

## The directive, verbatim

Answers returned through the call-sheet artifact form, 2026-08-27:

> 1. Where do CI safety checks live: **Move to a separate job**
> 2. Stale-citation rule level: **Make it a hard error**
> 3. Bugs #685/#688: **Fix both now**
> 4. Next up: All issue created recently seems to be related to ci or code review
>
> Note: **If the fix is small always do it now.**

The note is a **standing instruction**, recorded here so it survives the session: a small fix is
taken immediately rather than filed. It composes with, and does not overturn,
ADR-20260826-084500's triage — "small" is the blocking-bucket test applied at authoring time, and
a finding that is not small still becomes an issue rather than a round.

## Consulted

Per CLAUDE.md, records created from a founder directive carry a `Consulted:` block. As with
ADR-20260826-084500, the mob was **not** re-convened, deliberately and stated rather than silently:
both questions were decided from rows whose evidence fields already carry the full option spaces,
sizings and counter-arguments accumulated by ninety-plus independent review passes — the
deliberation exists in the register, and the founder chose among recorded options. The lenses that
spoke through those rows: `farley` (the pipeline must keep reporting when one gate fails — the
skip-cascade argument that decides Q1), `young`/`evans` (a record citing an overturned decision as
live authority is a modelling defect — the coupling argument behind Q2), `beck` (a gate never seen
red is an unverified claim — the smoking that made Q2's flip safe). If any lens would have reversed
a choice, that is a challenge row against this ADR, not an amendment to it.

## Decision 1 — `GATE-STEP-LOCUS`: option (a)

The two executable gate steps (the register-check hook selftest and the decision-lookup stub
suite) move out of the always-run `changes` job into a sibling always-run **`gate-scripts`** job
that `codegen` aggregates by name.

What this buys, in the row's own sizing: a host-drift red in a gate suite no longer **skips**
`lint`/`specs`/`build-test`/`db-test`/`docs-validate` — their signal survives — and on the
docs-only lane (a push straight to `main` with no PR) `docs-validate` still runs, closing the
inversion where a docs push **landed on `main` with its only validator skipped** while `codegen`
redded. What it deliberately does **not** change: the job is **equally blocking** — a genuine gate
failure still reds the required check. The row's corrected evidence (review #91) is what made this
an honest choice: option (a) closes the skip cascade, not the merge block.

Cost accepted: one extra runner start per run, on a repository whose own workflow header records
Actions as free here.

This narrows `RETRIEVAL-QMD-CI`'s locus clause — that row authorized its step *"in the always-run
`changes` job"* and reserved moving it. `GATE-STEP-LOCUS` was the open row raised to own exactly
that question; deciding it IS the authorization. `RETRIEVAL-QMD-CI` (decided) is not edited — its
locus wording stands as history, and this ADR is the record a reader following it lands on.

## Decision 2 — `CITATION-RULE-LEVEL`: `err`

`decision-superseded-authority` — a tracked file in the agent corpus may not cite a superseded
decision row as live authority — flips from a ratcheted warning to a **hard `make validate`
error**.

The sequence this completes is gate-then-stabilize executed in full: the rule **shipped at
`warn`** (reviews #81/#82: a new blocking rule on the path feeding the required check must not
land with its default flipped in the same commit), the row held the flip as a separate recorded
decision, the gated form then ran over the real corpus and every live edit of the review rounds
with **zero false positives**, and the founder flipped it. What `err` buys back: the one-commit
supersession coupling `docs/decisions/README.md` requires is enforced **absolutely** again — a
stale citation can no longer land behind a baseline entry. Verified by plant: a
`Per row RETRIEVAL-QMD, …` line in `.claudeignore` reds `make validate` at `[error]`, exit 2.

Mechanical consequence, the level↔list coupling working as built: an error never enters
`warning_profile`, so the rule leaves `CORPUS_DERIVED_KINDS` and the partial-read floor in the
same change, and all three pins now assert the reverse direction — a future flip back to `warn`
cannot land without rejoining the list.

**Not decided here**, so the row's history stays honest: the second half of the row's question —
whether the exemption stays the implicit word `superseded` in the clause or becomes an explicit
marker on the citing line — remains open ground. The residuals are unchanged: the same-marker
join, and the fail-open posture on an unreadable corpus.

## Enforced by

Executable on both halves. Q1: `the_hook_selftest_runs_in_the_always_run_gate_job` and
`the_stub_suite_runs_in_the_always_run_gate_job` pin the steps in `gate-scripts` via
`assert_pinned_in_gate_job` (every job-scope mutant re-anchored — planted against the wrong job
they prove nothing); `codegen`'s `needs:` list is pinned as a literal including `gate-scripts`.
Q2: `a_superseded_row_may_not_be_cited_as_live_authority` pins the ERROR level with the decision
in its message; the `CORPUS_DERIVED_KINDS` pin and the partial-floor pin assert the departure.

## Consequences

- A gate-suite red now costs one job's signal instead of six, and the docs-only lane keeps its
  validator under every failure of a gate suite.
- A stale citation of a superseded row is unmergeable and unlandable, with rewording the only
  escape — the false-positive surface that made this a founder call is priced in the row and was
  measured at zero across the gated period.
- #685 and #688 are fixed in the same change under the standing note, and #688's N1 is resolved
  as *keep* (the three wildcard plants are fail-closed controls on `hides`, labelled as such).
