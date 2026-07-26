# Proposal template

Copy into `docs/proposals/PROP-YYYYMMDD-HHMMSS-<slug>.md`. The proposal is the **lasting artifact** —
the issue is a tracking point that disappears when the work is done. Write it so that someone reading
it in a year understands what was on the table and why one option won.

`PROP-20260726-013207` (reclamation) is the reference example; the 2026-07-26 review proposals
(`PROP-20260726-16*`, `-17*`) follow the same shape.

```markdown
# PROP-YYYYMMDD-HHMMSS — <title>

- **Status**: Proposed | Approved | Rejected | Superseded by PROP-…
- **Date**: YYYY-MM-DD
- **Tracking issue**: [#NN "<title>"](https://github.com/TheCaptainCompany/captain-food/issues/NN)
- **Realized by**: _(filled at completion)_

---

## 1. Context

What is true today, with evidence. Prefer a table of verified facts with `file:line` over prose —
a reader must be able to re-check every claim. Say plainly what the consequence is; do not soften it.

## 2. Recommended approach

The recommendation, in sequence, with the reason each step comes where it does. If ordering matters
(it usually does), say why.

## 3. Decisions surfaced

One subsection per decision. **Every decision gets a pros/cons table** with the recommendation marked:

### D1 — <the question>

| Option | Pros | Cons |
|---|---|---|
| **<recommended>** ✅ **recommended** | … | … |
| <alternative> | … | … |
| <status quo> | … | … |

Never present a bare "A vs B" without trade-offs. Include the status quo as an option when it is a
defensible choice — sometimes it is, and saying so is more useful than pretending otherwise.

## 4. Screen mockups

**One per use case.** ASCII wireframes are enough. Show what each actor sees and which command or
query the controls map to. Include the failure/empty states — they are where the design usually breaks.

## 5. Sequence diagrams

**One per load-bearing flow**, in mermaid, faithful to the hexagonal architecture: the aggregate or
process manager *decides* (pure), state is saved through the `Repository`, events are appended by
`PgEventStore`, and inbound facts arrive from adapters. Show the acceptance-first and
request-vs-report splits where they apply. See `docs/claude/mermaid.md`.

## 6. Alternatives considered for the cluster as a whole

Not the per-decision options above — the shape of the whole change. Include the "do nothing" and the
"do it all at once" options and why they lose. If an option quietly changes the product's scope or
market, say so explicitly.

## 7. Verification plan

Per issue: the `rules.yaml` rule, the behaviour tests **including the negatives**, and the
observability signal. State which tests must fail on `main` today — that is what proves the finding
was real.

## 8. Open questions for the product owner

Numbered, each mapping to a Dn above, each with the recommendation restated in one line so the
approver can answer without re-reading.

## 9. Refs

`file:line` evidence, ADRs, and full clickable issue links.
```

## Rules that are easy to get wrong

- **Issue references** in repo markdown must be full clickable links —
  `[#NN "<title>"](https://github.com/TheCaptainCompany/captain-food/issues/NN)`. GitHub does not
  auto-link bare `#NN` outside issues/PRs/commits.
- **Create the tracking issue first** (ADR-20260724-143000) and name it in the header. An issue-less
  proposal is invisible to the prioritised backlog.
- **Commit to `main`** — proposals and docs go straight to `main`, no branch, no PR.
- **Once approved, a proposal is a historical record.** Do not rewrite it to match what was built;
  divergences go in the realizing PR/ADR/STATUS (the honest-residuals rule).
- **ASCII only in Makefile recipes** if the change touches one (CLAUDE.md) — not a proposal concern,
  but the same class of trap.
