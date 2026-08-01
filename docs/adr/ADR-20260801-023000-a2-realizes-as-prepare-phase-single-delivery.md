# ADR-20260801-023000 — A2 realizes as a PREPARE phase + single fenced delivery (R2)

**Status**: Accepted (product-owner decision, in-session 2026-08-01: "Go R2") — REFINES
[ADR-20260731-203000](ADR-20260731-203000-runtime-d-choices-a2-b2-c2.md) D-A: the A2 STRATEGY
(no HTTP inside any transaction; retry-safety via Stripe idempotency key = orderId; every step
supervisable) stands; its realization is decided against the actual runtime primitive.
**Context**: [PROP-20260731-195500](../proposals/PROP-20260731-195500-runtime-d-pm-mailboxes-and-reminders.md)
§Decision D-A (both realizations as sequence diagrams),
[#272](https://github.com/TheCaptainCompany/captain-food/issues/272) /
[PR #273](https://github.com/TheCaptainCompany/captain-food/pull/273) D1.

## Decision

`PlaceOrder`'s mailbox delivery runs in TWO phases inside ONE delivery:

1. **PREPARE (no transaction open)**: validate the cart and price it via pool reads; on a
   deterministic rejection (CartEmpty, PriceMismatch, …) skip to the fenced commit with the
   REJECTED verdict. Otherwise call Stripe to create the PaymentIntent — idempotency key =
   `orderId` — still with no transaction open.
2. **FENCED COMMIT (the existing complete_fenced)**: record `PaymentIntentCreated` + open the PM
   row + flip the mailbox row's verdict atomically. A synchronous Stripe DECLINE commits as
   `REJECTED PaymentDeclined` — the SAME operationStatus rejection the legacy path produces.

A crash between the Stripe call and the commit leaves the row RECEIVED; redelivery re-runs
prepare and the idempotency key returns the SAME intent — no duplicate.

## Rejected: literal two-delivery A2 (R1)

The runtime's `complete_fenced` runs the handler inside the completion transaction with a sync
post-commit closure, so literal A2 needs a spawned gateway leg and a second row — and by then
the PlaceOrder row is terminal-SUCCEEDED, so a sync decline can only surface on `paymentStatus`:
a CLIENT-CONTRACT change (the legacy `PaymentDeclined` operation rejection disappears). Rejected
for exactly that: the whole resolver flip's invariant is that clients never notice the runtime
changed.

## Consequences

- The runtime gains a `prepare` phase primitive (host handler API: prepare → handle-in-tx);
  the PM-state hand-off and the second mailbox row of literal A2 are not built.
- The appendix `PlaceOrderProcess.throws` (incl. `PaymentDeclined`) stands as specced.
- D1 is UNBLOCKED; the flip still ships GATED (gate-then-stabilize), and `command_journal`
  retirement sequences after the default flip as already noted.
