# ADR-20260831-033621 — Customer credit is disposed of as a LEG of erasure: refund what is traceable to a capture, forfeit what is purely promotional, disclose the number before the irreversible act

**Status**: Accepted · **Date**: 2026-08-31 ·
**Decider**: the **FOUNDER / Tech CEO**, directive and rulings verbatim below ·
**Register row**: [CREDIT-AT-ERASURE](../decisions/CREDIT-AT-ERASURE.yaml) ·
**Issue**: [#764 "Customer credit survives erasure: a spendable balance outlives its subject"](https://github.com/TheCaptainCompany/captain-food/issues/764) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted as a **recorded decision**, and **not as legal clearance** — see §"What this record does not
do". The disposition rules below bind the erasure-engine chunk of
[#708 "No GDPR erasure flow exists for Customer"](https://github.com/TheCaptainCompany/captain-food/issues/708);
the DSL that expresses them (grant provenance, the settlement facts, the disclosure field, the copy)
is that chunk's work and is `HOLD: human`. Nothing in `specs/**` changed in the change that landed
this ADR.

## Enforced by

n/a — no behavioral guarantee **yet**. This ADR decides the disposition; the `rules.yaml` entries
that pin it (and the ADR-0032 tests they then force) are written by the erasure-engine chunk, which
owns the stored-event-shape change. The one test that is writable **today**, against the currently
refusing surface, is named in the follow-up actions.

## Context

[#764](https://github.com/TheCaptainCompany/captain-food/issues/764) was found by the mob checkpoint
on PR #763. `CustomerCredit` is a live aggregate (`specs/payments/actors.yaml:108`) on stream
`CustomerCredit-{customerId}`, with `CustomerCreditGranted` / `CustomerCreditConsumed`, a balance
projection and a customer-visible `customerCredit` query. The erasure journey designed in
[PROP-20260829-150752](../proposals/PROP-20260829-150752-customer-erasure.md) deletes `Customer-{id}`
and crypto-shreds its keys, and **never touches `CustomerCredit-{customerId}`**. As designed, an
erasure would leave a spendable balance belonging to a person who no longer exists, on a stream keyed
by the id we just erased, with a "funds in flight" precondition that is order-scoped only and cannot
see it.

The only recorded credit-lifetime decision before this one is **expiry after one year**
([ADR-20260726-163737](ADR-20260726-163737-reclamation-saga-and-credit-ledger.md), "Credit expiry" —
*"Expire after 1 year → CHOSEN default"*). Nothing recorded what happens to a balance at erasure.
The row is now launch-relevant rather than theoretical:
[ERASURE-LAUNCH-GATE](../decisions/ERASURE-LAUNCH-GATE.yaml) made
[#708](https://github.com/TheCaptainCompany/captain-food/issues/708) launch-gating on 2026-08-29
([ADR-20260829-145848](ADR-20260829-145848-the-founders-answer-sheet-of-2026-08-29.md)), so the
first successful erasure is irreversible and strands the money for real.

## The directive, verbatim

The founder's directive is the authority for this whole record:

> business — recommends: refund credit traceable to a captured payment, forfeit purely promotional credit, and disclose the balance at the confirmation step, before the irreversible act, so the subject decides with the number in front of them. Escheat and block-until-zero were both considered and rejected — escheat invents an unowned-funds posture we have no basis for, and block-until-zero makes a legal right hostage to a marketing balance.
>
> <== follow these guidelines

The mob then surfaced three questions the directive as stated had no branch for. The founder answered
**D1 → A, D2 → A, D3 → A**.

## Decision

### The disposition, from the directive

1. **A positive credit balance is DISPOSED OF AS A LEG of the erasure** — it never parks it. Credit
   traceable to a captured payment is **refunded**; purely promotional credit is **forfeited**.
2. **The balance is disclosed at the confirmation step**, before the irreversible act, so the subject
   decides with the number in front of them.
3. **Escheat is rejected** — it invents an unowned-funds posture we have no basis for.
   **Block-until-zero is rejected** — it makes a legal right hostage to a marketing balance.

### D1 — reclamation goodwill credit is REFUNDABLE (answer A)

This is the **third category the original ruling had no branch for**, and it is **100% of the credit
that can exist at V0**: `CustomerCreditGranted` is emitted only by the `CustomerCredit` aggregate
when `ReclamationProcess` resolves a claim as `GOODWILL_CREDIT`
(`specs/ordering/processmanager.yaml:189-190`, ADR-20260726-163737). It is neither "traceable to a
captured payment" in the direct sense nor "purely promotional": it is compensation for a defective
delivery of a sale the customer paid for. It is **refundable**.

- **Refunds go to the ORIGINAL CAPTURED INSTRUMENT.**
- **Capped at the un-refunded remainder of that capture.** The double-pay case is the reason: a full
  refund plus a goodwill credit on the same claim would otherwise pay €35 against a €30 sale.

### D2 — forfeiture is a rule of ACCOUNT TERMINATION GENERALLY, not erasure-specific (answer A)

Forfeiture of purely promotional credit applies uniformly to **closure, dormancy, the existing
one-year expiry and erasure alike**. It is not a consequence attached to asking to be erased.

The reason is **GDPR Art. 12(5)**: exercising a right must be **free of charge**, and a balance
extinguished *because* someone asked to be erased is arguable as a charge. Stating the rule at the
level of account termination removes the arguable charge — the subject loses nothing by choosing
erasure that they would not have lost by closing the account.

### D3 — a failed refund PROCEEDS AND IS RECORDED (answer A)

The erasure **completes on the Art. 12(3) clock**. A refund that fails does not hold it.

- The failure lands on the **pseudonymous receipt**.
- The amount becomes an **ordinary payable**.

This is the founder's own objection to block-until-zero, applied consistently: **a money problem
never holds a right hostage.** Refusing to erase because a Stripe refund bounced is block-until-zero
wearing a different hat.

## What is NOT decided — and this record does not imply otherwise

### Three open decisions

- **D4 — sequencing.** Whether the credit leg ships **inside**
  [#708](https://github.com/TheCaptainCompany/captain-food/issues/708) or after it: **OPEN**.
- **D5 — expiry shortening.** Whether the credit expiry shortens to ~180 days so that "traceable"
  implies "refundable" **by construction**: **OPEN**.
- **D6 — spend order.** Which pot drains first when credit is spent: **OPEN**. D6 is **free only
  until a promotional grant exists** — the moment two provenances can coexist on one balance, the
  choice starts costing money and is no longer free to defer.

### What this record does not do

**The three counsel questions on
[#764](https://github.com/TheCaptainCompany/captain-food/issues/764) are NOT discharged.** Legal's
verdict on them is **0 discharged, 1 narrowed, 2 untouched**:

1. *What instrument governs the disposition of unspent credit — customer funds to be settled, or a
   revocable promotional grant?* — **narrowed** by D1/D2 (the V0 population is answered as
   refundable; the instrument question itself is not answered).
2. *Is the credit ledger itself L123-22-covered, or must it be shredded with the rest of the
   subject's personal data?* — **untouched, and now HEAVIER**: both limbs of D1/D2 produce an
   **accounting movement someone may have to prove**, so the retained-versus-shreddable question
   applies to more of the ledger than it did before this ruling, not less.
3. *Is dark-mutation-plus-manual-channel a defensible Art. 12(3) posture pre-launch?* — **untouched**.

**No lens output, and no aggregation of lenses, is legal advice or clearance** (CLAUDE.md,
ADR-20260812-143619). `decided` here records a decision taken in the founder's capacity; it does not
convert a counsel question into an answered one, and the counsel-gated rows stay counsel-gated.

## Alternatives considered

- **Escheat the balance** — REJECTED by the founder: it invents an unowned-funds posture we have no
  basis for.
- **Block the erasure until the balance is zero** — REJECTED by the founder: it makes a legal right
  hostage to a marketing balance. D3 is the same rejection applied to the failed-refund branch.
- **Forfeit everything at erasure only** — REJECTED via D2: erasure-specific forfeiture is the
  Art. 12(5) exposure, cured by stating the rule at account-termination level.
- **Tell the subject to spend the credit before erasing** — REJECTED, and it is important that it was
  considered: **`re-login-cancels` forecloses it.** The grace window is cancelled by the user logging
  back in (PROP-20260829-150752 §"re-login-cancels is the user's act"), so a customer who logs in to
  spend the balance **cancels their own erasure**. Any "use it first" copy would be a lie.

## Consulted

All six lenses were asked. Every line below is what that lens brought; a lens never asked would be
indistinguishable from a lens with nothing to say, which is why the block is mandatory
(ADR-20260812-143619).

- **legal** — **0 of 3 counsel questions discharged, 1 narrowed, 2 untouched.** Named the **Art. 12(5)
  exposure** on erasure-specific forfeiture, and confirmed it is **cured by the uniform-termination
  framing** (D2). Two retention findings: the credit ledger carries **no `legalRetention:` marker**
  (verified — `CustomerCreditGranted`, `specs/payments/events.yaml:184-195`, has none) while
  `PaymentCaptured` and `PaymentRefunded` **both carry the 10-year one**
  (`FRENCH_COMMERCIAL_BOOKS_10Y`, `specs/payments/events.yaml:41,141`); and **the refund arm creates
  a new 10-year retained record naming the subject *as part of erasing them*** — which must appear in
  `retainedUnder` on the erasure receipt, or the receipt understates what survives.
- **business** — its **own recommendation, revisited**: it had assumed the subject could simply spend
  the credit instead, and **`re-login-cancels` forecloses that**, so any "use it first" copy would be
  a lie. Named the **double-pay cap** (a full refund plus a goodwill credit on one claim pays €35
  against a €30 sale). **Abuse is not the risk** — refunds return to the paying instrument, and the
  exposure is plausibly under €50/year at V0 against an OTP ceiling of ~€360/month already accepted.
  Flags that **goodwill grants remain unbudgeted with no bearer field**.
- **vernon** — the refund/forfeit split needs **provenance on the grant**, which is a
  **stored-event-shape change** (verified: `CustomerCreditGranted` carries only `customerId`,
  `amount`, `reclamationId`). The **decision belongs in `CustomerCredit`**, not the process manager:
  the PM Tells one command, folds nothing and branches on nothing. And `GrantCustomerCredit` is a
  **`send:` step executed in the PM's own thread** (`specs/ordering/processmanager.yaml:259-261`) —
  **an unlaned foreign-stream write on the money path, live today, and NOT in C3's twelve** (C3 is
  Payment ×7, DeliveryJob ×4, Cart ×1; `CustomerCredit` is not among them —
  [ADR-20260829-230418](ADR-20260829-230418-aggregates-own-the-facts-isolation-first.md)).
- **young** — the classification must be **computed and recorded at CONFIRMATION, not at execution**,
  because confirmation is the **last moment the correlation graph is intact**: after the shred, the
  link from a grant to the capture that justifies it may no longer be walkable. The **disclosed number
  is a read** and must never be recorded as authority; **at execution the fold of
  `CustomerCredit-{customerId}` is authoritative**. **The cancel path is the hazard** — a classified,
  disclosed, then cancelled erasure must leave the ledger untouched — and note the asymmetry: **a
  forfeit is trivially undoable, a refund is not.**
- **ux-designer** — when the split is **not computable**, the disclosure block is **ABSENT, not
  zero** — and that **is the V0 launch state** until provenance exists on the grant. The number
  appears at **both request and confirmation**, with **no third affordance beside it** (no "spend it
  first", no "keep my account" escape hatch dressed as help). The French **must not soften to
  *"ne sera plus disponible"*** — that implies dormancy rather than destruction and is a
  **legal-surface defect**, not a copy preference.
- **beck** — predicts the classifier will be called with **`CustomerCreditBalanceRow`**, which
  **carries no provenance** (verified: `customer_id`, `balance_cents`, `currency`, `created_at`,
  `updated_at` — `crates/application/src/generated/rows.rs:181-187`). A default therefore applies to
  **100% of balances**, and `default ⇒ forfeit` **silently forfeits every refund owed while every
  unit test stays green**. The counter-measure is **compiler-first** (ADR-20260803-234035): a
  **parameter type that row cannot satisfy**, so the mistake is unspellable rather than caught. And
  **`TestErasureIsNotBlockedByANonZeroCreditBalance` is writable TODAY** against the refusing surface
  — because a rejected option with no test is one somebody re-adds later as a "safety check".

## Consequences

### Positive

- The erasure-engine chunk has a **disposition rule per provenance** and can stop designing around an
  unanswered question; the `#764` blocker on that chunk is lifted.
- **D2 removes the Art. 12(5) argument entirely** rather than mitigating it: there is no charge for
  exercising the right, because the rule does not mention the right.
- **D3 makes the Art. 12(3) clock unconditional**, which is the same property that made
  block-until-zero unacceptable — the record is now consistent on it.
- The **cap** in D1 closes the double-pay hole before any grant volume exists.

### Negative

- **The split is not computable at V0.** `CustomerCreditGranted` has no provenance field, so until the
  stored-event-shape change lands, the disclosure block is ABSENT (ux) and any classifier default is
  the beck failure mode. This is a **known, dated gap**, not an oversight.
- **The refund arm writes a new 10-year retained record naming the subject while erasing them**
  (legal). It is defensible and it must be **disclosed in `retainedUnder`**; it is not free.
- The **credit ledger's retention status stays counsel-open (Q2) and is now heavier**, so the
  journey's shape — retain versus shred — is still not settled by this record.
- `GrantCustomerCredit` remains an **unlaned foreign-stream write on the money path** (vernon), and
  it is outside C3's declared twelve, so the isolation programme does not currently reach it.

### Follow-up actions

- **Erasure-engine chunk (`HOLD: human`, stored-event-shape class)** — grant provenance on
  `CustomerCreditGranted`, the settlement facts, the disclosure field on the confirmation step, and
  the copy (French: destruction, never dormancy). All of it is `specs/**` and none of it was in this
  records dispatch.
- **Write `TestErasureIsNotBlockedByANonZeroCreditBalance` today**, against the currently refusing
  surface (beck). Block-until-zero was rejected twice in this record; a rejected option with no test
  is one somebody re-adds later as a "safety check".
- **Make the classifier's input type unable to be `CustomerCreditBalanceRow`** (compiler-first,
  ADR-20260803-234035), so `default ⇒ forfeit` over a provenance-free row cannot be written at all.
- **Add `CustomerCredit` to the foreign-stream-append isolation programme's scope question** — it is
  a live unlaned money-path write that C3's twelve does not cover (vernon).
- **Include a customer WITH a non-zero balance in the restore/erasure drill** — a drill over
  zero-balance customers cannot see this class at all
  ([#764](https://github.com/TheCaptainCompany/captain-food/issues/764) checklist).
- **D4, D5, D6 need register rows** — proposed keys reported to the coordinator, who declares them
  (`docs/decisions/README.md`: an executor never files an out-of-dispatch decision file).
- **The three counsel questions stay owed**, `owner: counsel`; Q2 is re-stated as heavier by this
  ruling and should be carried into the counsel packet in that form.
