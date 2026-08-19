# BRIEF-20260818 — The pre-filled contribution, and the monthly invoice

**Date**: 2026-08-18 (evening) · **Lens**: `legal-specialist`, returning on the founder's ten answers ·
**Occasion**: **Q9** — *"We will apply the same logic, same mechanism"* (a suggested contribution
amount is **pre-filled** and can be lowered to zero) — and **Q7**, a **monthly shortfall invoice** to
restaurants ·
**Record**: [ADR-20260818-233000](../adr/ADR-20260818-233000-the-ten-answers-per-head-monthly-invoice-and-a-cagnotte-that-exists-only-in-prose.md) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

> **NOTHING IN THIS FILE IS LEGAL ADVICE OR CLEARANCE.** It is one agent lens reading public
> instruments. No aggregation of lens outputs becomes clearance either (CLAUDE.md,
> ADR-20260808-144738). Where the honest answer is *"this stays open, and here is what it risks"*,
> that is what is written. Nothing here authorises building the mechanism it describes: Q9 is a
> decision reversal and waits on its register row.

> **Attribution notice (revision 2).** In revision 1 the obligation map and the counsel questions were
> **composed by the executor** from a coordinator summary of the lens's return, not transcribed from
> the lens. In a document whose purpose is to be handed to an avocat, that is a correctness defect.
> This revision replaces every attributed passage with the lens's **own words**; blockquotes and the
> tables in §2, §3, §4, §5, §6 and §7 are verbatim. Prose outside them is the executor's framing and
> is not attributed to the lens. The defect is banked in
> [ADR-20260818-233000](../adr/ADR-20260818-233000-the-ten-answers-per-head-monthly-invoice-and-a-cagnotte-that-exists-only-in-prose.md) §10.

Companion brief, same day, different subject:
[BRIEF-20260818-counsel-packet-and-self-answer-triage](BRIEF-20260818-counsel-packet-and-self-answer-triage.md).

---

## 0. Source discipline, and the two scales used here

**The lens's own header, verbatim:**

> FETCHED = retrieved this session, named · PRIMARY-READABLE = a free named instrument · VERIFY-FIRST =
> training-based, cutoff 2026-05, unchecked. Nothing here is legal advice or clearance, and no
> aggregation of it becomes clearance (ADR-20260808-144738). Légifrance and economie.gouv.fr both
> returned **403 through the proxy today**, so every French article number is VERIFY-FIRST unless
> marked otherwise.

Two different three-point scales appear in this file and they are **not** the same scale:

- The companion brief's **triage** — (a) retirable by the team · (b) reducible to a cheaper
  professional · (c) irreducible.
- This file's **confidence grade** on a statement — **(a)** established against a fetched or freely
  readable primary source · **(b)** reasoned, with the reasoning shown so it can be attacked · **(c)**
  **not statable from memory** — it needs a lookup, and until then it is not written down at all.

**One thing is deliberately absent.** **No article number is given for the French transposition of
CRD Art. 22.** The transposing *instruments* are named in §3 because the lens named them; the current
Code de la consommation article they produced is not, because the lens refused to state it from memory
and graded any such number **(c)**. It is filed as counsel question **G2**. A wrong article number in a
repo record is worse than no number: it gets copied. Where a French code reference *does* appear below
(`C. conso. L121-x`, `C. ass. L310-1`, the CGI articles), it is inside the lens's verbatim text and
carries the lens's own VERIFY-FIRST or FETCHED marking there — read the marking, do not lift the number.

---

## 1. The instrument, FETCHED verbatim

**Source**: EUR-Lex, Directive 2011/83/EU on consumer rights (CRD), retrieved 2026-08-18 and
**re-verified against the same fetch artifact when this revision was written** (not against any
summary). Grade **(a)**.

> **Article 22 — Additional payments**
>
> Before the consumer is bound by the contract or offer, the trader shall seek the express consent of
> the consumer to any extra payment in addition to the remuneration agreed upon for the trader's main
> contractual obligation. If the trader has not obtained the consumer's express consent but has
> inferred it by using default options which the consumer is required to reject in order to avoid the
> additional payment, the consumer shall be entitled to reimbursement of this payment.

The **first** sentence is the obligation — *seek the express consent* — and it must stay in any
quotation of this Article: the second sentence only supplies the remedy for having failed the first.

Scope, same source, grade **(a)**:

> **Article 17 — Scope**
> … 2. Articles 19, 21 and 22 shall apply to sales and service contracts and to contracts for the
> supply of water, gas, electricity, district heating or digital content.

