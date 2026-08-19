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

Companion brief, same day, different subject:
[BRIEF-20260818-counsel-packet-and-self-answer-triage](BRIEF-20260818-counsel-packet-and-self-answer-triage.md).

---

## 0. Source discipline, and the two scales used here

Each claim carries one of: **FETCHED** (a URL retrieved this session, named), **PRIMARY-READABLE** (a
named instrument anyone can read for free), or **VERIFY-FIRST** (reasoning from training, cutoff
2026-05, not yet checked against a source). **Do not quote a `VERIFY-FIRST` line as established.**

Two different three-point scales appear in this file and they are **not** the same scale:

- The companion brief's **triage** — (a) retirable by the team · (b) reducible to a cheaper
  professional · (c) irreducible.
- This file's **confidence grade** on a statement — **(a)** established against a fetched or freely
  readable primary source · **(b)** reasoned, with the reasoning shown so it can be attacked · **(c)**
  **not statable from memory** — it needs a lookup, and until then it is not written down at all.

**One thing is deliberately absent.** No **French Code de la consommation article number** appears
anywhere in this brief. Légifrance was **unreachable through the proxy** this session (403), and any
transposition number from memory grades **(c)**. It is filed as open question **G2** and must be
looked up before anything cites it. A wrong article number in a repo record is worse than no number:
it gets copied.

---

## 1. The instrument, FETCHED verbatim

**Source**: EUR-Lex, Directive 2011/83/EU on consumer rights (CRD), retrieved 2026-08-18. Grade **(a)**.

> **Article 22 — Additional payments**
>
> Before the consumer is bound by the contract or offer, the trader shall seek the express consent of
> the consumer to any extra payment in addition to the remuneration agreed upon for the trader's main
> contractual obligation. **If the trader has not obtained the consumer's express consent but has
> inferred it by using default options which the consumer is required to reject in order to avoid the
> additional payment, the consumer shall be entitled to reimbursement of this payment.**

Scope, same source, grade **(a)**:

> **Article 17 — Scope**
> … 2. Articles 19, 21 and **22 shall apply to sales and service contracts** and to contracts for the
> supply of water, gas, electricity, district heating or digital content.

**Two things follow from the text alone**:

1. **The prohibited shape is the DEFAULT, not the tick-box.** The Article does not require that the
   extra payment be hidden, unfair, or hard to remove. It requires that consent be **express**, and it
   names the failing shape precisely: *a default option the consumer is required to reject*. A
   pre-filled amount is that shape.
2. **Lowerability is the definition of the shape, not a defence.** *"It can be lowered to zero"* is
   what makes it *"a default option which the consumer is required to reject"*. The founder's own
   description of the mechanic is a description of the conduct the Article addresses.

**The remedy is the finding.** Art. 22 does not attach a fine; it attaches **reimbursement of the
payment**. Applied to a funding model whose *whole* customer-side revenue is the contribution
(ADR-20260818-161500: there is no customer service fee at V0), the exposure is that **every
contribution ever collected under a pre-filled default is reimbursable, back to the first order** —
a rescission of the funding model, not a penalty line. That is why this brief exists before the
screen does.

---

## 2. The honest grading — and the counter-argument, stated fairly

**Grade: (b), leaning strongly inside the prohibition.** It is (b) and not (a) because two real
counter-arguments exist and neither is frivolous.

**The counter-argument, put at its strongest:**

1. **A *don* may not be a "payment" in the Article's sense.** Art. 22 speaks of *"any extra payment in
   addition to the remuneration agreed upon for the trader's main contractual obligation"* — arguably
   a payment forming part of the consideration, not a voluntary gift that buys nothing and is
   contractually severable from the meal.
2. **Captain may not be "the trader" of the main obligation.** Since
   [ADR-20260818-150000](../adr/ADR-20260818-150000-captain-is-the-tool-the-restaurant-carries-the-delivery.md),
   the **restaurant** sells the delivered meal; Captain is a party to neither supply. On that reading
   the *"remuneration agreed upon for the trader's main contractual obligation"* is the restaurant's
   price, and the contribution is not an add-on to Captain's own main obligation because Captain owes
   the consumer no main obligation.

