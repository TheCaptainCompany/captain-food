# ADR-20260818-161500 — Capture on delivered dissolves the refund gap; no service fee at V0; rider pay depends on a cause nothing records

**Status**: Accepted · **Date**: 2026-08-18 ·
**Decider**: the **FOUNDER / Tech CEO**, answering the "Three Edges Of A Refund" form ·
**Applies**: [ADR-20260808-195315](ADR-20260808-195315-customer-brief-answers.md) — *"Authorise on
checkout. Capture on delivered / picked up"*, his own decision of 2026-08-08 ·
**Narrows**: [ADR-20260818-150000](ADR-20260818-150000-captain-is-the-tool-the-restaurant-carries-the-delivery.md)
— the receivable-with-no-ledger problem and the set-off clause ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## 1. There is no customer service fee at V0

The `serviceFee` line is **zero**. The customer-facing money surface at V0 is the **voluntary
contribution** ([ADR-20260808-203443](ADR-20260808-203443-tips-voluntary-contributions-funding-model.md)),
not a fee.

This also **resolves `evans`' naming contradiction** rather than deferring it: a fee is compulsory, a
contribution is not, and the funding model says contribution. There is no fee to misname because
there is no fee. The field survives in the price model as a zero; **the words around it must stop
calling it a fee to the customer.**

## 2. The rider's pay on a failed run depends on the cause — and no cause is recorded

Rider never arrived, customer absent, wrong address are different situations and get different
answers.

**The blocking finding, verified this turn**: `DeliveryCancelled` carries `reason` as a **nullable,
free-text `string`, maxLength 500** (`specs/delivery/events.yaml:86-96`), and `deliveryJobId` is the
only required field. **Free text cannot drive a payment rule.** A cause that decides whether money
moves must be a **typed, closed set** — a `DeliveryFailureCause` scalar — declared before any rule
references it, with the free-text `reason` retained alongside for the human detail.

Scope: this is Captain's own riders. For partner riders the answer lives in the **restaurant ↔
partner** contract, which ADR-20260818-150000 records Captain is not a party to.

## 3. Capture on delivered — the gap does not need absorbing, because it never opens

> *"We capture the payment on order delivered so there is no money taken from the customer or from
> the restaurant on refund if the delivery failed. We will just not capture the payment."*

**This is not a new decision.** It applies the founder's own ruling of 2026-08-08 — *"Authorise on
checkout. Capture on delivered / picked up"* — and it is the right answer, better than either option
the form offered.

**What it dissolves**, in one step:

- **No capture on a failed delivery ⇒ no refund ⇒ no gap.** The authorisation is released; nothing
  ever left the customer, so nothing has to come back from anyone.
- **The receivable with no ledger is gone.** `vernon`'s *"a liability with no ledger becomes a
  spreadsheet"* does not arise on this path: there is no negative balance to hold, and **the ledger
  aggregate the mob had scoped as a proposal is not needed for it.**
- **`legal-specialist`'s set-off clause requirement narrows.** Netting a refund off future payouts is
  set-off on the restaurant's money and needs an express written right with a cap and a contestation
  route — but **there is nothing to net when nothing was captured**, so that clause is scoped to the
  reclamation path rather than to every failed delivery.
- **"The restaurant carries it" changes shape, and improves.** On a failed delivery the restaurant
  loses the food it cooked and receives nothing. That **is** carrying it — achieved by **not
  capturing** rather than by clawing money back. No debit, no surprise on a bank statement, no
  set-off. The strongest possible version of the founder's own rule.

## What survives, and must not be lost in the simplification

Three things are narrowed, not eliminated. Naming them here so a later session does not read this ADR
as closing more than it closes.

1. **Post-delivery reclamations.** A delivery that **succeeded**, followed by a complaint — cold food,
   missing items — happens **after capture**. There, money has moved and a refund does move it back,
   so the over-cap question returns: the refund can exceed what the restaurant received once the
   delivery has been paid out. **The option space `vernon` identified is still real; it is now scoped
   to the reclamation path only**, which is far smaller than every failed delivery.
2. **The rider on an uncaptured order.** If a Captain rider is owed for a run whose order was never
   captured, that money exists nowhere in the current flow — there is no captured amount to pay it
   from. Under answer 2 it is the restaurant's cost, but the mechanism for collecting it is undesigned.
   **This is the "second bill" `business-specialist` named, and answer 3 does not pay it.**
3. **The disclosure still matters.** The restaurant's exposure on a failed delivery is now *the food
   it cooked* rather than *a debit from its account* — smaller and cleaner, but not zero. It still
   belongs on the activation screen in euros, per ADR-20260818-150000.

## Consulted

The whole roster was briefed on this material **four times on 2026-08-18** — the accounting posture,
the self-billing chain, "Captain is just the tool", and the refund edges — and their findings are
carried in ADR-20260818-134500 and ADR-20260818-150000. **No fifth round was run**, deliberately:
answers 1 and 3 **narrow** scope by applying decisions already recorded, and `holub` had already
banked that a further round on this subject would be ceremony rather than review. Answer 2 opens one
new spec need (a typed failure cause), which is ordinary work under the lifted freeze and goes through
the normal dispatch route.

**Verified this turn rather than assumed**: the capture-on-delivered ruling exists and is his
(`ADR-20260808-195315:23`), and `DeliveryCancelled.reason` is nullable free text
(`specs/delivery/events.yaml:86-96`).
