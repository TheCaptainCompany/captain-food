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
> prove: a local green excludes the gate-set comparison, which is the whole point of the opt-out.**
