# PROP-20260725-013008 — In-app order conversations (customer / restaurant / rider / admin messaging)

- **Status**: Proposed
- **Date**: 2026-07-25
- **Tracking issue**: [#129 "Epic: in-app order conversations (customer/restaurant/rider/admin messaging) with private notes, translation, in-thread refunds, images"](https://github.com/TheCaptainCompany/captain-food/issues/129) (every proposal MUST have one, ADR-20260724-143000; always the full clickable link, never a bare number)
- **Realized by**: (pending)
- **Related**: [#127 "Notification channel cascade: push -> RCS -> SMS fallback"](https://github.com/TheCaptainCompany/captain-food/issues/127) (push delivery this depends on) · ADR-20260720-015500 (acceptance-first write model) · ADR-0041 (envelope metadata: the acting user is `domain_events.user_id`, not payload) · ADR-0015 (identity wrapped behind our GraphQL).

> **Status:** Proposed — plan-mode proposal. **No `specs/**` or code changed yet.** On approval it
> becomes an ADR that lands with the first implementation slice. This document is the record of the
> design and the decisions it asks the product owner to make; it is intentionally scoped as an
> **epic** and decomposed into shippable sub-issues (§9).

---

## TL;DR

A **per-order conversation**: one message thread bound to a single order, with the customer, the
restaurant, the rider, and (on escalation) an admin as participants. It reuses the machinery we
already have — an **event-sourced aggregate** (`Conversation`), the **role-pathed GraphQL ACL** for
the private/staff-only messages, the **acceptance-first write model** for posting, and the existing
**refund path** for in-thread refunds — rather than inventing parallel mechanisms. The order's own
status transitions **fold into the same timeline**, so "where's my order" and the conversation are
one history.

Two parts are genuinely new infrastructure and carry the real risk: **image attachments** (object
storage + moderation + GDPR retention) and **client-side translation persisted server-side** (the
Google Translate API key cannot live in the browser). Both are called out as decisions, not assumed.

This is **post-V0** (V0 is PMF in Tours: order → pay → track). What is worth doing **now** is
*reserving the data model* so that "order status participates in the thread" is not a retrofit.

## 1. The feature (product-owner requirements, 2026-07-25)

Captured verbatim, then mapped to the model in §2:

1. Messaging between **customer**, **restaurant**, **rider**, and **admin**.
2. Messages from restaurant / rider / admin can be **PRIVATE** — staff-only, **not visible to the customer**.
3. **Auto-translation** via **Google Translate, client-side**, and **the translation is saved on our server**.
4. **Admin can be added** to a discussion when the restaurant or rider feels it legitimate to fix an issue.
5. **Quick-reply / suggested-message** UX (like Uber Eats).
6. **Customer can chat directly with the restaurant only if the restaurant authorises it.**
7. **Push notifications required.**
8. **Order status participates** in the discussion — a clear, single history of what happened.
9. **Abuse:** the restaurant can **mute** a participant **for X minutes or indefinitely, for this order**, and it **must be justified with a reason**.
10. In the discussion the restaurant can **accept a partial or full refund request**.
11. **Images:** restaurant (order photo before packaging), rider (proof-of-delivery photo), customer (reclamation photo).

## 2. How it maps onto the architecture

### 2.1 A `Conversation` aggregate, event-sourced (one per order)

New aggregate in `crates/domain` (spec: `entities.yaml` + `events.yaml` + `commands.yaml` +
`actors.yaml`), keyed by `orderId` (a conversation's identity IS its order). Business events
(payloads business-only, ADR-0041 — the acting user is `domain_events.user_id`, never a field):

| event | payload (business only) |
|---|---|
| `ConversationOpened` | `orderId`, `restaurantId` (customer↔restaurant chat allowed? = a restaurant setting, §2.4) |
| `MessagePosted` | `conversationId`, `body`, `visibility` (`PUBLIC` \| `INTERNAL`), `authorRole` (CUSTOMER\|RESTAURANT\|RIDER\|ADMIN), `originalLocale`, `attachmentIds[]`, `quickReplyId?` |
| `MessageTranslationAdded` | `messageId`, `locale`, `text` (the persisted client-side translation, §3) |
| `ConversationParticipantAdded` | `conversationId`, `participantRole: ADMIN`, `reason` (the escalation) |
| `ParticipantMuted` | `conversationId`, `mutedRole`, `until?` (absent = indefinite), **`reason` (required)** |
| `ParticipantUnmuted` | `conversationId`, `mutedRole` |
| `MessageAttachmentAdded` | `attachmentId`, `kind` (`ORDER_PHOTO`\|`DELIVERY_PROOF`\|`RECLAMATION`), `storageRef`, `contentType` |

Because it is event-sourced, the read view is a **fold**; there is no separate mutable chat table to
keep consistent.

### 2.2 Order status folds into the timeline (requirement 8)

The order already emits status events (`OrderPlaced`, `OrderAccepted`, `OrderReady`,
`DeliveryStatusUpdated`, …) on `domain_events`, all keyed by `orderId`. The conversation read model
(`View_OrderConversation`) is a fold over **both** the order's status events **and** the
conversation's message events for that `orderId`, ordered by `occurredAt`. "Status participates in
the discussion" is then free — no status is *copied* into a message; the timeline query simply
merges two event streams that already share a key. This is the single strongest reason the feature
fits our model cleanly, and the reason to **reserve the `orderId`-keyed conversation identity now**.

### 2.3 Privacy / visibility = the role-pathed ACL we already have (requirements 2)

Each `MessagePosted` carries a `visibility`. The `messages` field on the conversation read type is
resolved per role: the **CUSTOMER** `/public`/`/customer` schema path returns only `PUBLIC`
messages; **RESTAURANT / RIDER / ADMIN** paths return `PUBLIC` + `INTERNAL`. This is exactly what
the per-role `/{role}/graphql` filtering (api.yaml `@auth`) was built for — no new authorization
engine, and the customer literally cannot request the internal notes (they are absent from their
schema projection, not merely hidden in the UI).

### 2.4 Customer↔restaurant chat is opt-in per restaurant (requirement 6)

A restaurant setting `customerChatEnabled` (a field on the restaurant aggregate). When false, the
customer surface renders the thread **read-only** (status + any public restaurant/rider messages,
e.g. proof-of-delivery) but the customer compose box is absent and `postMessage` for a CUSTOMER
author is rejected (an anticipated error). Restaurants that don't want to run a chat desk are never
forced to.

### 2.5 Admin escalation (requirement 4)

`ConversationParticipantAdded { participantRole: ADMIN, reason }` — emitted by a RESTAURANT- or
RIDER-authored command (`escalateToAdmin`). It grants the ADMIN role read/write on that one
conversation and raises an operator signal (an observability contract). Admin is not a silent
lurker: the escalation is a recorded, reasoned event.

### 2.6 Mute-with-reason (requirement 9)

`MuteParticipant { conversationId, mutedRole, until?, reason }` → `ParticipantMuted`. The **reason is
required** — omitting it is an anticipated error (`errors.yaml`, the invariant pattern), so
"justified" is enforced by the write model, not UI convention. `until` absent = indefinite; a muted
participant still **receives order-status updates** (legally/operationally they must learn their
order state) but cannot `postMessage` on that conversation. Who may mute whom is an authz rule
(restaurant may mute customer/rider on its orders; admin may mute anyone; a customer cannot mute).

### 2.7 In-thread refund reuses the EXISTING refund path (requirement 10)

Critically **not** a second refund mechanism. A customer's refund *request* and the restaurant's
*acceptance* are chat actions that **feed the commands we already have** — the restaurant accepting a
partial/full refund emits the existing refund/cancel command, and Stripe reports `PaymentRefunded`
as an **inbound fact** (per CLAUDE.md's request/report split). The conversation records the
request/acceptance as messages for the audit trail; the *money* moves through the one refund path.
The proposal's job here is to *bind* the chat action to that path, not to duplicate it.

### 2.8 Quick replies (requirement 5)

A **static** canned-response set per role for V0 (translation-catalogued keys, so they localise for
free), surfaced as suggestion chips. `MessagePosted.quickReplyId` records which suggestion was used
(useful signal later). ML-generated suggestions are explicitly deferred — static first.

### 2.9 Push notifications (requirement 7)

A new `MessagePosted` (public, addressed to a participant who isn't looking) fans out through the
**notification cascade of [#127](https://github.com/TheCaptainCompany/captain-food/issues/127)**
(push → RCS → SMS). Messaging is arguably #127's strongest justification. Until #127 lands, V0-of-
messaging can degrade to in-app-only + the existing order-status notifications.

## 3. Translation — the shakiest part, decided explicitly (requirement 3)

Requirement: translate **client-side with Google Translate**, and **persist the translation on our
server**. Two real constraints:

- **The Google Cloud Translation API is paid and key-based; the key cannot live in the browser** (it
  would be extracted and abused). "Client-side" in practice means the client *initiates* translation
  through a **thin BFF proxy** (`/internal/translate`, our key, rate-limited, per-tenant metered).
  The legacy free website widget (`translate.google.com` element) is deprecated and unsuitable for a
  product surface.
- **"Saved on our server" is the right instinct**: translate a message **once** (on first demand for
  a target locale), store it as `MessageTranslationAdded { messageId, locale, text }`, and serve the
  cached translation thereafter — never re-billing, and the thread is fully readable offline/on SSR.

Message storage shape: `{ originalLocale, body, translations: { <locale>: text } }` — the fold
carries the original plus any cached translations; the reader's resolved locale (the #110 chain)
picks which to show, falling back to the original with a "translated" affordance.

**Decision needed:** translate-on-post (eager, every message pre-translated to the other participant
locales — simpler UX, higher cost) vs translate-on-demand (lazy, only when a reader needs another
locale — cheaper, a first-view latency). Recommendation: **on-demand + cache**, metered per tenant.

## 4. Images — object storage, not an upload field (requirement 11)

Three attachment kinds: `ORDER_PHOTO` (restaurant, pre-packaging), `DELIVERY_PROOF` (rider),
`RECLAMATION` (customer). This needs:

- **Object storage** (Supabase Storage — same provider family, EU/Frankfurt, ADR-0042): the event
  carries a `storageRef`, not bytes. Upload via a signed URL; the domain event references the stored
  object.
- **Moderation + abuse**: user-supplied images on a public-ish surface. At minimum content-type/size
  limits, per-participant rate limits (ties to mute), and a report path; automated moderation is a
  later hardening.
- **GDPR / retention**: `DELIVERY_PROOF` photos capture doorsteps/people and `RECLAMATION` photos may
  show anything — **personal data**. They need a retention window (align with the journals/mirror
  retention policy, #18) and inclusion in the data-subject-erasure story. This is a privacy decision,
  not just a storage one.

## 5. What to reserve NOW vs build later (phasing)

**V0 (Tours PMF) does not need the chat**, but retrofitting "order status in the thread" later is
costly. So:

- **Reserve now (cheap, spec-only):** the `orderId`-keyed `Conversation` identity and the principle
  that the conversation read model folds order-status events. This is a paragraph in the model + an
  ADR, no runtime.
- **Post-V0, phased (the sub-issues of §9):** text conversations + visibility ACL first; then push
  (needs #127); then in-thread refund binding; then images; then translation; then admin escalation
  and mute. Each is independently shippable behind the acceptance-first write model.

## 6. Completeness obligations (ADR-0032)

Every new command/event/error added by this epic also needs a **behaviour test** (+ its `rules:`
link), every new query/mutation a **story step**, every new business rule a **test** — enforced by
`make validate`. The proposal does not weaken the gate; the sub-issues carry the tests/stories/rules
as part of their DoD. New rules this introduces: private messages never reach the customer schema;
mute requires a reason; a muted participant still receives status; in-thread refund goes through the
one refund path; customer compose requires `customerChatEnabled`.

## 7. Observability (repo rule)

Contracts in `specs/observability.yaml` for: message-post acceptance→delivery, admin escalation
(operator signal), translation proxy calls (cost/latency/error), attachment upload+moderation
outcomes, and push fan-out (via #127). A mute or a translation-proxy failure is an operator signal,
never a silent degradation.

## 8. Decisions this proposal asks the product owner to make

1. **Translation cadence:** on-demand+cache (recommended) vs eager pre-translation. And: confirm a
   BFF translation proxy (our key) is acceptable, since a browser-side key is not.
2. **Image retention window** for `DELIVERY_PROOF` / `RECLAMATION` (GDPR), and whether V0-of-messaging
   ships images at all or defers them.
3. **In-thread refund scope:** does the chat action *trigger* the existing refund command (recommended)
   or only *record intent* for a human to action in the back office?
4. **Mute authorization matrix:** confirm restaurant→(customer,rider), admin→anyone, customer→none.
5. **Rider participation default:** is the rider always a participant once assigned, or only when the
   restaurant/customer pulls them in?
6. **Phasing:** confirm post-V0, and whether to land the "reserve the data model now" slice
   immediately.

## 9. Decomposition into sub-issues (created on approval)

1. **Reserve the conversation data model** (spec-only): `Conversation` identity + order-status-fold
   principle + ADR. *(shippable now)*
2. **Text conversations + visibility ACL**: `Conversation` aggregate, `postMessage`, PUBLIC/INTERNAL,
   the per-role read projection, customer-chat opt-in. SDUI thread screen on each surface.
3. **Order-status timeline fold**: `View_OrderConversation` merging status + message events.
4. **Push on new message**: bind to the #127 cascade.
5. **In-thread refund binding**: chat action → existing refund path; Stripe inbound reconciliation.
6. **Attachments**: Supabase Storage + signed upload + the three kinds + limits/retention.
7. **Translation**: BFF proxy + `MessageTranslationAdded` cache + reader-locale selection.
8. **Admin escalation + mute-with-reason**: `escalateToAdmin`, `MuteParticipant`, authz + operator signals.
9. **Quick replies**: static canned set (catalogued keys) + suggestion chips.

## 10. Considered alternatives

- **A generic (not order-scoped) chat / inbox.** Rejected for V0-shaped scope: order-scoped
  conversations get identity, ACL, retention, and the status-fold "for free" from the order they hang
  off; a standalone inbox would need its own identity, threading, and lifecycle with no product need
  yet.
- **A third-party chat SDK** (Sendbird/Stream/Intercom). Fast to bolt on, but it forks the source of
  truth (chat lives outside our event log), duplicates identity/ACL, sends customer+order data to
  another processor (a GDPR + "ethical alternative" positioning cost), and can't fold order-status
  events. Rejected for the core; a support-desk integration could still sit *beside* it later.
- **Storing message rows in a mutable table** (not events). Rejected — it would be the one write path
  that bypasses the event log, losing the audit trail, the status-fold, and replay, for no gain over
  a projected read model.
- **Server-side translation instead of client-initiated.** Kept open: the requirement says
  client-side, but the persisted-cache design makes the *initiator* (client vs server) a small
  detail — both route through the same BFF proxy and store the same `MessageTranslationAdded`. Noted
  so the implementation can choose without reopening the design.
