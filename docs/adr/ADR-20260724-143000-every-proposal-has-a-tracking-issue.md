# ADR-20260724-143000 — Every proposal has a tracking issue

- **Status**: Accepted (product-owner directive, 2026-07-24)
- **Date**: 2026-07-24
- **Amends**: ADR-20260724-135945 (proposals are committed to the repo)

## Context

Proposals now live durably in `docs/proposals/` — but a proposal without an issue is invisible to
the prioritised backlog: nothing carries its Value Size / Effort / Impact / Priority, nothing puts
it on the board, and nothing marks it claimed or done. The first proposal under the new convention
(PROP-20260724-133700) happened to have an issue (#96) only because it descended from one — and its
issue text had silently drifted three revisions behind the proposal's scope.

## Decision

**No proposal without a tracking issue.** Create the issue before or with the proposal if none
exists; name it in the proposal's header (`Tracking issue: #NN "<title>"`); keep the two in step —
a re-scoped proposal retitles/rescopes its issue in the same change. The issue is the backlog
handle (fields, board, claim flow); the proposal is the rationale record; the ADR is the decision.

## Consequences

- Proposals cannot be lost: the board shows them, the claim flow works them, the stale-claim reaper
  sees them.
- One extra step at proposal time; convention recorded in `docs/proposals/README.md` and CLAUDE.md.
- Applied retroactively to PROP-20260724-133700: #96 retitled/rescoped to revision 4.
