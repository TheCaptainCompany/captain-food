# ADR-20260809-002500 — Quick-wins option-(b) diff approved as written; D6 endpoint decided as the step-DSL extension (build it now)

**Status**: Accepted · **Date**: 2026-08-09 · **Decider**: the customer (product owner), via the
interactive decision brief's answer sheet · **Tracking**:
[#348 "Epic: the rider/delivery write surface does not exist (24 of main's 32 validator warnings)"](https://github.com/TheCaptainCompany/captain-food/issues/348)

## The answers

- **Card 9 — "Approve rewritten diff as written."** The option-(b) rewrite of
  [PROP-20260808-233000](../proposals/PROP-20260808-233000-customer-anxiety-quick-wins-spec-diff.md)
  is approved as exact text: `orderId` joins the four delivery payloads (required on the three
  single-emitter events, nullable on `DeliveryStatusUpdated` by the orphan doctrine), OrderTracking
  gains `DeliveryPickedUp` in `fedBy` + the `delivery_status` lineage, the checkout FAILED state
  and its four translation keys land, and the application carries the §7 sweep — including the
  projection worker's stream widening to the full `DeliveryJob-%` family, without which the fix is
  spec theater. Vehicle: apply-now by the run (card 5, confirmed twice).
- **Card 10 — "(iii) build the DSL extension now"** (audit finding A3). The declared `sends:`
  annotation is NOT the endpoint: the step DSL gains real conditional branching so the
  ReclamationProcess's 3-way resolution (REFUND / REPLACEMENT / no-op) is EXPRESSED in the spec,
  the hand-written wrapper seam retires, and the validator sees the dispatch instead of trusting a
  declaration. This is ADR-20260808-235113 applied to the repo's own DSL. The customer's note
  ("Let's discuss") is honoured by sequencing DESIGN FIRST: the architect prepares the
  DSL-extension proposal (syntax options with pros/cons, sequence diagrams, migration of the
  existing seam), which is the discussion surface; implementation follows approval.
- **Cards 1–8 re-confirmed** (unchanged); **card 11 (identity-bridge home) stays OPEN** at the
  customer's request — slice 3 [#415](https://github.com/TheCaptainCompany/captain-food/issues/415)
  is NOT dispatched until it is answered, so no option gets embedded by default.

## Consequences

- The quick-wins application is claimed and executed under the normal protocol (claim → draft PR →
  gates → independent review → ready+auto-merge), touching `specs/**` under this recorded
  exact-text approval.
- The D6 `sends:` mechanism is **not** implemented as previously queued: slice 2's validator work
  narrows to the PM-send credit (`BindCartToCustomer`, `GrantCustomerCredit` — no spec change), and
  `PlaceReplacementOrder`'s coverage now waits on the DSL extension rather than on an annotation.
  PROP-20260808-141817's D6 row and the DECISIONS.md "D6 endpoint" row are updated to record this.
- ADR-20260808-234907's card-2 approval of `sends:` is **superseded on the endpoint question** by
  this decision: `sends:` was approved as the mechanism when the alternative was doing nothing;
  the customer has now chosen the expressible-in-spec final shape.
