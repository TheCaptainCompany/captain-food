# ADR-20260814-141350 — Collection orders capture at READY, not at pickup (refinement of ADR-20260808-195315 §1.2)

**Status**: Accepted · **Date**: 2026-08-14 · **Decider**: the founder / Tech CEO, verbatim below ·
**Refines**: [ADR-20260808-195315 §1.2 "Capture timing"](ADR-20260808-195315-customer-brief-answers.md)
· **Refines the just-shipped behaviour of**
[#544 "Capture on delivered"](https://github.com/TheCaptainCompany/captain-food/issues/544) /
[PR #545](https://github.com/TheCaptainCompany/captain-food/pull/545) ·
**Register**: [DECISIONS](../proposals/DECISIONS.md) §41 (CAP-READY) ·
**Session**: https://claude.ai/code/session_01XTrJE7m5TGkKRRPKj5ZFqZ

## The directive (verbatim, 2026-08-14)

> "For the pickup order the payment captured must happen when the order is prepared don't you think?"

## What this refines

[ADR-20260808-195315 §1.2](ADR-20260808-195315-customer-brief-answers.md) recorded capture timing
**per service type**, in the customer's own words:

> "Authorise on checkout. Capture on delivered / **picked up** / paid in advance for at-table service."

The just-shipped [#544/PR #545](https://github.com/TheCaptainCompany/captain-food/pull/545)
implemented capture on the **one** handover fact the Order lifecycle exposes — `OrderDelivered`
(READY→DELIVERED) — for BOTH service types, because collection reuses that same terminal fact
(`markOrderDelivered`, `specs/ordering/actors.yaml:80,132`; `OrderDelivered` description "delivered /
picked up", `specs/ordering/events.yaml:242-243`). The current process manager captures on
`OrderDelivered` for both types (`specs/payments/processmanager.yaml:33-63`).

The founder now **refines the "picked up" leg**: for a COLLECTION order, capture at **prepared /
ready**, i.e. one lifecycle step earlier than the pickup fact.

## Decision

**Capture triggers per service type, at each type's LAST controlled moment before the order can
become un-collectable:**

- **DELIVERY** → capture on `OrderDelivered` (the rider drop; unchanged from #545).
- **COLLECTION** → capture on **`OrderMarkedReady`** (READY; the kitchen has finished and the food is
  waiting on the counter). This is EARLIER than the pickup fact.
- **AT-TABLE** → advance capture at checkout (recorded in §1.2, still unbuilt; no `AT_TABLE`
  `ServiceType` exists — `specs/common/scalars.yaml:271-273` = `[DELIVERY, COLLECTION]`).

### The pinned COLLECTION event: `OrderMarkedReady`

There is **no distinct "prepared" event between preparing and ready**. The prep events are
`OrderPreparationStarted` (`specs/ordering/events.yaml:199`, prep START → PREPARING) and
`OrderMarkedReady` (`specs/ordering/events.yaml:229`, prep FINISH → READY, "ready for
pickup/delivery"). "Prepared / ready" in the directive **is** `OrderMarkedReady` — the READY state
(`specs/ordering/actors.yaml:79`). That is the event the COLLECTION capture keys on.

### Why this is the *consistent* rule, not a special case

Capture at the restaurant's LAST CONTROLLED moment before handing the order to a party who could make
it un-collectable:

- Delivery's last controlled step is the rider drop, so capture-on-`OrderDelivered` is capture at that
  moment.
- Collection has **no fulfilment step after the kitchen finishes** — collection is the *customer's*
  action, not a platform step. So READY is collection's last controlled moment.

Capturing a collection order at "picked up" would leave the restaurant exposed to cook-then-no-show:
the food is made and paid for by no one until an action the platform does not control occurs.
Capture-at-READY for collection is therefore MORE symmetric with capture-on-delivered for delivery,
not less.

## Consequences

- **Empty log → additive, no migration.** No `PaymentCaptured` for a collection order has been
  recorded against `OrderDelivered` in production (the log is empty; #544 landed inside the empty-log
  window). Moving the collection trigger to `OrderMarkedReady` reshapes no stored event and needs no
  upcast.
- **The release/refund partition shifts for one narrow collection case.** Today a READY collection
  order is still AUTHORIZED, so a restaurant cancellation from READY
  (`specs/ordering/actors.yaml:83`, `from: [ACCEPTED, PREPARING, READY]`) RELEASES the hold
  (void, `PaymentSettlementProcess` AUTHORIZED arm). After this change the READY collection order is
  already CAPTURED, so the same cancellation routes to **REFUND** (`RefundProcess`'s CAPTURED arm),
  not release. This is correct — money moved — but it is a real behaviour change worth pinning in a
  test. The customer cannot cancel from READY (`CancelOrderByCustomer` is `from: [PLACED]` only,
  `specs/ordering/actors.yaml:82`), so restaurant-cancel is the only pre-collection abort path.
- **No-show is now the restaurant's protection, by design.** A collection order captured at READY and
  never collected stays CAPTURED — the restaurant keeps the money for food it made. That is the point.
- **The AR-2 authorization guard is untouched.** Capture keys on the presence of a Captain
  authorization (payment_intent_id present AND payment_status AUTHORIZED), never the lifecycle fact
  alone — `$0` replacements and future external orders remain structurally skipped
  (`rules.yaml#/PaymentCapturedOnFulfilment`, ADR-20260813-233418 AR-2).
- **At-table advance-capture arm is unaffected** and stays unbuilt, with the recorded build constraint
  (materialize on `PaymentCaptured`-from-`PENDING`) still owed when it lands
  ([DECISIONS §38 carry-forward](../proposals/DECISIONS.md)).

## The legal caveat (honest — an obligation map, NOT clearance)

Capturing a collection order at READY takes payment **before possession transfers** (collection). No
lens output, and no aggregation of lenses, is legal advice or clearance (ADR-20260812-143619). The
verdict of the legal lens is that this is a **lawful prepayment** shape, not a genuinely problematic
one — prepayment before possession is ordinary commerce (click-and-collect, deposits), and the
customer already consented at checkout to "authorize now, charge on fulfilment"; this refinement only
fixes what "fulfilment" means for collection. It is therefore recorded as decided. But it **sharpens
two already-open counsel questions**, both build constraints and neither a blocker to the decision:

- **CAP-3 (L221-5 disclosure).** For a collection order the pre-contract disclosure must now state
  that the charge occurs when the order is **READY** — before the customer collects — not at
  collection. The existing disclosure question is sharpened by the earlier charge moment.
- **CAP-5 (VAT tax-point).** Capture (encaissement) now **precedes possession** for collection, so the
  tax-point is no longer coincident with the physical handover it was coincident with for delivery.
  The unbuilt VAT-receipt engine ([#174](https://github.com/TheCaptainCompany/captain-food/issues/174))
  must resolve, per counsel, whether the fait générateur / exigibilité for a collection sale attaches
  at READY (encaissement) or at collection (delivery of goods), and key the receipt accordingly.

Both are recorded in
[BRIEF-20260814-capture-on-delivered-counsel-packet](../legal/BRIEF-20260814-capture-on-delivered-counsel-packet.md)
as a CAP-5/CAP-3 collection addendum. They gate the unbuilt receipt engine and the checkout
disclosure copy, not this capture-trigger decision.

## Follow-up (a fast-follow to #544, NOT claimed here)

The `PaymentSettlementProcess` capture trigger must branch per service type (see the tracking-issue
text drafted in the 2026-08-14 architect run report). This ADR records the decision only; an executor
implements it after, under the ordinary gates. `PaymentCapturedOnFulfilment`'s rule text (which today
reads "OrderDelivered — the one fact for DELIVERY and COLLECTION alike",
`specs/common/rules.yaml:16`) is updated in the same slice — that spec edit implements *this recorded
decision*, so it is not itself a decision reversal.

## Consulted (ADR-20260812-143619 — one line per lens)

- **architect**: The rule is now internally consistent — capture at each type's last controlled moment
  — and it is additive on an empty log. The one thing that must not be lost is the release→refund
  shift for a READY-then-cancelled collection order; pinned it in the ADR and the issue.
- **payments**: Feasible and idempotent by construction — `OrderTracking` already carries
  `service_type` (`specs/database/tables/projection_tables.yaml:519-520`) and
  `DeliveryDispatchProcess` already branches on it from `OrderMarkedReady`
  (`specs/delivery/processmanager.yaml:29,38-39`), so the same guard shape is proven. A collection
  order that later reaches `OrderDelivered` is no longer AUTHORIZED, so the delivered arm's guard skips
  — no double capture. Nothing else in my lens.
- **business**: Net positive. Cook-then-no-show is the exact loss capture-at-ready removes, and
  collection no-show is materially higher than delivery no-show. The only downside — customer charged
  before collecting — is fair because the food was made for them, and it is what every click-and-collect
  merchant already does.
- **legal-specialist**: Defensible as a lawful prepayment, not a blocker — recorded as decided. But it
  moves the charge before possession for collection, which sharpens CAP-3 (disclosure: charged at
  ready) and CAP-5 (tax-point decoupled from handover). Neither clears without a French avocat; no lens
  output is clearance. The receipt engine and the checkout copy carry the constraint.
- **dba**: Empty log, additive trigger, no migration — the versioning story is trivial here. No stored
  `OrderDelivered`-driven collection capture exists to upcast.
- **ux-designer**: The checkout disclosure copy must say, for collection, "charged when your order is
  ready" — a real string change that rides the receipt/disclosure work, flagged so it is not dropped.
- **graphql-architect / observability**: Nothing in my lens (the settlement-funnel `bam` projection
  already owed under [DECISIONS §38](../proposals/DECISIONS.md) will simply see the capture edge move
  earlier for collection).
