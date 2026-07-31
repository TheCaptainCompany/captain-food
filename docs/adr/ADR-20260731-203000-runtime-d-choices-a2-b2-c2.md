# ADR-20260731-203000 — Runtime D choices: two-phase payment delivery, chained PM facts, event-lineage reminder triggers

**Status**: Accepted (product-owner decision, in-session 2026-07-31: "A2, B2, C2")
**Context**: [PROP-20260731-195500](../proposals/PROP-20260731-195500-runtime-d-pm-mailboxes-and-reminders.md)
(options + pros/cons + sequence diagrams), [#272 "Runtime D"](https://github.com/TheCaptainCompany/captain-food/issues/272),
[ADR-20260731-122500](ADR-20260731-122500-the-mailbox-is-the-only-door.md)

## Decisions

1. **D-A = A2 — two-phase payment delivery.** A `PlaceOrder` mailbox delivery NEVER calls Stripe
   inside the fenced completion transaction. Delivery 1 validates, prices, and commits the frozen
   checkout fast; a post-commit step calls Stripe with idempotency key = `orderId` and enqueues
   the outcome as a fact on the same order lane; delivery 2 records `PaymentIntentCreated`.
   No HTTP inside any DB transaction (peak-safe), retry-safe by construction (Stripe idempotency
   key + mailbox pk dedupe), every step a supervisable mailbox row.
2. **D-B = B2 — chained PM facts.** Stripe facts keep landing on the Payment aggregate's lane
   (recording unchanged, stream ownership untouched); the delivery's post-commit hook enqueues a
   PM-addressed copy on the order's lane (deterministic id `UUIDv5(orderId, factType)`,
   cause-chained). The saga runner retires — every reaction becomes a fenced, visible delivery.
3. **D-C = C2 — event-lineage reminder triggers.** A reminder's `schedule.when` is a list of
   `events.yaml` refs (`on: [OrderDelivered, OrderCancelled, …]`): the validator proves every
   listed event exists and belongs to the actor's lineage; the codegen emits the scheduling call
   at exactly those append sites. No prose triggers, no new predicate language.

## Consequences

- The realizing branch (fresh from `main` after
  [#270](https://github.com/TheCaptainCompany/captain-food/pull/270) merges) implements the #272
  checklist under these shapes; the PM-state store carries the frozen checkout between the two
  deliveries of A2.
- The `runtime-d-specs` scratch branch and the review-fix branch are deleted — their surviving
  content lives in the proposal, the parked PR comment, and the merged PR.
