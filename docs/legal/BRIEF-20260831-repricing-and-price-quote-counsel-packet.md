# BRIEF-20260831 — Repricing and the priced quote token: the obligation map

**Date**: 2026-08-31 · **Lens**: `legal-specialist` ·
**Occasion**: the design of the priced quote token
([PROP-20260831-134539](../proposals/PROP-20260831-134539-priced-quote-token.md), tracking
[#816 "Display/charge divergence is undetected: the expectedTotal equality check never runs in production"](https://github.com/TheCaptainCompany/captain-food/issues/816)),
which asks what the law requires when the price that was **displayed** and the price that would be
**charged** disagree ·
**Session** (of the recording run): https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

> **NOTHING IN THIS FILE IS LEGAL ADVICE OR CLEARANCE**, and no aggregation of it becomes clearance
> (ADR-20260812-143619, CLAUDE.md). **No counsel is engaged** — the founder, 2026-08-31: *"Not
> scheduled. We are on our own for now until the product is ready."* That constraint changes **which
> questions the team can retire itself**. It does not convert an open question into a closed one.
> Where the honest answer is "this stays open and here is what it risks", that is what is written.

> **PROVENANCE — and it is itself a finding.** The obligation map below is the `legal-specialist`
> lens's return of 2026-08-31. It reached the coordinator and **never reached the repository**:
> `grep -rn "QT-1\|QT-10" docs/ specs/` returned **0 hits** at the time this file was written. It is
> **relayed here by the coordinator from a lens return that was not otherwise persisted** — it is
> the lens's analysis, not the coordinator's, and it must not be read or cited as the coordinator's.
> This is the second occurrence of the same defect class in two weeks
> ([BRIEF-20260818](BRIEF-20260818-counsel-packet-and-self-answer-triage.md) records the first: a
> ten-question packet summarised in one line of an ADR and never landed), and it is a coordinator
> defect under the rule that GitHub and the session are never the record.
>
> **What that costs, stated plainly.** The executor writing PROP-20260831-134539 was handed the
> labels `B1–B5` and `QT-1…QT-10` but not their text. It **correctly refused to reproduce them from
> memory** — inventing a lens return is worse than declaring the gap — and left an unchecked
> `Concerns` entry, which mechanically blocks that proposal from `Approved`. **This file exists to
> discharge that entry**; §6 reconciles the numbering.

---

## 0. Source discipline used here

Same discipline as [BRIEF-20260818 §0](BRIEF-20260818-counsel-packet-and-self-answer-triage.md).
Each claim carries **FETCHED** (a URL retrieved, named), **PRIMARY-READABLE** (a named instrument
anyone can read for free), or **VERIFY-FIRST** (reasoning from training, cutoff **2026-05**, not
checked against a source). **Do not quote a `VERIFY-FIRST` line as established.**

**Nothing in this brief was FETCHED. Every article number below is VERIFY-FIRST — including where
the RULE itself is graded (a).** The grade attaches to *the obligation existing*, never to *the
numbering being right*. The egress result of the session that produced this map:

| Source attempted | Result |
|---|---|
| `legifrance.gouv.fr` | **HTTP 403 — egress-policy denial** |
| `economie.gouv.fr` | **HTTP 403 — egress-policy denial** |
| `eur-lex.europa.eu` | **HTTP 202 with a zero-byte body**, on two different URL forms |

The same denials are recorded on
[#817](https://github.com/TheCaptainCompany/captain-food/issues/817) and in
[PROP-20260831-134539 §8](../proposals/PROP-20260831-134539-priced-quote-token.md).

**Two things in this map are exactly the facts most likely to have moved since the cutoff**: the
**Omnibus transposition** references (the French ordonnance and its codified articles) and the
**currency of the food-service *arrêté***. Verify those two first.

**Grades**: **(a)** the rule exists and is not in doubt · **(b)** the rule exists, its application
here is arguable · **(c)** no obligation identified, recommended anyway.

---

## 1. The instruments — the obligation map

| Instrument | Requirement here | Grade |
|---|---|---|
| C. conso. **L112-1** + the general *arrêté* on consumer price information | the displayed price is the **total to be paid, TTC, all charges included**; the price announced is the price practised | **(a)** principle / **(b)** which *arrêté* |
| C. conso. **L221-5** (→ **L111-1**) | pre-contractual information *"avant que le consommateur ne soit lié"*, the total price among it | **(a)** |
| C. conso. **L221-14** (CRD **2011/83 Art. 8(2)**) | immediately before ordering, a **clear legible recap of the total**, and a button unambiguously stating the **obligation to pay**. **Sanction: the consumer is not bound** | **(a)** exists / **(b)** applied |
| **CRD Art. 22** (C. conso. ~**L221-16**) | any **additional payment** requires **express consent**; consent inferred from a default the consumer must reject is void and reimbursable | **(a)** rule / **(b)** is a reprice an "additional payment" |
| C. conso. **L121-2 / L121-3 / L121-4** | charging above, or silently substituting, an announced price. **L132-2: up to 2 years and €300 000, raisable to 10% of turnover** | **(a)** offence exists / **(b)** isolated corrected mismatch |
| C. civ. **1127-1 / 1127-2** (*double-clic*) | the contract forms on the consumer's confirmation after seeing the detail and the total | **(a)** / VERIFY-FIRST numbering |
| C. conso. **L212-1 + R212-1** | a term reserving the right to **unilaterally modify the price** is on the blacklist | **(a)** rule / **(b)** exact item |
| C. conso. **L221-13** | contract confirmation on a **durable medium** carrying the L221-5 information — a repriced order's confirmation must carry the **final** price | **(a)** / VERIFY-FIRST |
| C. conso. **L221-28 3°** | perishables are exempt from withdrawal — **no 14-day escape hatch** to cure a bad reprice | **(a)** |
| **Omnibus / Dir. (EU) 2019/2161** (Fr: ord. **2021-1734**) | prior-price / 30-day rule on *price-reduction announcements*; personalised-pricing disclosure; raised penalties | **(b)** |
| **DSA Reg. (EU) 2022/2065 Art. 25** | no interface that deceives, distorts or impairs a free and informed decision | **(a)** rule / **(b)** applied |
| **P2B Reg. (EU) 2019/1150 Arts. 3, 3(2)** | holding a restaurant to a withdrawn price is a **term**, needing plain-language disclosure and 15-day notice | **(b)**, gated on the funds posture |
| **CRD Art. 6a** (Fr ~**L111-7-1**) | if Captain is an intermediary: state **who the consumer contracts with, and how obligations are shared** | **(b)** — **absent from the repo entirely** |
| **CGI 289 / annexe II 242 nonies A** | the receipt shows **base HT per rate, rate applied, tax amount** | **(a)** |

### On which *arrêté* reaches an online storefront — **(b)**

The lens's view: the classic restaurant text (*arrêté* of **27 March 1987**, *affichage* in
establishments serving meals — VERIFY-FIRST on date and currency) is drafted around the **physical
establishment**. An online storefront is more naturally reached by the **general L112-1 *arrêté***
on consumer price information plus the **distance-selling regime**.

**Do not assert to counsel that the restaurant *arrêté* does not apply online.** Ask it — **QT-2**.

---

## 2. Contract formation — the load-bearing conclusion

Two characterisations are available:

1. **The storefront is an *offre*, the click is acceptance.** C. civ. **1127-2** fixes formation at
   the confirming click. An **upward reprice afterwards is then a unilateral modification of a
   formed contract**, needing mutual consent (C. civ. **1193**) — which **no interstitial fixes**,
   because the interstitial is offered after the contract already exists.
2. **The storefront is an invitation to treat**, and `AcceptOrder` (the restaurant's acceptance) is
   the acceptance that forms the contract.

**Characterisation (2) is not freely available**, for three reasons:

- **The design *looks* like (1).** A pay button, an authorization hold, a confirmation screen, and
  **no unilateral customer cancel at `PENDING`**. What the interface tells the consumer is what
  counts, not what the CGU asserts.
- **The CGU term that would buy (2) is itself on the R212-1 blacklist** — a term stating "the
  contract forms only on restaurant acceptance", while the consumer cannot withdraw during that
  window, is precisely the shape the blacklist targets. That is **QT-1**.
- **L221-14's sanction runs the other way.** The remedy for a defective pre-contract confirmation is
  that *the consumer is not bound* — the regime is built to bind the trader early and the consumer
  late, not the reverse.

**State plainly, and this is the conclusion the design must be built on: the binding price is the
one displayed at the confirming click, not the price at restaurant acceptance.**

**Then design past it.** After the confirming click the checkout may charge **exactly the quoted
amount, or REFUSE** — never more. A staleness check is a **pure refusal**, never an auto-accept.
That is safe under **both** characterisations, which is what lets the epic ship without waiting on
QT-1.

---

## 3. The disclosure floor — what a reprice screen must contain

| Element | Grade | Note |
|---|---|---|
| **The new total TTC** | **(a)** | Not optional under any reading |
| **A fresh POSITIVE act** — re-arm the pay button, never pre-arm it | **(a)** rule / **(b)** characterisation | CRD Art. 22 express consent |
| **Silence-as-consent, or a countdown that auto-accepts** | **NOT AVAILABLE** | CRD Art. 22 voids consent inferred from a default the consumer must reject; DSA Art. 25 makes a countdown-to-charge a textbook manipulative pattern |
| **Old total + the delta** | **(b)** — **not compelled by any article the lens can name** | But silently substituting a higher figure into the same UI slot is the omission shape of L121-3. **Recommend it as a deliberate honesty decision, not as a cited obligation — do not overclaim it** |
| **The reason, and which lines moved** | **(c)** on obligation, **recommend anyway** | It is the artifact you want in a dispute |

---

## 4. Direction asymmetry — up and down are not symmetric

**Upward.** The whole regime targets it. Express positive re-consent — **(a)**.

**Downward.** **No consumer-protection article is breached by charging less** — **(b), and it must
be marked as an argument from absence**, which is the weakest kind and the kind most likely to be
wrong for a reason nobody in this repo has thought of.

Downward still moves risk to three other places:

1. **The restaurant.** Charging the *quoted, higher* price when the live price has fallen inside the
   window — **QT-5**.
2. **The receipt and the VAT decomposition** (§5).
3. **Symmetry as evidence.** An asymmetry that **favours the consumer** is defensible *because it
   tracks harm*. The reverse reads as designed-for-the-platform under **DSA Art. 25**.

---

## 5. Absorbing the difference — it dissolves one question and opens harder ones

Consumer-side, absorbing the delta **largely dissolves** the disclosure question — **(b)**.
Elsewhere it creates worse ones.

**It breaks a stored invariant.** `specs/common/entities.yaml:21` states
`total = articles + delivery + serviceFee`, with `articles` described at `:29` as *"Food sub-total
TTC (100% to the restaurant, minus its service contribution)"*. Absorbing needs a **new line** ⇒ a
**stored event shape** ⇒ a **migration** with its versioning story recorded before it lands ⇒
**`HOLD: human`** (CLAUDE.md rule (2)).

**And the VAT characterisation decides both the rate and the taxpayer** — three candidate shapes,
and they do not land in the same place:

| Characterisation | Effect |
|---|---|
| **Rabais** | Reduces the **food base**, at the **food rate** |
| **Third-party payment / subvention complément de prix** | Enters the **restaurant's** base, at the food rate (VAT Dir. **Art. 73** / CGI **266**) |
| **Captain reduces its own fee** | Reduces **Captain's** 20%-rated service base; the food line is untouched |

**Lens recommendation: absorb by reducing Captain's own fee.** With the cap stated: that is
**blocked on [`docs/decisions/CAPTAINNET-ZERO.yaml`](../decisions/CAPTAINNET-ZERO.yaml)** (open,
**founder-owned**, *"Is captainNet zero, or is it exactly the contribution?"*) — **if `captainNet`
is zero there is nothing to absorb from**. **Do not open a new register row for this**; it resolves
into that one.

**A receipt showing a total different from the price list is compliant provided it decomposes** —
list price plus an identified reduction, **per rate** (CGI annexe II **242 nonies A**).

---

## 6. Retention and evidence

**The trader bears the burden of proving the pre-contractual information was given** (C. conso.
~**L221-7 / L221-11**, VERIFY-FIRST).

[ADR-20260810-112836](../adr/ADR-20260810-112836-cart-priced-live-on-read.md) accepted, at line 97,
that *"The transient price a guest once saw is not in the log. Accepted: the audit-grade price is
the ORDER price, which is (CheckoutSnapshot)."* That was **correct while display and charge were
structurally identical**. **The quote token retires that premise** — the token becomes the only
evidence of what was displayed. So: **record the minimum artifact set as a domain event, not a log
line.**

### Two clocks. Do not conflate them.

- **The accounting clock** — `FRENCH_COMMERCIAL_BOOKS_10Y`
  (`specs/common/configuration.yaml:903`, C. com. L123-22 + CGI 242 nonies A, 3650 days) is for the
  **sale**. **A quote that never became an order is not a *pièce justificative***, and holding it
  ten years is **over-retention under GDPR Art. 5(1)(e)**.
- **The evidential clock** is different (C. civ. **2224** 5y / C. conso. **L218-2** 2y,
  VERIFY-FIRST) and wants a **third window** — proposed **5 years for a quote attached to a placed
  order**, **~90 days for an abandoned one**.

**The number is counsel's. That a third window is needed is self-answerable**, and it is the part
the team should act on now.

**The grounds differ, and the privacy notice must name both**: **Art. 17(3)(b)** for the accounting
record, **Art. 17(3)(e)** for the evidential quote.

### The erasure interaction — a launch-gate hazard

If the quote event lives on the **Cart** stream and carries a `legalRetention` marker, it can take
the Cart actor **out of stream deletion** and **break the launch-blocking erasure gate**
([`docs/decisions/ERASURE-LAUNCH-GATE.yaml`](../decisions/ERASURE-LAUNCH-GATE.yaml)).

**The clean shape, and it is free if chosen now: make the quote pseudonymous by construction** —
cart id, offer ids, prices, versions, rates. **No contact data.** Chosen later it is a migration.

---

## 7. Counsel packet — QT-1 … QT-10

### (a) IRREDUCIBLE — a French consumer-law practitioner

- **QT-1 — the formation moment.** When does the contract form under C. civ. **1127-2** for this
  flow, and would a CGU term stating *"the contract forms only on restaurant acceptance"* be abusive
  under **R212-1**, given the consumer cannot cancel at `PENDING`? (b)
- **QT-2 — which price-display text reaches an online food storefront**: the sector *arrêté* (27
  March 1987) or the general L112-1 *arrêté* plus the distance regime — and what must appear **next
  to a menu item** and **next to the total**? (b)
- **QT-3 — the reprice confirmation's required content.** Old total and delta, or the new total
  alone? Is a **fresh positive click** required? Is **any** implied acceptance available? (a/b)
- **QT-4 — the confirm control.** **Ask about the SHIPPED control, not the one this brief was first
  written against** (changed by [#833](https://github.com/TheCaptainCompany/captain-food/pull/833),
  2026-08-31). Today the French button reads **`"Commander avec obligation de paiement — 23,50 EUR"`**
  (`specs/screens/restaurant_frontoffice.translations.yaml:132-136`) and the order summary is
  **not collapsible** (`specs/screens/restaurant_frontoffice.yaml:476`). *Before #833* it read
  `"Commander — 23,50 €"` over a `collapsible: true` summary — retained here only so the question
  below stays intelligible; **counsel is not being asked about that string.** The questions:
  - does the safe-harbour formula **followed by the total** satisfy **L221-14 / CRD Art. 8(2)**,
    given that Art. 8(2) subpara. 2 requires the button be labelled *"only with the words 'order
    with obligation to pay' or a corresponding unambiguous formulation"*? Is an appended amount
    inside or outside that limb? **This is the live question** — the formula itself is verbatim, the
    suffix is ours. (a exists / b applied)
  - the total renders as **`23,50 EUR`, not `23,50 €`** (one shared `format_currency`,
    `crates/web/src/renderer.rs`). Does the ISO code satisfy the display regime **QT-2** asks about,
    on the confirm control specifically?
  - does an **always-visible** order summary satisfy the recap limb, and was the previous
    collapsible one a breach for orders placed before 2026-08-31? (b)
- **QT-5 — the pinned higher price.** May we charge the pinned, higher quoted price when the
  restaurant **lowered** its price inside the window — is that a *prix pratiqué* breach, or a
  *pratique commerciale trompeuse*? (b)

### (b) REDUCIBLE — a cheaper professional finishes it

- **QT-6 — absorbing: characterisation and receipt presentation.** *Expert-comptable* work, and a
  **candidate for a *rescrit fiscal*** under LPF **L80 B** (see
  [BRIEF-20260818 §3(b)](BRIEF-20260818-counsel-packet-and-self-answer-triage.md)). Blocked
  upstream on `CAPTAINNET-ZERO` (§5).
- **QT-7 — the evidential retention period**, and **whether an abandoned quote needs retaining at
  all**. The framework is readable; the **number** is the ask.

### (c) GATED ON THE FUNDS POSTURE — resolve into BRIEF-20260818 §3(c) Q10, **do not ask separately**

- **QT-8 — who is the *professionnel*** for the displayed food price, and what **CRD Art. 6a /
  L111-7-1** marketplace disclosure is owed? (b) — **absent from the repo entirely.**
- **QT-9 — the restaurant-facing leg.** Is binding a restaurant to a **withdrawn** price
  enforceable; what **P2B Art. 3 / 3(2)** disclosure and notice does it require; is there
  **C. com. L442-1** significant-imbalance exposure? (b)

> These two are the **same characterisation question** as
> [BRIEF-20260818 §3(c) Q10](BRIEF-20260818-counsel-packet-and-self-answer-triage.md) (the funds
> posture / merchant of record) and **must not be sent as separate questions** — a platform that is
> merchant of record is arguably not an intermediary at all, so Art. 6a and P2B do not even engage.

### (d) SELF-ANSWERABLE — record, do not send. **QT-10 — the design fences**

1. **Never render a downward reprice as a *discount*** — no strike-through, no reference price, no
   *"économisez"* (Omnibus prior-price rule).
2. **Never vary the reprice policy per customer** (personalised-pricing disclosure).
3. **No personal data in the quote payload** (§6).
4. **A modal, not a toast**, carrying `role=alert` (DSA Art. 25 + EAA / RGAA).
5. **The quote pins the TAX RATE, not only the price.**

### (e) NEW AT SLICE 2 ROUND 2 — CQ-1 … CQ-4 (the tax-rate leg)

Named at the slice 2 round 2 presentation pass (`legal`, [PR #920](https://github.com/TheCaptainCompany/captain-food/pull/920)): `AsOfCatalog`/`OfferPrice` pins whichever `TaxRate`
object the coordinate froze (F2/D3), but which rate that IS, on which order, is not decided anywhere
in the code — only carried as an open note (`PROP-20260831-134539` §6 D3). Four questions on that
leg, not answered here:

- **CQ-1 — mode selection.** Which of `TaxRate.delivery` / `.collection` / `.eatIn` applies to a given
  order is undecided (PROP §6 D3 note 1). Is the applicable mode the SERVICE MODE at order time, or
  at coordinate time — and is that a legal question at all, or purely an implementation choice?
- **CQ-2 — the null-mode `defaultTaxRate` fallback lives on ANOTHER stream.** When `collection`/`eatIn`
  is absent, a downstream `unwrap_or(delivery)` — or a fallback to the restaurant's `defaultTaxRate`
  (`specs/network/entities.yaml:103`) — reads a DIFFERENT aggregate's CURRENT state, which this
  coordinate does not pin. Is a rate resolved partly from a frozen coordinate and partly from live
  state at sale time defensible as "the rate that applied", or does the fallback need its own freeze?
- **CQ-3 — option-level rates.** Only `Product` carries `taxRate`; `ProductItemOption` does not, so an
  alcohol option inherits its parent product's food rate under this design (PROP §6 D3 note 2). Is
  "one rate per priced line" a requirement in French VAT practice for a composite line (a pizza plus a
  wine option), or is the parent-product rate an acceptable simplification?
- **CQ-4 — a statutory rate change between the coordinate and the sale.** If the LEGAL rate itself
  changes (a VAT-rate law, not a menu edit) between when a quote is minted and when the order is
  placed, does the frozen-at-coordinate rate remain compliant, or must a statutory change always
  override a pinned quote regardless of the display-guarantee mechanism this design builds?

Questions, not answers — none of the four is resolved by this round; they extend the packet's
funds/rate-leg gap already named above rather than opening a new one.

### (f) NEW AT SLICE 3a — CQ-5 … CQ-6 (the proof burden and the freeze boundary)

Named at the slice 3a briefing (2026-09-06, `legal`, PR #922): slice 3a mints a coordinate on the
`cart.current` read but signs, carries and stores nothing — the record says plainly it does NOT
discharge B1 and does NOT close #816. Two questions on what 3b's signed quote will need to satisfy,
not answered here:

- **CQ-5 — reproducible recomputation vs. a rendered artifact.** Does the L221-7-shape proof burden
  accept a REPRODUCIBLE SERVER RECOMPUTATION at the stored coordinate (re-price at V, compare to the
  charge) as sufficient evidence of what was displayed, or does it require an artifact of what the
  SCREEN actually rendered (a snapshot of the rendered price, not merely a recomputable one)? Does
  signing the coordinate (3b) change which of the two the burden accepts?
- **CQ-6 — the freeze boundary's granularity.** Must the delivery fee and the platform fee freeze
  INSIDE the same coordinate as the catalog lines (one single frozen total), or is a PER-COMPONENT
  freeze defensible — each fee/line frozen at its own coordinate/moment, with only the displayed
  TOTAL being the customer-facing commitment?

Questions, not answers — carried forward to 3b/4's design, where the signed quote and its stored
shape are decided.

### (g) NEW AT SLICE 3b+4 — CQ-7 (the refusal's information duty)

Named at the slice 3b+4 briefing (2026-09-06,
[ADR-20260906-192007](../adr/ADR-20260906-192007-slice-3b-and-the-command-change-land-as-expand-contract-behind-an-interlocked-write-door-with-the-refusal-set-enumerated.md)):
that record enumerates a refusal set (structural rejections, `QuoteNoLongerHonoured`, a fold
technical error) that all surface through **one cause-neutral customer screen** — ux's draft, quoted
in that ADR as a draft for counsel, never clearance — and requires that a `PaymentIntent` be created
before verify runs (D-F: verify sits in the pre-payment guard block, before the Stripe call) but is
never captured on any refusal path. Two questions on what that screen must say, not answered here:

- **CQ-7a — does a no-cause refusal discharge the information duty on a refused distance order?**
  The consumer sees "we could not confirm your total" with no cause named (deliberately, per the
  design's own fence against implying "the price changed" for causes that are not price changes at
  all — a forged token, a stale cart edit). Does French distance-selling information-duty law
  (the L112-1/L221-5 posture already carried at §7's L5, this brief's §3 disclosure floor) accept a
  refusal that states no cause at all, or does refusing an order still trigger a duty to state SOME
  reason, even a generic one, distinct from the duty to disclose a REPRICE (which this design is
  careful never to name)?
- **CQ-7b — must "nothing was charged" say anything more when a `PaymentIntent` was created and not
  captured?** The design's copy says "your card was not charged and no authorization was taken" —
  but a `PaymentIntent` in manual-capture mode (`ADR-20260808-195315` §1.2) DOES place an
  authorization hold at creation, released (not "never taken") on cancellation or expiry. Is stating
  "no authorization was taken" accurate consumer-facing language for a hold that WAS placed and then
  released, or does the information duty require distinguishing "no charge, and any hold has been
  released" from "no hold was ever placed" — and does the timing of the release (immediate vs. the
  card network's own settlement window) change the answer?

Questions, not answers — carried forward to the 3b+4 deliverable's own copy review (the team's
reviewer pass, not counsel, discharges nothing here); **prepared for counsel, never clearance.**

---

## 8. Sequencing — and this is the practical output of QT-8 / QT-9

**Build the customer-facing half now** — *never charge more than displayed*. It is safe under
**both** funds postures, so it waits on nothing.

**Do not build the restaurant-facing half** — *we will hold you to a withdrawn price for N minutes*
— until the posture is chosen. Under one branch it is a **purchase commitment**; under the other a
**unilateral contractual constraint on a business user**, and those two carry different obligations.

---

## 9. Blockers B1–B5

- **B1 — the display guarantee is claimed and never enforced.** `specs/ordering/rules.yaml:54-65`
  (`ServerPriceAuthority`) asserts the *LEGAL DISPLAY GUARANTEE (Code de la consommation
  L112-1/L221-5 posture)* and names the `expectedTotal` equality check as *"the enforcement"* — and
  that check never runs
  ([#816](https://github.com/TheCaptainCompany/captain-food/issues/816), whose body was corrected
  2026-08-31). **The quote must be non-nullable on the command.**
- **B2 — no upward charge after the confirming click, ever.** Charge the quoted amount or refuse.
- **B3 — no implied acceptance.** No countdown to charge, no pre-armed button, no auto-accept on
  expiry.
- **B4 — the quote pins the TAX RATE**, or it hardcodes
  [BRIEF-20260818 §5](BRIEF-20260818-counsel-packet-and-self-answer-triage.md)'s recorded VAT
  blocker into a brand-new surface. **`HOLD: human` — stored shape.**
- **B5 — L221-14.** An obligation-to-pay button and an **uncollapsed** total recap —
  [#817](https://github.com/TheCaptainCompany/captain-food/issues/817).

---

## 10. Reconciliation with PROP-20260831-134539 §8 (`L1–L7`)

PROP-20260831-134539 §8 was reconstructed from **primary repo records only**, under its own `L1–L7`
labels, precisely **because this return had not landed**. The two numberings are
**cross-referenced, not competing**: `L1–L7` are the proposal's *design-facing* constraints; `QT`
are *questions for counsel* and `B` are *build blockers*. Neither renumbers the other.

| PROP §8 | This brief | Relationship |
|---|---|---|
| **L1** contract may form at the confirming click | **QT-1** + §2 | Same question. §2 supplies the three reasons characterisation (2) is not freely available, which §8 did not have; L1's answer — *F4 makes it moot* — is **confirmed correct** and is the same move as **B2** |
| **L2** no implied acceptance | **QT-3** + **B3** + §3 | Same constraint; QT-3 is the counsel question behind it |
| **L3** countdown / validity timer is a dark-pattern surface | **QT-10 fence 4** + §3 | Same; this brief adds the CRD Art. 22 limb (voidness) alongside the DSA Art. 25 limb |
| **L4** a downward move may never be presented as a discount | **QT-10 fence 1** + §4 | Same. §4 adds that "no article is breached by charging less" is an **argument from absence** |
| **L5** the legal display guarantee | **B1** | Same; B1 adds *the quote must be non-nullable on the command* |
| **L6** obligation to pay + legible recap | **QT-4** + **B5** | Same, but the SUBJECT moved: #833 shipped the safe-harbour formula and un-collapsed the recap on 2026-08-31, so QT-4 now asks whether the formula **plus an appended total** satisfies Art. 8(2) subpara. 2's *"only with the words"* limb — not whether `"Commander — 23,50 €"` did |
| **L7** nothing in ordering/payments carries VAT | **B4** + **QT-6** | Same; B4 states it as a build blocker on the quote's shape |
| — | **QT-2** which *arrêté*; **QT-5** the pinned higher price; **QT-7** the evidential clock; **QT-8/QT-9** the funds-posture leg | **New ground** §8 did not reach. QT-7 and §6 also **retire a premise of ADR-20260810-112836:24** that §8 did not identify |

**The `Concerns` entry is discharged by this file** for the part it names — *"the lens's own return
must be attached, and its numbering reconciled with §8"*. What it does **not** discharge, and what
the proposal's author should decide: whether §8 is now **rewritten** to cite this brief instead of
carrying its own reconstruction. That is the proposal's call, not this brief's.

---

## 11. Hygiene the lens flagged — **re-verified against the worktree**

Both citations were relayed as `UNVERIFIED input`. Both were checked at
`origin/main` `c9607e3a`, 2026-08-31.

- **`specs/screens/restaurant_frontoffice.yaml:518` — the line is real, the characterisation needs
  narrowing.** Line 518 is
  `on_error: { type: show_toast, variant: error }` — the **generic** error handler on the
  `place_order` action, not a purpose-built price-change disclosure. **The concern survives the
  correction and is arguably worse for it**: a `PriceMismatch` rejection
  (`specs/ordering/rules.yaml:59`) reaches the customer today **only** as an anonymous error toast,
  through a component declared at `:178` as
  `toast_notification, position: top_center` with no focus or dismissal semantics. A transient,
  often non-focusable, dismissible-before-read surface disclosing a **material price change** is a
  **DSA Art. 25** concern **plus an accessibility defect** (EAA in force for new e-commerce services
  since **2025-06**). **QT-10 fence 4** is the fix: a modal with `role=alert`.
- **`specs/ordering/errors.yaml:250-262` — exact, verified.** The `PriceMismatch` block is those
  lines precisely. The copy says *"Les prix ont changé depuis l'affichage du menu. Veuillez vérifier
  votre panier et réessayer."* / *"Prices have changed since you loaded the menu. Please review your
  cart and try again."* — **it does not say what moved.** Both `en` and `fr` need rewriting when the
  reprice UI supersedes it.

---

## 12. What stays OPEN, with its risk named

- **QT-1 — the formation moment.** Carried, not closed. **Mitigated to near-zero by B2**: if the
  charge can only ever be the quoted amount or a refusal, the design is safe under both branches.
  **This is why B2 is a blocker and not a preference.**
- **QT-8 / QT-9 — the funds posture.** Not open *here*; open in
  [BRIEF-20260818 §3(c) Q10](BRIEF-20260818-counsel-packet-and-self-answer-triage.md) and §7 there.
  Its consequence for this work is §8: **the restaurant-facing half is not buildable yet.**
- **QT-6 — absorbing.** Blocked upstream on `CAPTAINNET-ZERO`, founder-owned.
- **QT-7 — the third retention window.** The **need** is self-answered here; the **number** is not.
  Shipping a quote event on the 10-year accounting window is **over-retention**, and shipping it
  with contact data on it is a **launch-gate break** (§6).