Art. 22 attaches no fine. It attaches **reimbursement of the payment**. Applied to a funding model
whose *whole* customer-side revenue is the contribution (ADR-20260818-161500: there is no customer
service fee at V0), the exposure is a **rescission of the funding model retroactive to the first
order**, which is why the lens grades the severity as it does in §4.

---

## 2. Inside or outside Art. 22 — the lens's four-row analysis, verbatim

- **Textual reading — Inside.** "The text does not say 'pre-ticked box' — it says *'default options
  which the consumer is required to reject in order to avoid the additional payment.'* A pre-filled 2 €
  the customer must drag, clear or re-type to 0 is a default that must be **rejected** to avoid the
  payment. Lowerability is the definition of the prohibited shape, not a defence against it — a
  pre-ticked box is also untickable."
- **The genuine counter-argument.** "Art. 22 catches *'an extra payment in addition to the remuneration
  agreed upon for the trader's **main contractual obligation**.'* A gratuitous contribution is not
  consideration for anything, and per
  [ADR-20260818-134500](../adr/ADR-20260818-134500-the-invoice-chain-restaurant-to-customer-rider-to-restaurant.md)
  the *seller* of the meal is the restaurant, not Captain. Two arguable escapes: (i) a *don* is not a
  'payment' in the Art. 22 sense; (ii) Captain is not the trader of the main obligation."
- **Why the lens does not find it comfortable.** "It proves too much: it would legalise any
  pre-selected charge simply by labelling it a gift and routing it to an affiliate of the seller. The
  consumer-protection interpretive canon and effet utile both cut the other way, and DGCCRF has
  historically treated pre-checked donations as the same practice (VERIFY-FIRST). And even outside
  Art. 22, the practice is squarely inside the **UCPD 2005/29** transposition (C. conso. L121-1 et
  seq., VERIFY-FIRST numbering) as a potentially misleading/aggressive practice, where the penalties
  are much larger than reimbursement."
- **Honest grade.** "**(b) — interpretation, leaning strongly INSIDE.** Not 'unsettled' in the sense of
  a 50/50: the zero default is unambiguously lawful and needs no opinion; the pre-fill is the only
  version that requires one. Anyone who tells you a pre-filled amount is *clearly* outside Art. 22 is
  guessing in the direction the product wants."

---

## 3. Transposition status — verbatim, and why no article number appears

> "French transposition: it exists — CRD was transposed by **loi n° 2014-344 du 17 mars 2014
> ('Hamon')** and recodified by **ordonnance n° 2016-301** into the Code de la consommation. **I will
> not state the current article number from memory** and Légifrance was unreachable through the proxy
> today. That is one Légifrance lookup, and it is question **G2**. Grade of the *existence* of the
> French rule: **(a)**. Grade of any article number I might supply: **(c) — unknown, do not build on
> it.**"

The refusal is kept visible on purpose. It is the discipline, not a gap in the brief.

---

## 4. The obligation map — the lens's table, verbatim

| Instrument | Who is liable | Artifact that proves compliance | Grade | Severity if ignored |
|---|---|---|---|---|
| CRD 2011/83 **Art. 22** + its FR transposition | the **trader soliciting the payment** = Captain (grade **(b)** — the restaurant is the seller of the meal, Captain solicits its own contribution in the same checkout) | a stored record, per order, of **the default that was presented** and **the affirmative act** that set the amount | (a) rule exists / (b) application | **Every contribution ever collected is reimbursable.** Not a fine — a rescission of the funding model, retroactive to the first order |
| UCPD → C. conso. L121-x | Captain | the checkout copy + screen version, retained | (b) | DGCCRF administrative fine (ceiling VERIFY-FIRST, materially larger than reimbursement) + injunction |
| CJEU *Tolsma* / VAT scope | the collecting entity | the contribution's characterisation carried consistently in the event, the Stripe metadata and the invoice | (b) | VAT assessed on contributions received, retroactively, with penalties |

The middle row's `L121-x` is the lens's own placeholder, and it stays a placeholder: see §3.

---

## 5. DSA Article 25 — named and dismissed, verbatim

> "DSA 2022/2065 **Art. 25** (interface design / no dark patterns; preselection is the archetype) — but
> Art. 25 sits in Section 3, from which **Art. 19 exempts micro and small enterprises**, and Art. 25(2)
> defers to the UCPD where that applies. Net: DSA is probably *not* the operative instrument here; the
> UCPD and Art. 22 are. Grade **(b)**. Naming it and dismissing it is part of the map."

Removing DSA Art. 25 is not comfort: it leaves CRD Art. 22, which has no size threshold and a
self-executing consumer remedy, and the UCPD, which Art. 25(2) points at directly.

