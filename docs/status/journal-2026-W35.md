# Status journal — 2026-W35

Journal entries for ISO week 2026-W35, newest first, in the order they were written.
Current state: [`../STATUS.md`](../STATUS.md).

> **Week boundary, recorded once so the next reader does not hunt**: 2026-08-24 is ISO week **35**,
> but several entries dated 2026-08-24 sit in [`journal-2026-W34.md`](journal-2026-W34.md) — earlier
> sessions filed them there before this file existed. They are left where they are rather than
> rewritten; W34 is the place to look for the earlier part of that day.

> ✅ **2026-08-24 — RETRIEVAL-QMD-CI decided: the decision-lookup stub suite now runs in CI, and
> the mob's briefing found four disarm mutants alive in the gate it was told to copy.** Founder
> approval of the open challenge row filed earlier the same day. **The instrument, this time,
> is the one the register defines**: closing a challenge IS the supersession move, so
> `RETRIEVAL-QMD-CI` -> `decided` (`decided_by: ADR-20260824-205911`) and `RETRIEVAL-QMD` ->
> `superseded` + `superseded_by: RETRIEVAL-QMD-CI` land in ONE commit. **The claim originally
> written here — that `decision-reconsiders-shape` "rejects either half alone" — was FALSE, and the
> independent review disproved it by construction**: only the challenge-side direction was
> enforced. A target `superseded` by a still-`open` challenge passed `make validate` with zero
> errors, leaving the register in exactly the split state `docs/decisions/README.md` forbids — a
> superseded row whose authority points at a question nobody answered. The mirror rule is now added
> and planted red. **That fix was then ALSO wrong, and the second review disproved its claim the
> same way**: the mirror rule demanded the challenge be `decided`, so it FALSE-REDDED a legal
> two-link chain — the next legal move on this very chain, and the one the rollback path instructs —
> and its error text told the reader to *"return the row to `decided` and drop its
> `superseded_by`"*, i.e. to undo a correctly executed supersession, the exact corruption the README
> forbids. **A gate that teaches the corruption is worse than the gap it replaced.** Both are fixed;
> the legal chain is now planted GREEN alongside the reds.
>
> **One required correction was DECLINED, with evidence.** The review also required reddening a
> supersession that carries no `reconsiders` edge at all. Implementing it broke two PRE-EXISTING
> tests (`a_fully_valid_corpus_is_green`, `supersession_is_a_dag_walked_by_identity` — whose
> *"A → B (open) terminates: green"* case is exactly that shape). Those encode a deliberate design,
> and CLAUDE.md says a failing behaviour test means fix the generator, never the test. So the rule
> was reverted and the boundary recorded in the test file instead, with the reason, so the next
> session does not re-implement it. **A reviewer requiring a change is not authority to break the
> suite that already answers the question.**
>
> **Cost that earned the rule: two record cycles asserted this coupling was total before anyone
> constructed the state, and the first fix for it shipped a false red. A claim about a gate is worth
> nothing until the gate has been asked to be wrong — in BOTH directions. "I added the rule" is not
> "I checked the rule's boundary."** Because the successor becomes the CHAIN
> HEAD, it carries the predecessor's controlling content forward with one clause narrowed. On
> [#679](https://github.com/TheCaptainCompany/captain-food/pull/679) (open at the time of writing —
> this line is not a merge claim), issue
> [#678 "Wire the decision-lookup hermetic stub suite into CI (RETRIEVAL-QMD-CI)"](https://github.com/TheCaptainCompany/captain-food/issues/678).
>
> **What the briefing caught, before any CI code existed — the reason the mob happens first.**
> `beck` read the precedent this dispatch told me to mirror,
> `the_hook_selftest_runs_in_the_always_run_changes_job`, and found it **green under four mutants
> that fully disarm the step it claims to pin**: a trailing `|| true`, a trailing `; exit 0`,
> `continue-on-error: true` on the step, and a step-level `if:`. `ci.find(cmd)` is a substring
> search, so none of them are visible to it. The new pin therefore asserts the step's **whole
> line** (`assert_eq!(step_line.trim(), "run: <cmd>")`) plus three job-wide properties, and the
> precedent got the same line-equality assertion in the same change — the job-wide ones cover its
> step too. **Cost that earned the rule: zero, this time — the gate had never been asked to be
> wrong.** A pin that proves WHERE a step is proves nothing about whether it can fail.
>
> **A second vacuity, in the suite itself, found by both lenses independently.** `skipped()` does
> not count as failure and `exit "$fail"` was the only verdict, so cases that stop running report
> a green over a fraction of the suite. **The figure first written here — "6 passed, 0 failed, 12
> skipped" — was invented**, and the review caught it: there is exactly ONE `skipped()` call site
> (T15g), so the reachable vacuous green was 53/54, not 6/54. The guard is still worth having; the
> number was not derived from the code, which is the exact shape ADR-20260817-105845 targets. This
> is the `DB_TESTS_REQUIRED=1` shape of
> [#230](https://github.com/TheCaptainCompany/captain-food/issues/230) transposed. Fixed INSIDE
> the script (`EXPECTED_CASES=54`), not in the YAML, so the CI step stays one bare line the pin
> can match exactly and adding a case must move the number in the same diff. **That first guard
> was itself wrong twice**, both found by the review: keyed on `pass != 54`, it fired on a REAL
> wrapper failure and then told the reader to *"adapt the case to the host"* — the
> weaken-the-assertion move its own next line forbids, a signpost pointing away from the bug the
> step exists to find — and it incremented `fail` on top of the real failure, so the exit status
> stopped being the failure count. It also turned T15g's deliberate macOS skip into a hard red,
> contradicting two comments in the same file. The invariant is now COMPLETENESS, not passes:
> `pass + fail + skip == EXPECTED_CASES`, which survives a legitimate skip (loud, still green) and
> still reds the moment cases stop running.
>
> **Planted-red, ten mutants, each with its own message** (a gate never seen red is an unverified
> claim): M1 step deleted · M2 step moved into `lint:` · M3 `|| true` · M4 `; exit 0` · M5
> step-level `continue-on-error` · M6 step-level `if:` · M7 job-level `if:` on `changes` · M8 case
> count drifts (54 vs declared 55) · M9 the count guard removed from the suite · M10 a real
> wrapper defect (`qmd_lock_binding_ok` always accepts) — which reds 6 lockfile-integrity cases,
> proving the suite can still testify about the thing it exists to protect. All ten red; `ci.yml`
> and both scripts restored byte-identical afterwards.
>
> **AND THE PIN WAS STILL DISARMABLE — the eleventh mutant, found by the independent review.** Ten
> planted mutants bought confidence in a test that asserted
> `!contains("\n        if:")` — an **indentation string, not the property**. Hoisting the same
> condition onto the `- ` item line (`- if: …` / `name:` / `run:`) is a spelling GitHub Actions
> accepts, and every assertion stayed green while the step never ran. The same test had a second
> hole: a decoy `run: <cmd>` line inside an earlier block scalar satisfied the line-based
> `assert_eq!` while the real step carried `|| true`. **The lesson is the repo's own rule, learned
> the expensive way: enumerating spellings is not a gate.** The pin now asserts over PARSED YAML —
> the step is located at `jobs.changes.steps` by its `run` VALUE and its key set must be exactly
> `{name, run}`, which leaves nowhere to put `if`, `continue-on-error`, `shell` or `env` in any
> spelling. Re-planted: M11 hoisted `if:` RED · M12 decoy line + `|| true` RED · M13 `shell:` RED ·
> M14 job-level `defaults:` RED · M15 renaming `lint:` GREEN, and that green is the fix working —
> the pin no longer depends on that anchor. **M14's spelling was later deliberately invalidated**
> (see below): job-level `defaults` PRESENCE is legal now; what reds is a `defaults.run.shell`
> that is not a bare shell name.
>
> **ROUNDS 3 AND 4 — the same mistake twice more, and a false red of my own.** The third review
> found the **twelfth** mutant one scope UP: a workflow-level `defaults.run.shell` that drops the
> step script (`bash -c "exit 0" {0}` passes it as `$0`), disarming BOTH pinned gates at once. My
> fix banned the KEYS `defaults` and `env` at the root — **which reds ordinary CI work**: 4 of the
> repo's 15 workflows already carry a root `env:`, so `CARGO_TERM_COLOR: always` would have failed
> with a message accusing the author of disarming the ask-gate. The suite's own T15g reasoning
> applies to its own guard. And banning the key at the root missed the **thirteenth** one scope
> DOWN: a job-level `env: {BASH_ENV: preamble.sh}` — bash sources `$BASH_ENV` before a
> non-interactive script, so two lines kill both gates. Both were the same error: **keying on key
> PRESENCE instead of dangerous CONTENT.** Now both scopes check content (bare shell name;
> no PATH/BASH_ENV/ENV/SHELLOPTS/LD_PRELOAD/LD_LIBRARY_PATH/BASH_FUNC_*, plus each gate script's
> self-verification opt-out name). Planted: M19 job `BASH_ENV` · M20/M21 shell-drops-script at job
> and workflow scope · M22 workflow `PATH` · M23 job `LD_PRELOAD` → RED; `CARGO_TERM_COLOR`,
> `RUST_LOG`, `shell: bash` → GREEN.
>
> ⚠️ **This paragraph recorded a red that stopped existing two commits later, and the eighth review
> measured it back to green.** The refactor extracting `shell_ok` deleted the `env_ok(changes_val)`
> call in the same hunk, so M19 and M23 — the job half — passed again while this entry, `ci.yml`'s
> comment and the ADR all still said they were closed. **The lesson is not "re-check the journal":
> it is that a guard deleted during a refactor is invisible unless something plants it red.** Never
> change structure and behaviour in one step (Beck), and pin a guard from a test that fails when it
> is removed, not from a sentence.
>
> **The fourth review then showed my "honest boundary" was itself an overclaim.** The comment said
> no test of this class could close the sibling-step vector — and the cheapest disarm in the entire
> M1–M23 corpus was exactly that: **one extra step** running
> `printf '#!/bin/sh\nexit 0\n' > .../stub-tests.sh`, leaving both gates GREEN and CI green. A
> re-pointed checkout (`repository:`/`ref:`), a `container:`, a self-hosted `runs-on` and a
> `defaults.run.working-directory` were all green too. **Declaring a vector unclosable while
> claiming to narrow the cheap spellings, when the cheapest one was untouched, is an overclaim
> about an overclaim.**
>
> **ROUND 5 then broke the instrument I had just built.** I replaced the sibling-step hole with
> `steps.len() == 4` — and the fifth review found the **fourteenth** mutant, two lines added INSIDE
> an existing step, which a count cannot see; and pointed out that the count itself would red on
> the next innocent step addition (that job gained a gate step twice in four days), whose
> one-character "fix" reopens the vector. The count pin is gone. What replaced it pins the
> PROPERTY: no non-gate step may mention `.claude/`, `GITHUB_ENV` or `GITHUB_PATH`; no checkout may
> re-point `repository`/`ref`; a checkout must exist. Innocent additions and reordering stay green.
>
> **ROUND 6 found four more, and one of them ended the arms race.** A `uses: actions/github-script`
> with a `with: script:` payload (no `run:` at all, so a run-only scan never saw it); any
> non-checkout action; and `defaults.run.working-directory` at WORKFLOW scope — the same
> one-scope-up miss as the twelfth mutant, again. All three are closed: the scan now serializes the
> whole step, `uses:` is allow-listed to `actions/checkout`, and the `defaults.run` key restriction
> applies at both scopes. **The fourth is not closable and that is the finding**: a step that
> rebuilds the path (`d=".cl""aude"`) or never names it (`find . -name 'stub-tests.sh' | while
> read -r f; ...`) defeats any substring scan. So the residual is now stated as what it is — this
> narrows the CASUAL and ACCIDENTAL spellings, the ones a session reaches for under time pressure,
> which is the failure mode this repo has actually hit. **It is not a defence against someone with
> commit access who wants the gate off. No CI-config test can be.** Six rounds of new spellings is
> the evidence for stating a boundary instead of another completeness claim.
>
> **And the precondition classifier from round 5 was reverted in round 6.** It routed on file
> existence (`-e`) while the guards test non-emptiness (`-s`), and it named causes that are
> impossible for the site it printed them on — T15g constructs no bun, no controlled PATH and no
> python3 shim, yet got a message naming all three, and lost the disjunct that was the real cause.
> A message naming three impossible causes is worse than the ambiguous one it replaced. The
> per-site labels are back, each naming its own disjuncts; `ci.yml` now tells the reader to
> classify by which disjunct fired rather than by a rule that could not sort 8 of 17 sites.
>
> **ROUND 7 ended the needle-chasing by changing what is verified.** Two more spellings walked
> past the scan, one character apart: `cd .claude && ...` and `working-directory: .claude` (the
> needle was `.claude/`, with the slash). And the workflow TRIGGER was unpinned — `paths-ignore:
> ['docs/**']` removes both gates from the docs-only push path, which is the dominant path for
> this repo, with every assertion green. Both are closed. But the durable fix is the one the review
> recommended: **the suite now testifies about its own inputs.** In CI, before reporting anything,
> `stub-tests.sh` verifies that it and the wrapper are byte-identical to their committed blobs.
> **The claim that this "kills the ENTIRE overwrite class" was the FOURTH completeness claim in
> this chain written before it was checked, and the eighth review disproved it two ways in one
> pass**: only `stub-tests.sh` carried the block, so a `find -exec cp` step replaced
> `register-check-selftest.sh` with `exit 0` and every assertion stayed green; and the block itself
> was disarmed by a `git` shell function sourced through job-level `env: BASH_ENV`, printing
> `54 passed` over a visibly tampered wrapper.
>
> ⚠️ **THE NINTH REVIEW THEN DISPROVED THREE OF THE FOUR FIXES FOR THAT, AND THE FAILURE MODE IS
> THE ONE WORTH KEEPING.** *"Both gate scripts carry the block"* does not close the mutant it was
> written for: **a block inside a script goes away when the script is REPLACED**, so
> `find -name 'register-check-selftest.sh' -exec cp exit0.sh {} +` still left both gates green. The
> pin was a substring scan that matched **inside comments** — `# ` in front of every line left four
> pins green over a script verifying nothing — and its plant-red fixture held only two of the four
> scripts, so that block had never been exercised by anything in the repo. And the test a comment
> named as the thing preventing the `env_ok` regression from recurring, `both_scopes_reject_
> execution_altering_env`, **did not exist**: the plants were manual and reverted, i.e. the round's
> own headline lesson failing inside its own retraction.
>
> All of it is fixed by making the guards mutual rather than reflexive: **each script verifies the
> WHOLE four-file gate set**, so replacing either guard is caught by the other; the pin ignores
> comment lines and requires both lists complete; the fixture covers all four with the
> guard-replacement case planted red; and `both_scopes_reject_execution_altering_env` now exists,
> mutating the REAL `ci.yml` at every scope and on both trigger halves, with innocent controls that
> must stay green (no count stated — it drifted in three records at once; the test asserts its own
> corpus size now). A fourth disarm
> of the same class fell out of it: **`GIT_DIR` redirects the ORACLE, not the binary** — `git
> cat-file blob HEAD:<path>` reads a decoy repo whose HEAD holds the tampered bytes and reports OK
> — reachable through a job-level `env:`. Closed with `unset "${!GIT_@}"` in both scripts and a
> `GIT_*` PREFIX ban in `env_ok`, because enumerating that family is how the previous two misses
> happened.
>
> **Round 10 then found the fix for that had the same shape one level in**, and it was the
> `claude-review` BOT that found it, not a team reviewer — the first time in this chain the required
> check earned its keep. `assert_gate_script_self_verifies` kept TWO needle lists, one matched
> against the raw file and a shorter one against the comment-stripped source, and the needle added
> in round 9 went into the raw list only. Deleting the real `unset "${!GIT_@}"` from both scripts
> left the pin green, satisfied by the header comment DESCRIBING the deleted line — the round-9
> defect (`raw.contains` matching inside a comment) recurring for the one needle added after round
> 9. **Two lists that must agree will diverge**: there is now one. And no test had ever SET a
> `GIT_*` variable, so that defence was asserted by string match and never by behaviour; the
> runtime plant-red now builds a decoy repo whose HEAD holds the tampered bytes and points
> `GIT_DIR` at it.
>
> ⚠️ **ROUND 10 THEN SHOWED THAT SENTENCE WAS FALSE ABOUT HALF ITS OWN SUBJECT**, and both the team
> reviewer and the `claude-review` bot found it independently. The plant-red test copied all four
> scripts into its fixture but only ever EXECUTED `stub-tests.sh`, so the hook selftest's block was
> still held up by a substring scan — and a scan cannot tell live code from dead code. Changing its
> opt-out default to `${REGISTER_CHECK_ALLOW_DIRTY:-1}` disarmed it in CI with every needle, every
> gate path and all 285 tests green, which reopens the round-9 mutant because that block is the
> ONLY detector for wholesale replacement of its twin. **Adding a script to a fixture as a TARGET
> is not exercising its BLOCK** — the stated defect and the stated remedy were different things.
> Both guards are executed now, and `:-1}` and `|| true` are planted red.
>
> Two more from the same pass: **four files claimed the pin "runs in the `codegen` job, a different
> job with its own checkout"** — `codegen` is a pure aggregator with one step, no checkout and no
> cargo; the tests run under `cargo test --workspace` in `build-test`. A thirty-second check,
> landed by this branch, inside the paragraph announcing it was stating a boundary rather than a
> claim. And **the oracle was not independent of the workspace**: `printf 'exit 0' > register-check.sh
> && git commit` makes HEAD agree with the tampered disk, so both guards printed OK over a dead
> gate. Closed by pinning the comparison to `$GITHUB_SHA` — which a later step cannot change for an
> earlier one — and forbidding that key in `env:` at every scope.
>
> **The rule this chain has now re-learned SEVEN times**: no assertion about a guard ships until the
> guard has been made to fail FROM A TEST IN THE REPO — and "the guard" means every instance of it,
> not the one the test happens to invoke. A reviewer's manual
> plant, reverted, is a story about a gate — not a gate. The correct verb is **DETECT**, not
> prevent, and it is not a defence against arbitrary code running before it.
>
> **Two more shape errors the same review found, worth more than the mutants**: the block ran only
> when `GITHUB_ACTIONS=true`, which fails OPEN — the on-switch was an ordinary environment variable
> settable from the very surface it defends against. It is now default-on with a named opt-out
> (`DECISION_LOOKUP_ALLOW_DIRTY` / `REGISTER_CHECK_ALLOW_DIRTY`), both forbidden as CI `env:` keys,
> and `stop-gate.sh` — the interactive path where editing a hook is the normal loop — opts out
> visibly at its one call site. And the block **had never been seen red anywhere**: local runs skip
> it by design and nothing constructed a tampered tree, so its only proof of life was a reviewer's
> manual run. `the_gate_self_verification_reds_on_a_tampered_script` now builds a throwaway git
> repo, tampers each script in turn, and asserts a non-zero exit naming the file. **Cost that
> earned it: six rounds of mutants that were all one shape, then two rounds proving the fix for
> them was itself unpinned and unexercised.**
>
> A note for whoever next pins a workflow key: **`on:` resolves differently per YAML version** —
> 1.1 (PyYAML) makes it the boolean `true`, 1.2 (serde_yaml) keeps it the string `"on"`. A lookup
> written for the wrong one returns None and the assertion passes VACUOUSLY. The pin accepts both.
>
> **Cost that earned the rule, stated once: three consecutive rounds of "I closed it" were wrong,
> and each time the wrongness lived in a sentence claiming completeness. Prefer naming what a
> guard does NOT reach over asserting it reaches everything — and then check that sentence too.** The precedent test was moved onto the same helper; it
> had every one of these holes.
>
> **Banked, with attribution**: an invited-lens depth miss, not roster width — `beck` was briefed on
> exactly *"what mutant must redden the new pin"* and answered by enumerating ten spellings instead
> of making the property unrepresentable. A wider roster would have put more readers in front of the
> same enumeration, so it does not return to the founder and the reversibility class stands.
>
> **STOP RAISED, NOT OVERRIDDEN — the #677 half does not land yet.** `farley`'s register check
> surfaced [DECISIONS §45 **REV-1**](../proposals/DECISIONS.md): the founder decided on 2026-08-17,
> **against the team's own recommendation**, that `claude-review` comes OUT of the required
> checks — and it was **never executed** (403 from the session's agent proxy on the ruleset write
> path, an open action on
> [#593 "The claude-review bot gate blocks every merge when it cannot run"](https://github.com/TheCaptainCompany/captain-food/issues/593)).
> Hardening a check that is decided-out but still required turns every "the reviewer could not
> post" into a repo-wide merge stop — which is #593 verbatim, the failure that produced REV-1, and
> `claude-code-review.yml` records `api_error_status: 429` / out-of-credits runs from **today**.
> Worse, it is **self-blocking**: the revert PR would itself need `claude-review` green to merge,
> and the only key is the admin ruleset path that 403s. The ordering, not the instrument, is the
> defect — REV-1 first, then the hardening. **Reversibility class corrected**: a self-blocking
> merge gate is not "reversible" because its diff is small; the merge machinery is a named
> `HOLD: human` class whatever the diff looks like.
>
> **Recorded because it is not derivable from the code**: a workflow file cannot make itself
> required or un-required, and cannot stop branch protection accepting `skipped` — so every YAML
> edit in `claude-code-review.yml` is a **verdict-honesty** fix and never a requiredness fix.
> Anyone reading #677 as "option 2 solves requiredness" is reading it wrong.>
> **Round 10, and the finding is in the half nobody looked at.** Six rounds hardened
> `on.pull_request` — content-based bans, the default `types` triad, the `branches` containment
> check with its `!`-exclusion analysis. The `on.push` half, three lines above it, still banned
> only `paths`/`paths-ignore`, and its branch check opens `if let Some(seq) =
> push.get("branches")` — so it did **nothing at all** when `branches` was absent.
> `branches-ignore: ['main']` needs no `branches` key; `tags: ['v*']` in place of the branch list
> makes the trigger tag-only. Each removes BOTH gate steps from the push lane with every assertion
> green — and the push lane is the one CLAUDE.md routes spec- and docs-only changes down, with no
> PR to fall back on. **The generalisable shape: when one of two symmetric surfaces gets six
> rounds of attention, the other has silently become the cheap way in.** Both halves now carry
> the same ban list, tag filters are ruled on as a PAIR (`tags` alongside `branches` is legitimate
> release plumbing and has a green control; `tags` alone is the defect), and both holes are
> planted mutants in the corpus.
>
> **The false-red instrument, one more time, in the rule written to stop stale citations.** The
> `decision-superseded-authority` rule accepted the bare article `the` as a citing token, so
> `mirrors the <KEY> rollout` or `narrower than the <KEY> surface` would each red `make validate`
> as a **hard error** with no escape but rewording. None of them tells a session to cite a dead
> row. Every green control had avoided putting `the` immediately before a key, which is why the
> controls missed it. `the <KEY>` now counts only when the word AFTER the key is a citing noun.
> **And the clause-scoped `superseded` exemption is per-LINE**: a multi-line comment explaining
> the very incident that motivated the rule did not inherit it, so the wrapper's own retraction
> comment redded. The prose was corrected to name the row as superseded in the citing clause,
> which is what it always meant — recorded because the next author of a multi-line comment about a
> dead row will hit exactly this.>
> **Rounds 11 and 12 — the reviewer found the half of the CI surface nobody had pointed a guard at,
> twice in a row, and the shape is the same both times.** Round 10 was `on.push` sitting three lines
> above six rounds of `on.pull_request` hardening. Round 12 was **`build-test`** — the job that
> actually RUNS `assert_pinned_in_changes_job`, `assert_gate_script_self_verifies` and the mutant
> corpus — with no assertion over its own `env:` or `defaults.run` at all, while the `changes` job it
> polices was governed at three scopes. A `build-test`-scope `env: {LD_PRELOAD: …}` or `{BASH_ENV: …}`
> makes **every pin in the file vacuous** with `changes` green and `codegen` aggregating green, and
> the next commit is then free to disarm the gate step itself: mutants twelve and thirteen, one job
> over. The guards are extended there and planted red; a `build-test` step that rewrites the pin's
> own SOURCE before `cargo test` is named as a residual instead, because banning the path is the
> enumeration instrument this file has retracted three times and would red
> `cargo test --manifest-path tools/codegen-rs/Cargo.toml`. **Generalisable: the guard goes where the
> author was looking. Ask what runs the guard.**
>
> **The citation rule could not see the citation form the register mandates.** `docs/decisions/<KEY>.yaml`
> — the exact-row-resolution path `SKILL.md` and `CLAUDE.md` both prescribe as *the* authority path —
> matched no arm, so the HIGHEST-authority way to point a session at a dead row walked past the gate
> while the weaker prose forms (`row X`, `the X decision`) were caught. One arm closes it, and adding
> it immediately exposed that **the new arm and the existing clause exemption did not compose**: the
> `.yaml` extension put a `.` right after the key, and `.` was an unconditional clause boundary, so
> the clause truncated to the path and the `superseded` exemption could never be reached. A `.` now
> ends a clause only when it ends a SENTENCE (followed by whitespace or nothing). **Both of those were
> found by GREEN CONTROLS, not by reading the code** — which is the argument for writing the control
> in the same commit as the arm, every time.
>
> **`GITHUB_SHA` as the oracle: recover before refusing.** Pinning the gate scripts' oracle to
> `GITHUB_SHA` (round 10's own fix) made an ordinary push to `main` able to red an unrelated PR — on
> `pull_request` that sha is the merge commit as of QUEUE time, `actions/checkout` resolves
> `refs/pull/N/merge` at FETCH time, and CLAUDE.md makes docs- and spec-only pushes straight to `main`
> the dominant lane, so the window is hit routinely. Because `changes` is the always-run job that
> everything else `needs:`, the consequence was the whole pipeline red on an untouched diff. Both
> scripts now try `git fetch --no-tags --depth=1 origin "$_ref"` before refusing — GitHub's
> upload-pack serves fetch-by-SHA — and the refusal is unchanged if that fails, so fail-closed is
> intact. **Recorded because it is not derivable: a gate that pins an oracle to an event-time sha
> inherits that event's races, and a fail-closed gate on an always-run job spends them at full
> pipeline width.**
>
> **The blast radius is now a ROW, not a banked paragraph** — `GATE-STEP-LOCUS`, open: do executable
> gate steps belong inside `changes`, or in a sibling always-run job that `codegen` aggregates
> equally? Everything carries `needs: changes`, so a red there skips `lint`, `specs`, `build-test`
> and `db-test` and loses all build and test signal for a reason unrelated to the diff — and the
> stub suite's own step comment enumerates three host-drift classes as expected failure modes.
> `RETRIEVAL-QMD-CI` authorizes that step IN THAT JOB, so #679 could not move it; a banked paragraph
> is invisible to the next author and the precedent sets itself by default.>
> **Round 13 — the rule was line-scoped in a corpus that hard-wraps at ~100 columns, and that broke
> it in BOTH directions.** `decision-superseded-authority` decided everything inside one physical
> line. So (a) a backticked key that merely happened to land at the start of a continuation line was
> read as a citation opening a line, and (b) the documented escape — putting the word `superseded`
> in the clause — stopped working the moment the wrap pushed that word to the next line. Both land
> as a **hard `make validate` error** that blocks every push, and the author's only remaining moves
> are rewording or re-wrapping: *a red whose escape is silence, on exactly the prose the rule wants
> people to write.* **Today's corpus was green BY LUCK** — two sites repeat "superseded" on both
> wrapped lines and one is a single long line. **Every green control in the test was a single line,
> which is precisely why the class was invisible to it.** The scanner now joins consecutive non-blank
> lines into one unit (leading `#`/`//`/`>`/`-`/`*` markers stripped) before scanning, which also
> *improves* detection: a citation split across a wrap was missed before and is caught now. **The
> generalisable rule: a line-scoped check over hard-wrapped prose has a defect wherever the wrap
> falls, and single-line fixtures cannot see it.**
>
> **The gate compared a RAW blob against a SMUDGED worktree file.** `cat-file blob | cmp` reads git's
> own EOL translation as tampering: the committed blobs are LF, `ci.yml`'s drift step records that
> this repo is authored on Windows, `stop-gate.sh` carries a Cygwin branch, Git for Windows defaults
> to `core.autocrlf=true` and there is no `.gitattributes`. A **completely clean** Windows checkout
> therefore failed all four comparisons and printed *"Something modified a gate script … A green here
> would be a lie"* — with nothing in the message, `SKILL.md` or `workflow.md` mentioning line
> endings, so the remedy a reader reaches for is deleting the block the header begs them not to
> delete. **CI is Linux-only, so the plant-red fixture builds and reads on one platform and is
> structurally blind to the class.** Now object-id against object-id (`git hash-object` runs the
> same clean filter git runs on commit), which detects tampering identically and removed the `cmp`
> dependency outright — a required binary nothing calls can only produce a false refusal.
>
> **`make stub-tests` exists now, and the sentence that hid its absence is retracted.** The header of
> `stub-tests.sh` said *"SKILL.md, workflow.md, the Makefile and stop-gate.sh all carry this"* about
> `DECISION_LOOKUP_ALLOW_DIRTY`. **Three of the four carried nothing of the sort** — they name
> `REGISTER_CHECK_ALLOW_DIRTY`, a different variable for the other script, and none of them invokes
> this suite. The sentence hid the real gap: the hook selftest got `make hooks-test` and a stop-gate
> step so its edit-and-re-run loop kept working, and **the suite this entire change is about got
> neither**. Same shape as the round-9 `make hooks-test` trap, in the comment written to close it.
>
> **Two guards that could not fail, both closed by asserting what they were about.**
> `assert!(real.is_empty())` over the citation corpus is *satisfied* by an empty corpus, and
> `claude_citation_corpus` returns `Vec::new()` on every failure path — so narrowing the pathspec or
> dropping `"md"` from the extension filter removed `SKILL.md` from the walk with every test green.
> It now asserts four named files are reached AND plants a citation red **through the real corpus**.
> Separately, deleting the `main.rs` call site left `cargo test --workspace` green, so the rule could
> stop running inside `make validate` with no red anywhere — `the_citation_rule_is_wired_into_the_validator`
> closes that half. And `SKILL.md`'s `**54 cases**` was a derived number with nothing re-deriving it:
> the pin now parses `EXPECTED_CASES=` out of the script and asserts the doc states the same integer
> (planted red at 55 before landing).
>
> **Round 14 — my own fix for round 13 was wrong in the PERMISSIVE direction, which is the one that
> matters.** Joining wrapped lines fixed both false reds, and it also grew the window the
> `superseded` exemption searches from a line to a whole paragraph. `logical_units` stripped `-` and
> `*` like comment markers, so two adjacent BULLETS became one unit and an unrelated one could
> silence a live citation:
>
> ```text
> - Per row OLD-ROW, open a reversal decision before changing the pin
> - (that row is superseded)
> ```
>
> Neither `(` nor `)` is a clause boundary, so `superseded` landed inside the citing clause and the
> stale instruction went **green** — where scanning line 1 alone had **redded** it. That is
> `activation_fail`'s exact shape, i.e. the motivating incident, silenced by the commit written to
> protect it. **All three new controls were in the false-red direction; there was no plant for the
> direction the change actually moved.** A markdown continuation is INDENTED, not re-marked, so a
> `- `/`* `/`1. ` line now starts a new unit while `#`, `//` and `>` stay continuations (a shell
> comment block repeats them on every line). Two reds and two greens planted, and the block rule is
> planted red by removing it.
>
> **The generalisable rule, and it is the sharpest one this chain produced: when a change loosens
> and tightens at once, the controls you write will be for the direction you were thinking about.
> Name the direction the change MOVED and plant that one.** The docstring also claimed joining
> "strictly IMPROVES detection" — true of split citations, false of the exemption, and the bullet
> rule is what finally makes the sentence true. Retracted in place.
>
> Minor, same round: the finding's location had become the paragraph's first line rather than the
> citing one. `spans` maps the offset back, and a case pins it — an unpinned improvement is what
> this PR keeps retracting.
>
> **Round 15 — my fix for the EOL false red opened a new route into the oracle, and my fix for the
> merge-ref race was a sentence.** Both are the same failure: a remedy adopted faster than its
> justification was checked.
>
> **`hash-object` applies CLEAN FILTERS, and that is the knob that makes the comparison lie.** Git
> locates its global config through `$XDG_CONFIG_HOME/git/config` and `$HOME/.gitconfig` — neither is
> a `GIT_*` name, so `unset "${!GIT_@}"` does not touch them and the `GIT_*` prefix ban does not
> cover them. Global config sets `core.attributesFile`; an attributes file binds a
> `filter.<x>.clean` driver; a driver that re-emits `cat-file blob <ref>:<path>` reproduces the
> committed id for **every** path, so all four comparisons match over tampered scripts and both
> guards print OK. That is review #9's `GIT_DIR` decoy, one config-lookup mechanism over. Closed
> with `hash-object --no-filters` (which disables clean filters AND eol conversion, so no
> configuration reachable from the environment, `.git/config` or an attributes file can affect the
> hash) plus an **explicit** CR strip as the single translation this gate accepts, and `HOME`/
> `XDG_CONFIG_HOME` banned as CI `env:` keys with three planted mutants. `GIT_CONFIG_GLOBAL=/dev/null`
> was the tempting fix and is wrong: it also drops `core.autocrlf`, reinstating the false red the
> previous round removed. **`tr` is now inside the oracle, so it is resolved on the same pinned PATH
> and `unset -f`'d with `git`** — a tool the oracle calls is part of the oracle.
>
> **THE FETCH RECOVERY IS REMOVED, and the removal is the finding.** Round 11 asked for
> `git fetch --no-tags --depth=1 origin "$_ref"` before refusing, and I landed it with the sentence
> *"upload-pack serves fetch-by-SHA, so one fetch usually turns the refusal back into a
> verification"* — **an antecedent-free claim of exactly the shape ADR-20260817-105845 governs, in a
> branch whose thesis is that no completeness claim ships before it is checked.** The case it
> targets is an ORPHANED merge commit, reachable from no ref once the base moves, which is precisely
> the unadvertised-object case a server refuses; so the recovery most likely no-ops in the one
> situation it exists for. **Nothing planted it**: the only test on that path uses a fixture repo
> with no `origin`, so the fetch failed instantly on "no such remote" and proved only the refusal
> that already existed — deleting the whole block redded nothing. And `--depth=1` against a checkout
> deliberately fetched at `fetch-depth: 0` writes `.git/shallow` and shallows the workspace for every
> later step. Removed rather than kept on a hope, with the reasoning in the script so the next
> session does not re-add it. **A reviewer asking for a fix is not evidence the fix works.**
>
> **`make stub-tests` landed with no record moving** — and `PROP-20260822-171212`, rewritten in the
> same PR, listed *"a Makefile target"* as an unqualified non-goal. The row handles this class by
> NAMING it (clause (b) carries `make hooks-test` and the `stop-gate.sh` step); the sibling target
> was not named anywhere. Third drift of the enumeration the row's own CLAUSE HISTORY exists to
> track. Both amended: neither target carries QMD behaviour, both merely pass the opt-out to a
> script already on the decided surface, and **a default-on self-verification block with no
> interactive entrypoint gets DELETED rather than opted out of** — which is why the target is
> ergonomics for a decided surface and not a widening of it.
>
> Also stale in the same commit that removed the dependency: `SKILL.md` still required `cmp` as a
> host tool, which would have sent a maintainer on a `git`-but-no-`cmp` host to opt OUT of the
> supply-chain gate on a host where it runs fine — **the exact false refusal removing `cmp` was
> justified by, re-entering through the doc.** Same paragraph said the comparison is "at `HEAD`"
> when in CI it is at `$GITHUB_SHA`, so the FATAL string it quoted did not match what a reader sees.
>
> **Round 16 — the corpus proved four assertion families and the helper had twelve.** Every plant in
> `both_scopes_reject_execution_altering_env` mutated an `env:` key, `defaults.run`, an `on:` filter
> or `runs-on`. Everything else in `assert_pinned_in_changes_job` was held up by a SENTENCE: the
> `{name, run}` key-set lock — *the "make it unspellable" property the whole helper was rewritten
> around* — the `run` equality, the job-level `if`/`continue-on-error`/`strategy`/`needs` ban, the
> `container`/`services` ban, the `.claude`/`GITHUB_ENV`/`GITHUB_PATH` needle scan (which had only a
> GREEN control, for the mutant that motivated it), the `uses:` restriction, both checkout
> `repository`/`ref` bans, and the JOB-scope `working-directory`. **Delete any one and every red and
> control stayed exactly as it was** — the round-8 regression verbatim, where `env_ok(changes_val,…)`
> vanished in a refactor and was measured green again two rounds later. Fourteen plants added; with
> the key-set assertion removed the suite now reds naming exactly the three mutants that need it.
>
> **The rule this earns, and it is the one that keeps recurring in a new costume: a corpus-size
> floor counts PLANTS, not COVERAGE.** `must_red.len() >= N` cannot see which assertions have one,
> so it grows monotonically while the uncovered set grows with it. The question a mutation corpus
> has to answer is not "how many mutants" but "which assertion does each one pin, and which
> assertion has none" — and the only way to answer it is to delete an assertion and watch.
>
> **Round 17 — a plant can apply, change the file, and still pin nothing.** The entry labelled
> `build-test decoy checkout` inserted a job-level `env: {GIT_DIR: …}`: a third copy of the `env_ok`
> plants beside it, not a `with: {repository|ref}` on a checkout. So the assertions it was named for
> had no plant at all. Correcting the label was not enough — the corrected mutation ALSO pinned
> nothing, because `ci.replacen(anchor, .., 1)` rewrites the FIRST match in the whole file and
> `steps: / - uses: actions/checkout@v5 / - uses: dtolnay/rust-toolchain@stable` occurs in **five**
> jobs. The plant mutated `lint`, satisfied `assert_ne!(mutated, &ci)` — the file *did* change — and
> was vacuous. `runs-on: ubuntu-latest` appears **seven** times and its two plants land in `changes`
> only because `changes` happens to be the first job, which stops being true the moment a job is
> reordered. Closed with an `in_job(job, from, to)` slicer that mutates inside one named job and
> asserts the anchor is present there, so a drifted anchor reds instead of silently mutating
> somewhere else. **The rule: `assert_ne!(mutated, original)` proves a mutation APPLIED, never that
> it applied WHERE THE LABEL SAYS. In a file of near-identical job bodies those are different
> claims, and only the second is coverage.**
>
> **`opens_a_parenthetical` accepted any `(` or `[`**, which made a markdown link's TEXT
> (`[KEY](path)`) and any parenthetical mention (`(KEY was the first attempt)`) into hard
> `make validate` errors — on `CLAUDE.md` among others, with rewording as the only escape. The form
> the arm exists for is `SKILL.md`'s `` (`KEY`, decided … `` — a BACKTICKED key inside a paren — so
> it now requires exactly that, and `[` is dropped entirely: a link's TARGET is a path, which the
> `docs/decisions/<KEY>.yaml` arm reaches with the right semantics, and its TEXT is not a citation.
> Three green controls added; **no control covered either spelling, which is the third time in this
> chain that a class was invisible because the fixtures did not contain it.
>
> **Round 18 — `env_ok` reads a MAPPING and cannot see a runtime export.** `build-test`'s `env:` was
> guarded at job and step scope from round 12, but two lines with no `env:` key anywhere —
> `run: echo "LD_PRELOAD=/tmp/x.so" >> "$GITHUB_ENV"` before `cargo test --workspace` — put **every
> pin in the file** under an attacker-chosen environment with `changes` green and `codegen`
> aggregating green. The `changes` job already banned any non-gate step from *mentioning*
> `GITHUB_ENV`/`GITHUB_PATH`; `build-test`, the job that actually runs the pins, did not. Closed,
> with two plants. **The generalisable shape: a guard that inspects DECLARED configuration is blind
> to the same thing done imperatively, and the two scopes will not notice they disagree.**
>
> **The residual paragraph was a completeness claim again.** It enumerated exactly one remaining
> `build-test` route, which reads as "and no others" — the shape ADR-20260817-105845 exists to stop,
> and the reason this round found a second one. It now says it is *the routes closed, not a claim
> that no others exist*.
>
> **The quotable evidence line could be green over an incomplete run.** `RESULT: n passed, 0 failed`
> printed BEFORE the completeness check, so a run where two dozen cases never executed emitted a
> clean-looking headline and only then `INCOMPLETE: 30 of 54` — and the PR body, the ADR and
> `RETRIEVAL-QMD-CI`'s evidence all cite **the `RESULT:` line** as the measurement. This suite's own
> thesis turned on itself. The headline now carries `<accounted>/<EXPECTED_CASES>`, so no quotable
> line can be green on an incomplete run.
>
> **A comment credited a test that never opens the file it was credited for.**
> `stop-gate.sh` said `assert_gate_script_self_verifies` forbids the opt-out names as CI `env:` keys;
> that test asserts things about the two SHELL SCRIPTS and reads no workflow at all — the ban lives
> in `env_ok` inside `assert_pinned_in_changes_job`. A maintainer following the name would have
> found no `env:` handling and concluded the ban was refactored away. **Round 9's own finding — "a
> comment named a test as the thing preventing the regression; that test did not exist" — recurring
> one file over, in a sentence written after it.**
>
> **Round 19 — round 14's defect, one marker over.** The bullet rule ended a unit on a list marker;
> nothing ended it when a COMMENT marker stopped. So a `#` block and the executable line beneath it
> became one scanning unit and the clause exemption read straight across the prose/code boundary:
>
> ```sh
> # kept for history: the old row is superseded
> echo "Per row OLD-ROW: open a reversal decision"
> ```
>
> No `;`, `—` or sentence dot anywhere in the join, so the whole thing was one clause, it contained
> `superseded`, and the **live citation in the code went green** — where line-scoped scanning had
> redded it. `decision-lookup.sh`'s `activation_fail` has exactly that layout and stayed caught only
> because its comment happens to end in a sentence dot: **a guard that depends on nobody reflowing a
> comment is not a guard.** Closed by treating a change of marker CLASS (comment/quote ↔ none) as a
> block start, planted red, with a same-marker wrapped control so genuine wraps still join.
>
> **Three times now the same defect has come back wearing a different marker** — `-`/`*` (round 14),
> `1.` (round 14), `#` (round 19) — and each time the controls were all *same-marker*, so the class
> was invisible. The lesson is not "add the next marker": it is that **a fixture set drawn from the
> shape you were thinking about proves only that shape.** Where a rule keys on a lexical feature,
> the controls have to vary that feature deliberately, not incidentally.
>
> **Round 20 — the same stale citation, a FOURTH time, in the sentence that retracted the third.**
> The PR body said *"Measured on a GitHub runner, at THIS head: `RESULT: 54 passed, 0 failed` … run
> X on `<sha>`"*, immediately after a sentence explaining that the PREVIOUS version of that line had
> cited a run four commits behind and was therefore retracted. It went stale again at the same
> distance, on the same script, and one of the intervening commits had rewritten the very block the
> figure was evidence for. **The number was still right — the reviewer re-derived it — so this is a
> citation defect, not a wrong measurement, which is exactly why it kept surviving.**
>
> **The rule, and it is structural rather than a resolution to be more careful: a run id plus a sha
> pinned in prose goes stale on EVERY subsequent push, by construction.** Four occurrences on one
> branch is not four lapses of attention; it is a format that cannot hold. The body now cites no run
> and no sha — it names the check and the invariant (`the latest green `changes` job prints
> `RESULT: n/n cases accounted for``), which is true at every head and re-derivable by anyone in one
> click. Where a figure must be pinned to a commit, the pin belongs in something regenerated, never
> in prose a later push invalidates silently.
>
> Also this round, all from the same review: `claude_citation_corpus`'s SCOPE bullet said `.claude/**`
> while the code applied an undocumented `md|sh|json|yaml|yml` allowlist — latent today, and the
> precise shape of the two-statements-of-one-scope divergences this file has already retracted twice;
> `shell_code_only` drops whole-LINE comments only, while the pins consuming it claimed "a copy
> inside a comment does not count" (boundary stated rather than a shell tokenizer written, since
> the executed tamper test already covers the exploit); and neither `make help` nor `workflow.md`
> named `make stub-tests` — the target that exists *because* a default-on block with no interactive
> entrypoint gets deleted rather than opted out of. Both now also say what those targets do NOT
> prove: a local green excludes the gate-set comparison, which is the whole point of the opt-out.
>
> **Round 21 — the rule could HANG `make validate`, and five other holes in the same instrument.**
> Six findings, all in the citation rule and its pin, all planted red before landing.
>
> **The hang is the one that matters.** `line[from..].find("")` is `Some(0)` unconditionally and the
> advance is `from = at + key.len()`, so a superseded row with `key: ""` spins on the first unit of
> the first file forever. Reachable by template copy-paste: `parse_decision_rows` accepts an explicit
> empty string (only a YAML *null* is rejected) and `valid_key` is applied to the FILE STEM, never to
> the field. So the run that should have reported `decision-key-file-mismatch` — an issue already
> sitting in the same list — instead prints nothing and hangs, locally and in the `codegen`/`specs`
> jobs until GitHub's six-hour timeout. **A gate that cannot report is the exact shape this whole
> change argues against, reproduced inside it.** Guarded by a test that runs the rule in a thread
> with a 10s deadline — **an ordinary assertion cannot fail a test that never returns, and a hung
> `cargo test` reads as a slow machine.**
>
> **The exempt window was far wider than anyone had measured.** `—` was a clause boundary; the ASCII
> `--` that the shell and YAML half of the corpus writes throughout was not (the Makefile is
> ASCII-only *by rule*). So in exactly the files this rule targets, a clause ran until a `;` or a
> sentence dot — five joined lines in `activation_fail` — and one `superseded` silenced every
> citation after it. Third time this defect has appeared: adjacent bullet (round 14), comment-block
> join (round 19), and now *inside* the unit. And a bare `#`, the paragraph separator inside every
> comment block, did not end a unit either, so the "a backticked key opens the unit" form could
> never fire for any paragraph after the first — in the two gate scripts, which are long
> `#`-separated blocks.
>
> **Two false reds that would have been hard `make validate` errors with rewording as the only
> escape.** `next_word` trimmed forward over *all* non-alphanumerics, walking past the very sentence
> dot that ended the clause to read a citing noun from the next sentence — so `narrower than the
> <KEY>. Decision rows are cheap…` redded, and a `superseded` in that next sentence could not reach
> back to exempt it. That phrase is named in the docstring as a case that must stay green; it stayed
> green only because of what the next word happened to be. And a **backticked** markdown link still
> redded through the line-start arm, because `[` is in the trim set — re-admitting exactly what round
> 17 removed from `opens_a_parenthetical`. Both green controls used the *unbackticked* `[KEY]`, the
> spelling nobody writes: **every row key in this repo is backticked.**
>
> **And the pin panicked on a strictly WIDER trigger.** `on: [push, pull_request]` and `on: push` are
> both `Some(..)`, so the `Bool(true)` fallback never fired and `.and_then(as_mapping)` panicked with
> *"ci.yml must declare `on:`"* — about a workflow that declares it. Fifth retraction of the same
> instrument on this branch, and the paragraph directly below it makes precisely this argument one
> level down and stops.
>
> **The through-line of rounds 14, 19 and 21, stated once: every green control was written from the
> shape the author had in mind.** Same-marker wraps, unbackticked brackets, `the` without a following
> sentence break, `—` and not `--`. A control set proves the cases it contains, and its silence about
> everything else reads exactly like coverage.
>
> **Round 22 — a scope exclusion falsified by the diff that shipped it, and an arm dead by
> construction.**
>
> The corpus excluded `.github/workflows/**` on the stated ground that workflow row references are
> *"provenance comments on decided work, not instructions to follow"* — while **this same change**
> added to `ci.yml`, directly above the step it governs: *"Authorized by decision row
> RETRIEVAL-QMD-CI … that row authorizes THIS STEP AND ITS PIN AND NOTHING ELSE in CI."* That is a
> normative instruction to the next author, in the `row <KEY>` form the rule recognises everywhere
> else. And supersession on this chain is routine, not hypothetical: `RETRIEVAL-QMD` was superseded
> **two days** after being decided. So a session adding a second CI step would follow a dead row into
> `reconsiders: <superseded row>` and hit `decision-reconsiders-shape` — with `make validate` green
> the whole way, because the file was out of corpus. `SKILL.md` and `decision-lookup.sh` were fixed
> by hand for exactly that shape and put in corpus; `ci.yml` carried it and was not. Workflows are in
> now (green today, so it is a pure widening), and the reach is pinned.
>
> **The `superseded_by` arm could never fire.** The clause exemption ran before `cites`, and
> `superseded_by` *contains* "superseded" — so `last == "superseded_by"` guaranteed the exempting
> substring was in the clause, which guaranteed `continue`. A `.claude/**` file mirroring a row's
> fields stayed green after that successor was itself superseded further down the chain, which is
> the register's next state now that this change builds its first two-link chain. Of the three field
> forms in that arm, **one was dead and two had no control at all** — held up by the comment above
> them rather than by anything that reds when they are removed. Round 9's own lesson, recurring in
> the arm added to satisfy it. Fixed by blanking the citing token before testing for the exempting
> word: an explanation still exempts, **a field name no longer exempts itself**.
>
> **The rule worth keeping: a token that carries the exempting word is not evidence of an
> explanation.** Any escape hatch keyed on a substring will eventually be satisfied by the very
> construct it is meant to catch, and the arm then reads as coverage for as long as nobody plants
> it.**
>
> **Round 23 — my fix for round 21's false red opened a total bypass, and the half-applied sweep
> struck again one line over.**
>
> The sequence-`on:` arm I added last round ended with `return`, and its comment said *"it carries no
> filters, so there is nothing else to check"* — true of the TRIGGER, and this function is not only
> the trigger. Everything below was skipped: both `shell_ok`/`env_ok` scopes, the whole `build-test`
> block, `runs-on`, `container`/`services`, the job-level escape ban, the per-step scan, **and the
> `{name, run}` key-set lock plus `run` equality — the one property the helper was rewritten
> around.** One line of `on: [push, pull_request]` and both gate pins pass vacuously: the steps could
> carry `|| true`, a step-level `if:`, or be deleted outright, unseen.
>
> **What caught it today was an accident in a different test.** The trigger plants anchor on
> `"  push:"`, which disappears under that mutation, so `assert_ne!` fired — with a message pointing
> at the PLANTS. The obvious repair is to re-anchor or drop them, after which both pins are silently
> vacuous forever. **Shape #1 from `docs/claude/sessions/gates.md` §19 — "a corpus-size floor counts plants, not
> coverage" — reproduced in the helper the list was written for, one round after writing it.**
>
> A related trap in the plant itself: replacing just the `on:` LINE leaves the old mapping body
> dangling under a sequence, which is invalid YAML — so the mutant reds on the PARSE rather than on
> the property, and the green control for a legitimate list trigger reds with it. **A plant that
> fails for the wrong reason is worse than none: it reports the guard working while proving
> nothing.** The whole `on:` block is swapped now.
>
> **And the hermeticity verdict was still below the headline** — one round after moving completeness
> above it for exactly that reason. The `.qmd/` fingerprint incremented `fail` after `RESULT:` had
> printed, so a run that dirtied the real repo cache emitted a clean headline and only then
> `repo .qmd/ CHANGED -- VIOLATION`. Exit status right; the line every record nominates as the
> measurement green over a violation of the invariant the file's own header names FIRST. The
> headline now carries it. **A lesson applied to the instance that taught it, and not to its
> siblings, is the half-applied sweep this branch keeps landing — the fourth time.**
>
> **Round 24 — a host-capability precondition that FAILS instead of skipping, in the one job every
> other job `needs:`.** T16's ASCII-locale probe ended `verdict bad` when the locale-dependent
> `open()` did not raise — i.e. when the host has no genuine ASCII locale. That is a host capability,
> and **PEP 686 makes UTF-8 mode the default from Python 3.15**, so `PYTHONUTF8=0` stops restoring
> an ASCII locale by that route and the probe stops raising. A *when*, not an *if* — and the trigger
> is a runner-image bump that touches nothing in the repo. Consequence: `changes` reds, `lint`,
> `specs`, `build-test`, `db-test` and `docs-validate` all skip on `needs: changes`, the required
> `codegen` check reds, and **every PR and every docs-only push to `main` is blocked with no
> validator, build or test signal.**
>
> The suite's own T15g already resolves the analogous filesystem-encoding case with `skipped()`, on
> the stated grounds that *"a hard red on every Mac would train readers to discount reds"*. Same rule,
> one class over. Routed through `skipped()` **with T15g's control discipline**: a pure-ASCII read
> under the same env must succeed first, so a genuinely broken interpreter still fails loudly rather
> than being laundered into a skip — which is the swallow T15g's control exists to prevent. Verified
> both ways: UTF-8-default host → `SKIP`, 54/54 accounted, exit 0; missing control file → `FAIL`.
>
> **And the sibling preconditions were looked at and deliberately KEPT loud** — T3/T3b/T3c and
> T15g's own control are all "the harness could not build its setup", where a skip would swallow
> ENOSPC/EROFS/EACCES. Written down in the script, because the previous four rounds each landed a
> lesson on the instance that taught it and not on its siblings, and *"I checked the others"* is
> worth nothing unless the next reader can see it.
>
> **Round 25 — a step-level `shell:` is not a `defaults.run` key, so `shell_ok` could never see one.**
> Round 12 extended the guards to `build-test` precisely because *"nothing bounded the job the pins
> RUN in"* — and that extension covered `env:` and `defaults.run` and stopped at the step boundary.
> `shell: bash -e -c "exit 0" {0}` on the `cargo test --workspace` step makes GitHub pass the script
> as `$0`, so it never runs, the step exits 0, and `assert_gate_script_self_verifies`,
> `the_gate_self_verification_reds_on_a_tampered_script`, the stub-suite pin and the citation rule
> **all go vacuous in one commit** with `changes` green and `codegen` aggregating green. The twelfth
> mutant, one scope below where it was closed.
>
> **And `specs` — the job that actually runs `make validate`, i.e. the new citation rule — had no
> guard at all**, at any scope. Both closed, three plants, each verified to red by name when its
> guard is removed.
>
> **Fenced code is deliberately NOT exempt, and it is now written down.** The sibling
> `decision-card-row` rule tracks fences and skips them, so the two disagree on purpose. A card's
> fenced block ILLUSTRATES a form; a `.claude/**` doc's fenced block is the thing a session COPIES —
> and the motivating incident was a session doing exactly what a doc showed it. So a fenced
> `reconsiders: <dead row>` is the most dangerous spelling in the corpus, not the safest, and the
> exemption that is right for cards would be backwards here. Recorded because it was previously true
> by accident, and *"the next author should not have to derive which is which."*
>
> **Round 26 — a finding that was WRONG, and the half of it that was still worth acting on.**
> The review held that a `CLAUDE.md`-only push never reaches `decision-superseded-authority`:
> `CLAUDE.md` is on the docs-only allowlist, `specs` carries `if: docs_only != 'true'`, so the rule
> is skipped and `codegen` accepts the skip. **It is not.** `docs-validate` carries the complement
> `if: docs_only == 'true'` and runs the SAME canonical validator command, and the aggregator
> asserts BY NAME that `docs_only=='true'` implies `docs-validate == success`. Both halves were
> already pinned by `the_docs_only_fast_path_never_covers_the_gate_or_workflow_paths`. The reviewer
> looked at `specs` and stopped; an earlier round had it right. **Checked before believing, because
> two reviewers had contradicted each other on this exact point** — and the workflow, not either
> comment, settled it.
>
> **What was real underneath:** if `docs-validate` is the only job running the citation rule on the
> `CLAUDE.md`-only lane — the lane `CLAUDE.md` itself routes straight to `main` with no PR — then it
> is exactly as load-bearing as `specs`, and last round I guarded `build-test` and `specs` and left
> it out. Same half-applied sweep, one job over, in the round that named the half-applied sweep.
> It is in the guard loop now, with a plant.
>
> **The rule: a wrong finding can still name a real gap.** Refuting the claim is not the end of the
> work — the question is what the claim was reaching for, and whether that part holds.
>
> **Round 27 — five findings, and the sharpest is that my own hermeticity fix broke the arithmetic
> it was folded into.** Round 23 hoisted the `.qmd/` fingerprint above the headline so the quotable
> line could carry its verdict — and put `fail=$((fail + 1))` above `accounted=$((pass + fail +
> skip))`. A hermeticity violation is **not a case verdict**, so 54 green cases plus a dirtied cache
> printed `55/54 cases accounted for` followed by `INCOMPLETE: … the harness broke, or
> EXPECTED_CASES is stale` — **false twice over**, double-counted into `fail`, and inviting the one
> repair that would hide the violation permanently *and* break the SKILL.md pin. The invariant
> stated thirty lines below says exactly why: *"a FAILURE IS A VERDICT, so real failures keep the
> count balanced"* — true of `verdict bad`, which consumes a declared case; false of this increment,
> which does not. Now `54/54 … VIOLATION` with `INCOMPLETE` silent, verified by forcing a mismatch.
>
> **A dash is a clause boundary looking BACK, not looking FORWARD**, and the asymmetry is principled
> rather than a patch. `;` and a sentence dot separate independent statements; a dash introduces an
> **appositive about the thing just named**. Round 21 made `--` a boundary because one `superseded`
> written earlier was silencing a live citation five lines later — correct, and it also made the most
> idiomatic explanation in this repo (`` - `KEY` — superseded by the chain head ``) a hard error with
> rewording as the only escape. Backward-only keeps both. **Every green control had separated its
> explanation with `;`, ` -- ` or a dot — three separators that behave identically — so the control
> set was structurally silent about this spelling.**
>
> **Two green controls could not fail**: they cited `NEW-ROW`, which is not a row in the fixture,
> only `OLD-ROW`'s `superseded_by` VALUE — so the scanned key set never contained it. One carried a
> comment claiming it pinned the list-marker drop in `logical_units`; **deleting that drop left it
> green** while a numbered-item citation silently stopped being detected. Rewritten against the row
> that is actually scanned, with the RED plant that dies when the marker-drop goes.
>
> **And the corpus test redded on a HOST condition with a message blaming the author** — the same
> class as round 24's locale probe, one file over. `git ls-files` failing (`dubious ownership`, the
> ordinary shape of a bind-mounted container checkout) yields an empty corpus, and the loop then
> reported *"the citation corpus no longer reaches … Got 0 files"*, sending the reader after a
> pathspec nobody narrowed. Worse, the inversion: in that environment `make validate` goes
> deliberately quiet while `cargo test` screams about the wrong cause. Git usability is probed and
> skipped now; the hard assertions are kept for when git WORKS, because **an empty corpus from a
> broken git and a shrunken corpus from an edited pathspec are different claims, and only the second
> is coverage.**
>
> **`GATE-STEP-LOCUS` was under-priced.** Its cascade enumeration stopped at four jobs and omitted
> `docs-validate` — the sharpest edge, because GitHub skips a job whose dependency FAILED without
> evaluating its `if:`, and `docs-validate` is the only validator on the docs-only push lane. So
> option (b) is not free on the lane this team ships down most often. Added to the row's `evidence:`,
> since the row is the artifact whoever closes it will price from, and **an enumeration that stops at
> four reads as complete.**
>
> **Round 28 — the same list diverged a third time, so it stopped being prose.** Round 22 added
> `.github/workflows` to the citation corpus and updated the function and its SCOPE docstring — and
> not `RETRIEVAL-QMD-CI` clause (d), nor the ADR paragraph. Both are restatements of the one list,
> and both were wrong for a round. That matters more than a wording tidy: **the row is what a reader
> consults to learn what `decision-superseded-authority` was allowed to cover**, so a row stating the
> corpus short sends the next author hunting a red somewhere else — and `ci.yml` now carries
> `Authorized by decision row RETRIEVAL-QMD-CI`, which becomes exactly that red the moment this row
> is superseded, on a chain whose own evidence says supersession here is routine.
>
> **Prose cannot be made to track prose by intention, and this branch has now proved it twice on one
> list.** So `the_records_state_the_same_citation_corpus_as_the_code` reads the pathspecs **out of
> the source** and requires every one to appear in both records. Adding a seventh reds until the
> records say so.
>
> **It paid for itself immediately: the new test caught a FOURTH omission in the same sentence** —
> the ADR had never named `.claudeignore`, from the outset, and neither I nor three reviewers had
> noticed while all of us were looking straight at the divergence. That is the whole argument for
> preferring an executable check over a careful reading, stated better by the check than by this
> paragraph.
>
> **Round 29 — I extended three guards to three jobs and left the two beside them at one.** Round 25
> moved `shell_ok`/`env_ok`/`step_shell_ok` to `build-test`, `specs` and `docs-validate`, with the
> stated reason that `specs` runs the validator and `docs-validate` is the only coverage of
> `CLAUDE.md` on the docs-only lane. The very next block — the `GITHUB_ENV`/`GITHUB_PATH` runtime-
> export scan and the decoy-checkout check, i.e. the two doors `env_ok` structurally *cannot* see —
> stayed `build-test`-only. **Two lists of one scope, in the round that extended the other three.**
>
> **And the consequence is sharper on the jobs I left out.** `claude_citation_corpus` fails OPEN, so
> a shimmed `git` exiting 0 with empty stdout yields an empty corpus and `decision-superseded-authority`
> reports *nothing at all* — `make validate` green, `codegen` green, the rule silently vacuous. One
> list now, three plants.
>
> **Fail-open is fine; fail-open-and-SILENT is what made that a route rather than a limitation.**
> "No stale citations" and "did not look" printed identically. `claude_citation_corpus` now returns
> whether it could read, and the caller emits `decision-citation-corpus-unreadable` — verified by
> running the binary with a `git` that exits 1: 92 warnings instead of 91, with the diagnostic, and
> the baseline untouched on a normal run. **A gate that cannot look must not read as a gate that
> looked and found nothing** — which is this branch's own thesis, applied to the one place it had
> been written as a deliberate silence.
>
> Also: `on.push.branches` read `.and_then(as_sequence)`, so a scalar skipped the containment check
> silently, while the `pull_request` arms fail closed on the same shape. GitHub would reject that
> workflow anyway — the defect is that **the two halves disagree about a mistake one of them
> catches**, which is exactly how the push half came to be six rounds behind the `pull_request` half
> to begin with.
>
> **Round 30 — the mutation helper I wrote to stop mislabelled plants was itself mislabelled.**
> `with_trigger` spliced from `on:` all the way to `jobs:` — and in this `ci.yml` that span also
> contains `permissions: {contents: read}`, so both plants using it silently dropped the workflow's
> permissions block. `assert_ne!` was satisfied and nothing reads `permissions` today, so neither
> plant was proving anything false **yet**. **That is shape #2 of `docs/claude/sessions/gates.md` §19 — a
> mutation that applies somewhere other than where its label says — in the helper written three
> rounds earlier to close exactly that class.**
>
> Latent, not harmless: the moment a `permissions` assertion lands (an over-broad
> `permissions: write-all` on `changes` is the obvious next hardening on this file's trajectory) the
> GREEN CONTROL `on: as a list of events` reds for a reason unrelated to the list form, and the
> one-line repair a reader reaches for is loosening the new assertion — the false-red-with-an-
> obvious-wrong-repair shape this helper has retracted five times. Bounded to the trigger block now,
> with an assertion that the splice does not span `permissions:` so the bound cannot quietly widen
> again.
>
> **The rule this earns: a helper that constructs test inputs needs the same scrutiny as the
> assertions it feeds.** Three rounds of "is this plant pinning what it claims" all pointed at the
> plant LIST; none pointed at the function building the plants. A mislabelled mutation is
> indistinguishable from coverage whether the mislabelling is in the entry or in the machinery.
>
> **Round 31 — the VERIFICATION RECIPE was written from the intended shape, not from the log.** The
> body told a reader to *"read its last three lines"* and promised a `RESULT:` line, then
> `N/N cases accounted for`, then `repo .qmd/ untouched`, plus two `self-verification: OK` lines. What
> the step actually prints on a green run is **one** line carrying all three clauses — because round
> 27 folded them into it — and the `self-verification` lines are the **first** line of each gate
> step, ~54 case lines earlier, with the register-check one in a *different step entirely*. A reader
> following it literally finds one line where three were promised and no self-verification line, and
> cannot tell whether the run is wrong or the instruction is.
>
> **That is the branch's own thesis one level up**: the paragraph exists *because* four run-id
> citations went stale, and its replacement was itself unverified against the thing it describes.
> Checked against a real run this time, and the recipe now names the step, the line position and the
> exact text. **A verification recipe is a derived claim. It goes stale, and it needs the same
> antecedent as any other.**
>
> Also raised and deliberately NOT applied: `changes` carries no `timeout-minutes`, so a hung
> `python3` probe holds the pipeline for GitHub's 360-minute default. Real and cheap — and
> `RETRIEVAL-QMD-CI` authorizes one step and its pin, not other changes to that job, so it went into
> `GATE-STEP-LOCUS`'s evidence for whoever closes the row rather than into `ci.yml` here.
>
> **Operational, and it nearly cost more than the fix: `git stash pop` with nothing stashed popped
> an UNRELATED entry** — held work from another branch, labelled *awaiting founder approval* — and
> conflicted. `git stash` is a no-op on a clean tree but `pop` is not; it takes the top of a stack
> that may not be yours. The entry survived only because the conflict made the pop fail. **Never
> pair a bare `stash`/`stash pop` around a command: check `git stash list` first, or don't stash.**
>
> **Round 32 — a finding I DECLINED, and the part of it that was right.** The review held that the
> supersession-coupling mirror closes one spelling rather than the direction: it sits inside the
> `reconsiders` loop, so `X superseded_by Y` with `Y` open and no challenge edge is green — and
> proposed moving the invariant onto the `superseded_by` edge.
>
> **Declined, with evidence.** That state is legal by a PRE-EXISTING test:
> `supersession_is_a_dag_walked_by_identity` asserts *"a chain terminating in a live row is legal"*,
> and it is what a MIGRATION produces — a row replaced by a successor that never formally challenged
> it, whose own question is still being settled. Redding it breaks that test and
> `a_fully_valid_corpus_is_green`, and CLAUDE.md is explicit: a failing behaviour test means fix the
> generator, never the test. **This is the second time a review has asked for this same widening**
> (round 2 asked for the no-`reconsiders` case), which is itself the signal: the boundary was
> recorded only in the DAG test and in a comment beside the rule, so each reader rediscovers it as
> an oversight.
>
> **What the review was right about is the CLAIM, not the code.** Clause (a) said the arm closes
> *"the direction the ADR wrongly claimed was already total"* — and it closes that direction **in the
> `reconsiders:` spelling of it**. My own record overclaimed, in the clause describing a rule added
> to fix an overclaim. Corrected, and the boundary is now pinned by a green control **beside the
> rule** rather than only in a test two hundred lines away.
>
> **The rule: when a reviewer proposes the same widening twice, the defect is usually that the
> boundary is undiscoverable from where they are reading.** Moving the control next to the rule costs
> nothing and is the difference between a decision and an apparent oversight.
>
> **Round 33 — a FIFTH rider named in no record, which is occurrence three of the drift the row
> exists to track.** The `.gitignore` `__pycache__`/`*.pyc` entry landed in the same commit as the
> four named ride-alongs and appears in no clause, no ADR paragraph and no journal line. The row's
> own CLAUSE HISTORY already records this twice — *"an earlier version said TWO and omitted (c) …
> the next version said THREE while FOUR had landed"* — so this is the third occurrence, **inside
> the paragraph that documents the pattern.**
>
> The entry earns its clause rather than being dropped: the event is **observed on this branch** —
> a `.pyc` with no source in the tree was committed once by an over-broad `git add` (review #8) — so
> it guards the `git add`, not the suite. Nothing in-tree runs a `.py` FILE (both call sites use
> `python3 -c`, which writes no bytecode), and that is now stated rather than implied.
>
> **Three retractions of the same number did not stop it, so the number stopped being prose.**
> `the_ride_along_count_matches_the_clauses_named` derives the count from the clauses the sentence
> enumerates: a sixth rider reds until the row names it. Planted with the exact historical drift —
> understating FIVE as FOUR reds with *"the row says FOUR (4) … but enumerates 5 clauses"*. It
> counts only inside the ride-along sentence, because the CLAUSE HISTORY quotes `(c)` and `(d)` when
> narrating past misses and counting those would make the check drift with the prose it pins.
>
> **The rule: a count that has been retracted twice will be retracted a third time.** The second
> retraction is the signal to derive it, not to write the new number more carefully — this branch
> has now spent three rounds proving that on one sentence, after proving it on the mutant corpus and
> on the citation corpus.
>
> **Round 34 — every guard on this branch bounds a job that CARRIES a pin. None bounded the job
> that REPORTS.** `codegen` is the required status check on `main`, and it was the one job the
> scope guards skipped: `build-test`, `specs` and `docs-validate` were added over rounds 12, 23 and
> 27, and the aggregator was never in the list. A job-scope `defaults.run.shell: bash -c "exit 0"
> {0}` under `codegen:` makes its single aggregation step pass the script as `$0` and never execute
> it — the step exits 0 having asserted nothing, the job succeeds, and **the required check is green
> with every gate job red**, while every mutant and every pin in the file stays green because none
> of them ever looked at that job. `continue-on-error` at job or step scope does the same; so does
> dropping `if: always()`, because GitHub skips a job whose `needs:` dependency failed and branch
> protection accepts `skipped` — the property the whole docs-only design rests on, turned around.
>
> **Three comments in `tests.rs` reasoned about `codegen` in prose** (*"`changes` green and
> `codegen` aggregates green"*) with not one assertion over it. That is the "held up by a sentence"
> shape, in the file that names it. Nine plants now cover the job — and removing the guards leaves
> all nine green, which is how they were measured.
>
> **Round 34b — the docs-only allowlist test reads the `case` arms, and one appended line overrides
> every arm.** `docs_only=true` inserted before the `GITHUB_OUTPUT` echo touches no arm, so
> `the_docs_only_fast_path_never_covers_the_gate_or_workflow_paths` stays green — verified by
> planting it — while classifying every push as docs-only, skipping `lint`, `specs`, `build-test`
> and `db-test`. That is shape #4 (a guard that inspects declared configuration is blind to the same
> thing done imperatively) landing on the guard written to protect the pins from exactly that class.
>
> **No shape test can close it, because the disarm is a legal rewrite of a shell script.** So the
> new `the_docs_only_detector_classifies_by_behaviour_not_by_shape` EXTRACTS the step's `run:` body
> from the parsed workflow and RUNS it against a throwaway git repository, asserting the value
> written to `$GITHUB_OUTPUT` over eleven cases — four fail-open, and three (`.github/**`,
> `.claude/**`, `specs/**`) that are the coupling the older test was written for. The planted
> disarm reds nine of the eleven.
>
> **Its own fixture reproduced shape #3 twice in one sitting**, which is why the cases are isolated
> now: `$GITHUB_OUTPUT` lived inside the worktree, so the next case's `git add -A` committed it and
> `README.md` came back `false` for a path the allowlist covers; and the cases stacked commits, so
> once `crates/x.rs` had been touched every later case reported `false` for a reason unrelated to
> the path under test — four plants passing for the wrong reason. Each case now branches off `base`.
>
> **Round 34c — the seven shapes existed only in the PR body**, while four committed sites cited
> them by number. CLAUDE.md says GitHub is never the record. They are now
> [`docs/claude/sessions/gates.md` §19](../claude/sessions/gates.md), with the two record-side rules
> (a count retracted twice will be retracted a third time; a verification recipe is itself a derived
> claim) beside them, and all four citations re-pointed at the section.
>
> **Round 35 — a warning that says "Not an error" over a code path that exits 1, and a fix whose
> printed remedy poisons the artifact it names.** `decision-citation-corpus-unreadable` was added in
> round 27 at `Level::Warning`, and §17 is exact-match **in both directions**: a kind absent from
> `warning-baseline.json` scores `0 -> 1 (NEW warning kind)` and `make validate` **fails**. So on the
> first tree where `git ls-files` exits non-zero — `fatal: detected dubious ownership` on a
> bind-mounted checkout, a `git archive` extraction, a container stage without `.git` — the gate reds
> with a message naming neither git nor the corpus. **And the remedy that message prints is a trap in
> both directions**: `make warning-baseline` (which CLAUDE.md also prescribes) commits a baseline
> asserting *the citation gate checked nothing*, and that baseline then reds `1 -> 0 (kind
> eliminated)` on every host where git works, where the next reader's remedy is to put it back.
>
> **A signal that depends on the HOST has no stable value to commit**, and the ratchet's whole
> contract is byte-stability. `RATCHET_EXEMPT` in `warning_baseline.rs` carries exactly one kind,
> with the reasoning, and `only_host_dependent_warnings_are_exempt_from_the_ratchet` asserts both the
> list and that the committed artifact does not already carry the kind — because if a previous
> `make warning-baseline` had blessed it, the exemption would turn that entry into a permanent red.
> The posture is fail-open and **loud**: the warning still prints, so "did not look" and "found
> nothing" still read differently. Two prose sites that had gone false with it are corrected in
> place, including `tests.rs`'s *"`make validate` goes deliberately QUIET"* — the stated argument for
> the `SKIP:` branch beside it.
>
> **Round 35b — the site the records cite to prove the corpus is covered was the site still missed.**
> `opens_a_parenthetical` required the backticked key to be ADJACENT to its `(`, which is
> `SKILL.md:326`'s spelling. `SKILL.md:193` — the line the code comment NAMES as the form the
> marker-only-line fix restores — is `` (decided 2026-08-24,\n`RETRIEVAL-QMD-CI`) ``: a wrapped
> continuation, so the join puts the key mid-unit behind a comma that is not in the trim set. It
> redded only because three other lines in the same file do, so an author fixing those passes over
> it. **Coverage by accident**, in the one site named as evidence. The test is containment now, and
> the backtick still carries the whole distinction — a link target inside parens and an unbackticked
> mention both have green controls beside the red.
>
> **Round 35c — an abbreviation dot is inside a sentence and is followed by a space.** So
> ``Per row `OLD-ROW`, i.e. the row superseded by `NEW-ROW`.`` was a hard error: the forward scan
> stopped at the dot in `e.`, and the `superseded` that explains the citation fell outside the
> clause. The docstring calls that sentence legal. Two structural rules — a token containing an
> interior dot (`i.e`, `e.g`, `a.k.a`), and a five-name closed list (`cf`, `vs`, `viz`, `resp`,
> `approx`). **`no.`, `etc.` and `al.` are deliberately excluded**: each is also an ordinary
> sentence-final word here, and admitting them extends clauses past real sentence ends — the
> permissive direction, where a live stale citation gets exempted. A miss costs a reword; a false
> accept costs the gate. Invisible to the control set for the same reason review #25 gives for the
> em-dash: every green control separated its explanation with `;`, ` -- ` or a plain sentence dot.
>
> **Round 35d — five assertions with no plant, one of them the element whose own comment calls it
> load-bearing.** Of the job-key ban `["if", "continue-on-error", "strategy", "needs"]` only two were
> planted; of `["container", "services"]` only one; the non-string `runs-on` arm and the
> must-check-the-repository-out assertion had none. **Deleting `"needs"` was a one-character edit
> that left every mutant and every control behaving identically.** Five plants added, and each was
> measured against its own assertion — remove the assertion, that plant and only that plant survives
> — because a plant that reds for the wrong reason is worse than none.
>
> **Round 35e — two records corrected.** The ADR's Consequences said a host drift *"reds a cheap
> job"*, which `GATE-STEP-LOCUS`, filed in the same commit, contradicts in its own words. And that
> row's `timeout-minutes` deferral cited `RETRIEVAL-QMD-CI` as though it governed the whole workflow
> file, which the row explicitly disclaims (*"says nothing about unrelated CI work"*). The honest
> reason is scope discipline — this PR's verified claim is one non-comment line pair in `ci.yml` —
> and it now says so, next to the note that the new pin permits `timeout-minutes` and that after this
> PR it is the only mitigation left.
>
> **Round 36 — the hermeticity clause said "untouched" on a host where it could not measure.**
> `fingerprint()` is `find -printf | sort | md5sum`; `-printf` is GNU-only and `md5sum` is
> coreutils-only. On a BSD/macOS host both fail, stderr goes to `/dev/null`, and the substitution
> collapses to the **empty string — for `BEFORE` and for `AFTER` alike**, so `[ "$BEFORE" = "$AFTER" ]`
> held unconditionally. A maintainer on macOS who has run the wrapper once (so `.qmd/` exists), edits
> it and runs `make stub-tests`: the suite writes to the real cache and the quotable line says it did
> not. **The `.qmd/`-absent case still caught creation** (`absent` is not the empty string) — it was
> *modification of an existing cache* that went silent, which is the state a developer host is in and
> a CI runner never is, so CI could not see it. Round 22 moved this verdict above `RESULT:` precisely
> so the line every record quotes cannot be green over a violation, and the clause reproduced the
> defect inside it. A host capability is loud, not red: the third clause now reads
> `hermeticity NOT MEASURED (needs GNU find -printf + md5sum)`. Verified both ways by running the
> suite with `md5sum` off `PATH`.
>
> **Round 36b — `PYTHON*` was a prefix ban where only four names inject code.** `LD_` and `GIT_` earn
> their prefixes because the whole family redirects execution or the oracle; `PYTHON*` does not.
> `PYTHONUNBUFFERED`, `PYTHONDONTWRITEBYTECODE` (which this branch's own `.gitignore` rider exists to
> compensate for), `PYTHONHASHSEED`, `PYTHONWARNINGS` are the commonest Python-CI idioms there are,
> and each redded **both** gate pins with a message accusing its author of disarming the ask-gate —
> **and the one-line fix that message invites is deleting the arm, which reopens the `PYTHONPATH`
> route it was written for.** Narrowed to the closed set (`PYTHONPATH`, `PYTHONHOME`,
> `PYTHONUSERBASE`, `PYTHONEXECUTABLE`, plus `PYTHONSTARTUP` which cannot reach `python3 -c` but
> costs nothing to keep); two new reds, three new green controls. Sixth retraction of the same
> instrument — this file's own rule, applied to this file's own guard.
>
> **Round 36c — the durable record stated a bare derived number, twice, with two values.**
> `gates.md` §19 said **thirty-three** while the PR body said **thirty-one**, and both split the same
> set as "the first thirteen" + "the last eighteen" = 31. In the section whose closing bullet is *"a
> count retracted twice will be retracted a third time — derive it."* The total is gone from the
> durable record; the PR's round table is where one can be re-derived from something.
>
> **Round 37 — the row priced what a `changes` red DESTROYS and never what it PREVENTS.**
> `GATE-STEP-LOCUS` enumerated the lost validator, workspace build, wasm check, both test suites and
> `docs-validate`. The consequence a founder prices first is one nobody had written down: `codegen`
> is the **required check on `main`**, it reds on `needs.changes.result == failure`, and the posture
> is auto-merge-on-green — so **nothing in the repository merges at all** until it clears, on any
> branch, for any diff. Compose that with the two facts already in the row — the step's own comment
> names three host-drift classes as *expected* failure modes, and `changes` carried no
> `timeout-minutes` — and a runner-image bump touching none of this repo had a live path to *no
> checkout, dispatch or payments fix merges for six hours*, at Friday/Saturday 19:00–21:30. Option
> (b) does not read as free once that is stated.
>
> **The mitigation is applied here, and the reason it was twice deferred was wrong both times.**
> First it cited an authorization the row disclaims (corrected in round 35). Then it cited scope
> discipline — this PR's reviewed "exactly one non-comment line pair in `ci.yml`" claim. **That is a
> description of a diff, not a value**: the hazard is one *this PR creates* (it is the change that
> puts a python3-heavy suite in the job everything `needs:`), the fix is one line, and leaving a
> known repository-wide six-hour merge block in place is not the conservative choice merely because
> it keeps a diff smaller. `timeout-minutes: 10` on `changes`, orders of magnitude above what that
> job legitimately does, so it bounds a **hang** and nothing else. *(This line said `~30×` and the
> multiplier was measured wrong — see round 48.)*
>
> **The cap is half the pin.** `timeout-minutes: 600` is the 360-minute default with extra steps, so
> `assert_pinned_in_changes_job` requires the key *and* bounds it to `1..=30` — two plants
> (removal, and 360) and two controls (5, 25). `GATE-STEP-LOCUS` stays **open**: the
> in-job-vs-sibling-job question is untouched, and under option (a) the timeout belongs on the
> sibling job instead. Both records say so.
>
> **Round 38 — `continue-on-error` was banned at both ends of the job graph and nowhere in the
> middle.** `changes` bans it in its key list; `codegen` gained a ban in round 34. The four jobs
> between them — `lint`, `specs`, `build-test`, `db-test` — and `docs-validate` had none, and every
> one of them is aggregated by `codegen`. One line on `build-test`:
>
> ```yaml
>       - name: Unit tests ...
>         continue-on-error: true
>         run: cargo test --workspace
> ```
>
> reds every assertion in this file, is swallowed, reports `success` to `needs`, and **the required
> check on `main` is green with the gate red** — with every plant and every pin still passing,
> because none of them looked at that key outside the two ends. **On `docs-validate` it is worse**:
> the aggregator's by-name assertion is `[ "$DOCS_VALIDATE" != "success" ]`, which a swallowed
> failure also satisfies — the docs-only lane's only validator reports success having validated
> nothing, and the check written to catch exactly that passes with it.
>
> **The guard is derived from `codegen`'s own `needs:`**, not from a hand-written job list, so a job
> ADDED to the aggregator is covered the moment it is added. A second hand-kept list here is the
> two-lists-of-one-scope divergence this file has now retracted four times. Six plants, one per
> spelling and scope; removing the guard leaves all six green.
>
> **And the red is actionable rather than a dead end**: a step legitimately allowed to fail is
> written `run: <cmd> || true` in this repo — every `Evidence:` step in `ci.yml` already is — which
> keeps the failure inside the step's exit status instead of hiding it from `needs`. The message
> says so, because a ban whose escape is silence is the instrument this file spends thirty rounds
> retracting.
>
> **Round 39 — the widest guard in the file hung off a locator that could silently find nothing.**
> Both job-scope blocks used `if let Some(job)`, so renaming or deleting a job made every assertion
> under it vanish **green, with no message** — shapes #1 and #3 of `gates.md` §19, in the block whose
> own comment calls it the widest consequence in the file, two screens from a locator that already
> uses `.expect("… has moved or been renamed — re-point this test, do not delete it")` for exactly
> this reason. `specs`, `build-test` and `docs-validate` are pinned indirectly by another test's
> literal `needs:` string; **`codegen` — the required check — is in no such list.** Branch protection
> would eventually say "Expected — waiting for status" on a PR, but not on the docs-only lane, which
> pushes straight to `main` with no required-check evaluation at push time.
>
> **And the measurement corrected the plant labels.** Reverting all three locators to the silent
> form leaves only the `specs` rename surviving: the `codegen` rename is already caught by round
> 38's `expect` on `codegen`'s `needs:` list, because that guard must read the job before it can
> derive anything from it. So the plant labelled *"the codegen job is renamed away"* does **not**
> pin the `.expect` it sits beside — it pins that *some* assertion still sees the rename. That is
> stated in the comment rather than left for the label to imply, which is shape #2 landing on a
> plant added to close shape #1, one round after the same thing happened in round 34's fixture.
>
> **Round 40 — the fix for a MISS became a false red, and its comment claimed the opposite.**
> Round 35 replaced the parenthetical arm's adjacency test with bare containment (`depth > 0`) and
> shipped a sentence saying it *"does not widen the class the adjacent form already accepted"*. **It
> does.** `the pin was rewritten (the `KEY` experiment was contaminated) in round 4` became a hard
> `make validate` error — while the **identical clause without the parentheses stays green**, because
> `last == "the"` and `experiment` is not a citing noun. The docstring names that exact sentence as
> a case that must stay green; punctuation the author did not think they were choosing decided it.
> `decision-superseded-authority` is an ERROR, so it reds `specs` *and* `docs-validate` and the
> required check with them: nothing merges until the sentence is reworded or the word `superseded`
> is injected into it. **A red whose escape is silence, from the arm added to close a miss** — and
> the backtick is no distinguisher, because every row key in `CLAUDE.md`, `SKILL.md` and the register
> is backticked, so containment fires on the house style.
>
> **A parenthetical must CITE, not merely CONTAIN**: a citing word must appear inside it before the
> key. Measured both ways — restore bare containment and the plain mention reds; keep adjacency only
> and `SKILL.md:193` goes missing again. **And the first green control I wrote for it was itself
> wrong**: `` (`KEY` and its successor differ here) `` reds on the *adjacency* arm, pre-existing
> behaviour with nothing to do with containment. Shape #3, caught by the suite in the same minute —
> which is the argument for adding controls rather than reasoning about them.
>
> **Round 40b — the mob roster was sized on the wrong axis, and every checkpoint attribution below
> it inherits the defect.** The ADR sized its briefing from the reversibility class (*reversible*,
> 2–3 lenses); the PR declares **`HOLD: human`** — CI gate machinery, and this change now guards the
> required status check itself. CLAUDE.md's tiebreaker is explicit that **the `HOLD: human` axis wins
> when the two disagree**, so the briefing should have been the full mob. Both checkpoint banks
> attribute their misses to *"invited-lens depth, not roster width"* — **and each was made from
> inside the narrow roster**, which cannot rule out what a lens never briefed would have seen. Since
> only a roster-width attribution returns to the founder, an incorrectly sized roster ruling out
> width from within itself is the loop ADR-20260816-134352 exists to close. Recorded as an open
> correction rather than repaired by a late briefing: banking a roster after the diff exists is not
> the same act as briefing one before any code, and pretending otherwise is the divergence this
> branch retracts five times elsewhere. **The next change to this surface briefs the full mob.**
>
> **Round 40c — `timeout-minutes: 10` bounds a HANG and nothing else.** The likelier event is a RED,
> with the identical blast radius: `changes` fails, five jobs skip, `codegen` reds, nothing merges.
> And that case is **designed in** — T3/T3b/T3c and T15g's ASCII control are deliberately hard
> `verdict bad` rather than skips, because a harness that cannot build its own setup must not be
> laundered into a green. Right for the case; it is also what makes a runner-image bump a
> repository-wide merge block at 19:00 on a Friday. Said out loud in both records so nobody prices
> the timeout as covering both — only `GATE-STEP-LOCUS` option (a) closes the red case.
>
> **Round 41 — a finding DECLINED with evidence, and the overclaim it exposed in our own comment.**
> The review held that pinning the oracle to `GITHUB_SHA` converts the `pull_request` merge-ref race
> into a repository-wide merge block, and proposed falling back to `refs/remotes/pull/N/merge`.
> **The mechanism does not hold**: `actions/checkout` verifies exactly that condition and refuses.
> After fetching it calls `testRef(git, settings.ref, settings.commit)`, retries once with a
> SHA-targeted refspec on a full fetch, and then throws *"The ref '<ref>' does not point to the
> expected commit"*. So either checkout fails and this step never runs, or `GITHUB_SHA` is present —
> **verified by checkout itself** — and the `rev-parse` refusal cannot fire for that reason. Read off
> `actions/checkout`'s `src/git-source-provider.ts`, not inferred.
>
> **The proposed fallback is also the wrong trade**: a local ref name is forgeable by any earlier
> step with `git update-ref`, which is the exact property `GITHUB_SHA` was chosen for after review
> #10 moved `HEAD` by committing. It would trade the oracle away for an availability problem this
> path does not have.
>
> **But the finding was still worth its round, because our own comment claimed the thing it
> claimed.** The block said *"It is not hypothetical … the workspace no longer holds the object
> GITHUB_SHA names"* — written when the pin landed, never checked against `actions/checkout`, and
> repeated verbatim in both gate scripts. **A wrong finding can still name a real defect**, and here
> the defect was ours: an antecedent-free mechanism claim, in the branch whose thesis is that no
> completeness claim ships before it is checked. Corrected in both copies with the source it was
> read from.
>
> **The availability half of the finding is real and is left where it belongs.** When checkout does
> fail, `changes` fails, every sibling job skips and the required check reds — but that is
> `actions/checkout`'s behaviour in *every* job in this workflow, not something the pin introduced,
> and `GATE-STEP-LOCUS` option (a) is what bounds it. Recorded rather than patched.
>
> **Round 42 — round 36's hermeticity fix reached one site out of ten.** `fingerprint()` learned to
> say `NOT MEASURED`; the identical GNU-only construct survived at **nine case sites** (T15b–T15h,
> T15j, T15k), where `find -printf` and `md5sum` failures are swallowed by `2>/dev/null` and **both**
> substitutions collapse to the empty string, so `[ "$fp_b" = "$fp_a" ]` holds unconditionally. And
> there it is not a headline clause but **the verdict itself** — the clause each case name advertises
> (*"… cache untouched"*). Nine cases printed `PASS` having measured nothing. Same file, same commit,
> the half-applied sweep this branch keeps landing.
>
> The cases now `skipped()` on a `can_fingerprint` probe rather than passing, so the completeness
> arithmetic stays balanced and the loss is loud: forced false, the suite prints
> `45 passed, 0 failed, 9 skipped (host capability) -- 54/54 cases accounted for`. **CI is Linux, so
> none of this ever skips there** — which is exactly why the class was invisible.
>
> **Round 42b — `step_shell_ok` was applied to four jobs and not to the one the helper is named
> for.** Review #23 added it for `changes` and it ended up on `build-test`, `specs`, `docs-validate`
> and `codegen` only. **Not exploitable today, and the reason matters**: the gate steps are key-set
> locked so they cannot carry `shell:`, and on `detect` a script-dropping shell means
> `$GITHUB_OUTPUT` is never written, `docs_only` is empty, and `!= 'true'` runs the FULL gate —
> fail-open. But that property lives in `ci.yml`, not in the pin, and `GATE-STEP-LOCUS` option (a) is
> precisely the change that would make a `changes` output consumed fail-CLOSED. Closed, with a plant.
>
> **Round 42c — the superseded row's index arrow deleted the option-space authority from the
> register.** Replacing `-> {decided_by}` with `-> superseded by {head}` routed the reader correctly
> and left **`PROP-20260822-171212` in no row of the index at all** — the design document for this
> whole chain, rewritten in this same PR to name the head. The routing argument requires an **order**,
> not a deletion: a superseded ROW is a dead end (its `reconsiders:` is rejected, citing it is
> gate-rejected), its deciding RECORD is not. Both now, successor first, and the test that pinned the
> drop (`!line.contains("PROP-OLD")`) is re-pointed to assert the order instead.
>
> **Round 43 — the two trigger halves disagreed again, in the half nobody revisited.** Review #27
> taught the `push.branches` arm that GitHub applies `!` patterns as **removals**; the
> `pull_request.branches` arm never learned it, so `['**', '!main']` passed an assertion whose own
> message says *"must still admit PRs targeting `main`"*. It fails **closed** (a fork produces no
> push, so that trigger is a fork PR's only coverage — the PR then stalls on "Expected — waiting for
> status" forever), which makes it narrower than the push-side hole, but an assertion claiming a
> property it does not test is the class this file spends forty rounds removing. One matcher now,
> used by both halves.
>
> **And the matcher only ever asked about `main`**, while the comment three lines above justifies the
> check on the `NN-slug` branches too — `!*` was caught **incidentally**, because `*` also matches
> `main`. `!*-*` and `!2*` leave `main` alone, remove every feature branch, and went green, silently
> dropping the pre-PR validation `ci.yml`'s header states as the reason `on.push` exists at all.
>
> **Round 43b — the tamper fixture inherited the developer's git config.** `the_docs_only_detector…`
> sets `GIT_CONFIG_GLOBAL=/dev/null` and says why; the two fixture repos in
> `the_gate_self_verification_reds_on_a_tampered_script` did not. On a maintainer host
> `commit.gpgsign` panics the test with a GPG error where the reader expects a tamper verdict,
> `core.hooksPath` runs arbitrary local hooks inside the fixture, and `core.autocrlf` CR-strips the
> committed blobs — **reintroducing, in the harness, the exact smudge class review #13 rewrote the
> block under test to remove.** Worse than a red: the guard's CR-strip fallback probably absorbs it,
> so the test would pass for a reason nobody chose. Invisible in CI, which is Linux with a clean
> `HOME`.
>
> **Round 44 — `hides_main` closed its class by enumeration, the instrument this file retracts two
> screens away.** GitHub branch filters accept `?` and `[...]`, not just `*`: `!mai?` and `!m[a]in`
> both remove `main` while a trailing-`*` special case says otherwise. The matcher now **fails
> closed** on either metacharacter — a deliberate narrow exclusion is argued in a comment rather than
> by growing a special case, which is the `LD_*`/`GIT_*` prefix reasoning applied to globs.
>
> **Round 44b — round 37's timeout closed one door of six.** `codegen` is `if: always()`, and
> **`always()` still WAITS for a `needs:` job to finish** — so a job that *hangs* keeps the required
> check queued indefinitely, auto-merge never fires, and nothing merges. That is the identical
> end-state the `changes` cap was added for, and `build-test`/`db-test` — far longer-running, with a
> workspace build and a service container — were still sitting at the 360-minute default, where a
> registry stall or a postgres container that never becomes healthy produces it. Every aggregated job
> now carries a cap, asserted over the list **derived from `codegen`'s own `needs:`** like the
> `continue-on-error` sweep beside it. The heavy jobs get 60 minutes: far above a cold-cache
> workspace build, because a cap that reds honest work is the instrument this file has retracted six
> times. **The hang class is now bounded workflow-wide; the RED class is not**, and only
> `GATE-STEP-LOCUS` option (a) closes that.
>
> **Round 44c — a new open row, `DISPATCH-CARD-CITATION`.** The citation rule excludes `docs/**` on
> the argument that *"a record ABOUT a supersession must name the superseded row"* — true of ADRs and
> proposals, whose job is to narrate history, and **not true of `docs/dispatch/**`**, which is an
> instruction surface a session executes. That is the same property that puts `.claude/**` in the
> corpus. So a card may point a session at a dead row with `make validate` green;
> `validate_dispatch_card_rows` checks that the reference *resolves* and says in as many words that
> status is deliberately not checked. Latent (no card carries a `Decision row:` line today, and the
> ask-gate reads the row file at the point of need), and narrowing a recorded exclusion is a decision
> about the rule's scope rather than a bug fix — so it is a row, not a rider.
>
> **Round 45 — the timeout comment stated a bare multiplier, and it was wrong.** *"~30x the observed
> duration"* — review #45 measured the `changes` job at **14s** on a real run, which makes it ~43x.
> Wrong in the conservative direction, which is exactly why it would have survived: **a citation
> defect reads as correct whenever someone checks the value instead of the antecedent.** The figure
> is gone rather than corrected, in both the workflow comment and the assertion beside it — the
> multiplier was never the argument, "orders of magnitude above seconds of shell" is.
>
> Both of that review's findings — the timeout scoped to one of six aggregated jobs, and
> `hides_main` deciding its class by a trailing `*` — were already closed in round 44. Its verdict:
> *"no blocking correctness defect found"*, `HOLD: human` correctly declared, and the stated evidence
> re-derived independently on the head (both gate steps' `self-verification: OK`, and
> `54/54 cases accounted for`).
>
> **Round 46 — of the three routes the V3 header calls "closed, each because a review demonstrated
> it", one was pinned by nothing.** `unset -f` (a `git` shell function) has a needle; `unset
> "${!GIT_@}"` (the `GIT_DIR` decoy) has a needle *and* a behavioural plant; **the PATH shim had
> neither.** Rewriting `_git="$(PATH="$_vpath" command -v git || true)"` to
> `_git="$(command -v git || true)"` and deleting the `_vpath` line left `cargo test --workspace`
> entirely green. `env_ok`'s `PATH` ban does not reach it — that ban is about `ci.yml`, and the
> header scopes `_vpath` to an **inherited** environment (a composite action, a runner image, a local
> invocation), which is the case `env_ok` structurally cannot see. Shape #1 in the needle list
> written to close it, on the one route of the trio whose defence is a **value** rather than a
> statement.
>
> **And my plant for it reproduced shape #3 twice before it was worth anything.** Version one used a
> `#!/bin/sh\nexit 0` shim: with `_vpath` gone the guard still redded — an oracle that returns
> *nothing* is caught by the empty-oid refusal, not by `_vpath`. Version two made the shim a faithful
> attack (delegate to the real git, but answer `rev-parse <ref>:<path>` with the **worktree** file's
> hash, so committed and live agree over the tamper) and **still** measured green, because the
> assertion was `!out.status.success()` — and `stub-tests.sh` in that fixture exits non-zero anyway,
> having no wrapper for its 54 cases. The assertion had never been about the shim.
>
> Version three asserts the **verdict** (`self-verification: OK` absent, `differs from the committed
> blob` present), which is what the sibling cases beside it already read. Measured: delete `_vpath`
> from both scripts and drop the two new needles, and the guard prints `self-verification: OK` over a
> visibly tampered file while the test reds by name. **The rule: a plant against a defence-in-depth
> value has to be checked against the state where only that value is missing** — anything else is
> measuring the guard next door.
>
> **Round 47 — the completeness invariant is an equality, so it ratchets DOWN silently.**
> `pass + fail + skip == EXPECTED_CASES` catches a case that stops *running* (the round-17 defect)
> and not one that is *removed*: delete a case, decrement the literal, and every gate in the repo is
> green — **and the SKILL.md pin follows the decrement rather than resisting it**, because it derives
> its number from that same literal. So the suite's own rule three lines from the invariant
> (*"Never delete a case and never weaken an assertion to recover green"*) was the one thing in that
> block that stayed **prose**, in a branch whose whole argument is that prose can be ignored and a
> gate cannot. Shape #1 one direction over: an equality is a floor *and* a ceiling, and only the
> ceiling was load-bearing.
>
> `MINIMUM_CASES` lives in the **Rust test, not next to `EXPECTED_CASES` in the shell** — the point
> is that lowering it costs a second edit in another language and another file, with the reason
> attached. Planted with the exact two-token erosion: `EXPECTED_CASES=53` plus the matching SKILL.md
> edit now reds by name where it used to be green.
>
> **Also recorded, deliberately not taken**: `.claude/hooks/stop-gate.sh` is the only thing that runs
> the ask-gate selftest on every *interactive* turn and is **not** in the four-file `GATE_SET`, so
> emptying it disarms that gate locally with every pin green. CI catches the disarmed state on push
> (the `changes` job invokes the selftest directly, not through stop-gate), and `stop-gate.sh`
> predates this branch — widening the set is a change to the set's boundary, not a fix to anything
> here. The omission is now written down at the `GATE_SET` declaration itself, because **a set whose
> omissions are undocumented is how the next omission gets argued from silence.**
>
> **Round 48 — the retraction of a bare number was itself half-applied, and it landed on the one
> site that is not the record.** Round 45 removed `~30×` from the `ci.yml` comment and said why. The
> identical figure stayed live in the **ADR**, in **`GATE-STEP-LOCUS`** and in the **journal** — and
> CLAUDE.md is explicit that GitHub is never the record and that decisions live in the ADR and the
> row. So the fix reached a source comment, which carries no authority, and missed the three that
> do. `GATE-STEP-LOCUS` is **open**, and its sizing paragraph — where the number is doing the
> arguing between options (a) and (b) — is exactly what whoever closes it reads.
>
> Measured again independently: the `changes` job runs **14s**, so 10 minutes is nearer **43×**. The
> figure is gone from all three rather than corrected, because the argument was never the multiplier.
> **This is the half-applied-sweep class this branch catalogues at rounds 33, 36 and 42, landing on
> the retraction of a half-applied claim.**
>
> **Round 48b — the PR body's `ci.yml` scope statement was three rounds behind its own diff.** It
> said *"Two things … everything else added to that file is comment"*; the diff adds
> `timeout-minutes` to **seven** jobs plus a standing assertion that every job the aggregator
> consumes declares one in `1..=60` — a repo-wide constraint on all future jobs. The six extra caps
> *are* recorded in `GATE-STEP-LOCUS` by value with the `always()`-still-waits argument, so this was
> a body↔diff gap rather than an unrecorded rider — but on a branch whose thesis is that every
> completeness claim was written before it was checked, the first artifact an independent reviewer
> reads should not be narrower than the diff. Corrected in the body and in the row's sentence that
> asserted the body was accurate.
>
> **Round 48c — two smaller things named rather than left implied.** The `1..=60` ceiling is one
> `build-test`/`db-test` sit *at*, so the assertion's message now names the intended move (raise the
> bound **and** the job's value in the same commit, with the observed duration that justifies it)
> instead of leaving the reader to invent the one-character edit this file retracts elsewhere. And
> the clean-run assertion in the tamper test reads the self-verification **verdict**, not the exit
> status — that fixture has no `.claude/settings.json` and no wrapper, so both guards exit non-zero
> for unrelated reasons. **A green assertion that proves less than it reads as is how the next one
> gets written**, so it now says so.
>
> **Round 49 — the one finding in twelve rounds that changes behaviour, and it was latent.**
> `exempt_text` computed byte offsets from `line` and applied them to `line[..].to_lowercase()`.
> **Unicode lowercasing is not length-preserving** (`ẞ` U+1E9E is 3 bytes → `ß` at 2; `İ` U+0130 is 2
> → `i̇` at 3), so one such character before the citing token drifts every later offset.
> `is_char_boundary` stops the panic and nothing else: the blanking lands beside the token, or is
> skipped. **The skip is the direction that bites** — `superseded_by` is left intact, exempts itself,
> and the arm that whole block exists to resurrect is dead by construction again. That is review
> #21's defect reopened, and this change creates the register's **first two-link chain**, so "the
> successor is superseded later" is the next state, not a corner case.
>
> Fixed by ordering: blank on the original slice, lowercase after — **unrepresentable rather than
> guarded**. And the control that proves it took six spellings to find: with ONE `İ` the drifted
> offset still lands on a char boundary and the blanking merely misses beside the token, which reds
> for the wrong reason; **two** put it inside the combining mark, `is_char_boundary` fails, the
> blanking is skipped, and the citation goes green. `ẞ` drifts the other way and never reaches the
> skip — the obvious fixture is not the discriminating one. Found by trying candidates against both
> implementations, not by reasoning about one.
>
> **Round 49b — `can_fingerprint` was stated twice, in the commit that introduced it.** `fingerprint()`
> carried its own copy of the same host-capability test. One decides **nine case verdicts**, the other
> decides the **headline hermeticity clause every record quotes** — so a disagreement is round 42's
> finding re-created in the other direction: cases printing `cache untouched` under a headline saying
> `NOT MEASURED`, or the reverse. One statement now.
>
> **Round 49c — my round-45 edit left a truncated sentence.** Deleting the multiplier took the clause
> head with it: *"…gone rather than corrected. --" / "say that out loud, because…"*. In the rationale
> block for the one line the body defends, on a branch whose thesis is that a comment is a claim a
> reader can check.
>
> **On the owed full-mob briefing, stated plainly rather than left implied.** The review argues the
> diff is still open, so the briefing that CLAUDE.md's tiebreaker owed could be held now, and that
> merging with the correction open converts a recoverable process miss into precedent. That is
> right. **This session cannot hold it** — it is operating under an explicit constraint against
> convening the lens agents — so the correction stays open and this line is the record of why, not a
> disagreement with the argument. It is a decision-queue item for the founder or for whoever runs the
> next session on this surface, and the lenses whose absence is load-bearing were named by the review:
> **farley** (this change can stop every merge in the repository), **beck** (nine of ten
> `can_fingerprint` sites had no control, and round 49's finding had none either), **dba**
> (`db-test`'s 60-minute cap against a service container).