**Why the lens declines to rest on either.** Taken together they prove too much: they would legalise
**any** pre-selected charge, provided it is labelled a gift and routed to an affiliate of the seller.
That is precisely the arbitrage a consumer-protection provision is read to defeat, and the second
argument gets weaker, not stronger, the more the interface is Captain's — the consumer sees one
checkout, one total, one act. Additionally, argument 2 sits in tension with the payment posture the
repo already records: Captain collects the buyer total on its own Stripe balance
(BRIEF-20260818 §2). **A reading that requires the consumer to understand a three-party supply chain
in order to lose a protection is not a reading to build a funding model on.**

**What would move the grade to (a) either way**: G1 and G2 below. Both are lookups, not litigation.

---

## 3. The obligation map

| # | Obligation / regime | Instrument | What it bites on here | Grade | Open question |
|---|---|---|---|---|---|
| 1 | Express consent to any extra payment; default options prohibited | **CRD 2011/83 Art. 22** (FETCHED) | The **pre-filled** contribution amount at checkout | (a) on the text; **(b)** on its application here | G1 |
| 2 | Art. 22 applies to **service** contracts, not only sales | **CRD 2011/83 Art. 17(2)** (FETCHED) | Removes the "this is a service, not a sale" escape | (a) | — |
| 3 | National transposition, its wording and any national add-on | French **Code de la consommation** — **article number deliberately NOT stated** | The number that would actually be pleaded in France | **(c)** — Légifrance 403 this session | **G2** |
| 4 | Whether a voluntary contribution is **outside the scope of VAT** | **CJEU *Tolsma* C-16/93** (PRIMARY-READABLE) — no direct link / genuinely voluntary | A pre-filled default is **evidence against** voluntariness — the same fact weakens two regimes at once | (b) | G3 |
| 5 | Design of online interfaces / "dark patterns" | **DSA Reg. (EU) 2022/2065 Art. 25** | **Named and dismissed** — see §4 | (b) | G6 |
| 6 | Unfair commercial practices (misleading / aggressive) | **UCPD 2005/29/EC** (VERIFY-FIRST on its French transposition) | The residual route once DSA Art. 25 defers; survives the micro-enterprise exemptions | (b) | G6 |
| 7 | B2B invoice mentions, retention, and **e-invoicing** | CGI arts. 289 / 289 bis / annexe II 242 nonies A; calendar FETCHED in the companion brief | **Q7's monthly shortfall invoice is a French domestic B2B invoice** — see §5 | (a) on the calendar; (b) on the treatment | G4 |
| 8 | A pooled fund compensating the public for a risk | Insurance-operation characterisation (VERIFY-FIRST) | Q8: the cagnotte bears refunds. The silhouette appears **the moment the CGU promise it** | (b) | G5 |
| 9 | Who is soliciting the funds, in what capacity | Association vs SASU standing; public solicitation of funds regime (VERIFY-FIRST, **no numbers stated**) | The site **already publishes an association and an RNA number** (`docs/STATUS.md:2165`) while ADR-20260808-195315 records a SASU | (b) | **G7** |

---

## 4. DSA Article 25 — named, and dismissed

It is named because it is the provision everyone reaches for on a pre-selected default, and dismissed
because on these facts it does not carry the finding.

- **Art. 19** exempts micro and small enterprises from Arts. 20–28 (and 30–32). Captain, when it
  exists, is one. Grade (b), VERIFY-FIRST on the exact carve-out list.
- **Art. 25(2)** defers: the dark-patterns prohibition does not apply to practices already covered by
  the **UCPD** or the GDPR.

**The consequence is not comfort.** Removing DSA Art. 25 leaves **CRD Art. 22**, which has no
size threshold and a **self-executing consumer remedy**, and the **UCPD**, which the DSA points at
directly. The size exemption removes the regulator with the biggest press release, not the exposure.

---

## 5. What Q7 creates that did not exist: a Captain-issued B2B invoice

Q7 (*periodic invoice only — monthly, computed after the period closes*) makes Captain an **issuer of
French domestic B2B invoices to restaurants**. Today the repo has no such artifact at all.

- The e-invoicing calendar (**FETCHED** in the companion brief, DILA/service-public): every French
  VAT-liable business must be able to **RECEIVE** via a *plateforme agréée* from **2026-09-01**;
  **issuing** for PME and micro from **2027-09-01**. Sanctions escalate from mise en demeure to €500,
  then €1 000 per rolling 3-month period; e-reporting failures €500, capped €15 000/year. Retention:
  **10 years**.
- **Nothing bites while there is no entity. It bites the day the entity is registered** — and Q4 says
  the entity is coming.
