# ADR-20260724-135945 — Proposals are committed to the repo (docs/proposals/)

- **Status**: Accepted (product-owner directive, 2026-07-24)
- **Date**: 2026-07-24
- **Amends**: the operating model (docs/PLAYBOOK.md planning-vs-execution split; CLAUDE.md
  non-negotiables)

## Context

Plan-mode proposals — the approach put in front of the product owner for approval, including the
alternatives that were weighed and rejected — lived only in the assistant's session-local plan file,
which is overwritten per task and lost with the session. ADRs capture the *decision*; nothing durable
captured the *proposal*: months later there is no way to reconstruct why an approach was chosen over
its alternatives, or what scope the approver actually saw when saying yes.

## Decision

Every proposal presented for approval is **committed to `docs/proposals/`** as
`PROP-YYYYMMDD-HHMMSS-<slug>.md` (convention in [docs/proposals/README.md](../proposals/README.md)):
the proposal text as presented, alternatives considered, the approver's scope choices, and a status
header linking to the realizing PR/ADR. Approved proposals are historical records — never rewritten
to match what was later built; divergences are recorded in the realizing change (the
honest-residuals rule).

Placement in the flow: a spec-touching proposal lands in the same change as the approved spec edit;
a code-only proposal lands with the claim or the PR. `docs/proposals/**` counts as docs for the
spec/docs-straight-to-main rule.

## Consequences

- The "why" survives: decision archaeology needs only the repo (proposal → ADR → PR → STATUS).
- Slight ceremony per planned task (one committed file); accepted — the cost of losing rationale is
  what prompted the directive.
- Backfill: the one surviving session plan (frontend split 4/4, #87) is committed as the inaugural
  `PROP-20260723-150500-frontend-split-4.md`; earlier proposals are unrecoverable — this ADR marks
  the cutoff.
