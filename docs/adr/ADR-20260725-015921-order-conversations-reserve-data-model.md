# ADR-20260725-015921 — Adopt the order-conversations (messaging) design; reserve the Conversation data model now

## Status

Accepted

<!-- Realizes PROP-20260725-013008. Accepted because the "reserve the data model" slice is realized in
     this same change (the entities.yaml reservation note below + this ADR). The runtime slices it
     authorizes stay Proposed-in-effect until each ships — see Follow-up actions. -->

## Context

The product owner asked (2026-07-25) for **in-app order conversations** — a per-order message thread
between the customer, the restaurant, the rider, and (on escalation) an admin, with private staff
notes, client-initiated translation persisted server-side, quick replies, opt-in customer↔restaurant
chat, push notifications, order-status woven into the timeline, mute-with-reason, in-thread refunds,
and image attachments. The full design, its constraints, and the decisions it surfaces are in
[PROP-20260725-013008](../proposals/PROP-20260725-013008-order-conversations-messaging.md) (tracking
issue [#129 "Epic: in-app order conversations…"](https://github.com/TheCaptainCompany/captain-food/issues/129)).

Two forces shape *how much* to do now:

- **This is post-V0.** V0 is PMF in Tours (order → pay → track). The chat is not on that path, and the
  Rust runtime (`crates/`) does not exist yet (ADR-0034), so nothing here can be *built* — it can only
  be *specified*.
- **Retrofitting "order status participates in the thread" is expensive.** The conversation read model
  is a fold over the order's own status events **and** the conversation's message events, **both keyed
  by `orderId`**. If the conversation's identity is chosen as anything other than `orderId`, that fold
  becomes a join/backfill later. Fixing the identity now costs a paragraph; fixing it later costs a
  migration.

Adding *validated* DSL types now (a real `Conversation` aggregate with events/commands) would trip the
ADR-0032 completeness gate — forcing tests, stories, rules, api ops, and screens for a feature whose
load-bearing decisions (§8 of the proposal) the product owner has not all made. That is the wrong
order: decide, then specify.

## Decision

**Adopt the proposal's design** as the intended shape of the feature, and **land only the "reserve the
data model" slice now** — spec-narrative + this ADR, no new validated types, no runtime:

1. **Reserve the identity.** A future `Conversation` aggregate is **keyed by `orderId`** — a
   conversation's identity *is* its order. Recorded as a note in `specs/entities.yaml` (comment,
   regenerates nothing) pointing here.
2. **Reserve the fold principle.** The conversation read model (`View_OrderConversation`) will fold
   **both** the order's status events **and** the conversation's message events for that `orderId`,
   ordered by `occurredAt`. "Status participates in the discussion" is then free — no status is copied
   into a message.
3. **Adopt the mechanism reuse** the proposal argues for, so the later slices don't reopen it:
   event-sourced aggregate (no mutable chat table); the role-pathed GraphQL ACL for PUBLIC/INTERNAL
   visibility (the customer schema projection cannot return INTERNAL messages); the acceptance-first
   write model for posting; the **existing** refund path for in-thread refunds (request vs. report
   split — the chat action triggers the refund command, Stripe reports `PaymentRefunded` inbound); and
   the #127 notification cascade for push.
4. **Decompose the rest into 9 sub-issues** (proposal §9), each independently shippable behind the
   acceptance-first write model, carrying its own ADR-0032 completeness set (tests + rules + stories).

### Product-owner decisions (proposal §8), resolved

The product owner directed "do this completely" (2026-07-25). Adopting the proposal's recommendations:

| # | Decision | Resolution |
|---|---|---|
| 1 | Translation cadence + BFF proxy | **On-demand + cache**, via a BFF `/internal/translate` proxy holding our key (a browser-side key is not viable). |
| 3 | In-thread refund scope | The chat action **triggers the existing refund command** (not a second mechanism, not record-only). |
| 4 | Mute authorization matrix | restaurant → (customer, rider); admin → anyone; customer → none. |
| 6 | Phasing | **Post-V0**, and **land the reserve slice now** (this ADR). |

Two decisions are **left open** and carried to their implementing slice, because the proposal offers no
single recommendation and they should not be fabricated while unconfirmed:

- **#2 Image retention window (GDPR)** for `DELIVERY_PROOF` / `RECLAMATION`, and whether the first
  image slice ships at all — to be set alongside the journals/mirror retention policy
  ([#18](https://github.com/TheCaptainCompany/captain-food/issues/18)) when the attachments slice is picked up.
- **#5 Rider participation default** (always a participant once assigned, vs. pulled in on demand) —
  proposed default *"participant once assigned"*, to confirm when the text-conversation slice is built.

## Alternatives considered

- **Build the full domain model now.** Rejected: it forces the ADR-0032 completeness set for a post-V0
  feature and bakes in the two still-open decisions. Decide first, specify per slice.
- **Reserve nothing; design it all when the chat is built.** Rejected: the `orderId` identity + the
  status-fold are the one thing that is a costly retrofit if guessed wrong later — the proposal's
  strongest reason the feature fits our event model.
- **A generic (non-order-scoped) chat, a third-party chat SDK, or mutable message rows.** All rejected
  in the proposal (§10): they fork the source of truth off the event log and lose identity/ACL/
  retention/status-fold that order-scoping gives for free.

## Consequences

### Positive
- The costly-to-retrofit decision (conversation identity + status-fold) is fixed now, cheaply.
- The epic is decomposed into shippable, individually-gated sub-issues; concurrent sessions have a
  single record of intent and phasing.
- No unconfirmed decision is baked into validated DSL; the gate is not weakened.

### Negative
- The reservation is narrative only (a comment + this ADR); it is not enforced by `make validate` until
  the `Conversation` aggregate is actually specified. The sub-issues carry that enforcement.
- Two open decisions (#2, #5) remain — visible on their slices, not silently defaulted.

### Follow-up actions
- Sub-issues (proposal §9), created with this ADR, in phasing order: text conversations + visibility
  ACL → order-status timeline fold → push on new message (#127) → in-thread refund binding → attachments
  → translation → admin escalation + mute-with-reason → quick replies.
- Each sub-issue lands its own ADR-0032 completeness set and the rules named in proposal §6 (private
  messages never reach the customer schema; mute requires a reason; a muted participant still receives
  status; in-thread refund uses the one refund path; customer compose requires `customerChatEnabled`).
- Observability contracts (proposal §7) land with the slices that introduce their workflows.
