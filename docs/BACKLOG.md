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
- **Value Size** (org field, T-shirt `XS`–`XL`) = how much value the issue delivers, graded from
  its **Impact** section (what it unblocks / what breaks if delayed). Informational — it explains
  the Priority placement, it does not sort.
- **`impact/*` label** (repo label, `impact/XS`…`impact/XL`) = the same T-shirt as Value Size,
  mirrored as a label so the value is visible on issue lists and cards outside the project.
  The former `size/*` labels are **retired** (labels can't sort, and they displayed effort where
  value should be).
- **Effort** (org field, `Low`/`Medium`/`High`) = delivery cost, derived from the issue's
  **Estimation** section (ADR-20260720-143000 shirt-size table: XXS–S → `Low`, M → `Medium`,
  L and up → `High`). Effort is displayed for planning but **never drives the order** — value does.
- **Type** = `Foundation` (non-functional: contracts, security, invariants, observability,
  retention, codegen/operating-model) or `Feature` (functional, user-visible capability) —
  matching the two value tiers; `Bug`/`Task` for the rest.
- Estimation rules are unchanged: ADR-20260720-143000 (the shirt-size **estimate** now lives only
  in the issue body's Estimation section + the Effort field, no longer as a label).

## Triage of new issues

A new issue gets, at triage time: the standard pre-task sections (ADR-20260720-143000), a **Type**
(`Foundation`/`Feature`/`Bug`/`Task`), then on the project **Priority + Value Size + Effort** and
the mirroring **`impact/*` label**, using the definitions above. The product owner adjusts its row
position in the project if the default bucket placement isn't enough.
