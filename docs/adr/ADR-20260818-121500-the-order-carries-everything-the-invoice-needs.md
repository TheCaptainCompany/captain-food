# ADR-20260818-121500 — The order carries everything the invoice needs, VAT included: a defect against a principle already recorded

**Status**: Accepted · **Date**: 2026-08-18 ·
**Decider**: the **FOUNDER / Tech CEO**, restating a principle he had already given ·
**Restates, does not create**: [ADR-20260808-234907](ADR-20260808-234907-answer-sheet-confirmations-dqw1-option-b.md)
(D-QW1 option (b) — **self-contained events**) and
[ADR-20260719-014434](20260719-014434-checkout-snapshot-on-paymentintentcreated.md)
(`PaymentIntentCreated` carries a self-contained checkout snapshot) ·
**Relates**: [BRIEF-20260818](../legal/BRIEF-20260818-counsel-packet-and-self-answer-triage.md) §5,
which found the gap · CLAUDE.md non-negotiable rule (2) — a stored shape change is a **migration** ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## The ruling, verbatim

> *"The order must have all the information it needs to generate the invoice.*
> *It's a principle I already said and seeing that the vat is not inside the order does not respect
> this principle.*
> *And yes the order must have the VAT info."*

## He is right that it was already said, and that matters for how this is handled

This is **not a new decision**. The self-contained-fact property is recorded: D-QW1 option (b) chose
self-contained events over reference-and-resolve, `PaymentIntentCreated` already carries a
self-contained checkout snapshot precisely so a later reader needs no other stream, and
[PROP-20260726-170000](../proposals/PROP-20260726-170000-event-log-integrity-evolution-and-erasure.md)
names *"the 'events are self-contained facts' property"* as a thing a proposed change would break.

So the correct framing is **a defect against an adopted principle**, not a decision reversal and not
a new option space. Nothing needs re-deciding; something needs fixing, and the reason it is being
fixed is already in the register.

## The measured gap

Found by `architect` and confirmed by `legal-specialist`, verified against the tree:

- `TaxRate` exists (`specs/common/entities.yaml:76`, per service mode, mirroring HubRise) and
  `TaxRatePercent` (`specs/common/scalars.yaml:240`).
- It hangs off **mutable current state only** — the catalog item (`specs/catalog/entities.yaml:113`)
  and the restaurant-account default (`specs/network/entities.yaml:103`).
- Grepping `tax` across `specs/ordering/entities.yaml`, `specs/ordering/events.yaml`,
  `specs/payments/*.yaml` and `specs/database/tables/projection_tables.yaml` returns **zero hits**.
  `OrderLineItem` (`specs/common/entities.yaml:135-164`) carries `unitPrice`, `lineTotal`,
  `selectedOptions` — **no rate, no tax amount**. `PaymentBreakdown` has eight `Money` fields and no
  tax decomposition.

**The consequence, stated as the failure rather than the omission**: an accounting fold over
`domain_events` would have to join **today's** catalog to price **yesterday's** VAT. A menu reprice
or a rate change silently rewrites history. That is a non-replayable projection, and for a statutory
ledger it is a defect, not a trade-off — the rate that applied is a **fact about the sale**, not a
property of the product today.

## What follows

- **The order's stored shape gains its tax basis**: per line, the rate that applied and the tax
  amount; at order level, a tax decomposition on the breakdown. The rate is **frozen at the moment
  of sale**, like every other price in the snapshot.
- **It is a migration** (CLAUDE.md rule 2, stored event shapes): the versioning story is recorded
  before it lands, stored events are immutable, upcasting never mutation. Class **`HOLD: human`**.
- **Verify, do not assume, that no instances exist.** Production is suspended and
  [DECISIONS §45 PROD-1](../proposals/DECISIONS.md) records that there is no real end user — if
  `domain_events` holds no order instances, the versioning story is short. **That is a fact to
  establish, not a convenience to presume**, and it is the whole difference between a rename and an
  upcaster.
- **The engine is not the expensive part.** `business-specialist`: the real cost is **collecting the
  correct rate per menu item from every restaurant at onboarding** — France applies more than one
  food rate and takeaway and alcohol split it further. That is a funnel cost, not a software cost,
  and **if it is not collected no compliant receipt can be issued at all.** The onboarding design
  owes a field before the emitter owes a column.
- **This narrows a precondition the repo framed too broadly.** Per BRIEF-20260818 §1 (fetched
  source), **B2C invoices are out of scope of the e-invoicing reform entirely** — so the customer
  receipt is CGI 289 / annexe II 242 nonies A, the €25 TTC *note* rule, and e-reporting. The
  e-reporting feed is built from **our** order and payment data, which is exactly the data this ADR
  says the order must carry. The two obligations meet on the same field.

## What this does NOT decide

Whose supply the food and the delivery fee are — that follows from the merchant-of-record question,
which BRIEF-20260818 §2 shows the repo currently answers **two opposite ways**. The order must carry
the tax basis either way; **which party's VAT it is** is the open question, and it is in the founder's
decision queue.

## Consulted

The whole roster was briefed on 2026-08-18 on the accounting posture; this ADR records a founder
restatement made during that briefing. The gap was found by **architect** (the fold would join
today's catalog to price yesterday's VAT), confirmed independently by **legal-specialist** (zero
`tax` hits across ordering and payments), with the onboarding-cost consequence from
**business-specialist** and the freeze-at-sale framing from **young**'s standing position that a
stored event is an immutable historical fact.
