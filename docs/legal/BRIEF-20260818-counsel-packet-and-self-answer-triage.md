# BRIEF-20260818 — The counsel packet, and what the team can answer itself

**Date**: 2026-08-18 · **Lens**: `legal-specialist`, with `business-specialist` on the cost ranking ·
**Occasion**: the founder ruled *"I don't have the money to ask counsel so we have to answer
ourselves"*, and separately that he will self-host Odoo for accounting ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

> **NOTHING IN THIS FILE IS LEGAL ADVICE OR CLEARANCE**, and no aggregation of it becomes clearance
> (ADR-20260808-144738, CLAUDE.md). A budget constraint changes **which questions we can retire
> ourselves**. It does not convert an open question into a closed one. Where the honest answer is
> "this stays open and here is what it risks", that is what is written.

**Why this file exists at all**: the ten-question packet raised on 2026-08-18 was summarised in one
line of [ADR-20260818-094500](../adr/ADR-20260818-094500-staff-auth-mechanism-and-refund-approval-stays-with-the-restaurant.md)
and **never landed as a record** — a coordinator defect under the rule that GitHub and the session
are never the record. Q1–Q9 below are reconstructed from that line; the original Q10 could not be
recovered verbatim and is re-stated by the lens.

---

## 0. Source discipline used here

Each claim carries one of: **FETCHED** (a URL retrieved this session, named), **PRIMARY-READABLE**
(a named instrument anyone can read for free), or **VERIFY-FIRST** (reasoning from training, cutoff
2026-05, not yet checked against a source). **Do not quote a `VERIFY-FIRST` line as established.**

---

## 1. The external clock — FETCHED

