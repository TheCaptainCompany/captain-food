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
- The register's own gate enforces the supersession coupling **where a challenge edge exists**, and
  that qualifier is load-bearing. Two earlier versions of this section claimed the coupling was
  total; both were disproved by construction, in successive reviews. **What is enforced, precisely:**
  (1) a `decided` challenge whose target is not `superseded` by it reds (pre-existing); (2) a target
  `superseded` by a challenge that has not itself CLOSED — `decided`, or `superseded` further down a
  chain — reds (added here). Both are planted red, and a legal two-link chain `A ← B ← C` is planted
  GREEN, because the first version of (2) demanded `decided` and false-redded exactly that: the next
  legal move on this chain, and the one this proposal's own rollback path instructs.
  **What is NOT enforced, deliberately**: a supersession carrying **no** `reconsiders` edge at all is
  legal. The second review required making it red; implementing that broke two PRE-EXISTING tests
  (`a_fully_valid_corpus_is_green`, `supersession_is_a_dag_walked_by_identity`, whose *"A → B (open)
  terminates: green"* case is exactly this shape). Those encode a deliberate design — a row can be
  superseded by a successor that never formally challenged it — and CLAUDE.md is explicit that a
  failing behaviour test means fix the generator, never the test. The required correction was
  therefore **declined, with evidence**, and the boundary is recorded in the test file so it is not
  re-litigated.
  **The cost that earned this note: a claim about a gate is worth nothing until the gate has been
  asked to be wrong, in both directions. Two record cycles asserted this coupling before anyone
  constructed the state, and the first fix for it introduced a false red of its own.**

## Context

[`RETRIEVAL-QMD`](../decisions/RETRIEVAL-QMD.yaml) adopted the advisory-retrieval integration and
enumerated what it does **not** authorize — among them *"hooks, CI/validator/agent-contract
changes"*. The follow-up chain (PRs #672–#676) then grew
`.claude/skills/decision-lookup/scripts/stub-tests.sh` to **54 hermetic cases**
(measured on `2fb3bd3c`, `main` at the time this row was decided, by the suite's own `RESULT`
line — an earlier draft of this ADR said "from 34 to 54" and the 34 could not be reproduced at
any commit in the named range, so the unverifiable endpoint is dropped rather than restated) and
`SKILL.md` declared it the **executable authority** for a wrapper that carries a supply-chain gate:
a pinned, scriptless `bun` install of `@tobilu/qmd`, a structural `bun.lock` version-integrity
binding, and delete-wholesale handling of a corrupt index.

That produced a contradiction the repo's own rules do not tolerate: a declared executable authority
that nothing executes. Its green reported the author's machine. "A gate cannot be ignored" is the
reason this repository prefers executable over prose — an unrun suite is prose.

**The evidence that this is not theoretical.** During PR #676 the step was briefly wired. Its
**first CI run went red at 26/52** and exposed a host dependency the suite had carried unnoticed:
the wrapper preflights `command -v bun`, so on a runner without `bun` every cache-building case
failed its precondition. The suite claimed hermeticity while requiring a real `bun` to be installed.
The defect was fixed in the very next commit on that same branch — not, as an earlier draft of
this ADR said, "independently" — but it was found **only** by running the suite off the author's
machine.

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

**Banked again after rounds 3 and 4** (same attribution — invited-lens depth, not roster width):
parsed YAML was not the end of it. A **twelfth** mutant sat one scope up (workflow-level
`defaults.run.shell` dropping the step script) and a **thirteenth** one scope down (job-level
`env: BASH_ENV`), and my fix for the twelfth introduced a **false red on ordinary CI work** by
banning key presence rather than dangerous content. The fourth round then found the cheapest
disarm in the whole corpus untouched — a sibling step overwriting the gate script — while the
guard's comment claimed no test of that class could close it. Rounds 5 and 6 then broke and
rebuilt the instrument twice more — a step-COUNT pin that a two-line edit inside an existing step
walked past, then a run-only substring scan that a `uses:` payload walked past. What stands now
pins the property (no non-gate step may mention the gate scripts or rewrite their environment,
anywhere in its definition; `uses:` allow-listed; `defaults.run` restricted at both scopes) and
**states a boundary instead of a completeness claim**. Round 7 then replaced the
approach rather than the boundary: each gate script now verifies that it and the script it guards
are byte-identical to their committed blobs before reporting anything.

**Round 8 found that fix unpinned, unexercised, half-applied and itself disarmable** — and found
that the same commit's refactor had silently deleted the `env_ok` call at job scope, reopening the
thirteenth mutant this ADR two paragraphs above records as closed. Four corrections landed:

1. **`env_ok` restored at job scope.** A guard removed during a refactor leaves no trace unless
   something plants it red — that, not the mutant, is the finding.
2. **Both gate scripts carry the block.**
3. **The block is hardened and default-ON.** It was disarmed by a `git` shell function sourced via
   job-level `env: BASH_ENV`; it now `unset -f`s and resolves its tools on a fixed PATH (`git`/`cmp`
   as of this round; `cmp` was dropped and `tr` added in review #15, when the comparison became
   object-id against object-id). And it
   ran only when `GITHUB_ACTIONS=true` — an on-switch settable from the surface it defends against,
   i.e. fail-open. It is now always on, with a named opt-out that CI may not set.
4. **It is pinned and planted red.**

**Round 9 then found three of those four corrections were themselves false, and the fourth
incomplete.** Recorded in full because the pattern is now the finding:

- **#1's "planted red at both scopes so a refactor cannot delete it invisibly again" was a
  SENTENCE.** The plants were manual and reverted; the test the comment named,
  `both_scopes_reject_execution_altering_env`, did not exist. The round's own headline lesson —
  *pin a guard from a test that fails when it is removed, not from a sentence* — was failing inside
  its own retraction. That test now exists, mutates the REAL `ci.yml` at every scope and on both
  trigger halves, and carries innocent controls that must stay green. **No count is stated here**:
  the ADR, the journal and the PR body all said "20 mutants, 5 controls" while the test carried 21
  and 7 — a derived number in three records with nothing re-deriving it, which is exactly what
  ADR-20260817-105845 forbids. The test now asserts its own corpus size, so the only number lives
  where it cannot drift from the arrays it measures. There are THREE `env_ok` call sites, not two -- an earlier version of this line said "either", and review #10 found the per-STEP one unguarded, the round-8 refactor regression reopened one scope down. All three are now planted red.
- **#2 did not fix the mutant it named.** A block inside a script goes away when the script is
  REPLACED, so putting one in `register-check-selftest.sh` cannot detect
  `find -name 'register-check-selftest.sh' -exec cp exit0.sh {} +`. Both gates stayed green. Each
  script now verifies the WHOLE four-file gate set, so replacing either guard is caught by the
  other, and the pin asserts both lists are complete.
- **#4's pin was a substring scan that matched inside comments.** Prefixing `# ` to every line of
  the block left four pins green over a script that verified nothing; and the plant-red fixture
  held only two of the four scripts, so `register-check-selftest.sh`'s block had never been
  exercised by anything in the repo. The scan now ignores comment lines and the fixture covers all
  four, with the guard-replacement case planted red.
- **The pin for the newest needle was itself vacuous, and `claude-review` caught it** (round 10,
  the first finding on this PR from the bot check rather than a team reviewer). The test kept TWO
  needle lists — one matched against the raw file, a shorter one against the comment-stripped
  source — and `unset "${!GIT_@}"` was added to the raw list only. Deleting the real line from both
  scripts left the pin GREEN, satisfied by the header comment *describing* the line that no longer
  existed, which re-opens the oracle-redirect route for every `GIT_*` source that is not `ci.yml`:
  an inherited runner environment, a composite action, a local invocation. **Two lists that must
  agree will diverge**; there is now one, checked where it has to hold. And nothing had ever SET a
  `GIT_*` variable in a test, so the defence was asserted by string match and never by behaviour —
  the runtime plant-red now builds a decoy repo whose HEAD holds the tampered bytes, points
  `GIT_DIR` at it, and requires the block to notice anyway.
- **A new disarm of the closed class**: `GIT_DIR` redirects the *oracle* rather than the binary —
  `git cat-file blob HEAD:<path>` reads a decoy repo whose HEAD holds the tampered bytes and
  reports OK — and it was reachable through a job-level `env:`. The scripts now
  `unset "${!GIT_@}"` and `env_ok` bans the whole `GIT_*` prefix, because enumerating this family
  is precisely how the previous two misses happened.

**THE BLAST RADIUS, recorded because the briefing did not put it to anyone.** Every other job
carries `needs: changes`, so a red stub suite SKIPS `lint`, `specs`, `build-test` and `db-test` —
you lose all feedback on an unrelated build for a reason that has nothing to do with it. The
`register-check-selftest.sh` precedent already had that coupling, but it is ~200 ms of pure shell
with no external dependency; this suite shells out to `python3`, constructs non-UTF-8 paths,
symlinks, an ASCII-locale probe and a poisoned sqlite file, and its own step comment enumerates
three distinct host-drift classes. `farley` was briefed on *"should the step be blocking"* and said
yes; **nobody was asked whether it should be blocking FROM INSIDE `changes`**. A sibling always-run
job, also aggregated by `codegen`, would be equally blocking without collapsing the rest of the
pipeline's signal. Not changed here — the step is where the row authorizes it — but the question is
recorded rather than left implicit, because the answer nobody was asked is the one that surprises
the next on-call (review of PR #679).

**THE FORK-PR RESIDUAL, stated in the record and not only in a source comment.** The `changes` job
executes two shell scripts *from the PR head* on every `pull_request`, forks included — unchanged in
kind from the `register-check-selftest.sh` step added 2026-08-21, with a `contents: read` token and
no secrets exposed. But the honest reading of "the gate scripts are the ones in the commit CI
checked out" is that **on a fork PR they are the FORK'S**, and the self-verification compares them
against the fork's own merge commit. It proves internal consistency, not provenance. A reviewer
reading a fork PR must still read the diff of those two scripts; nothing here substitutes for that.
Recorded here because a residual that lives only in a code comment is one nobody finds at the moment
it matters (review of PR #679).

The honest verb is **DETECT**: the script still runs and refuses to report. It is not a defence
against arbitrary code running before it, and a commit that changes the gate scripts in the same
change remains a code review's job. **The recurring defect is not the mutants; it is
that each round's completeness claim was written before it was checked** — round 7's was the
fourth, and it was wrong in two independent ways at once. TWO register-machinery
rules land here, not one: the supersession-coupling mirror arm, and
`decision-superseded-authority`, which fails when any file **in the corpus named by
`claude_citation_corpus`** — the one place that set is stated, and deliberately wider than the
`.claude/** plus the ignore files` this paragraph first claimed: it is a `git ls-files` walk that
also reads `CLAUDE.md`, the resident index, and the `Makefile` — cites a row whose status is
`superseded` as live authority. It is the executable form of
CLAUDE.md's grep-the-old-term rule, earned because this change flipped `RETRIEVAL-QMD` to
`superseded` and left EIGHT sites citing it, one of them the wrapper's runtime failure message on
the rollback path. Both are governed by `docs/decisions/README.md`, not by this row's QMD surface — it is
the remediation of a claim this ADR itself made, which is why it rides along.
