---
name: architecture-review
description: >
  Run the recurring critical architecture review of Captain.Food — a functional and technical audit
  against the current `main`, from the perspective of a 30-year food-and-delivery architect. Use when
  the user asks to review the system, audit for gaps or holes, check for regressions or drift, or on
  the scheduled daily run. Reports what CHANGED, files only genuinely NEW findings as issues, and
  writes proposals (the lasting artifact) for anything that carries a real design decision. Does not
  modify `specs/**` itself (the executor does — the freeze was lifted 2026-08-10), never claims or
  starts work on an issue.
---

# Architecture review — Captain.Food

A recurring, evidence-based audit of the whole system. The **proposal is the deliverable**; the issue
is only a tracking point that disappears when the work is done.

## Non-negotiables

- **You do not edit `specs/**` — but the team now does** (ADR-20260810-221840; the freeze is lifted).
  The architect audits and hands off; DSL edits are the executor's. Classify a spec need as AMBER only
  when a **recorded decision** is missing/contradicted or the shape is already emitted, stored or
  promised — never merely because it touches `specs/**`.
- **Never claim or start work** on an issue. Prioritisation is a product-owner decision made in the
  GitHub Project.
- **Proposals and docs go to `main` directly** — no branch, no PR (product-owner directive). Run
  `make rust` first if the change regenerates anything.
- **Verify in code, cite `file:line`.** Never report from memory or from a previous run's summary.
  Findings that cannot be evidenced do not get filed.
- **Dedup before analysing**, not after.

## Procedure

### 1. Sync and orient

```
git checkout main && git pull origin main
```

Read the current `docs/status/journal-YYYY-Www.md` week file (and the preceding one if the review
spans a rollover) and `git log --oneline -20` — what shipped since the last review? `docs/STATUS.md`
carries durable state and the journal index, not the record of what shipped.

### 2. Dedup (do this BEFORE looking for anything)

List open issues and skim recently closed ones:

- `mcp__github__list_issues` (state OPEN) — the full open backlog
- read `docs/proposals/` — the option analysis for known clusters is already written

The 2026-07-26 baseline filed **#166–#205**. Do not re-report anything already covered by:

| Area | Tracked by |
|---|---|
| Read-side per-instance authz | #144 + PROP-20260725-185140 |
| Write-side per-instance authz | #178, #205 + PROP-20260726-171500 |
| Reclamation lifecycle | #151 epic (#153–#160) |
| Messaging / notifications transport | #129 epic, #127, #132 |
| File attachments | #134 |
| Order operational safety | #198 + PROP-20260726-164500 |
| Money / fees / payouts / VAT / capture | #199 + PROP-20260726-165000 |
| Catalog / allergens / photos / merchandising | #200 + PROP-20260726-165500 |
| Event log integrity / evolution / erasure | #201 + PROP-20260726-170000 |
| Observability / scale | #202 + PROP-20260726-170500 |
| Spec-to-UI contract integrity | #203 + PROP-20260726-172000 |
| Delivery execution | #204 + PROP-20260726-172500 |

A finding already tracked is **not** a finding. Say "still open, unchanged" at most.

### 3. Run the checks

Load `references/checklist.md` for the full probe list with the exact commands and the expected
current state of each. Cover, at minimum:

- **Order lifecycle** — acceptance timeout, opening hours, capacity, ETA, scheduling, modification.
- **Money** — does `crates/application/src/pricing.rs` still zero the fee legs? Connect/transfers?
  VAT computed? Invoice? Outbound `Idempotency-Key`?
- **Authorization** — any query or mutation added since the last run that trusts a caller-supplied
  id or an optional filter. *This is the highest-value regression class — check it every run.*
- **Runtime correctness** — drain visibility guard, the `head - head` lag computation, poison-event
  handling, leader election, event versioning.
- **Observability** — is there a telemetry dependency in `Cargo.toml` yet?
- **Compliance** — allergens, GDPR erasure, receipts, privacy/terms.
- **Gate integrity** — `make validate` and `make rust`; and by hand, for any screen touched since the
  last run, whether its action `variables` satisfy the bound mutation's `required` fields (the
  validator does not check this yet — #169).