---

## 6. THE COUNSEL PACKET — G1–G7, verbatim

Reproduced exactly as the lens wrote them, parentheticals included: each parenthetical carries why the
question is cheap to answer, which is what makes the packet usable by whoever is handed it.

1. **G1** — Does association RNA **W372020229** exist and is it the publisher of `join.captain.food`?
   If yes, is it already inside the *appel public à la générosité* regime by publishing a solicitation,
   and what does *"the company will be created"* create alongside it? *(Fact question first, legal
   second; the repo asserts the RNA only at `docs/STATUS.md:2165`.)*
2. **G2** — **The article number.** Which current Code de la consommation article transposes **CRD
   Art. 22** (additional payments / default options), and what sanction attaches beyond the directive's
   reimbursement remedy? *(One Légifrance lookup; Légifrance was 403 through our proxy today.)*
3. **G3** — Is a **pre-filled, lowerable, gratuitous contribution to the platform operator**, collected
   inside a checkout whose main supply is the restaurant's, an *"extra payment in addition to the
   remuneration agreed upon for the trader's main contractual obligation"* within Art. 22 — given that
   Captain is **not** the seller of the meal (ADR-20260818-134500)? If Art. 22 does not catch it, does
   the UCPD transposition?
4. **G4** — Does a **pre-filled default** defeat the *Tolsma* (C-16/93) "no direct link / genuinely
   voluntary" characterisation, i.e. does the mechanic itself bring the contribution into **VAT
   scope**? *(Ask alongside the expert-comptable / a rescrit per BRIEF-20260818 §3(b) — and note that
   an incomplete description voids the L80 B guarantee, so the pre-fill must be described in the
   rescrit if it is adopted.)*
5. **G5** — By legal form (association 1901 / SASU / SCIC): what is the **nature of a customer
   contribution**, may it lawfully **fund consumer refunds** for a restaurant's non-performance, and
   does a pooled fund used to compensate order failures approach an **opération d'assurance**
   (C. ass. L310-1) if the CGU ever promise it?
6. **G6** — The **monthly shortfall invoice**: taxable supply at 20%, or is any cost-sharing exemption
   (CGI 261 B) reachable? And does it trigger the *plateforme agréée* obligation on Captain's issuing
   date rather than the restaurant's?
7. **G7** — **P2B**: must the shortfall **method** be in the T&Cs before the first restaurant signs,
   and does introducing or changing it engage the Art. 3(2) 15-day notice? Is Captain within the
   Art. 11 small-enterprise exemption from the internal complaint system?

**Standing, unretired**: BRIEF-20260818 §3(c) Q10 (the funds posture) remains the first euro of any
legal budget, ahead of every question above. Q4's answer narrows it to one question; it does not close
it.

---

## 7. The rest of the lens's return, verbatim

### 7.1 Q7 and e-invoicing — what the monthly invoice creates

> "B2C invoices are **out of scope** of the reform (FETCHED, BRIEF-20260818 §1), which is why the
> restaurant→eater series was untouched. **Captain→restaurant is B2B domestic and is in scope.** From
> **2026-09-01** the restaurant must be able to *receive* via a *plateforme agréée*; from
> **2027-09-01** a PME must *issue* that way. Q7 creates the **first Captain-issued B2B invoice in the
> entire model**, so Q7 is what pulls Captain into the PA obligation. Sanction shape: mise en demeure →
> €500 → €1 000 per rolling 3-month period (CGI arts. 289 bis, 290, 290 A, 1788 D, FETCHED)."

### 7.2 P2B

> "Art. 3(1) plain and intelligible, in the T&Cs BEFORE it applies; Art. 3(2) advance notice with a
> **15-day minimum**; Art. 12 named mediators; Art. 11 internal complaint handling, from which small
> enterprises are exempt (VERIFY-FIRST). Grade **(a)** on the obligation, **(b)** on whether the
> shortfall counts as a 'charge' vs a 'condition' — either way it is in the terms."

### 7.3 Q8 — the insurance silhouette, and the design consequence available today

> "A pooled fund, fed by contributions, used to compensate members of the public for a risk has the
> **silhouette** of an insurance operation (Code des assurances L310-1). Today it is defensible
> precisely because he called it *'very exceptional'* — discretionary, ad hoc, unpromised. It stops
> being defensible the moment the CGU promise it. Grade **(b)/(c)** — I cannot ground the boundary and
> I am not going to pretend to. **Design consequence available today with no counsel: keep the refund
> discretionary in the terms, never a promised indemnity.**"

