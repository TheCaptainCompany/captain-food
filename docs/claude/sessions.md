# Claude rules — working a session (environment limits, gate choice, context economy)

Hard-won operational knowledge. Everything here cost real time in a real session; none of it is
derivable from the code. Read it before a long or exploratory session — especially one that talks to
a third-party dashboard, reads binary files, or runs the gate more than once.

Related: [codegen.md](codegen.md) (what each gate does) · [loops.md](loops.md) (budgeted autonomous
runs) · [../PLAYBOOK.md](../PLAYBOOK.md).

The rules live in [`sessions/`](sessions/); this page is the index.

## [`sessions/gates.md`](sessions/gates.md)

- [1. Pick the cheapest gate that proves the change](sessions/gates.md#1-pick-the-cheapest-gate-that-proves-the-change)
- [8. Generated code can enforce something the spec does not say](sessions/gates.md#8-generated-code-can-enforce-something-the-spec-does-not-say)
- [8b. A guard over Rust STRUCTURE must parse the AST, not the text](sessions/gates.md#8b-a-guard-over-rust-structure-must-parse-the-ast-not-the-text)
- [11. Installing a dev tool: crates.io works, GitHub release downloads do not](sessions/gates.md#11-installing-a-dev-tool-cratesio-works-github-release-downloads-do-not)
- [12. A workspace GLOB member cannot bootstrap itself](sessions/gates.md#12-a-workspace-glob-member-cannot-bootstrap-itself)
- [13. Build the narrow graph, not just the workspace](sessions/gates.md#13-build-the-narrow-graph-not-just-the-workspace)
- [`make rust` does not compile the application](sessions/gates.md#make-rust-does-not-compile-the-application)
- [18. A CI-workflow change: does it fit the job's timeout, and does it regress the rollback path?](sessions/gates.md#18-a-ci-workflow-change-does-it-fit-the-jobs-timeout-and-does-it-regress-the-rollback-path)

## [`sessions/environment.md`](sessions/environment.md)

- [2. Disk is a fixed per-session allowance, and `df` lies about it](sessions/environment.md#2-disk-is-a-fixed-per-session-allowance-and-df-lies-about-it)
- [3. Keep MCP output small — it is the biggest context cost available](sessions/environment.md#3-keep-mcp-output-small--it-is-the-biggest-context-cost-available)
- [4. This container cannot read PDFs](sessions/environment.md#4-this-container-cannot-read-pdfs)
- [17. The container can restart mid-dispatch — put the handoff in the PR, not in the session](sessions/environment.md#17-the-container-can-restart-mid-dispatch--put-the-handoff-in-the-pr-not-in-the-session)
- [A mob aggregation exceeds the Bash output cap — read it in slices, never `cat`](sessions/environment.md#a-mob-aggregation-exceeds-the-bash-output-cap--read-it-in-slices-never-cat)
- [The disk cost of a parallel mob review, and what to reclaim first](sessions/environment.md#the-disk-cost-of-a-parallel-mob-review-and-what-to-reclaim-first)

## [`sessions/evidence.md`](sessions/evidence.md)

- [5. Establish a third-party integration's shape BEFORE naming anything](sessions/evidence.md#5-establish-a-third-party-integrations-shape-before-naming-anything)
- [6. Verify a config key's real consumer before declaring it](sessions/evidence.md#6-verify-a-config-keys-real-consumer-before-declaring-it)
- [7. A green deploy job does not mean the new code is running](sessions/evidence.md#7-a-green-deploy-job-does-not-mean-the-new-code-is-running)
- ["Verbatim" is a mechanical check, not a careful read](sessions/evidence.md#verbatim-is-a-mechanical-check-not-a-careful-read)
- [A card that says "carry lens X's return" must point AT the lens's own return](sessions/evidence.md#a-card-that-says-carry-lens-xs-return-must-point-at-the-lenss-own-return)
- [14. A green review job does not mean a review happened](sessions/evidence.md#14-a-green-review-job-does-not-mean-a-review-happened)
- [15. Read what a gate EXCLUDES before treating it as evidence](sessions/evidence.md#15-read-what-a-gate-excludes-before-treating-it-as-evidence)
- [Grepping for a type name does not find where that type is INJECTED](sessions/evidence.md#grepping-for-a-type-name-does-not-find-where-that-type-is-injected)
- [A handoff's "remaining work" list is a claim, not an inventory](sessions/evidence.md#a-handoffs-remaining-work-list-is-a-claim-not-an-inventory)
- [A "seen red" claim must name HOW the test was made to fail](sessions/evidence.md#a-seen-red-claim-must-name-how-the-test-was-made-to-fail)
- [Running a mutation by hand: `git checkout <file>` reverts to HEAD, not to your work](sessions/evidence.md#running-a-mutation-by-hand-git-checkout-file-reverts-to-head-not-to-your-work)

## [`sessions/workflow.md`](sessions/workflow.md)

- [10. Commit the durable artifact, not the conversation](sessions/workflow.md#10-commit-the-durable-artifact-not-the-conversation)
- [16. A lens invited late still pays — and the ones you skip are the ones that disagree](sessions/workflow.md#16-a-lens-invited-late-still-pays--and-the-ones-you-skip-are-the-ones-that-disagree)
- [A red CI job that never ran your code: `429` from `codeload.github.com`, and why re-pushing makes it worse](sessions/workflow.md#a-red-ci-job-that-never-ran-your-code-429-from-codeloadgithubcom-and-why-re-pushing-makes-it-worse)
- [An executor session CANNOT mark a PR ready for review or arm auto-merge — plan the handoff](sessions/workflow.md#an-executor-session-cannot-mark-a-pr-ready-for-review-or-arm-auto-merge--plan-the-handoff)
- [The worktree is SHARED — "already on `main`" has a shelf life of one tool call](sessions/workflow.md#the-worktree-is-shared--already-on-main-has-a-shelf-life-of-one-tool-call)
- [Rescue an agent killed mid-edit with a `wip:` commit that says what was NOT verified](sessions/workflow.md#rescue-an-agent-killed-mid-edit-with-a-wip-commit-that-says-what-was-not-verified)
- [The stop hook cannot see in-flight work — its prompt is not a signal that anything is finished](sessions/workflow.md#the-stop-hook-cannot-see-in-flight-work--its-prompt-is-not-a-signal-that-anything-is-finished)
- [A denied tool call is a DECISION — never re-issue it through a different tool](sessions/workflow.md#a-denied-tool-call-is-a-decision--never-re-issue-it-through-a-different-tool)
- [The claim-time draft PR needs an empty commit first — the REST API refuses a zero-commit branch](sessions/workflow.md#the-claim-time-draft-pr-needs-an-empty-commit-first--the-rest-api-refuses-a-zero-commit-branch)
- [A commit touching `CLAUDE.md` or `.claude/agents/*.md` needs in-conversation user approval](sessions/workflow.md#a-commit-touching-claudemd-or-claudeagentsmd-needs-in-conversation-user-approval)
- [One more shell trap in commit messages](sessions/workflow.md#one-more-shell-trap-in-commit-messages)
- [Asking the founder a decision — use the form template](sessions/workflow.md#asking-the-founder-a-decision--use-the-form-template)
- [A record that pins a fact to "in flight" expires, and nothing detects it — date the claim instead](sessions/workflow.md#a-record-that-pins-a-fact-to-in-flight-expires-and-nothing-detects-it--date-the-claim-instead)

## 9. This file is your obligation, not just your reference

**Every session records what it learned** (ADR-20260730-034635), in the same change as the work. That
is why this file exists, and it is how it stays worth reading.

- **Where it goes**: operational findings → the `docs/claude/sessions/` topic file that owns them —
  environment limits and tool behaviour → [`environment.md`](sessions/environment.md); gate costs and
  what a gate proves → [`gates.md`](sessions/gates.md); verification and "seen red" →
  [`evidence.md`](sessions/evidence.md); workflow traps → [`workflow.md`](sessions/workflow.md) — or
  the relevant other `docs/claude/` topic file. Decisions → an ADR. Option spaces → a proposal +
  tracking issue. State → durable state in [`STATUS.md`](../STATUS.md); dated status history at the
  top of the applicable [`docs/status/journal-YYYY-Www.md`](../status/).
- **Prefer executable over prose.** If the lesson can be a validator rule, a behaviour test or a hook,
  write *that* — prose can be ignored, a gate cannot. `makefile_recipe_lines_are_ascii` turned a
  one-off Makefile breakage into a codegen test so it could not silently return; that is the bar.
- **Bar for an entry**: not derivable from the code, and it would cost the next session time. State
  the concrete cost that earned it — "one `search_issues` call returned six complete epics" is a rule;
  "be careful with MCP output" is noise.
- **Sharpen, don't duplicate.** Extend the existing rule rather than adding a near-identical one; two
  overlapping rules mean neither is trusted.
- **Writing nothing is a valid outcome.** A session that learned nothing transferable adds nothing.
  If this file ever reads like a diary it has failed, and the fix is deletion, not more headings.
