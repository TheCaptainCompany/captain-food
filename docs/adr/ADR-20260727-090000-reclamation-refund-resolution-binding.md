# ADR-20260727-090000 — Reclamation refund resolution: driving the existing refund path to settlement

## Status

Accepted

<!-- Realizes the FULL_REFUND / PARTIAL_REFUND automation flagged by ADR-20260726-163737 (#158) and
     surfaced by the architecture review (#158/#207); extends the ReclamationProcess saga introduced by
     ADR-20260726-163737 and reuses the RefundProcess of issue #25. Tracking issue #207 (epic #151),
     part A. -->

## Context

`ReclamationResolved{resolution: FULL_REFUND | PARTIAL_REFUND}` recorded the decision but did **nothing** —
the refund arm was an honest flag (ADR-20260726-163737): a single 2-way saga branch could not isolate
credit / refund / replacement, and the existing refund path requires a **separate manual `ApproveRefund`**
to move money. This ADR wires the refund arm.

The existing refund path is `RefundRequested → RefundProcess opens a PENDING_APPROVAL run (RefundOpened on
the Payment) → ApproveRefund → RefundProcess drives Stripe (RefundApproved) → PaymentRefunded settles`.
For a **claim resolution the restaurant has ALREADY decided** — the resolution IS the approval — so the
refund must settle **without a second manual click**, and **without a second money mechanism**:
`RefundProcess` must stay the one thing that talks to Stripe.

## Decision

- **Reuse `RefundProcess`, do not duplicate Stripe.** On `ReclamationResolved(FULL_REFUND | PARTIAL_REFUND)`,
  the `ReclamationProcess` saga (its hand-written wrapper seam, the same seam that owns the branch
  decision and the REPLACEMENT arm) drives `RefundProcess`'s two existing application legs **in order,
  within one reaction**:
  1. `refund::on_refund_requested` — OPENS the `PENDING_APPROVAL` run and delivers `RefundOpened` to the
     Payment (so `View_PendingRefunds` folds the queue from the log);
  2. `refund::approve_refund` — because the resolution IS the approval, immediately drives the **Stripe
     refund** (delivers `RefundApproved`, run → `APPROVED_AWAITING_SETTLEMENT`).
  The inbound `PaymentRefunded` later closes the run through the normal `on_payment_refunded` leg. This is
  "the saga issuing both commands in order"; the human `ApproveRefund` click is simply performed by the
  saga. No parallel refund path, no duplicated gateway logic.
- **Amount.** `FULL_REFUND` refunds the order's **captured total** (read cross-aggregate from
  `OrderTracking` by `orderId`, mirroring how the refund legs read order state); `PARTIAL_REFUND` refunds
  the recorded `refundAmount`. `RefundOpened` carries the full eligible total (existing semantics — the
  decision may approve less); `RefundApproved`/Stripe carry the settled amount.
- **Amount safety.** A `PARTIAL_REFUND` over the captured total is **rejected** before any Stripe call
  with the new `errors.yaml#/RefundExceedsCaptured` (`rules.yaml#/RefundResolutionCappedAtCaptured`). A
  `FULL_REFUND` uses the captured total and is always within bounds.
- **Nothing captured.** If the order has no `CAPTURED` payment, the arm is a benign no-op (there is
  nothing to refund) — the same guard `RefundProcess` already applies.
- **Idempotency.** Deterministic per `reclamationId`: a re-delivered `ReclamationResolved` finds the run
  already `APPROVED_AWAITING_SETTLEMENT`/settled (the `RefundProcess` state-row admission guard) so
  `on_refund_requested` skips and the saga does **not** re-approve; `RefundOpened`/`RefundApproved` are
  also idempotent on the Payment's own fold. A partial failure after opening leaves the run `PENDING`, so
  a retry re-approves (correct at-least-once behaviour) without a second `RefundOpened`.
- **No new domain payload / no parallel command.** The saga synthesises the open trigger in-memory to
  reuse `on_refund_requested`; the auditable facts remain `ReclamationResolved` (the origin) and
  `RefundOpened → RefundApproved → PaymentRefunded` on the Payment stream. No change to `RequestRefund` /
  `RefundRequested` payloads.

### Runtime wiring

The `ProcessManagerRunner` gains a `PaymentService` (defaulting to `FailClosedPaymentGateway`, injected
with the real Stripe gateway by the composition root exactly like the GraphQL `approve_refund` mutation),
and the `Reclamation` dispatch now hands the saga the refund-process state store, the order read model,
and the payment gateway.

## Consequences

- The refund arm is a **real money path**: `make rust` behaviour tests assert a `FULL_REFUND` settles the
  captured total, a `PARTIAL_REFUND` settles the partial, an over-total partial is rejected, and a
  re-delivery does not double-refund.
- `ReclamationProcess` now depends (at the application boundary) on `RefundProcess`'s legs — an
  intentional orchestration-of-an-orchestrator, kept in the hand-written seam because a 4-way
  credit/refund/replacement/no-op split is not expressible in the current step DSL. The generated linear
  branch still isolates only the `GOODWILL_CREDIT` grant.