### 7.4 Q8 — credit where due: the set-off clause is no longer needed on this path

The set-off finding from the ADR-20260818-150000 appendix

> "**narrows to nothing** if the cagnotte absorbs it. Nothing is debited from the restaurant, so no
> set-off clause is needed for this path. That is a real reduction in the terms."

### 7.5 Q8 — privacy on a public refund line

> "a public line item that identifies **which restaurant** caused a refund is a reputational
> disclosure, and where the restaurateur is a sole trader it is personal data with a named natural
> person. Aggregate it."

### 7.6 Q5 — the credit, and what it costs elsewhere

> "the cleanest answer of the ten… Captain takes no compulsory money from the consumer at all. That
> materially improves the **PSD2 Art. 3(b)** commercial-agent argument and makes the 'gratuit, 0 % de
> commission' claim toward consumers true without qualification."

And the coupling:

> "the customer's relief is the restaurant's increase. Restaurants now carry **100%** of the shortfall,
> and the restaurant is the party whose disclosure is regulated (P2B). Q5 raises the magnitude of the
> Q7 disclosure obligation by exactly the amount it removes from consumers."

### 7.7 Q4 — the five items that are urgent *before* incorporation

The legal form is upstream of what a "contribution" legally is, so these five are ordered against the
incorporation date, not against the product backlog. Verbatim:

1. "the legal form is upstream of what a 'contribution' legally IS, deciding three regimes at once —
   association 1901 (*don*; loi 91-772 appel public à la générosité, préfecture declaration above a
   decree threshold, VERIFY-FIRST on the figure; *reçu fiscal* under CGI 200/238 bis requires *intérêt
   général*, which a platform serving commercial restaurants may fail on lucrativité) / SASU
   (presumptively commercial income, outside-VAT depends entirely on *Tolsma*) / SCIC (member
   categories, 51%+ member control, mandatory *révision coopérative*)";
2. "the statutes must confer *encaissement pour compte de tiers* and the capacity to receive
   contributions — so mentions légales and the ADR-134500 §4 mandate cannot be finalised before the
   statutes, and a mandate signed by nobody or by the founder personally confers no agency";
3. "Stripe onboarding is a **one-shot description** happening at incorporation, and must carry the
   accurate sentence already recorded in ADR-20260818-150000's legal appendix";
4. "the segregation decision — is the cagnotte a distinct account whose balance equals the published
   total? free before the account exists, a money migration after";
5. "e-invoicing PA choice belongs in the incorporation sprint."

### 7.8 The cagnotte is a fold, not an account

> "if the public page shows a 'cagnotte' and a customer reasonably understands a segregated fund, the
> description has to match the mechanism (misleading-commercial-practice exposure, C. conso. L121-2
> family, VERIFY-FIRST on numbering — grade **(b)**). And commingled funds sit in the estate on
> insolvency; a 'cagnotte' implies they do not."

### 7.9 Addendum — the surfaces that are outside this repo's gates

There is no repo-held copy of the published page (GitHub Pages via CNAME, ADR-0036), so the Q5 and Q7
page corrections **cannot be closed by a diff in this repo** and `make validate` stays green while
promise and model diverge; the evidence artifact for a pre-contractual statement is a **dated capture**,
and the ADR's verbatim block is an agent's transcription, not a capture (grade (a) on the principle,
(c) on whether such history already exists). It sharpens the Q9 card, because the **per-order consent
artifact is the only one of the three surfaces that would sit inside this repo's gates** — carrying the
presented default, the chosen amount and an affirmative-act flag, which is exactly the artifact §4 row 1
names as the proof of compliance.

---

## 8. What stays open, with its risk named

- **G2 and G3 unanswered while the mechanism ships** = accepting a **reimbursement tail over the whole
  contribution history**, not a fine. Cheap to answer now, uncapped later, because the tail grows with
  every order.
- **G4 unanswered** = the same fact (a pre-filled default) weakening the VAT position and the consumer
  position simultaneously, with VAT computed on the gross if it resolves badly.
- **G1 unanswered** = a live public surface soliciting funds in the name of an entity whose capacity
  nobody in this repo has verified.
- **G5 / G6 / G7 unanswered** = the terms are written before it is known whether the shortfall is a
  'charge' or a 'condition' under P2B, and before the invoice's rating is settled.
- **Q10, the funds posture**, stands ahead of all seven and is not retired by any of them.

**If the mechanism is built before G2 and G3 are answered, that should be recorded as a decision taken
knowingly by the founder** — with the reimbursement remedy in front of him — and not left as a gap that
later looks like nobody asked.
