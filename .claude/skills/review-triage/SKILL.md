---
name: review-triage
description: >
  Triage independent-review findings on a pull request and decide, per finding, whether it is fixed
  in this PR or filed as an issue — so a review round ends in a shipped PR rather than another
  round. Use whenever review feedback arrives on a PR you are driving (a review comment, a review
  summary, a batch of PR notifications), before fixing anything, and when deciding whether a PR is
  ready to merge. Also use when a PR has already taken several rounds and you are wondering whether
  to keep going. Carries the round ceiling and the escalation. Not for writing reviews — that is the
  `reviewer` agent; this is for RECEIVING them.
---

# Review triage — a round ends in a decision, not in another round

**The failure this exists to stop.** A review always finds something; that is what it is for. If
every finding becomes a fix and every fix becomes a push and every push becomes a review, the loop
has no termination condition and will run until someone outside it intervenes. On PR #679 that was a
night and 114 commits. The last four passes each concluded *no blocking defect in the shipped
behaviour*, while the rounds themselves introduced three regressions the author then had to fix —
a half-fix, three silently deleted tests, and an unclearable-red bug in the fix for the previous
round's finding. **Past some point the loop stopped catching defects and started manufacturing
them.** Nothing in the reviews was wrong. The process had no stopping rule.

Founder directive 2026-08-26, recorded in
[ADR-20260826-084500](../../../docs/adr/ADR-20260826-084500-one-review-pass-per-presentation-and-findings-are-triaged-not-chased.md).

## 1. Triage every finding before fixing any of them

Read the whole batch first. Fixing as you read is how a batch becomes a round.

Put each finding in exactly one bucket. **When you cannot tell, look at the failure it describes and
ask whether it can happen on `main` today** — that question decides more cases than the labels do.

**BLOCKING — fix in this PR:**
- Wrong behaviour on a path this PR touches, reachable on the current tree.
- A gate this PR adds that does not gate, or gates the wrong thing.
- Anything in the `HOLD: human` class: money movement, stored event shapes, migrations, erasure,
  legal surfaces, the merge machinery itself.
- A claim in a record or comment that this PR's own diff falsifies. Cheap to fix, and a false
  record outlives the PR.

**NON-BLOCKING — one issue, linked from the PR, not another round:**
- **Latent**: the code path exists but nothing on this tree can reach it. Check it, do not assume
  it — "no corpus-derived kind is in the committed baseline" is a fact you can read in thirty
  seconds, and it converts a scary finding into a follow-up.
- Gate quality: the gate works, and its *own* tests or diagnostics could be sharper.
- Record wording that is imprecise but not false.
- Hardening beyond what the PR set out to do.

**NOT A FINDING — reply, do not change:**
- A decision the founder owns. Say so and leave it open.
- A claim you verified and found wrong. Answer with the evidence; reviewers are sometimes wrong and
  a reviewer's antecedent is not established fact.
- An echo of your own earlier comment.

## 2. Ship when no blocking finding remains

Not "when the reviewer is satisfied" — a reviewer is never finished, by construction. File the
non-blocking findings, name them in the PR, merge.

**A review pass fires on presentation, not on push** — and since ADR-20260828 (founder directive:
the CI auto-review is retired, the team reviews its own work) the pass is the TEAM's independent
reviewer-agent read of the full branch diff, run in-session before the PR is marked
ready-for-review (the standing CLAUDE.md rule). One pass per presentation, not one per push; a
genuinely fresh look after a substantial rewrite is one deliberate re-presentation. `@claude` in a
PR comment remains available for an on-demand look the founder asks for.

## 3. The ceiling: three rounds, then it goes to the founder

If a PR reaches a **third** review round, stop. Do not open a fourth. Bring the founder: what
shipped, what is still open, and your recommendation. Three is a ceiling and not a target — most
work should ship on one.

This number is a judgement, not a measurement. What is measured is the branch that earned it: the
useful blocking findings arrived early, and the late rounds returned gate-quality findings and
author-introduced regressions.

## 4. Two things that make a round cost more than it should

- **Never verify a fix by re-reading it.** Plant the defect, watch the test go red, restore, watch
  it go green. A fix "verified" by reading is verified against the tree in your head.
- **Never quote a test total as evidence.** It moves for several reasons at once. If a commit claims
  an assertion, check that assertion is in the commit, by name. This is how three tests were
  deleted and reported green.

## 5. Record what the round taught, once

Durable shapes go to `docs/claude/sessions/gates.md` §19, in the same change. Not the PR body —
CLAUDE.md says GitHub is never the record, and a PR body that grows a ledger is a sign the loop is
running.
