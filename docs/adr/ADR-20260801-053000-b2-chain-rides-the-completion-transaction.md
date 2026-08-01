# ADR-20260801-053000 — B2 realizes in-transaction, with per-fact chain identity

**Status**: Accepted — REFINES [ADR-20260731-203000](ADR-20260731-203000-runtime-d-choices-a2-b2-c2.md)
D-B (B2) at the two points its sketch left open, decided against the actual runtime while
realizing [#272](https://github.com/TheCaptainCompany/captain-food/issues/272) D1
([PR #273](https://github.com/TheCaptainCompany/captain-food/pull/273)).
**Context**: [PROP-20260731-195500](../proposals/PROP-20260731-195500-runtime-d-pm-mailboxes-and-reminders.md) §Decision D-B.

## Decision

1. **The chain hop rides the RECORDING transaction, not a post-commit hook.** B2's sketch said
   "the delivery's post-commit hook enqueues a PM-addressed copy"; realized, the Payment lane's
   fenced completion transaction itself inserts the copy. A post-commit hook leaves a crash
   window in which the payment fact is durable but its saga hop is lost — exactly the
   recorded-payment-nobody-acts-on failure the mailbox exists to close. In-tx, the record and
   the hop commit or roll back together; only the post-commit NUDGE (latency, not correctness)
   stays outside.

2. **Chain identity is `UUIDv5(orderId, "{factType}:{causing mailbox row id}")`**, not the
   sketch's `UUIDv5(orderId, factType)`. The causing row's id is itself deterministic
   (`UUIDv5(source:external_id)`), so the chained identity stays stable under webhook
   redelivery — but two DISTINCT same-type facts on one order (a second attempt's
   `PaymentFailed`, a second partial refund's `PaymentRefunded`) each keep their own hop, where
   the sketch's key would silently swallow the second.

## Consequences

- A crash between recording and chaining is impossible by construction; the lost-hop class of
  incident cannot exist.
- The saga runner's Stripe-fact triggers retire behind the same `PM_MAILBOX_DELIVERY` gate
  (PlaceOrderProcess whole; RefundProcess keeps its refund-OPENING order-fact legs — they are
  outside D-B's Stripe-facts scope and stay on the runner until their own runtime item).
- The proposal's D-B section is rewritten to this realized shape (living document,
  ADR-20260801-020000).
