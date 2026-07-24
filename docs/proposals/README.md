# Proposals — the pre-decision record

**Every proposal presented for approval is committed HERE before (or with) its execution**
(product-owner directive, 2026-07-24; ADR-20260724-135945). ADRs record what was *decided*;
proposals record what was *put on the table* — the option space, the trade-offs weighed, and the
rationale as it stood at approval time. Losing them means losing the ability to remember *why* we
chose what we chose.

## Convention

- One file per proposal: `PROP-YYYYMMDD-HHMMSS-<slug>.md` (same collision-safe date-time ids as ADRs).
- **Every proposal has a tracking issue** (ADR-20260724-143000): create one BEFORE or WITH the
  proposal if none exists, name it in the header, and keep the two in step (a re-scoped proposal
  retitles/rescopes its issue). An issue-less proposal is invisible to the prioritised backlog and
  gets lost — the issue is what carries Value/Effort/Impact/Priority and a place on the board.
- Header block:

  ```markdown
  # PROP-YYYYMMDD-HHMMSS — <title>
  - **Status**: Proposed | Approved | Rejected | Superseded by PROP-…
  - **Date**: YYYY-MM-DD
  - **Tracking issue**: #NN "<title>" (REQUIRED — create it if missing)
  - **Realized by**: PR #NN / ADR-… (filled at completion)
  ```

- Body = the proposal as presented for approval: context, the recommended approach, **alternatives
  considered and why they lost**, scope decisions the approver made (e.g. AskUserQuestion answers),
  and the verification plan.
- The file is a HISTORICAL RECORD once approved — do not rewrite it to match what was eventually
  built; divergences are noted in the realizing PR/ADR/STATUS instead (the honest-residuals rule).
- Plan-mode flow: the plan written for approval IS the proposal — commit it here verbatim when
  approved (spec-touching plans land in the same change as the spec edit; code-only plans land with
  the claim or the PR).

Related: [docs/adr/](../adr/) (decisions) · [docs/STATUS.md](../STATUS.md) (state) ·
[docs/BACKLOG.md](../BACKLOG.md) (prioritisation method).