- **The headline conflict is not a legal one but it is a promise one**: the published page says
  *"0 € d'abonnement"* beside what will be a recurring monthly bill. That belongs in the register row,
  not here.
- **The VAT treatment of the shortfall invoice is genuinely open** (G4): a share of pooled platform
  costs, invoiced monthly to restaurants, is not obviously the same supply as a commission — and the
  answer interacts with the standing question (G7) and with whether the contribution is inside or
  outside VAT (G3). One expert-comptable sitting, not an avocat.

---

## 6. The one design consequence that is free today

Stated separately because it costs nothing **only while the event log is still empty**:

- **Keep the refund discretionary in the terms** (Q8). A pooled fund that *promises* to compensate
  consumers for a risk is closer to an insurance operation than one that may, at Captain's discretion,
  make a customer whole. The word chosen in the CGU is the whole difference, and it is free to choose
  now.
- **Aggregate any public refund line.** Naming which restaurant caused a refund is a reputational
  disclosure and, for a sole trader, personal data about a named natural person.
- **If a contribution screen is ever built, it needs its own consent artifact** — not `serviceFee`,
  not `TipRecipient: CAPTAIN` — carrying the **presented default**, the **chosen amount** and an
  **affirmative-act** flag. Under Art. 22 the defence is evidence of express consent; a stored amount
  with no record of what was presented cannot produce it. This is the only one of the three affected
  surfaces that would sit **inside** this repo's gates: the `/tarifs` page and the CGU do not
  (ADR-0036 — GitHub Pages via CNAME, outside this tree), so their corrections **cannot be closed by a
  diff here** and someone must confirm the page changed.

---

## 7. Counsel questions G1–G7

For an expert-comptable (G3, G4), a free lookup (G2), and an avocat or a free orientation channel for
the rest. Ordered by what unblocks the most.

- **G1 — Does the pre-filled contribution fall inside CRD Art. 22 as transposed in France**, given
  that (i) it is presented as a *don* rather than as consideration, and (ii) Captain is not the trader
  of the main obligation (the restaurant sells the delivered meal, ADR-20260818-150000), while Captain
  operates the interface and collects the buyer total? If yes, does the reimbursement remedy reach
  **every** contribution collected under that shape?
- **G2 — What is the French transposition of Art. 22** (article, wording, any national add-on, and the
  applicable sanction)? *Deliberately unanswered in this brief; Légifrance was unreachable. One
  lookup.*
- **G3 — Is the contribution outside the scope of VAT** under *Tolsma* (no direct link, genuinely
  voluntary), and **does a pre-filled default defeat that**? If it is inside scope, **who is the
  taxable person**, given the payment-agent posture and the merchant-of-record contradiction recorded
  in BRIEF-20260818 §2?
- **G4 — How is the monthly shortfall invoice to restaurants characterised and rated** — a share of
  pooled costs, a commission, or a subscription — and what mentions must it carry? Does it fall under
  the e-invoicing/e-reporting calendar as soon as the entity is registered?
- **G5 — At what point does the cagnotte bearing customer refunds become an insurance operation**
  requiring authorisation, and does keeping the refund **discretionary** in the CGU (rather than a
  promised indemnity) keep it outside?
- **G6 — Independent of Art. 22, is a pre-selected default an unfair commercial practice under the
  UCPD** as transposed, and do the DSA's micro/small exemptions (Art. 19) plus Art. 25(2)'s deferral
  leave the UCPD as the operative route?
- **G7 — Which entity solicits the contribution today**, and in what capacity? The site already
  publishes an association and an RNA number while a SASU is recorded as the intended structure; if
  the association is publicly soliciting contributions, what does that regime require of it **now**,
  not at incorporation? External artifacts must name the capacity the statutes actually confer.

---

## 8. What stays open, with its risk named

- **G1/G2 unanswered while the mechanism ships** = accepting a **reimbursement tail over the whole
  contribution history**, not a fine. It is cheap to answer now and uncapped later, because the tail
  grows with every order.
- **G3 unanswered** = the same fact (a pre-filled default) weakening the VAT position and the consumer
  position simultaneously, with VAT computed on the gross if it resolves badly.
- **G7 unanswered** = a live public surface soliciting funds in the name of an entity whose capacity
  nobody in this repo has verified.

**If the mechanism is built before G1 and G2 are answered, that should be recorded as a decision taken
knowingly by the founder** — with the reimbursement remedy in front of him — and not left as a gap
that later looks like nobody asked.
