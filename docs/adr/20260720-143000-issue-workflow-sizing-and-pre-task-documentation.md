# ADR-20260720-143000 — Issue workflow: sized, pre-task-documented issues; PRs as the post-task record

## Status

Accepted — §4's simplest-first ordering (and §1's rank stamp) amended by
[ADR-20260720-213024](20260720-213024-value-first-issue-prioritisation.md): the queue is now
ordered by **value** (foundations/cross-functional first, then features by value stream).
Sizing, pre-task documentation and the PR-as-post-task-record rules stand unchanged.
Further amendment (2026-07-20, product-owner directive): §1's **`size/*` label scheme is
retired** — labels were not sortable and displayed effort where value should be. The shirt-size
**estimate** now lives only in the issue body's Estimation section, mirrored coarsely by the org
**Effort** field; issue labels now carry **`impact/*`** (the issue's value T-shirt, mirroring the
org **Value Size** field). See [docs/BACKLOG.md](../BACKLOG.md).

## Context

Remaining-work tracking moved from `docs/STATUS.md` tables to GitHub issues (#12–#28). Two questions
followed:

1. **Prioritisation.** The product owner wants to attack issues "simplest first, up to the most
   consuming/impacting", without re-deriving that ordering every session, and without Scrum
   ceremonies (no sprints, no story points, no velocity theatre). Development is AI-native: the unit
   of work is a focused agent session gated by `make validate` + tests, not a human-day, so classic
   estimation units mislead.
2. **Issue ↔ PR duplication.** Linking commits to issues looked redundant with PR descriptions:
   every commit already lands in a PR that explains itself. The resolution: an issue and a PR are
   two different documents in time. The **issue is the pre-task documentation** — what we agree to
   do and why, *before* work starts (the contract). The **PR is the post-task record** — what was
   actually done and why, *after* the fact (the evidence). Overlap between them is accepted and
   even desirable: divergence between the two is signal (scope drift), not waste.

## Decision

### 1. Every issue is sized with a shirt-size label, once

- Label scheme: `size/XXXS` … `size/XXXL` (9 steps). Labels are applied when the issue is triaged
  and only re-computed when **scope changes** — never re-estimated ritually.
- The size encodes the whole delivery cost (agent work + human review + gating), calibrated on this
  repo's AI-native workflow:

| Size | Agent sessions | Wall-clock | Agent cost (≈) | Human review |
|------|----------------|-----------|----------------|--------------|
| XXXS | <1 | <1 h | <€5 | minutes |
| XXS | 1 | ≤½ day | €5–10 | ~15 min |
| XS | 1–2 | ~½ day | €10–20 | ~30 min |
| S | 2–3 | ~1 day | €20–40 | ~1 h |
| M | 4–8 | 2–3 days | €50–100 | 2–3 h |
| L | 8–15 | ~1 week | €100–250 | ½–1 day |
| XL | 15–30 | 2–3 weeks | €250–500 | 1–2 days |
| XXL | 30–60 | 4–6 weeks | €500–1 000 | 3–5 days |
| XXXL | >60 | >6 weeks | >€1 000 | **must be split** |

  An **agent session** = one focused Claude Code session (~1–3 h wall clock) that ends in a gated,
  pushed change (`make validate` 0 errors, tests green). Costs are order-of-magnitude API spend, not
  accounting figures — they exist so that "is this worth it?" is answerable at a glance.
- A finer estimate (sessions / wall-clock / cost / review hours + dependency notes) lives in the
  issue body's **Estimation** section, plus a rank in the current simplest→largest ordering.

### 2. Every issue carries pre-task documentation

Standard sections, in order — written before implementation starts:

1. **Why now?** — the trigger: what changed that makes this the moment (unblocked dependency,
   risk materialised, decision recorded).
2. **What & why?** — the scope and rationale (the old free-form body content lives here).
3. **Impact** — what it unblocks, what breaks if delayed, blast radius (specs/codegen/runtime).
4. **Sequence diagram** — a small mermaid diagram of the target interaction, so the reader *sees*
   the flow instead of reconstructing it from prose. For pure-codegen issues this may be the
   generation pipeline rather than a runtime flow.
5. **Estimation** — size, sessions, wall-clock, cost, review, dependencies, rank.

### 3. PR = post-task record; duplication with the issue is accepted

- The PR description states what was done and why, as built (it may diverge from the issue — that
  divergence is reviewable information).
- Commits/PRs reference their issue (`#N`) so the before/after pair is navigable both ways.
- We do **not** try to deduplicate the two documents; the issue is frozen intent, the PR is outcome.

### 4. Flow, not Scrum

- No sprints, no estimation meetings, no velocity tracking. Prioritisation is a standing queue:
  cheapest-first among the impactful (the current rank is stamped in each issue), re-ordered only
  when new information arrives.
- Sizing is done by the agent at triage time and reviewed by the product owner asynchronously —
  a label edit is the whole "planning ceremony".

## Alternatives considered

- **Story points / Scrum estimation** — rejected: points calibrate a human team's velocity across
  sprints; here throughput is bounded by review bandwidth and agent-session count, which the shirt
  sizes capture directly.
- **Deduplicating issues and PR descriptions** (issue body auto-replaced by the PR text) — rejected:
  destroys the pre-task contract; scope drift becomes invisible.
- **Estimates only in labels (e.g. `cost/50-100`)** — rejected: label explosion; one size label +
  a body Estimation section keeps the label space stable.

## Consequences

### Positive
- Ordering the backlog is a label query, never a recomputation.
- Every issue is readable stand-alone (why now / what / impact / diagram) — onboarding and
  decision-making no longer require repo archaeology.
- Issue-vs-PR divergence becomes an explicit review signal.

### Negative
- Writing the pre-task documentation costs a triage pass per issue; accepted — it is exactly the
  thinking that would otherwise happen (undocumented) at implementation time.
- Cost figures are rough and will drift with API pricing; they are indicative, not billing.

### Follow-up actions
- All open issues (#12–#28) sized, ranked and rewritten to the standard sections (this change).
- New issues must be created with the sections + a size label; re-size only on scope change.
- If an issue reaches XXXL, split it before starting.
