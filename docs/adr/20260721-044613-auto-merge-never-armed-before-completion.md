# ADR-20260721-044613 — Auto-merge is never armed before completion (amends ADR-20260721-042018)

## Status

Accepted (product-owner directive, 2026-07-21)

## Context

ADR-20260721-042018 introduced the claim-time draft PR and listed "enable auto-merge" as one of
the completion-phase actions, but did not explicitly forbid doing it earlier. That is a real gap:
a claim-time draft PR is, by construction, close to a no-op diff against `main`, so its CI run
passes trivially. Draft status itself blocks merging regardless of auto-merge or check state — but
if a session armed auto-merge **at claim time** (misreading "immediately" as covering that step
too, or trying to "set and forget"), the PR would carry an armed auto-merge for the entire
duration of the work. A PR is later taken out of draft by explicit "ready for review" action; if
that ever happens before the work is actually complete — a misclick, a premature status flip, a
second person acting on the PR — an already-armed auto-merge merges the moment the current head's
checks are green, which they trivially can be on a small or stale diff. Because the PR body carries
`Closes #NN`, that merge would close the issue on unfinished work.

The fix is not structural (GitHub already prevents merging a draft); it is a sequencing rule that
was implied but never stated as a hard constraint.

## Decision

**Auto-merge is armed exactly once, in the same action as marking the PR ready for review, and
never before.** Concretely:

- At claim time (ADR-20260721-042018 step 1): open the draft PR. Do **not** call
  enable-auto-merge here, under any circumstance.
- At completion (ADR-20260721-042018 step 3): after local gates are green and the work is judged
  actually done, mark the PR ready for review **and** enable auto-merge together, as one
  indivisible completion step — never enable auto-merge on a PR that is still draft, and never
  mark a PR ready without immediately arming (or manually completing) its merge.
- If a PR must be taken out of draft for an unrelated reason (e.g. a reviewer wants to see it) before
  the work is complete, auto-merge must NOT be enabled at that time.

This closes the only path by which a claim-time draft PR could merge before the work behind it
exists.

## Consequences

- Positive: removes the one sequencing ambiguity in ADR-20260721-042018; "ready" and "auto-merge
  armed" become synonymous actions, so there is no window where an unfinished PR carries a live
  auto-merge.
- Negative: none — this only tightens wording, no new mechanism.

### Follow-up actions

- `docs/BACKLOG.md` claim protocol and `CLAUDE.md`'s issue-workflow rule are updated to state this
  explicitly.
