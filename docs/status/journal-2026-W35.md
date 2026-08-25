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
> the pin no longer depends on that anchor. The precedent test was moved onto the same helper; it
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
