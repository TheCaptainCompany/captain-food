# ADR-20260831-183847 — The ready flip is the coordinator's step, and always was

## Status

Accepted

- **Date**: 2026-08-31
- **Amends**: [ADR-20260815-115220](ADR-20260815-115220-auto-merge-on-green-by-default-hold-human-for-the-named-class.md)
  — only where it rewrote `.claude/agents/executor.md` step 7 into the executor's voice. Its
  *decision* (auto-merge-on-green by default, `HOLD: human` for the named class) is untouched.
- **Restores**: [ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
  §2, which already assigned this step and was never superseded.
- **Relates**: [ADR-20260815-134655](ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md)
  (no PR waits on founder review — unchanged) ·
  [ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)
  (argued in *Alternatives* below)

## Enforced by

n/a — no behavioral guarantee. This ADR allocates a step in the team's operating protocol between
two agent roles; it changes no product behaviour and no `rules.yaml` entry. The executable half of
the change it ships with is `tools/db-preflight.sh` (issue #830 D1), which is a different subject.

## Context — two ADRs already disagreed, and the environment cast the deciding vote

`.claude/agents/executor.md` step 7 has instructed the **executor** to:

> **Mark the PR ready for review and enable auto-merge together, as one indivisible step** — this
> is the default (ADR-20260815-115220) — then **supervise the checks until MERGED**.

**No executor session in this environment can perform either half of that step.** Verified
in-session on 2026-08-31, and each is independently sufficient:

1. Both operations are **GraphQL-only mutations** — `markPullRequestReadyForReview` and
   `enablePullRequestAutoMerge`. The session's GraphQL endpoint answers **HTTP 403** with a message
   that is explicit about the pin:

   > `This GraphQL query is not enabled for this session — only the pinned set of PR-review
   > operations is served. Use REST via 'gh api repos/{owner}/{repo}/...' instead.`

2. **`gh` is not installed** (`which gh` → exit 1) — so the remedy the 403 itself proposes does not
   resolve either. `.claude/settings.json` nonetheless carried 15 `Bash(gh …)` and 13
   `PowerShell(gh …)` permission entries, reconciled in the same commit as this ADR.

3. **REST cannot substitute.** There is no REST endpoint for auto-merge at all, and `draft` is not
   an updatable field on `PATCH /repos/{owner}/{repo}/pulls/{number}` — GitHub documents converting
   a draft to ready as a GraphQL-only operation. A `PATCH` carrying `"draft": false` is accepted and
   the field ignored, which is the worst shape available: a success response for an operation that
   did not happen.

**Measured cost**: ~8 minutes per executor run and an unfinishable closing step, reported
independently by two runs on 2026-08-30/31; a third called it the strongest item on its list.

### The fact was already recorded. That is the actual defect.

[`docs/claude/sessions/workflow.md`](../claude/sessions/workflow.md) has carried a section titled
*"An executor session CANNOT mark a PR ready for review or arm auto-merge — plan the handoff"* since
**2026-08-17** (#623). It records the 403, the 404 on `POST …/ready_for_review`, the silently-ignored
`PATCH {"draft": false}`, the absent `gh` — and it already draws this ADR's conclusion, verbatim:

> **What this means for the protocol.** ADR-20260815-115220's "mark ready and arm auto-merge
> together, as one indivisible step" is a COORDINATOR action, not an executor one.

So this was not an undiscovered fact. Three executor runs rediscovered it anyway, two weeks later,
and paid full price each time. **The reason is the one that matters here: the finding was written
into a topic file, and the instruction the executor is actually bound by — `.claude/agents/executor.md`
step 7 — still said the opposite.** A charter is loaded on every run; a topic file is loaded when
something suggests it should be, and nothing in step 7 suggested it, because step 7 read as a normal
executable instruction right up to the moment it failed.

This is CLAUDE.md's own rule about itself — *prefer executable over prose, and sharpen an existing
rule rather than appending a near-duplicate*. The correct response to a note that failed to bind
three times is not a fourth note. It is to change the binding text and record the reversal, so that
the next drift of step 7 has to argue with a decision instead of quietly restating one.

### The step was never the executor's to begin with

The capability finding is corroboration, not the argument. The argument is that two recorded ADRs
already disagreed, and the older, broader one is right.

[ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
§2, verbatim:

> **The session lead is a COORDINATOR, never an author.** The coordinator writes dispatch briefs,
> runs the mob loop, reads agent output critically, relays state, and handles GitHub mechanics
> (claims, PR bodies, comments, **ready + auto-merge**, supervision to MERGED).

That is the same step, named explicitly, assigned to the coordinator, and never superseded.
ADR-20260815-115220 lists ADR-20260810-011500 under `Relates` — but only for its authoring clause
("executor writes every phase"), and rewrote executor.md step 7 without noticing the GitHub-mechanics
clause it was contradicting.

The distinction that dissolves the conflict: **ADR-20260815-115220 settles *when* the ready +
auto-merge step is taken versus withheld. It never settled *who* takes it** — its own Context frames
the question as "auto-merge or a human merges", not as an allocation between roles. It changed the
addressee as a side effect of rewriting the charter in the executor's voice. So this is not an
amendment to its decision; it is a correction of who that decision speaks to.

## Decision

**The ready flip and the auto-merge arming are the coordinator's step.** The executor drives the
work to green and hands back; the coordinator marks the PR ready for review and, in the default
posture, enables auto-merge **together, as one indivisible step**, then supervises to MERGED.

Three things follow, and one thing explicitly does not:

1. `.claude/agents/executor.md` step 7 is rewritten in the coordinator's voice: the executor's
   closing obligation is to reach green, report, and **leave the PR in draft**. An executor that
   finds itself unable to flip a PR ready is now behaving correctly rather than failing a step.
2. `CLAUDE.md`'s issue-workflow bullet keeps its wording and gains the addressee, because the
   ambiguity of its unowned imperative ("mark the PR ready…") is what allowed the drift.
3. The **draft PR remains the interlock** (ADR-20260721-042018). Nothing about claim-time draft-PR
   creation changes, and an executor still never arms auto-merge.
4. **`HOLD: human` is untouched.** It still withholds auto-merge for the named class, and it still
   never means a founder wait (ADR-20260815-134655).

### This is a simplification of the handover, and NOT a loss of the auto-merge property

The card that dispatched this work asked which it is. It is a simplification, and the mechanism
shows why.

What converges is the **handover point**: under both postures the executor now stops at the same
place — green gates, PR in draft, report filed. That is one protocol ending instead of two, and the
executor no longer has to classify its own work to know how its run terminates.

What does **not** converge is the **merge condition**, which is where ADR-20260815-115220 actually
put the difference between the two postures:

| | default posture | `HOLD: human` |
|---|---|---|
| who flips ready | coordinator | coordinator |
| auto-merge armed | **yes**, with the flip | **no** |
| what merges the PR | the machine, on green | the coordinator, after the team's reviewer PASS |

Auto-merge-on-green survives intact: it is still armed at the ready flip, and the PR still merges
with no further session action once CI is green. The property ADR-20260815-115220 bought — that a
green PR does not wait on a human to notice it — is a property of *whether auto-merge is armed*, not
of *which agent arms it*. Moving the arming to the role that both owns GitHub mechanics and can
actually perform it costs that property nothing.

The honest residue: the coordinator session must be alive at hand-back to arm it. It already had to
be, to merge a `HOLD: human` PR and to dispatch the next chunk.

## Alternatives considered

- **(a) Record the split — chosen.** The executor drives to green and hands back; the coordinator
  owns ready + merge. Costs nothing to implement, is true today, and — decisively — is what
  ADR-20260810-011500 §2 already says. Choosing anything else would require superseding a clause
  nobody has argued against.

- **(b) Make the executor capable — install `gh`, or unpin the GraphQL mutations. Rejected, and its
  cheaper half does not work.** Installing `gh` would **not** restore the capability: `gh pr ready`
  is a thin wrapper over `markPullRequestReadyForReview` and `gh pr merge --auto` over
  `enablePullRequestAutoMerge`, so both would hit the same 403. The pin is on the **GraphQL
  endpoint at the agent proxy**, not on the CLI, and `gh` would fail one layer later with a worse
  message. Only unpinning the mutations would work, and that is an Anthropic-side session setting —
  not the team's to change, and not a change we would want unargued: the pin is a blast-radius
  control on a session that writes to a production repository. Against
  [ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md) ("final
  vision first"): (b) is not a final step being deferred for an intermediate one. Under
  ADR-20260810-011500 the coordinator-owned flip **is** the final step; (b) would be a *change of
  design*, and one that buys nothing the table above does not already give.

- **(c) Leave the prose and let each executor rediscover the gap. Rejected — and this is not
  hypothetical, it is the measured status quo.** `workflow.md` has carried the finding and the
  correct conclusion since 2026-08-17, and three independent runs still paid full price on
  2026-08-30/31. Option (c) has already been run as an experiment for two weeks; it does not work,
  because the topic file is not what binds the executor.

- **(d) Fix it as another prose note rather than an ADR. Rejected, for the same evidence.** A note is
  exactly what already exists and already failed. Beyond that: the correction reverses the operative
  addressee of a clause a previous ADR wrote, and per CLAUDE.md a change that contradicts a recorded
  decision is a decision event whatever the diff size. A corrected sentence would also be the
  *second* silent rewrite of this clause, which is how the drift happened the first time.

### On compiler-first (ADR-20260803-234035)

The floor asks whether the type system can make the mistake unspellable before prose is written.
Here it cannot, and neither can a gate — the subject is which of two agent roles performs an action
against a third-party API, and the "type system" that would have to hold it is the agent charter
itself. **Prose is the right instrument for this one**, and the dispatch card explicitly allowed
that conclusion.

What is *not* prose is the corroborating fact: the 403 is an executable, self-demonstrating
constraint. An executor that tries the step gets a specific, accurate error naming the pin. The
environment already enforces this decision; the record now agrees with it.

## Consequences

### Positive
- The documented protocol becomes executable end to end. No run ends on a step it cannot perform.
- The two postures share one executor ending, so an executor no longer classifies its own work to
  know how to stop.
- Two recorded decisions stop contradicting each other, resolved toward the one that was never argued
  against.
- `.claude/settings.json` stops claiming permissions for a binary that does not exist.

### Negative
- The coordinator session must be alive at hand-back to arm auto-merge. It already had to be.
- Merge latency now includes the hand-back hop. Bounded by the coordinator's response, not by CI.
- If `gh` is ever installed or the pin lifted, this ADR's capability paragraph becomes historical
  while its decision stands — the decision rests on ADR-20260810-011500 §2, not on the 403.

### Follow-up actions

**An earlier draft of this section said "None required … nothing else references the executor-owned
flip." That was false, and the review proved it in one `git grep`** — the correction had reached 2
binding sites out of 6. It is recorded rather than quietly deleted, because it is the same failure
this ADR is about: a record asserting a sweep is complete is exactly what stops the next reader
running the grep. CLAUDE.md already requires that grep after anything is renamed or reshaped; the
command is

```sh
git grep -n "ready + auto-merge\|enable auto-merge\|mark the PR ready"
```

**Swept in the landing commit** — each now names the coordinator:

| site | what it said |
|---|---|
| `.claude/agents/executor.md` step 7 | instructed the executor to perform the flip |
| `CLAUDE.md` issue-workflow bullet | unowned imperative ("mark the PR ready…") |
| `docs/STATUS.md` claim-protocol block | "on completion mark ready + enable auto-merge" — **loads every run**, second only to CLAUDE.md |
| `docs/BACKLOG.md` §4 | "Completion = ready + auto-merge + supervision"; CLAUDE.md calls this method **binding**. Now states both endings, by role |
| `docs/claude/sessions/evidence.md` (2) | "arm the PR on green gates", and "'PR armed and reported' is done" — which defined the executor's DONE as the impossible operation |
| `docs/claude/sessions/workflow.md` §"no REST equivalent" | a **second** section in the same file, ~210 lines below the first, still framing this as an environment limitation ("as things stand") and still assigning "supervise CI to green over REST" to the executor |

**Known remaining site, deliberately not swept here**:
`.github/workflows/dev-loop.yml:85` embeds an executor prompt ending *"then mark the PR ready for
review and STOP"* (line 91 already says "do NOT enable auto-merge"). It is a genuine binding site —
arguably the strongest, since it drives an unattended loop — but CI was out of #830's scope. It
needs one sentence, and it needs it before the next unattended run.

**Not a follow-up**: the `gh` permission entries in `.claude/settings.json` were **kept**, on
purpose — a permission is a conditional, not a claim the binary exists, and the `PowerShell(gh …)`
half is evidence of a second host where `gh` is the normal way in. The fact is recorded in a
`_comment_gh` key beside them instead.