Use parallel `Explore` subagents for breadth; verify their claims yourself before reporting.

### 3bis. The unrealized-directive sweep (ADR-20260813-233418) — run EVERY time

Before reporting, list the **dropped directives**: a decision marked **✅ DECIDED / Approved** (in
`docs/proposals/DECISIONS.md`, a proposal `Status`, or an `Accepted` ADR) whose realizing work is
**neither merged nor in-progress** — no open `status/in-progress` issue, no live PR, no merged
realizing PR/ADR. These are recorded intent silently waiting for a human to repeat it (the Uber Eats
directive sat approved-but-undone for two weeks; the capture posture drifted with no gate). Surface
them at the **top** of the report, ranked by the value method (`docs/BACKLOG.md`), so the next session
executes recorded intent without the founder re-stating it.

The signal is the **intersection** of a repo marker (a ✅/Approved decision) and live GitHub state (no
realizing work) — the offline validator cannot see PR/issue state, which is why this is a standing
review step and **not** a validator rule. Do **not** approximate it with "empty `Realized by` header":
~30 proposals carry an un-maintained `_(filled at completion)_` while already shipped, so that signal
is mostly false positives (ADR-20260813-233418 records the judgment).

### 4. Report

Lead with **what changed**, then the **unrealized-directive sweep** (§3bis), then only genuinely new
findings. A quiet day is a two-line report — do not pad it. Never restate the backlog.

### 5. File what is new

**Issue** (the tracking point) — follow `docs/BACKLOG.md` triage exactly:

- **Type**: `Foundation` (non-functional: contracts, security, invariants, observability, retention,
  codegen) · `Feature` (user-visible capability) · `Bug` · `Task`
- **`impact/*` label** — change size (blast radius), XS–XL
- **Org fields, all four**:
  - `Priority` — value bucket: `Urgent` = tier-1 contract/security/correctness/observability/NFR ·
    `High` = operating-model/codegen foundations · `Medium` = V0 features · `Low` = post-V0
  - `Value Size` (XS–XL) — how much value if completed, graded from the Impact section
  - `Impact` (XS–XL) — same value as the label
  - `Effort` — projected from Impact: XS/S → `Low`, M → `Medium`, L/XL → `High`
- **Body sections** (ADR-20260720-143000): Why now? · What & why? · Impact · Sequence diagram ·
  Estimation · Definition of done (ADR-0032) · Refs
- Reference every issue by **number and title**; in repo markdown use full clickable links.
- End the body with the Claude Code attribution footer.

**Proposal** (the lasting artifact) — required whenever a finding carries a real design decision.
See `references/proposal-template.md`. Per the 2026-07-26 product-owner directive it MUST include:

- **screen mockups, one per use case** (ASCII wireframes are fine);
- **sequence diagrams, one per load-bearing flow** (mermaid, hexagonally faithful — the aggregate or
  PM *decides*, saved through the `Repository`, appended by `PgEventStore`; see `docs/claude/mermaid.md`);
- **per-option pros/cons for every decision surfaced**, with the recommendation marked. A bare
  "A vs B" without trade-offs is incomplete.

Every proposal needs a tracking issue (ADR-20260724-143000) — create it first and name it in the
header. Commit proposals to `main`.

### 6. Keep STATUS current

Add a dated entry at the **TOP** of the applicable `docs/status/journal-YYYY-Www.md` for any
substantive finding, in the same change — newest first, never appended at the end; create the week
file from the established header if it does not exist. `docs/STATUS.md` is durable state and the
journal index, not the destination for dated entries — update it only where the current state it
describes actually changed.

## Judgement notes

Things this codebase's operating model systematically under-produces, because they are not
*spec-able* — check them deliberately, they will not surface on their own:

- notifications, images, telemetry wiring, hosting posture, legal documents, payout destinations;
- anything whose absence produces no validator error;
- **UI that promises capability the domain lacks** — screens ship widgets bound to declared `gap`s,
  and a live control that silently does nothing is worse than no control.

Conversely, do not re-litigate what the team has consciously accepted and documented in an ADR (V0
scale trade-offs, projection-on-read, no snapshots). Consciously-accepted is not a finding; state it
only if the assumption behind it has changed.
