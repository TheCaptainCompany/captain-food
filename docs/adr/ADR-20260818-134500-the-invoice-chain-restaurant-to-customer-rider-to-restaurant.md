# ADR-20260818-134500 — The invoice chain: restaurant → customer, rider → restaurant, Captain self-bills both

**Status**: Accepted · **Date**: 2026-08-18 ·
**Decider**: the **FOUNDER / Tech CEO**, answering the six-question decision form put to him after the
whole roster was briefed ·
**Resolves**: the contradiction recorded in
[BRIEF-20260818](../legal/BRIEF-20260818-counsel-packet-and-self-answer-triage.md) §2 — the repo
asserted two opposite payment postures ·
**Relates**:
[ADR-20260808-203443](ADR-20260808-203443-tips-voluntary-contributions-funding-model.md) (the
voluntary-contribution funding model, already decided) ·
[ADR-20260818-121500](ADR-20260818-121500-the-order-carries-everything-the-invoice-needs.md) (the
order carries the tax basis) · [ADR-0017](0017-3way-stripe-connect-split.md) (the three-way split,
which this ADR reframes) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## The answers

### 1. The invoice chain — answered as neither offered option

> *"Captain generates invoices from restaurants to customers. Captain generates invoices from riders
> to restaurants."*

Two supplies, and **Captain is a party to neither**:

| Supply | Supplier | Customer | Who issues the invoice |
|---|---|---|---|
| The meal | the restaurant | the eater | Captain, on the restaurant's behalf |
| The delivery | the rider (or the partner company) | **the restaurant** | Captain, on the rider's behalf |

**The delivery is supplied to the RESTAURANT, not to Captain and not to the eater.** That is the
part neither drafted option contained and it is the load-bearing half of the answer: the restaurant
buys delivery and sells a delivered meal. Captain never buys or sells either one.

**This resolves the contradiction.** `PROP-20260726-165000:92` — *"Restaurant is its own merchant of
record; Captain never holds partner funds"* — describes the **sale**, and it is right about the sale.
The five records saying *"Captain = merchant of record"* describe the **payment mechanism**, and they
remain accurate about the mechanism. The two were never talking about the same thing, and the ADR-0017
clause *"Captain is merchant of record → no extra PSP/EMI license"* is still a non-sequitur that needs
a real instrument behind it — **that part is not resolved by this ruling and stays open.**

### 2. Money resting on Captain's Stripe balance — kept, knowingly

> **A — Keep platform charges; accept the exposure knowingly and record it.**

The buyer's total is charged to Captain's platform account and transferred out after capture.
Combined with answer 1, this makes Captain's posture explicit: **Captain collects on behalf of the
restaurant, and pays the rider on behalf of the restaurant.** That is a payment-agent posture, and
whether it falls inside or outside the payment-services perimeter is the question BRIEF-20260818 §3(c)
marks as irreducible — no research retires it.

**Recorded as knowingly accepted, on this date, with the exposure named**: if the characterisation is
wrong, the consequences are retroactive (VAT on gross rather than on the fee, an uninsurable
regulatory tail, and Stripe able to close the flow). The founder has the exposure in front of him and
has chosen to proceed. **This ADR is that record.** It is a decision, not a gap.

### 3. Rider self-billing — a separate decision from the restaurant's

> **A — V0 self-bills partner COMPANIES only, never an individual rider.**

Self-billing Avelo37 or CoopCycle as a company is ordinary business-to-business practice. Self-billing
an individual rider is a different act, and the two must not be bundled into one "self-billing"
decision — bundling is the shape that makes the exposure invisible. Consistent with the
partners-first rider sequencing in [ADR-20260818-101500](ADR-20260818-101500-the-restaurant-signs-in-by-email-link-and-638-freezes-at-chunk-1.md).

### 4. The mandate — the team drafts, the founder reviews

> **A — Team drafts the mandate and the terms structure now; founder reviews.**

**This is a work authorisation.** Self-billing requires a written agreement from each supplier before
the first invoice, and a supplier keeps the right to contest one. **No contract or terms artifact
exists anywhere in the repository today.** The team produces a draft from primary sources, marked
where it is unsure. It is a draft for the founder to check — **not a cleared document, and nothing
the team produces is clearance.**

### 5. What Captain charges a restaurant at V0 — nothing, and the model was already decided

> **C — Nothing at V0; free during the pilot.** With the founder's note: *"We already discussed about
> that. We are taking a « pari » by considering that the voluntary contributions from customers will
> cover the costs of the company. With total transparency we will communicate about the situation
> with an expense and incomes open platform. In case it's not enough the remaining balance will be
> split between all restaurants."*

He is right that it was already decided:
[ADR-20260808-203443](ADR-20260808-203443-tips-voluntary-contributions-funding-model.md) records the
voluntary-contribution (HelloAsso) model with the public « pari », cascade pricing and the cagnotte.
**Asking it again was a coordinator defect** — a decision form must be checked against the register
before it is put, and this one was not.

**Two elements of the note are NOT in that ADR and are new here:**

- **The open expense-and-income platform.** Total transparency on the company's finances, published.
  That is a **product surface** — a public page fed by real figures — and nothing in `specs/**`
  carries it. It is also a commitment: once published, it cannot quietly stop.
- **The shortfall split.** *"In case it's not enough the remaining balance will be split between all
  restaurants."* This is a **contingent liability on every restaurant**, and it changes what "free
  during the pilot" means. A restaurateur who is told the platform is free and later receives a share
  of a shortfall has been surprised on money — the failure mode this project treats as the worst
  there is. **It must be in the terms drafted under answer 4, stated plainly, before the first
  restaurant signs.** How the split is computed — per restaurant, per order, per revenue — is
  undecided and is a further decision.

### 6. The customer's receipt — the restaurant's name

> **A — The restaurant's name, issued by Captain on their behalf.** Consistent with answer 1.

## Consequences

- **ADR-0017's three-way split is reframed, not reversed.** The money still moves in one Stripe
  operation, but the *legal* shape is now two supplies: the restaurant is paid for the meal, and the
  rider is paid for a service supplied **to the restaurant**. `riderPayout` is the restaurant's cost
  settled by Captain on its behalf, not Captain's purchase. Whoever next touches the split must
  describe it that way or the ledger and the invoices will disagree.
- **`captainNet` is zero at V0** and the company is funded by voluntary contributions. Any code or
  projection that assumes a positive platform take must tolerate zero.
- **Two invoice series, not one**: one per restaurant (to eaters) and one per rider or partner (to
  restaurants). Numbering belongs to the supplier, allocated write-side, gapless, with credit notes
  for corrections — never a projector, never Odoo.
- **The terms artifact is now on the critical path**, carrying at minimum: the self-billing mandate,
  the right to contest, and the shortfall-split clause.
- **Still open, and not resolved by any of this**: whether the collect-and-pay-on-behalf posture sits
  inside the payment-services perimeter. Recorded as knowingly accepted per answer 2.

## Consulted

The whole roster was briefed on 2026-08-18 on the accounting posture and again on the self-billing
question; their findings are in the session records and in
[BRIEF-20260818](../legal/BRIEF-20260818-counsel-packet-and-self-answer-triage.md). The two-direction
ambiguity that made answer 1 necessary was found by **architect**; the "do not bundle the rider
mandate with the restaurant mandate" framing is its recommendation, which the founder's answer 3
adopts. **legal-specialist** supplied the triage and the fetched sources, and issues no clearance
here or anywhere.
