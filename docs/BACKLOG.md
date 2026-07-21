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

## Claim protocol (multi-session safety) — claim → draft PR → supervised auto-merge

(ADR-20260720-233000, amended by ADR-20260721-042018.)

1. **Claim = label + comment + branch + draft PR, immediately** (before any implementation work):
   - add the **`status/in-progress`** label AND post a claim comment naming the **`NN-slug`**
     branch (issue number first). The label is the atomic, API-visible claim.
   - create branch `NN-slug` from latest `main`, push it, and **open a draft PR
     `NN-slug → main` right away** — body starting with **`Closes #NN`** plus the intended
     approach. From minute one the Development sidebar shows the branch + PR, the board flips to
     In progress (native workflow), and the stale-claim reaper sees linked-PR activity.
     Draft status is the interlock: GitHub refuses auto-merge on a draft, so the early PR can
     never merge half-done work.
2. **Never work an issue that carries `status/in-progress`** — pick the next unclaimed rank.
3. **Work happens on the PR**: push commits to `NN-slug`; the `ci` workflow gates every push.
4. **Completion = ready + auto-merge + supervision** (never end at "pushed, CI pending"):
   local gates green (`make rust`), STATUS.md/ADR updated in the same change, then mark the PR
   **ready for review**, **enable auto-merge** (repo default merge method), and **supervise until
   MERGED** — watch the checks, fix and push on any failure. The merge auto-closes the issue
   (`Closes #NN`), which ends the claim. Checks can't be made green / scope exploded? Comment the
   diagnosis on the PR — don't go silent.
5. Merge (or close) ends the claim naturally. Abandoning? Remove the label and close the draft PR.
6. **Board mirror (native Project workflows — no label trigger exists)**: enable
   "Pull request linked to issue → Status: In progress", "Pull request merged → Done" and
   "Item closed → Done". With the PR created at claim time, the Status column flips at claim time.
   Sessions never write the Status column directly — the label is the authoritative claim
   (full label→Status sync would need a PAT-scoped Action; deliberately not adopted).

**Auto-merge security posture** (analysis in ADR-20260721-042018): repo-level "Allow auto-merge"
grants no merge authority — it must be armed per-PR by a **write-access** user and merges under the
same `main` protection rules as a manual merge; fork/outsider PRs can't arm it and can't merge, so
a "fake empty PR" from outside just sits open. The load-bearing config is the **`main` ruleset**:
it MUST require the **`codegen`** status check (build + tests + validator + drift) — without a
required check, an armed auto-merge fires immediately. Residual (deliberate) trade: any
write-access session lands unreviewed code once CI is green — the executable gates are the review.

## Stale-claim reaper

`.github/workflows/stale-claim-reaper.yml` (hourly): a `status/in-progress` issue with **>24h**
of no activity (issue comments, linked-PR references — the reaper ignores its own comments)
loses the label and gets a "claim expired" comment → back to the queue. A crashed session can
never hold an issue hostage.
