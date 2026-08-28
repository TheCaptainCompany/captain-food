# ADR-20260828-023258 — Merge-on-green stands; the review pass lands after, and follow-ups absorb its findings

**Status**: Accepted · **Date**: 2026-08-28 ·
**Decider**: the **FOUNDER / Tech CEO**, verbatim below ·
**Closes**: [`docs/decisions/REVIEW-GATES-CRATES-MERGE.yaml`](../decisions/REVIEW-GATES-CRATES-MERGE.yaml) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted.

## The directive, verbatim

Answers returned through the call-sheet artifact form (round 3), 2026-08-28:

> 1. Reviews vs auto-merge: **Keep as is (merge on green, review lands after) - recommended**
> 2. Next up: **Stale-claim reaper bug (#642) - recommended**

Answer 2 is a backlog pick, recorded in the journal; answer 1 is the decision this ADR carries.

## The decision

The merge posture of ADR-20260815-115220 (ready-for-review + auto-merge as one indivisible step,
auto-merge on green by default) **stands unchanged for PRs touching `crates/**`**, knowing — and
accepting — that on a small PR the required checks go green (~5 min) before the review pass posts
(~10 min), so the review lands on an already-merged PR and its findings are executed as follow-up
PRs under the standing "if the fix is small always do it now" (ADR-20260827-081500).

What was measured and put to the founder (the JWKS chain, night of 2026-08-27/28:
[#684](https://github.com/TheCaptainCompany/captain-food/pull/684) →
[#692](https://github.com/TheCaptainCompany/captain-food/pull/692) →
[#693](https://github.com/TheCaptainCompany/captain-food/pull/693) →
[#694](https://github.com/TheCaptainCompany/captain-food/pull/694)): every PR in the chain merged
before its review arrived; every finding was real; every merged state was correct — the rounds
hardened guarantees, they never fixed a shipped defect. The cost is churn (four PRs where one or
two might have carried the chain, and the ADR-20260826-084500 round ceiling firing on volume
rather than disagreement); the benefit is that `main` advances immediately on green and a stuck
reviewer can never block shipping — the failure mode the founder removed deliberately with REV-1.
The founder chose the benefit.

Rejected in the same act: making the review pass a required check for `crates/**` PRs (option b —
reintroduces "reviewer down = merges blocked"), and the procedural delay of arming auto-merge only
after the first review posts (option c). Nothing here weakens the review itself: the pass still
fires on every presentation, is always triaged (blocking · non-blocking · not-a-finding), and its
findings still land — as follow-up PRs when the merge outruns them.

## Consulted

Per CLAUDE.md, records created from a founder directive carry a `Consulted:` block. The mob was
**not** re-convened, deliberately and stated: the option space was assembled in
`REVIEW-GATES-CRATES-MERGE`'s evidence field from the measured JWKS chain, and the founder chose
among recorded options through the call-sheet form. The lenses that speak through the record:
`farley` (perpetual releasability — a gate that can silently block all merges is a pipeline
defect, the argument that carried REV-1 and carries this); `beck` (the reviews' value survives:
every finding was verified by plant before its fix landed, so post-merge review loses no rigor,
only ordering); `holub` (four PRs for one subject is named waste — accepted here as the price of
an unblockable pipeline, which is why the churn is stated in the decision rather than hidden).
`architect`, `young`, `vernon`, `evans`, `dba`, `graphql-architect`, `ux-designer`,
`business-specialist`, `legal-specialist`, `observability-agent`: nothing in lens — the question
is merge mechanics, not domain, storage, API, UX, money or law.
