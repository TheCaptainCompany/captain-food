# 📋 Captain.Food — Backlog prioritisation

> Hand-maintained (NOT generated, outside `specs/` so it never affects the DSL).
> This file records **the prioritisation process and how value is defined** — it does NOT hold the
> ranking itself. Recorded decision: [ADR-20260720-213024](adr/20260720-213024-value-first-issue-prioritisation.md).

## Where priorities are defined

**The GitHub Project “Prioritized backlog” (Captain-Food org) is the single place where the backlog
order is defined and maintained.** Nothing in the repository duplicates the ranking — no rank
stamps in issue bodies, no ordered list here. Sessions (human or agent) read the board and **pick
work from the top**: `Urgent` → `High` → `Medium` → `Low`, row order within a bucket. Skipping the
top open item requires a stated reason (blocked, plan-mode approval pending, product-owner
directive) — not preference.

**Re-prioritisation is a product-owner decision, made in the project** (moving items between
Priority buckets / reordering rows). Agents never re-prioritise on their own; if the *method*
below changes, that change is recorded as an ADR amending/superseding ADR-20260720-213024.

## How value is defined (the ordering method)

The backlog is ordered by **value, not effort** (product-owner directive, 2026-07-20):

1. **First: foundations & cross-functional / non-functional** — work everything later stands on:
   API/write contracts, security (ACL), correctness invariants, observability, data
   retention/compliance, and the codegen operating-model wave (which cheapens all downstream work).
   Value here = risk retired × what it unblocks.
2. **Then: features, in value-stream order.** The V0 value stream runs
   **customer ordering** (the PMF funnel — nothing matters until a Tours customer can order)
   → **restaurant onboarding** (supply side, self-serve HubRise connect)
   → **delivery automation** (post-V0; manual/out-of-band in V0).

Within a tier, order stays dependency-consistent (an issue never ranks above one it needs).

## How an issue represents its value and cost

- **Priority** (org field) = the **value bucket** (from the method above):
  `Urgent` = tier-1 contract/security/correctness/observability/NFR foundations ·
  `High` = operating-model / codegen foundations ·
  `Medium` = V0 features in value-stream order ·
  `Low` = post-V0.
  Within a bucket the fine order is the **row order** on the board — no numeric value field is
  used for ordering.
- **Value Size** (org field, T-shirt `XS`–`XL`) = **how much value the issue brings if
  completed**, graded from its Impact section (what it unblocks / what breaks if delayed).
  Informational — it explains the Priority placement, it does not sort.
- **Impact** = **the size of the change on the code** (blast radius). One 5-step T-shirt scale
  (`XS`–`XL`, product-owner decision), in two places with the same value: the **org Impact field**
  and the **`impact/*` repo label** (visible on issue lists/cards). It replaces the former
  `size/*` labels; the finer XXS–XXXL granularity of ADR-20260720-143000's estimation table lives
  only in the body's Estimation section (an estimate beyond XL is a "split before starting" flag,
  not a field value).
- **Effort** (org field, `Low`/`Medium`/`High`) = delivery cost, the coarse projection of the
  Impact T-shirt (XXS–S → `Low`, M → `Medium`, L and up → `High`). Impact/Effort are displayed
  for planning but **never drive the order** — value does.
- **Type** = `Foundation` (non-functional: contracts, security, invariants, observability,
  retention, codegen/operating-model) or `Feature` (functional, user-visible capability) —
  matching the two value tiers; `Bug`/`Task` for the rest.
- Estimation rules are unchanged: ADR-20260720-143000 (the detailed shirt-size estimate still
  lives in the issue body's Estimation section; label/field above are its visible form).

## Triage of new issues

A new issue gets, at triage time: the standard pre-task sections (ADR-20260720-143000), a **Type**
(`Foundation`/`Feature`/`Bug`/`Task`), an **`impact/*` label** (change size, from the Estimation
section), and the org fields **Priority + Value Size + Impact + Effort**, using the definitions
above. The product owner adjusts its row position in the project if the default bucket placement
isn't enough.

## Claim protocol (multi-session safety)

1. **Before any work**: add the **`status/in-progress`** label to the issue AND post a claim
   comment naming the branch (`NN-slug`) you are opening. The label is the atomic, API-visible
   claim; the comment covers the window before the PR exists.
2. **Never work an issue that carries `status/in-progress`** — pick the next unclaimed rank.
3. Branch names are **`NN-slug`** (issue number first); the PR body carries **`Closes #NN`** —
   from then on GitHub's Development sidebar shows everyone the branch + PR for the issue.
4. Merge (or close) ends the claim naturally (the issue closes). Abandoning? Remove the label.
5. **Board mirror (native Project workflows — no label trigger exists)**: enable
   "Pull request linked to issue → Status: In progress", "Pull request merged → Done" and
   "Item closed → Done". The Status column therefore flips at PR-link time; during the short
   claim→PR window the claim is visible as the `status/in-progress` label chip + claim comment.
   Sessions never write the Status column directly — the label is the authoritative claim
   (full label→Status sync would need a PAT-scoped Action; deliberately not adopted).

## Stale-claim reaper

`.github/workflows/stale-claim-reaper.yml` (hourly): a `status/in-progress` issue with **>24h**
of no activity (issue comments, linked-PR references — the reaper ignores its own comments)
loses the label and gets a "claim expired" comment → back to the queue. A crashed session can
never hold an issue hostage.
