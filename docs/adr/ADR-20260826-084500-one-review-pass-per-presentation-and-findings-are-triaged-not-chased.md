# ADR-20260826-084500 — One review pass per presentation, and findings are triaged rather than chased

**Status**: Accepted · **Date**: 2026-08-26 ·
**Decider**: the **FOUNDER / Tech CEO**, verbatim below ·
**Prompted by**: [#679 "RETRIEVAL-QMD-CI decided: the decision-lookup stub suite runs in CI"](https://github.com/TheCaptainCompany/captain-food/pull/679),
merged `089a13b3` after 114 commits and a night of review rounds ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted.

## The directive, verbatim

> *"You have worked on the night on the same pr and create a lot of issues. I'm worried that we
> cannot finish the work we are in an infinite loop. Is it a good thing that you stop working and
> tell me what to do ?"*

and, on the remedy:

> *"put in place this rule and if we need to put it in a skill or agent don't hesitate. Do what's
> best for us."*

The answer to the first question was **yes**, and the session had not reached it on its own. That
is the fact this record exists to make sure the next session does not have to rediscover.

## Consulted

Per CLAUDE.md, records created from a founder directive carry a `Consulted:` block. **This one does
not, and the omission is deliberate and stated rather than silent**: the founder asked a direct
question about work already in flight and a mob consult would have added another round to a session
whose defect was rounds. The lenses that would have spoken are named instead, with what they would
have carried, so a later reader can weigh the shortcut:

- **holub** — the shortest path to working software; the night's marginal round bought none of it.
- **beck** — a test never seen red is an unverified claim; three tests were deleted and the suite
  reported green.
- **farley** — the pipeline should prove releasability, not consume it.

If any of them would have changed the ruling, that is a reversal and needs its own row.

## Context — what actually happened

PR #679's authorized deliverable was **one CI step and one test pinning it**. It merged at 114
commits and +10,933 lines across 27 files.

`.github/workflows/claude-code-review.yml` fired on `pull_request: [opened, **synchronize**,
ready_for_review, reopened]`. **`synchronize` fires on every push.** So: a review lands, it finds
something (it is built to; a good reviewer is never finished), the author pushes the fix, the push
fires the next review. **The cycle contains no terminating condition anywhere.** It is not a defect
in the reviewer and not laziness in the author — it is a loop that runs until something outside it
intervenes, and on this branch the thing outside it was the founder, at breakfast.

**The measured harm, which is the part that decides this:** the last four review passes each
concluded *no blocking defect in the shipped behaviour* — the findings were latent, gate-quality or
record wording. Over the same stretch the rounds introduced three regressions the author then had to
fix: a half-fix that made an input visible without making the metric sensitive to it; a range splice
that silently deleted three tests while the suite reported green; and an unclearable-red bug
introduced *by the fix for the previous round's finding*. **Past some point the loop stopped
catching defects and started manufacturing them.**

## Decision

**1. A review pass fires when work is PRESENTED, not when it is pushed.** `synchronize` is removed
from the review workflow's trigger. `opened`, `ready_for_review` and `reopened` remain — the moments
an author says *this is ready to be looked at*.

A fresh look after a substantial rewrite stays available and costs one deliberate act: convert the
PR to draft and back to ready, which fires `ready_for_review`. **Deliberate is the point.** It is a
decision to re-present, not a side effect of typing `git push`.

**2. Findings are TRIAGED, not chased.** Every finding lands in exactly one bucket — blocking (fix
here), non-blocking (one issue, linked, named), or not-a-finding (reply, change nothing). **A PR
ships when no BLOCKING finding remains**, never "when the reviewer is satisfied", because by
construction a reviewer is never satisfied. Buckets and worked examples:
[`.claude/skills/review-triage/SKILL.md`](../../.claude/skills/review-triage/SKILL.md).

**3. Three rounds is a ceiling.** At a third round, stop and bring the founder what shipped, what is
open, and a recommendation. Do not open a fourth. Three is a judgement, not a measurement — what is
measured is the branch above, where the useful findings came early and the late rounds returned
gate-quality findings and author-introduced regressions.

## Enforced by

**Mechanically, which is the level CLAUDE.md's compiler-first rule asks for before a gate is
written.** The loop is not *forbidden*; it is made unspellable. With `synchronize` gone there is no
path from a push to a review, so the cycle cannot close no matter how disciplined or undisciplined
the author is. The skill and this ADR govern what a session does with the findings it *does* get —
which is judgement, and cannot be mechanised.

## What this does NOT change

- **The independent-review requirement stands** (founder directive 2026-08-01): a PR is marked ready
  only after a reviewer-agent pass by eyes that did not write it, with the multi-lens fan-out for
  payments, migrations and erasure. This changes the review's CADENCE, never its existence.
- **`HOLD: human` is untouched.** A blocking finding in that class is blocking however late it
  arrives.
- **No gate is weakened.** Nothing here lowers a bar; it stops re-asking a question already answered.

## Consequences

- **Accepted cost, stated rather than glossed**: a finding that would have surfaced on round four
  now surfaces as an issue against `main` instead of a fix before merge. That is the trade — an
  issue on `main` is visible, rankable and cheap; an unbounded PR is none of those. For the class
  where it would be dangerous, `HOLD: human` already stops the PR.
- The reviewer's cost per PR stops scaling with the author's commit count.
- The next session inherits a stopping rule instead of the founder's attention.
