# ADR-20260720-233000 — Issue claim protocol + stale-claim reaper (multi-session coordination)

## Status

Accepted (product-owner directive, 2026-07-20; issue #39)

## Context

Concurrent sessions proved they can duplicate work the same day this was written (ADR-20260720-213024
was authored twice; caught only in review). Value order (ADR-20260720-213024) says WHAT is next but
not WHO holds it.

## Decision

1. **Claim before work**: add `status/in-progress` + a claim comment naming the `NN-slug` branch.
   The LABEL is the atomic claim (API-visible to sessions and Actions with the default token —
   unlike the Project Status column); the comment covers the pre-PR window; `Closes #NN` then makes
   GitHub's Development sidebar the durable branch/PR pointer.
2. **Never work a claimed issue.** Pick the next unclaimed value rank.
3. **Claims expire**: the hourly `stale-claim-reaper` workflow releases any claim >24h old with no
   issue comments and no PR/commit references since the claim (ignoring its own marker comments).
4. Method documented in `BACKLOG.md` (repo = method, GitHub = state).

## Alternatives considered

- Project Status column as the claim — rejected: org-project writes need PAT scopes the default
  Actions token lacks; labels work everywhere.
- State file in the repo — rejected: stale + merge conflicts between the very sessions it should
  coordinate.

## Consequences

- Positive: no duplicate implementations; zombie claims self-release in ≤25h; anyone can see who
  holds what (label + claim comment + sidebar).
- Negative: 24h is generous for agent pace — an abandoned claim blocks a rank for up to a day;
  tighten later if it bites.

### Follow-up actions
- Every session adopts the protocol immediately (#39 itself was claimed at creation).
