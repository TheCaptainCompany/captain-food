# ADR-20260824-205911 — The decision-lookup stub suite runs in CI; RETRIEVAL-QMD-CI becomes the controlling row

**Status**: Accepted · **Date**: 2026-08-24 ·
**Decider**: the **FOUNDER / Tech CEO**, approving the open challenge row `RETRIEVAL-QMD-CI` ·
**Closes**: [`docs/decisions/RETRIEVAL-QMD-CI.yaml`](../decisions/RETRIEVAL-QMD-CI.yaml) —
which `reconsiders: RETRIEVAL-QMD` ·
**Supersedes**: [`docs/decisions/RETRIEVAL-QMD.yaml`](../decisions/RETRIEVAL-QMD.yaml), whose
controlling content is carried forward in full by the successor row ·
**Design record**: [PROP-20260822-171212](../proposals/PROP-20260822-171212-qmd-phase0-bm25-advisory-retrieval.md)
(rewritten in this change) ·
**Issue**: [#678 "Wire the decision-lookup hermetic stub suite into CI (RETRIEVAL-QMD-CI)"](https://github.com/TheCaptainCompany/captain-food/issues/678) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted.

## Enforced by

**Executable, both halves.**

- `.github/workflows/ci.yml` runs `bash .claude/skills/decision-lookup/scripts/stub-tests.sh` as a
  step of the always-run `changes` job — the one job with a checkout and no `if:`, so the suite
  cannot skip on the docs-only path that bypasses the Rust gates.
- `the_stub_suite_runs_in_the_always_run_changes_job` (`tools/codegen-rs/src/tests.rs`) pins the
  step inside that job and asserts the job stays ungated, mirroring the
  `the_hook_selftest_runs_in_the_always_run_changes_job` precedent from the 2026-08-21 hardening.
- The register's own gate enforces the supersession coupling — **but only after this change**.
  `decision-reconsiders-shape` already red if a `decided` challenge row's target was not
  `superseded` by it. The MIRROR half did not exist: a target `superseded` by a challenge still
  `open` passed `make validate` with **zero errors**, leaving the register in exactly the split
  state `docs/decisions/README.md` forbids — a superseded row whose authority points at a question
  nobody has answered. The independent review of PR #679 disproved the "neither row can move
  without the other" claim empirically, by constructing that state. This change adds the missing
  rule and plants it red (`reconsiders_shapes_fire_red_and_the_legal_shapes_stay_green`: without
  it the split state returns `[]`). Recorded rather than quietly fixed, because the claim was
  written here first and believed.

## Context

[`RETRIEVAL-QMD`](../decisions/RETRIEVAL-QMD.yaml) adopted the advisory-retrieval integration and
enumerated what it does **not** authorize — among them *"hooks, CI/validator/agent-contract
changes"*. The follow-up chain (PRs #672–#676) then grew
`.claude/skills/decision-lookup/scripts/stub-tests.sh` from 34 to 54 hermetic cases and
`SKILL.md` declared it the **executable authority** for a wrapper that carries a supply-chain gate:
a pinned, scriptless `bun` install of `@tobilu/qmd`, a structural `bun.lock` version-integrity
binding, and delete-wholesale handling of a corrupt index.

That produced a contradiction the repo's own rules do not tolerate: a declared executable authority
that nothing executes. Its green reported the author's machine. "A gate cannot be ignored" is the
reason this repository prefers executable over prose — an unrun suite is prose.

**The evidence that this is not theoretical.** During PR #675 the step was briefly wired. Its
**first CI run went red at 26/52** and exposed a host dependency the suite had carried unnoticed:
the wrapper preflights `command -v bun`, so on a runner without `bun` every cache-building case
failed its precondition. The suite claimed hermeticity while requiring a real `bun` to be installed.
The defect is fixed independently (an exit-0 stub `bun` for the whole run, #676) — but it was found
**only** by running the suite off the author's machine.

**The instrument matters, and got it wrong once.** PR #676's first attempt appended an `AMENDMENT`
paragraph to the decided `RETRIEVAL-QMD` row's `evidence:`. [`docs/decisions/README.md`](../decisions/README.md)
forbids exactly that: a decided row is challenged by a NEW open row carrying `reconsiders:`, and
`superseded_by` is the ONE legal edit to it. The independent review caught it; the edit was reverted
and the CI wiring dropped from that PR. `RETRIEVAL-QMD-CI` was filed as an open row instead, and is
what the founder approved on 2026-08-24.

## Decision

**One.** The hermetic stub suite runs in CI, as exactly one step of the always-run `changes` job,
pinned by exactly one codegen test. Pure bash + `python3`, seconds, no Rust — it adds no dependency
to the pipeline that the pipeline did not already have.

**Two.** `RETRIEVAL-QMD-CI` closes as `decided` and `RETRIEVAL-QMD` flips to `superseded` with
`superseded_by: RETRIEVAL-QMD-CI`, in this change. This is not a judgement that the original
decision was wrong: the register's model is **one controlling record per key**, and closing a
challenge IS the supersession move. Because the successor becomes the chain head — the row a future
reader resolves to — it **carries the predecessor's controlling content forward in full**, with the
CI clause narrowed. Nothing is lost by the flip; the predecessor stays readable as history.

**Three.** The narrowing is that one clause, plus one naming correction: the successor row also
names `.claude/skills/decision-lookup/scripts/stub-tests.sh` in the authorized surface, which the
predecessor never did — the suite grew after that row was written and its enumeration was never
updated. That is a widening of the RECORD to match what was already tracked, not of the surface. Every other non-authorization of
`RETRIEVAL-QMD` stands verbatim in the successor: no hooks, no agent-contract changes, no validator
rule over `specs/**`, no GraphRAG, vector search, embeddings, model downloads, reranking, query
expansion, MCP, hosted services, credentials, or YAML decision-row indexing; no widening of package,
version, permissions or dependency shape. **QMD remains advisory only** — a result, or the absence
of one, still decides nothing, and no gate, hook, validator or agent contract consumes its output.
The CI step tests the *wrapper*; it does not run QMD, install anything, or touch a live `.qmd/`
cache.

## Alternatives considered

- **Leave the suite unrun and downgrade `SKILL.md`'s wording** to "executor-side authority". Honest,
  and costs nothing — but it answers a supply-chain gate's verification question with a promise, and
  the 26/52 first run is the standing counter-example to that promise being reliable.
- **Amend the decided row in place.** Rejected: it is the instrument the register forbids, and it
  was tried and reverted. Recorded here so the next reader does not re-derive it.
- **A separate optional workflow / a `make` target.** Rejected under *no polling, only pushing*'s
  sibling logic: a gate that runs only when someone remembers to run it is not a gate. The
  `changes` job is the one place a check cannot skip on the docs-only path.

## Consequences

- Every push now proves the wrapper's 54 cases on a machine that is not the author's. A host
  assumption that drifts (python3, bash, PATH) reds a cheap job instead of silently un-verifying the
  supply-chain gate.
- `tools/codegen-rs` gains one more artifact it pins. That is the accepted shape of this repo's
  CI-pinning rule and mirrors an existing precedent.
- The decision chain for QMD retrieval now has two links. A reader resolving `RETRIEVAL-QMD` is
  routed by `superseded_by` to the head; the register's DAG walk and the ask-gate already do this.

## Consulted

Records created from a founder directive carry one line per lens (ADR-20260812-143619). Reversibility
class: **reversible** — a CI step, a codegen pin and two register rows; no money movement, no stored
event shape, no legal surface, nothing Tours-facing. Briefing roster sized to the class (2–3 lenses,
ADR-20260816-134352), with the full-diff independent review still to come as the third look.

- **beck** — briefed on the gate's own verification: what mutant must redden the new codegen pin, and
  whether the precedent test it mirrors is itself vacuous-green in any way that should not be copied.
- **farley** — briefed on the pipeline: whether the always-run `changes` job is the right home for a
  skill's suite, what it costs per push, and whether the step should be blocking.
- **architect** — not separately convened: this ADR carries no backlog re-ranking and files no new
  audit finding; the register semantics it turns on are `docs/decisions/README.md`'s, already
  validator-enforced.
- **legal-specialist** — nothing in this lens: no personal data, no external artifact, no capacity
  statement. Recorded so a lens never asked is not mistaken for a lens with nothing to say.

**Banked at the checkpoint** (ADR-20260816-134352 / ADR-20260817-105845): the narrow roster **did
miss something**. The independent review found an eleventh disarm mutant — hoisting the step-level
`if:` onto the `- ` item line, a spelling GitHub Actions accepts and the pin did not — plus a
decoy-line hole in the same test, and disproved this ADR's own coupling claim. **Attribution:
invited-lens depth miss, not roster width.** `beck` was briefed on exactly the question *"what
mutant must redden the new pin"* and answered by enumerating ten spellings rather than by making
the property unrepresentable; a wider roster would have put more readers in front of the same
enumeration. It therefore does **not** return to the founder, and the reversibility class stands.
The correction is the one the repo's compiler-first rule points at: the pin now asserts over
PARSED YAML, so every spelling of the mutant is unrepresentable instead of enumerated.
