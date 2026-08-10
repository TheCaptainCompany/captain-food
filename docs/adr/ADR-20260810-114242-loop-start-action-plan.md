# ADR-20260810-114242 — Loop start: always show the product owner the action plan

**Status**: Accepted (product-owner directive, 2026-08-10, in-session)
**Extends**: [ADR-20260810-011500 "Team ownership: sessions start autonomously, and the coordinator never authors the diff"](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)

## The directive, verbatim

*"Always show me the action plan when you start the loop."*

## Decision

Every time a session STARTS a work loop — claims a chunk and dispatches the mob — the
coordinator presents the product owner a compact **ACTION PLAN**, before or with the first
dispatch. The plan names:

1. **The chunk** — issue/PR and lane.
2. **The phases** — and what each does.
3. **The checkpoints** — and who holds stop authority at each.
4. **The gates** — what must be green before ready + auto-merge.
5. **The out-of-scope fences** — explicitly, so deferrals are visible up front.
6. **Anticipated product-owner decision points** — if any are foreseen, named now.

**Format**: a table or tight list in chat. Repo records are unchanged — the PR body still
carries the mob evidence per
[ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md).

## Why

The product owner supervises by vision, not by reading transcripts. This composes with the
decision-queue-only escalation of ADR-20260810-011500: the plan is **TRANSPARENCY, not a
permission request** — the loop still starts without asking.

## Enforcement

CLAUDE.md carries this as a sharpening of the existing team-ownership non-negotiable rule
(same change as this ADR). It is a prose gate: the observable signature of compliance is an
action plan in the session transcript at every loop start, preceding or accompanying the
first mob dispatch.
