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
> no PATH/BASH_ENV/ENV/SHELLOPTS/LD_PRELOAD/LD_LIBRARY_PATH/BASH_FUNC_*). Planted: M19 job
> `BASH_ENV` · M20/M21 shell-drops-script at job and workflow scope · M22 workflow `PATH` · M23 job
> `LD_PRELOAD` → RED; `CARGO_TERM_COLOR`, `RUST_LOG`, `shell: bash` → GREEN.
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
> That kills the ENTIRE overwrite class — including the two spellings the residual had called
> unclosable — instead of chasing each one. Enforced in CI only, so a developer editing the wrapper
> locally is not redded. **Cost that earned it: six rounds of mutants that were all one shape, and
> a residual statement that was wrong about its own scope three rounds running.**
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
> Anyone reading #677 as "option 2 solves requiredness" is reading it wrong.
