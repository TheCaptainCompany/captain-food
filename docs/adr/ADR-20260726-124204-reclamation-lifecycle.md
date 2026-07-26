# ADR-20260726-124204 — Adopt the reclamation (customer claim/dispute) lifecycle

## Status

Accepted

<!-- Realizes PROP-20260726-013207. Accepted: the product owner made the §9 decisions (2026-07-26).
     Realized incrementally by the §10 sub-issues; the two automations (credit ledger, replacement
     order) are large enough that each will carry its own ADR at build time. -->

## Context

Today a customer complaint has one shape — a **refund request** (`RequestRefund` → `RefundProcess` →
approve/deny → Stripe); `RECLAMATION` exists only as a photo *kind*. There is no first-class claim
object with a lifecycle, categories, non-refund resolutions, reopen, or a claims queue. The design and
its rationale are in [PROP-20260726-013207](../proposals/PROP-20260726-013207-reclamation-lifecycle.md)
(tracking issue [#151](https://github.com/TheCaptainCompany/captain-food/issues/151)). The messaging epic
([#129](https://github.com/TheCaptainCompany/captain-food/issues/129)) now gives the discussion channel,
the refund flow gives one resolution, and the attachment framework
([#134](https://github.com/TheCaptainCompany/captain-food/issues/134)) gives the evidence photo; a
reclamation is the missing spine that turns them into a process.

## Decision

**Adopt the proposal's design** — an event-sourced **`Reclamation`** aggregate (keyed by a
client-generated `reclamationId`, correlated to its `orderId`), opened by the customer, discussed in the
order's existing conversation thread, resolved by the restaurant/admin — and build it per the
product-owner decisions below.

### Product-owner decisions (proposal §9), 2026-07-26

| # | Decision | Resolution |
|---|---|---|
| 1 | Identity / cardinality | **Multiple reclamations per order** — identity is `reclamationId`, correlated to `orderId`. |
| 2 | V0 resolution set | **Build the full set, including the automations**: `FULL_REFUND` / `PARTIAL_REFUND` (via the existing refund path), `REJECTED` (reason), **`REPLACEMENT` (an automated no-charge replacement order)** and **`GOODWILL_CREDIT` (a real customer credit-balance ledger)**. NOT recorded-intents. |
| 3 | Discussion | **Reuse the order conversation thread** (#129) — no per-claim thread; `Reclamation*` events weave into the same timeline. |
| 4 | Reopen window | **14 days** after delivery/rejection to open or reopen a claim. |
| 5 | Who may open | **Customer only** (no restaurant/admin-on-behalf in V0). |
| 6 | SLA | **In V0** — a first-response clock + an overdue flag. |
| 7 | Phasing | **V0** — this is the minimal claim experience being built for customers. Refund/reject is the core loop; the two automations (credit, replacement) and the SLA are part of V0 but sequenced after the core. |

### Scope note (the §2 expansion)

Decision #2 promotes what the proposal framed as *post-V0 automation* (§10.6) into **V0 scope**. This
materially enlarges the epic:
- **`GOODWILL_CREDIT`** requires a **customer credit-balance ledger** — a new event-sourced concept
  (credit granted by a claim resolution; credit applied at checkout; balance projection). This is its own
  sub-system and gets its own ADR when built.
- **`REPLACEMENT`** requires an **automated replacement order** — creating a no-charge order from the
  original and dispatching it. Its own sub-system, its own ADR.
The **refund** resolutions still reuse the existing refund path (request/report split — the claim
records the decision; `RefundProcess` moves the money), never a second money mechanism.

## Alternatives considered

Recorded in the proposal §11: keeping "reclamation = refund request" (rejected — no non-money outcome,
category, discussion, or reopen); modelling the claim as fields on `Order` (rejected — its own lifecycle,
several per order); a dedicated per-claim thread (rejected for V0 — the order thread exists); a
third-party helpdesk (rejected for the core — forks the source of truth off the event log). On decision
#2 specifically, "recorded intents first, automate later" was the proposal's recommendation; the product
owner chose to build the automations now as the minimal customer product.

## Consequences

### Positive
- One claim object ties discussion + money + evidence into a tracked process with categories (a quality
  signal) and reopen.
- Refund resolutions reuse the one refund path; no duplicate money mechanism.

### Negative
- Larger V0 than the proposal recommended: the credit ledger and replacement order are each substantial
  new sub-systems, sequenced after the core loop.
- Two new financial concepts (credit balance; no-charge replacement order) raise their own correctness
  and GDPR/retention questions, handled in their own ADRs.

### Follow-up actions
- Sub-issues (proposal §10, revised so the automations are V0), in build order: (1) `Reclamation`
  aggregate core — open/discuss/resolve-refund/reject + 14-day window + SLA fields; (2) read model +
  queries (my claims, claims queue); (3) weave `Reclamation*` into the conversation timeline; (4)
  evidence over #134; (5) SDUI (customer open-claim + my-claims; staff claims queue + resolve panel);
  (6) **customer credit ledger** (own ADR); (7) **replacement order** (own ADR); (8) SLA clock + overdue
  flag.
- Each sub-issue lands its own ADR-0032 completeness set (tests + rules + stories) and the rules named
  in proposal §6.
