# ADR-20260720-003142 — RefundOpened: the refund queue is folded from a domain fact, not PM state

## Status

Accepted

## Context

The refund workstream had one documented read-model gap left (`docs/sagas.md`): restaurants and admins
can decide refunds (`approveRefund` / `denyRefund`) but cannot SEE the queue — there was no
`pendingRefunds` query. Queries must read `View_*` read models folded from `domain_events`
(CLAUDE.md / ADR-0035/0039) — never process-manager state tables and never raw events. But the
"a refund was opened for decision" fact existed ONLY as a RefundProcess state mutation
(`refund_process_manager.process_status = PENDING_APPROVAL`): the opening legs
(`OrderRejectedByRestaurant`, `OrderCancelledByCustomer`, `OrderCancelledByRestaurant`,
`RefundRequested`) validated payment CAPTURED against OrderTracking and wrote the row without
recording any domain event. The decision facts (`RefundApproved`/`RefundDenied`) and the settlement
(`PaymentRefunded`) already land on the Payment stream. Fold views are same-stream folds anchored on a
creation event, so a queue view needs an anchoring fact on that same stream.

## Decision

Add the **`RefundOpened` domain event** (business payload: `orderId`, `restaurantId`,
`amount: Money` — the captured total eligible; `reason?`), **delivered by RefundProcess to the
Payment aggregate** in each opening leg AFTER the payment-CAPTURED guard — so the fact is recorded
exactly when a refund genuinely enters the queue, and the queue view needs no payment-status filter.
The Payment records it idempotently (`refund_opened` in its fold / `already_records`).

**`View_PendingRefunds`** then folds the FULL refund lifecycle on the Payment stream — anchored on
`RefundOpened` (status REQUESTED) with `RefundApproved` (APPROVED, `approved_amount_cents` possibly
partial), `RefundDenied` (DENIED) and `PaymentRefunded` (REFUNDED, `refund_id`) — a "pending" consumer
filters `status = REQUESTED`. The **`pendingRefunds`** query (roles RESTAURANT + ADMIN,
restaurant-scoped for RESTAURANT) reads it via the registered `Refund` output type.

Two generator refinements landed with this (fixed in the emitter, per the fold-view idiom):

- **Money split in fold views**: a column typed `MoneyCents`/`CurrencyCode` whose `from` property is
  `entities.yaml#/Money` extracts the `amountCents`/`currency` subfield
  (`payload->'amount'->>'amountCents'`), realizing the documented `*_cents` + currency convention for
  generated views.
- **FK navigation requires both ends registered**: a view fk whose aggregate (here Payment) is not a
  registered API type emits no navigation field — previously it generated SDL/Rust references to a
  nonexistent type.

## Alternatives considered

- **Query the `refund_process_manager` state table** — rejected: PM state is private runtime state
  (ADR-20260719-193500), not a read model; violates the CQRS read-side rule and couples the API to the
  saga's storage.
- **Fold the trigger events (`OrderRejectedByRestaurant`, …) on the Order stream** — rejected: a pure
  fold cannot reproduce the payment-CAPTURED guard (rows would appear for never-paid orders), and the
  decision facts live on the Payment stream (cross-stream fold is not the view idiom).
- **A materialized projection table fed by an app projector** — unnecessary: once the fact is in the
  log, the plain fold view (projection-on-read, V0 default) serves the queue; a projector remains a
  later optimization with no API change (ADR-0005/0035).

## Consequences

### Positive
- The refund queue is served from the event log, uniformly with every other read model; the PM state
  table stays private.
- `RefundApproved`/`RefundDenied` are now projected (their `event-not-projected` warnings disappear).
- The full lifecycle stays auditable in one view; consumers choose their slice by `status`.

### Negative
- The opening legs now append to the Payment stream (one extra write per opened refund) — bounded by
  the Payment fold's idempotency under re-delivery.
- `RefundOpened.amount` duplicates the order total at opening time (frozen, like other saga facts).

### Follow-up actions
- None blocking. If the queue gets hot, materialize `View_PendingRefunds` behind the same query
  (refines ADR-0005).