**Source**: `https://entreprendre.service-public.fr/vosdroits/F23208` ("Tout savoir sur la
facturation", DILA), fetched 2026-08-18.

- From **1 September 2026**, every French VAT-liable business must be **able to RECEIVE** electronic
  invoices, via a **plateforme agréée (PA)** or a *solution compatible raccordée à une plateforme
  agréée*. **Issuing** is staged: large and mid-cap **2026-09-01**; **PME and micro 2027-09-01**.
- **The official term is `plateforme agréée`, not "PDP".** Searching the old term returns stale
  guidance.
- Sanctions: failure to receive via a PA → mise en demeure, 3 months, then €500, a further 3 months,
  then €1 000 per rolling 3-month period. Failure to transmit e-reporting transaction/payment data →
  €500, capped €15 000/year. **CGI arts. 289 bis, 290, 290 A, 1788 D.**
- **B2C invoices are OUT of scope of e-invoicing entirely.** This narrows a precondition the repo
  currently frames too broadly: the "compliant receipt" duty is **CGI 289 / annexe II 242 nonies A**
  + the **€25 TTC** *note* rule for a service to a consumer + **e-reporting**, not e-invoicing.
- Invoice retention: **10 years**. Four new mandatory mentions phase in on the same calendar (client
  SIREN, delivery address where different, nature of the operation, and the *paiement d'après les
  débits* option).

**What this means for Captain, stated carefully**: Captain has no legal entity yet, so nothing bites
in September. **It bites the day the entity is registered.** The founder is standing up accounting
now, so the sequencing matters.

---

## 2. THE CONTRADICTION — the repo asserts two opposite payment postures

This is the most load-bearing finding in the brief and it is **not a wording problem**.

| Record | What it says |
|---|---|
| `docs/proposals/PROP-20260726-165000-marketplace-economics-and-money-movement.md:92` | ADOPTED option D1, stated pro: *"**Restaurant is its own merchant of record**; Captain never holds partner funds"* |
| `specs/common/entities.yaml:23` · `specs/architecture/c4-l3.yaml:114` · `docs/adr/0017-3way-stripe-connect-split.md:24` · `docs/adr/0028-pricing-3way-split-model.md:37` · `docs/integrations/stripe-process.md:261` | *"**Captain = merchant of record**"* — and c4-l3 describes the mechanism as a **PaymentIntent for the buyer total on the platform account**, transferring out after capture |

The mechanism described in the second row **is** holding third-party funds, which the first row names
as the problem in its own words.

**And a non-sequitur is riding on it.** `ADR-0017:24` states: *"Captain is merchant of record → no
extra PSP/EMI license (PSD2-ok)."* That is a legal conclusion with **no instrument cited**, and it
reads backwards: the exclusion normally relied on is the **commercial-agent exclusion, PSD2 Directive
(EU) 2015/2366 Art. 3(b)** (`VERIFY-FIRST`, including its CMF transposition), which requires acting
**on behalf of** the payee — i.e. precisely **not** being merchant of record. Whichever way it
resolves, the inference as written does not follow, and it has been copied into four other files.

**One of those records is wrong about what the system does with the customer's money. Which one is
wrong is a DECISION, not a typo, and it must not be closed by a wording edit.**

---

## 3. Triage of the ten questions

### (a) RETIRABLE by the team, against a named primary source

| # | Question | Source to read |
|---|---|---|
| Q7 | DSA trader traceability | **Reg. (EU) 2022/2065 Art. 30(1)(a)–(f)** enumerates the fields. Read **Art. 19** in the same sitting — `VERIFY-FIRST` — it exempts micro/small from Arts. 20–28 and 30–32. **Collect the fields anyway**: they are the same fields the contract and the invoice need |
| Q5 | Binding a legal person at self-registration | Free public registers: **annuaire-entreprises.data.gouv.fr** / **INPI RNE** give SIREN, legal form and *représentant légal*. "Can this person bind this restaurant" is answerable at zero cost and should be a **spec field, not a question** |
| — | Allergens (standing precondition) | **Reg. (EU) 1169/2011 Art. 14(1)(a)** (distance selling: particulars available *before* purchase), Art. 9(1)(c), Annex II. Liability for wrong restaurant data is contract, not statute → bucket (b) |
| — | Withdrawal right | **C. conso. art. L221-28** (perishables). Free on Légifrance |
| — | Consumer mediator | **C. conso. art. L612-1**; the **CECMC** publishes the referenced-mediator list. Obligation and register both public; the cost is the mediator's subscription, not a lawyer |
| — | Invoice/receipt mentions and retention | **FETCHED** above: CGI annexe II **242 nonies A**, 10 years, the **€25 TTC** *note* rule |
| — | Mentions légales | **LCEN loi n° 2004-575 art. 6-III**. Blocked only on §2's structure decision, because the capacity named must be the one the statutes actually confer |

### (b) REDUCIBLE — a much cheaper professional than an avocat finishes it

- **EXPERT-COMPTABLE — the direct answer to the founder's question, and yes, mostly.** Can
  definitively answer the **rate mapping** (CGI **279 m** 10% immediate consumption, **278-0 bis**
  5,5% non-immediate, **278** 20% alcohol — `VERIFY-FIRST` on numbering), the **invoice mentions**,
  the **FEC**, the **e-invoicing calendar and the choice of plateforme agréée**, and the
  subscription invoice to restaurants. Tax advice is within their statutory remit for clients whose
  accounts they keep, and they carry professional insurance for it. **What they cannot answer while
  it does not exist is what our contracts say** — the delivery fee's VAT treatment and the food's VAT
  follow from the legal shape, i.e. §2. Ask today and the honest answer is *"it depends on the
  posture you have not chosen"*.
- **RESCRIT FISCAL — a free binding written ruling from the tax administration exists.** **LPF art.
  L80 B**, guarantee in **LPF art. L80 A**; a formal written position on a completely and truthfully
  described situation binds the administration for that situation; general window ~3 months.
  `VERIFY-FIRST` on numbering, delay and filing channel — the official impots.gouv.fr page returned
  404 on fetch this session. **Two honest caveats: an incomplete description voids the guarantee,
  and an unfavourable answer binds you too.** Draft it with the expert-comptable.
- **BOFiP — opposable without asking anyone.** Published administrative doctrine
  (bofip.impots.gouv.fr) is opposable to the administration under **LPF art. L80 A**. Reading the
  restauration-VAT series retires most of the rate question at zero cost. `VERIFY-FIRST` on the exact
  BOI reference.
- **ACPR — a free channel probably exists, but it is not a ruling.** The **Pôle FinTech Innovation**
  is understood to be a free single entry point for new entrants asking whether an activity needs a
  status. `VERIFY-FIRST` — both acpr.banque-france.fr pages returned **HTTP 403** on fetch this
  session. It gives an **informal orientation, not a binding ruling and not clearance**; there is no
  L80-B equivalent on the ACPR side. Second free primary source: **REGAFI**, the register of
  authorised institutions and their agents.
- **Structure → CG Scop / Union régionale des Scop**, not an avocat: model statutes and formation
  accompaniment. Note the SCIC form carries a mandatory *révision coopérative* (`VERIFY-FIRST` on
  periodicity).
- **Protection juridique — the cheapest lawyer access is usually one already paid for.** RC Pro /
  multirisque professionnelle, a business bank account or a CCI membership frequently bundles a
  module with telephone access to lawyers and fee cover. **Check what is already held before assuming
  there is no budget.** **CCI Touraine** also runs low-cost advisory and *permanences juridiques*.
  `VERIFY-FIRST` on both.
- **Q3 Supabase processor vs joint controller** — half reducible to reading Supabase's published
  **Art. 28 DPA** against the ruled architecture (ADR-20260818-004646). Residue is (c).
- **Q9 Art. 33 on the refund queue** — **not a legal question first**: a fact-finding task the team
  can do. Do request logs retain enough to determine that no cross-tenant read occurred? If yes, the
  "unlikely to result in a risk" assessment can be **made and documented** under Art. 33(5). If no,
  it can only be asserted. Flagged on the #618 card; still open.

### (c) IRREDUCIBLE — a judgement about which regime applies, with the downside named

- **Q10 — the funds posture / merchant of record.** No research retires it; it depends on contracts
  that do not exist. **Downside if wrong**: (i) if Captain is genuinely merchant of record, Captain
  sells the food and owes output VAT **on the whole GMV**, not on its fee — get it backwards and the
  assessment is on the gross, retroactively, with penalties; (ii) **unauthorised provision of payment
  services is a criminal offence in France** (`VERIFY-FIRST` on the CMF article) — an uninsurable
  tail; (iii) Stripe can terminate a misdescribed flow, and the platform stops taking money on a
  Friday night. **This question gates Q8 (whose supply is the delivery fee), the receipt's shape, the
  consumer-law counterparty, and the whole P2B analysis. It is the first thing any professional you
  pay should be pointed at.**
- **Q1 — rider deactivation and rider status.** Irreducible and moving: LOM/ARPE plus **Platform Work
  Directive (EU) 2024/2831**, transposition due ~2026 (`VERIFY-FIRST` — exactly the kind of date that
  moves). **Downside**: URSSAF requalification with retroactive cotisations across the whole rider
  population; in a bad case *travail dissimulé*. For a pre-revenue cooperative, existential.
  **Design consequence that needs no counsel**: every feature in #348 that increases platform control
  — mandatory acceptance, imposed routes, sanction-shaped deactivation — is evidence toward
  reclassification, and deactivation with no notice, no reason and no appeal is the highest-risk
  shape available.
- **Q2 — phone as sole factor, Art. 32 proportionality.** The **Art. 35 DPIA is our obligation to
  perform and document**, not counsel's to grant. **Art. 36** prior consultation with the CNIL takes
  8 weeks, extendable by 6 — not a fast answer.
- **Q3 residue / the restaurant relationship — Art. 26 vs Art. 28.** Who determines purposes over
  customer data is a judgement. Downside: the wrong contract and no allocation of liability.
- **Q6 — P2B.** The *text* is retirable (Reg. (EU) 2019/1150 Arts. 3, 5, 11, 12, small-enterprise
  carve-out in Art. 11(5) — `VERIFY-FIRST`), but **whether Captain is an "online intermediation
  service" is the same characterisation question as Q10**. A platform that is merchant of record is
  arguably not an intermediary at all — which is why the two cannot be answered separately.

---

## 4. Self-hosted Odoo, in this lens

**It changes who operates the software. It changes no obligation, and it discharges less than it
looks.**

- **E-invoicing: it does NOT cover it, and this is the trap most worth naming.** From the fetched
  source, the invoice must be *"émise, transmise, reçue et conservée par l'intermédiaire d'une
  **plateforme agréée** ou d'une **solution compatible raccordée à une plateforme agréée**"*. A
  self-hosted Odoo is at best the *solution compatible* — **the instance is not the PA**; a PA is an
  accredited operator on a DGFiP-published list. Odoo SA may operate or be accredited as one
  (`VERIFY-FIRST`), but that would be **its service, not a self-hosted copy**. *"I self-host Odoo so
  accounting is covered"* is **false for e-invoicing**.
- **What it genuinely discharges**: Factur-X generation, the FEC, invoice numbering and mentions,
  10-year retention, the *piste d'audit fiable*. Real value — just not the routing.
- **What it does not touch**: the **e-reporting** obligation on B2C transactions (CGI 290 / 290 A) is
  fed by **our** order and payment data, not by Odoo's ledger — and our spec carries **no per-line
  VAT** to feed it (§5).
- **A new obligation it may CREATE**: software recording B2C payments falls under the anti-fraud
  certification regime (**CGI art. 286-I-3° bis**, NF525 or éditeur attestation) — `VERIFY-FIRST`. An
  attestation covers a **specific unmodified version**, and self-hosting-plus-customising is exactly
  how one is broken. Note the recording happens in **our** system, so the question lands on Captain's
  own payment path regardless of Odoo.
- **Personal data**: self-hosting makes the founder sole controller **and** infrastructure operator —
  no processor DPA to stand behind, **Art. 32** security is his, and Odoo enters the **Art. 30**
  record of processing, which does not appear to exist in this repo at all.
- **A genuine gift to #194**: accounting retention (**C. com. art. L123-22**, 10 years) is a legal
  obligation, which is the lawful ground under **GDPR Art. 17(3)(b)** to refuse erasure *of the
  accounting record specifically* while erasing everything else — an anchor the erasure epic lacks.
  The flip side: **an erasure that clears Postgres and leaves Odoo is not an erasure**, and Odoo is
  invisible to #194 today.

---

## 5. The blocker underneath all of it

**Nothing in the ordering or payments spec carries VAT.** `TaxRate` exists
(`specs/common/entities.yaml:76`, per service mode) and `TaxRatePercent`
(`specs/common/scalars.yaml:240`) — but grepping `tax` across `specs/ordering/entities.yaml`,
`specs/ordering/events.yaml`, `specs/payments/*.yaml` and
`specs/database/tables/projection_tables.yaml` returns **zero hits**. `PaymentBreakdown` has eight
Money fields and no tax decomposition.

So the placed order and its stored events carry **no VAT basis**, and `TaxRate` hangs off **mutable
current state** (the catalog item and the account default), never frozen onto `OrderPlaced` or the
checkout snapshot. An accounting fold would join **today's** catalog to price **yesterday's** VAT —
non-replayable, which for a statutory ledger is a defect and not a trade-off.

Adding tax to the stored order shape is **CLAUDE.md rule (2): a migration with a versioning story
recorded before it lands.** `HOLD: human`.

And `business-specialist`'s companion point: the real cost of "VAT per line" is not the engine, it is
**collecting the correct rate per menu item from every restaurant at onboarding**. That is a funnel
cost, not a software cost — and **if it is not collected, no compliant receipt can be issued at all**.

---

## 6. Where the money should go, if any ever exists

In this order, because item 1 gates 2–4:

1. **Which regime the funds flow falls under**, given Connect separate charges & transfers with the
   PaymentIntent on the platform account — and therefore who is the seller, who owes VAT on the food,
   who is the consumer's counterparty, and whether the commercial-agent exclusion is available.
   → *expert-comptable + rescrit fiscal for the VAT half; ACPR Pôle FinTech for the status half.*
2. **The rider control surface in #348**, described as designed rather than as intended.
   → *avocat en droit social, or nothing — there is no cheap channel here that can be named honestly.*
3. **The restaurant contract**, which then unlocks P2B, DSA and the delivery fee's VAT.
4. **The structure**, via CG Scop rather than an avocat.

## 7. What stays OPEN, with its risk named

Three, with no path to closing them by research: **the funds posture**, **rider status**, and
**whether the refund-queue exposure was a notifiable breach**.

**Proceeding past the first real payment without a qualified answer on the first of those is a
decision to accept a criminal-liability tail and a VAT-on-gross tail. It should be recorded as a
decision, taken knowingly by the founder — not left as an unfilled gap that later looks like nobody
asked.**
