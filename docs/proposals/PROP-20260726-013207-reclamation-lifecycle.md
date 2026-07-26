# PROP-20260726-013207 — First-class reclamation (customer claim/dispute) lifecycle

- **Status**: Proposed — plan-mode proposal. **No `specs/**` or code changed yet.** On approval it becomes an ADR that lands with the first implementation slice.
- **Date**: 2026-07-26
- **Tracking issue**: [#151 "First-class reclamation (customer claim/dispute) lifecycle — open, discuss, resolve (refund/replacement/goodwill/reject)"](https://github.com/TheCaptainCompany/captain-food/issues/151) (every proposal MUST have one, ADR-20260724-143000; always the full clickable link)
- **Realized by**: _(filled at completion — ADR + PR)_
- **Related**: [#129 "Epic: in-app order conversations (messaging)"](https://github.com/TheCaptainCompany/captain-food/issues/129) (the discussion channel — now live on the customer + restaurant surfaces) · [PROP-20260725-013008](PROP-20260725-013008-order-conversations-messaging.md) §2.7/§9.5 (in-thread refund binding, [#133](https://github.com/TheCaptainCompany/captain-food/issues/133)) · [PROP-20260725-120055](PROP-20260725-120055-generic-file-attachment-framework.md) ([#134](https://github.com/TheCaptainCompany/captain-food/issues/134), the `RECLAMATION` evidence photo) · the existing **refund flow** (`RequestRefund`/`ApproveRefund` + `RefundProcess`) · ADR-0041 (acting user is envelope, not payload) · CLAUDE.md request/report split.

> **Status:** Proposed. This document records the design and the decisions it asks the product owner
> to make. It is intentionally scoped as an **epic-candidate** and decomposed into shippable
> sub-issues (§10).

---

## TL;DR

Today a customer complaint has exactly one shape: a **refund request**. `RECLAMATION` exists only as a
photo *kind* in the messaging/attachment proposals. There is **no first-class claim object** — no
category, no non-refund resolution, no reopen, no status, no claims queue.

This proposes a **`Reclamation` aggregate** (event-sourced, correlated to an order): a customer **opens**
a claim (category + description + optional evidence photo), it is **discussed in the order's existing
conversation thread** ([#129](https://github.com/TheCaptainCompany/captain-food/issues/129)), and the
restaurant or an admin **resolves** it. Refund resolutions **feed the refund path we already have**
(request → the restaurant/admin approves → Stripe reports the fact) — *not* a second money mechanism.
Non-refund resolutions (replacement, goodwill, rejection-with-reason) are new. The claim carries a
tracked status, a reopen policy, and (later) an SLA.

It reuses three things already built or designed — the **conversation thread** (discussion), the
**refund path** (money), and the **attachment framework** (evidence) — and adds only the missing spine:
the claim's identity, lifecycle, categories, and non-refund resolutions.

## 1. The problem — "reclamation = refund" today

The current complaint path (implemented, not just proposed):

`RequestRefund` (customer) → `RefundRequested` → the **`RefundProcess`** process manager opens it for
decision → restaurant/admin `ApproveRefund` / `DenyRefund` → `RefundApproved` / `RefundDenied` → Stripe
reports `PaymentRefunded` (inbound). Read side: `View_PendingRefunds` → `pendingRefunds` → the
back-office refunds queue.

What that model cannot express:
- A complaint whose right outcome is **not money** (a remake, a redelivery, a goodwill gesture, or a
  reasoned "no").
- A **category** ("missing item" vs "cold" vs "never arrived") — so no analytics, no routing, no
  restaurant-quality signal.
- A **discussion** attached to the claim (now possible — the #129 thread exists).
- **Reopen** after a rejection, or a **time-bounded** claim window (the chargeback tail).
- A **claims queue** distinct from the refunds queue (a claim may be open with no refund yet).

## 2. The model

### 2.1 A `Reclamation` aggregate, event-sourced

New aggregate in `crates/domain` (spec: entities/events/commands/actors/errors/rules/tests), keyed by a
**client-generated `reclamationId`** (so a single order can carry more than one claim — see the §9.1
decision), correlated to its `orderId`. Business events (payloads business-only, ADR-0041 — the acting
user is `domain_events.user_id`, never a field):

| event | payload (business only) |
|---|---|
| `ReclamationOpened` | `reclamationId`, `orderId`, `category` (`ReclamationCategory`), `description`, `requestedResolution?` (`ReclamationResolution`, the customer's ask) |
| `ReclamationEvidenceAttached` | `reclamationId`, `attachmentRef` (opaque, via #134) |
| `ReclamationResolved` | `reclamationId`, `resolution` (`ReclamationResolution`), `note?`, `refundAmount?` (`Money`, for a partial) |
| `ReclamationRejected` | `reclamationId`, `reason` |
| `ReclamationReopened` | `reclamationId`, `reason` |

Status is derived in the read model (a fold), not stored — the same pattern as the conversation and the
refund views.

### 2.2 Categories and resolutions (new scalars)

- `ReclamationCategory` — `MISSING_ITEM | WRONG_ITEM | QUALITY | LATE_DELIVERY | DAMAGED | NOT_DELIVERED | OTHER`.
- `ReclamationResolution` — `FULL_REFUND | PARTIAL_REFUND | REPLACEMENT | GOODWILL_CREDIT | REJECTED`.
- `ReclamationStatus` (derived) — `OPEN → UNDER_REVIEW → RESOLVED | REJECTED`, with `REOPENED` folding back to `OPEN`.

### 2.3 Refund resolutions REUSE the existing refund path (the load-bearing reuse)

`FULL_REFUND` / `PARTIAL_REFUND` do **not** move money themselves. Resolving a claim that way **triggers
the refund command we already have** (`ApproveRefund` on the order, optionally partial via
`refundAmount`), and Stripe reports `PaymentRefunded` as an **inbound fact** — the CLAUDE.md request/
report split, exactly as [#133](https://github.com/TheCaptainCompany/captain-food/issues/133) binds the
in-thread refund. The `Reclamation` records the *decision*; the `RefundProcess` moves the *money*. One
refund path, one audit trail. (Whether the claim resolution emits `ReclamationResolved` **and** the
refund command in one handler, or a process manager reacts to `ReclamationResolved` and issues the
refund, is an implementation detail — the saga precedent is `RefundProcess`.)

### 2.4 Non-refund resolutions (the genuinely new part)

- `REJECTED` — a reasoned "no" (`ReclamationRejected`), reopenable within the window.
- `REPLACEMENT` / `GOODWILL_CREDIT` — see §9.2: for V0 these can be **recorded intents** (the resolution
  fact + a note, actioned operationally) without automating a replacement order or a credit-balance
  ledger, which are larger builds. The aggregate models the *decision*; automation follows if wanted.

### 2.5 Discussion reuses the order conversation (#129)

A reclamation does **not** get its own thread. The order already has a `Conversation` (identity = the
order); the claim's back-and-forth, the `RECLAMATION` evidence photo, and the status changes all live in
that one thread (the conversation read model can weave `Reclamation*` events into its timeline the same
way it folds order-status events). "Open a claim" is a structured overlay on the thread the customer is
already in — no second inbox.

### 2.6 Evidence reuses the attachment framework (#134)

`ReclamationEvidenceAttached.attachmentRef` is an opaque reference into the generic file framework
(`PROP-20260725-120055`), which already reserves the `RECLAMATION` kind with a **180-day** retention
(a claim/chargeback can run months). No bespoke storage here.

## 3. Read model + queries

A `Reclamation` read model (a `View_*` fold or a projection table if it carries the discussion) backing:
- `myReclamations` (CUSTOMER) — the customer's claims + status.
- `restaurantReclamations` (RESTAURANT/RESTAURANT_ACCOUNT/ADMIN) — the **claims queue**, filterable by
  status/category; the restaurant's quality signal.
- `reclamation(reclamationId)` — one claim's detail (links to its order + conversation).

## 4. SDUI surfaces

- **Customer** (`restaurant_frontoffice`): an "Open a claim" entry from the order (category picker +
  description + optional photo → `openReclamation`); a "my claims" list; the claim detail deep-links into
  the order conversation for the discussion.
- **Restaurant/admin** (`restaurant_backoffice`): a **claims queue** beside the refunds queue; a resolve
  panel (choose resolution; a refund resolution reuses the refund amount control that
  [#133](https://github.com/TheCaptainCompany/captain-food/issues/133) introduces; reject-with-reason).

All reuse the SDUI component vocabulary + the now-live conversation screens; only a category picker and a
claims-list card are plausibly new component kinds.

## 5. Lifecycle, reopen window, SLA

- **Status** folds from the events (§2.2). A claim can be `REOPENED` after `REJECTED` (or after an
  unsatisfactory `RESOLVED`) within a **window** (§9.4).
- **SLA** (a response-time target, overdue flagging) is **deferred** (§9.5) — a first-response clock is
  an observability/read-model addition, not a domain change.

## 6. Completeness obligations (ADR-0032)

Every new command/event/error also needs a behaviour test (+ its `rules:` link), every new query/mutation
a story step, every new rule a test — enforced by `make validate`. New rules this introduces (illustrative):
a reclamation targets a real order; a refund resolution goes through the one refund path; a rejection
carries a reason; reopen is allowed only within the window; only the customer opens/reopens, only
restaurant/admin resolves/rejects.

## 7. Observability

Contracts in `specs/observability.yaml` for: claim open → first response (the SLA clock), resolution
outcome distribution (refund vs replacement vs goodwill vs reject) by category, and the refund-path
hand-off (a claim-driven refund must correlate to its `RefundProcess` run).

## 8. Relationship to what exists

- **Refund flow** — reused for money (§2.3); the claim is the *why*, the refund is the *how much*.
- **Messaging (#129)** — reused for discussion (§2.5); [#133](https://github.com/TheCaptainCompany/captain-food/issues/133)'s
  in-thread "accept refund" becomes *"resolve the claim as a refund"*.
- **Attachments (#134)** — reused for evidence (§2.6).
- The claim is the missing **spine** that turns three capabilities into a process.

## 9. Decisions this proposal asks the product owner to make

1. **Identity/cardinality:** `reclamationId` (allow **several claims per order** — recommended, real
   orders have multiple issues) vs. one-claim-per-order (simpler, but conflates distinct issues).
2. **V0 resolution set:** ship **FULL_REFUND / PARTIAL_REFUND / REJECTED** first (all expressible via the
   existing refund path + a reason), with **REPLACEMENT / GOODWILL_CREDIT as recorded intents** (no
   automated re-order or credit ledger yet)? Or build the credit-balance / replacement-order automation
   up front (much larger)?
3. **Discussion:** reuse the **order conversation thread** (recommended, §2.5) vs. a dedicated per-claim
   thread.
4. **Reopen window:** how long after delivery/rejection may a customer open/reopen a claim (e.g. 14/30/180
   days — align with the `RECLAMATION` 180-day evidence retention and the chargeback tail)?
5. **Who may open:** customer only, or also restaurant/admin filing on the customer's behalf (phone claim)?
6. **SLA:** in V0 (a first-response clock + overdue flag) or deferred?
7. **Phasing:** V0 or post-V0? Refunds already exist, so a **minimal "claim → refund/reject with category
   + reason"** could ride close to V0; the non-refund resolutions and SLA are clearly post-V0.

## 10. Decomposition into sub-issues (created on approval)

1. **`Reclamation` aggregate** — open/resolve/reject/reopen + category/resolution/status scalars + the
   refund-path binding for refund resolutions (the RefundProcess hand-off). *(domain; the core)*
2. **Read model + queries** — `myReclamations`, `restaurantReclamations` (claims queue), `reclamation`.
3. **Weave `Reclamation*` into the conversation timeline** (#129 read model) — the discussion overlay.
4. **Evidence** — `ReclamationEvidenceAttached` over the #134 attachment framework.
5. **SDUI** — customer "open a claim" + "my claims"; restaurant/admin claims queue + resolve panel.
6. **Non-refund automation** (if approved) — replacement order + goodwill credit ledger. *(post-V0, large)*
7. **SLA + overdue flagging** — first-response clock (observability + read model). *(post-V0)*

## 11. Considered alternatives

- **Keep "reclamation = refund request" (status quo).** Rejected as the target state: it cannot express a
  non-money outcome, a category, a discussion, a reopen, or a claims queue — the whole point here.
- **Model the claim as fields on the `Order` aggregate.** Rejected: a claim has its own lifecycle (open →
  resolve → reopen), there can be several per order, and folding it into `Order` bloats the order
  aggregate and couples two lifecycles that evolve independently.
- **A dedicated per-claim conversation thread.** Rejected for V0 (§2.5): the order already has one thread;
  a second inbox splits the customer's context for no product gain. Revisit if a claim ever needs
  participants the order thread does not have.
- **A third-party helpdesk (Zendesk/Gorgias).** Rejected for the core: it forks the source of truth off
  the event log, duplicates identity/ACL, ships order+customer data to another processor (a GDPR + the
  "ethical alternative" positioning cost), and cannot fold the order/refund/conversation events. A
  support-desk *integration* could still sit beside it later.
