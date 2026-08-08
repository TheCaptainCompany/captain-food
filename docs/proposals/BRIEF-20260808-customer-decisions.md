# The customer's decision brief — what only you can decide, and why

> **ANSWERED 2026-08-08** — the customer answered all ten via the decision form:
> [ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md) records the seven
> decisions (1.1 · 1.2 · 1.3 · 1.5 · ch. 4 · ch. 5 · ch. 6); three moved to discussion
> (1.4 tips · ch. 2 erasure · ch. 3 admin-on-behalf). This brief stays as the argument record.

**Date**: 2026-08-08 · **Prepared by**: the five-lens register sweep (architect classification;
legal-specialist, business-specialist, graphql-architect, ux-designer, dba arguments), session
https://claude.ai/code/session_01AKgDqRbCcCxtUePWPRfxtp · **Companion record**:
[ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md) — the 30
decisions the team took by consent (your veto window is open on all of them).

The register held ~55 open decisions. After classification and the team's consent round,
**ten remain yours** — five of them one coherent money posture. Each entry below states the
question in plain words, the options, each lens's argument, the recommendation, and the reason
it is yours (liability, a reversal of your own recorded decision, a genuine value call, or a
contested question no evidence settles). Answer in any form — "chapter 1 as recommended" works;
so does a per-decision list. Answers land in DECISIONS.md; anything cross-cutting gets its ADR.

---

## Chapter 1 — The money posture (five decisions, one coherent choice)

These five chain: each downstream answer gets cheaper under the upstream recommendation. The
lenses converged on treating them as one posture. **Why yours: every one moves or holds real
money, and only you carry that liability.**

### 1. Payout posture — who holds the customer's money? (PROP-165000 D1)

**Options**: Stripe Connect, separate charges & transfers (a regulated institution holds funds;
restaurants are sellers of record; Captain invoices only its service fee) — vs — merchant of
record (Captain receives funds, then remits).

- **Legal**: receiving funds on behalf of restaurants is prima facie a payment service under
  PSD2 (grade a); operating one unauthorized is a sanctionable offence in France, and the
  commercial-agent exemption was deliberately narrowed for marketplaces acting for both sides
  (grade b). Connect is the standard mitigation: Stripe carries KYC/AML, funds never touch
  Captain, and the posture also fixes who issues receipts and how VAT is declared. Confirm with
  counsel **before the first real payment** — this gets more expensive with every order.
- **Business**: merchant-of-record has no margin worth having — the float on ~200 orders/week is
  worth under ~200 €/year in interest, against refund liability on Captain's balance sheet and
  a compliance cost line. Connect gives restaurants Stripe's automatic payout cadence (slow
  remittance is a documented independent-restaurant churn driver). The honest cost: some
  claimed restaurants will stall at the Connect onboarding step — instrument that funnel.

**Recommended: Connect, separate charges & transfers.** No lens argues otherwise.

### 2. Capture timing — when does the customer's money actually move? (PROP-165000 D2)

**Options**: authorize at checkout, capture on restaurant acceptance — vs — capture at checkout,
refund on rejection.

- **UX**: the entire difference is what a rejection *feels like*: a pending hold that silently
  disappears vs a debit line for food that never came, sitting on a French bank statement for
  5–10 business days. Under authorize-on-accept the checkout can honestly print *"vous ne serez
  débité que lorsque le restaurant accepte votre commande"* — a trust-building promise at the
  moment of maximum hesitation.
- **Business**: under capture-first, every rejection/timeout is a refund workflow that eats the
  non-returned Stripe processing fee (~0.50 € each) at a cold-start cohort's plausible 5–10%
  rejection rate — one order in ten converted into fee leakage plus a trust incident. Under
  authorize-then-capture the same event is a released hold: nothing moved, no refund, no
  chargeback surface.
- **Legal**: the sales contract really forms at acceptance; capturing at checkout debits a
  consumer for a contract that may never form — a DGCCRF-complaint surface (grade b). The CGV
  should state the order is an offer and the contract forms at acceptance; the ~7-day
  authorization life legally bounds scheduled orders (already chained to the scheduling row).

**Recommended: authorize at checkout, capture on acceptance.** Unanimous.

### 3. Acceptance timeout — what happens when nobody answers? (PROP-164500 D1+D2)

**Options**: auto-cancel + auto-approved refund at 5 min (per-restaurant override) — vs — longer
windows, admin escalation, or customer-initiated cancellation.

- **UX**: five minutes of post-payment silence is the peak of the anxiety curve; the window must
  be a designed sequence (accepted ✓ → notified → escalating reassurance at ~2–3 min), with the
  pressure countdown on the restaurant's queue, never the customer's screen. A fast honest "no"
  with money released, plus one-tap "restaurants ouverts près de vous", retains more customers
  than a rescued order arriving 50 minutes late. Invest in the cancellation screen — it is where
  the relationship survives or ends.
- **Business**: the refund is money you owe regardless — the only variable is speed; the asset
  at stake is the customer's annual stream (10–20 orders, 250–500 € GMV) and Tours-scale word of
  mouth, against zero incremental cost. This is Meyer's "mistakes well handled" made executable.
  Sequence the notification slice first so the timed-out population is genuine failures, not
  sleepy tablets.
- **Legal**: returning money unattended is safe in every direction — exposure lives in keeping
  it, never refunding it (grade a comfort). Under chapter 1.2 it degrades to a mere release.

**Recommended: auto-cancel + auto-approved refund, 5 min with per-restaurant override.**

### 4. Tips — does the tip button move real money? (PROP-165000 D5)

**Today's state is the hazard**: `OrderTipped` is recorded and **zero transfers move** — a tip
button whose money reaches no courier.

- **Business**: a deferred trust bomb (the 2019 DoorDash tip scandal forced a national model
  change). At Tours V0 utilization, 1–2 € tips are a 20–40% peak-hour earnings uplift — the
  cheapest rider-retention spend that exists, because it is the customer's money. Ride the same
  `transfer_group` as the payout leg. Keep `Tipper = RESTAURANT` — a restaurant tipping its
  regular courier is a priority signal money can't otherwise buy.
- **Legal**: a tip control that routes nothing is a misleading-commercial-practice risk (grade
  b) — the UI must not ship to consumers before the transfer leg exists. "100% goes to your
  courier" is both the compliant and the marketable posture. Verify the current pourboires tax
  regime's expiry with counsel.

**Recommended: yes — 100% pass-through on the Connect transfer mechanism, and the tip UI ships
only with the transfer leg.**

### 5. External orders — an Uber order in Captain's log (PROP-032306 D4)

**Options**: a distinct `ExternalOrderReceived` event — vs — overloading `OrderPlaced` with
nullable payment fields.

- **API**: nullable-by-default exists for fields that can *fail*, not for encoding a different
  business fact as an absent one. Nullable payment on `OrderPlaced` forces every money consumer
  (receipt, refund, VAT, payout projectors) to handle "maybe no money moved" on 100% of orders
  for a minority channel. The sibling event is the deprecate-never-break answer; both fold into
  one Order read model with a **schema-visible provenance field** — because an Uber order cannot
  be refunded or receipted through Captain, and a refund button rendered on one is a live
  control bound to nothing.
- **Legal**: receipts and VAT records must reflect who actually collected consideration — on an
  Uber order, Uber's rails. Keep Uber-order revenue out of Captain's VAT declarations and
  receipt numbering. Adjacent obligation logged: DAC7 platform reporting (counsel packet).
- **Business**: the per-channel net comparison ("your 18.90 € Uber order netted ~13 €; the same
  basket on Captain nets ~18 €") is your single best conversion instrument — a distinct event
  gives it to you for free as sales-channel reporting.

**Recommended: distinct `ExternalOrderReceived`, provenance schema-visible, shared acquisition
scalar in `specs/common`.**

---

## Chapter 2 — Account-level erasure scope (§1 C remainder)

**Question**: extend the order-level tombstone-then-stream-deletion (your ADR-20260731-160000)
to the customer account — identity, files, credit, conversations — and set the per-phase
retention windows. **Why yours: legal liability, and the order-level precedent was your own
override of the team's recommendation.**

- **Legal**: Art. 17 is not absolute — French accounting law *requires* keeping financial
  records ~10 years after the person is erased (grade a). Account erasure therefore means:
  identity deleted at the provider, files purged with `uploaded_by` nulled, marketing data gone,
  financial records retained de-identified. The deliverable is a **written retention schedule
  per data category** (CNIL expects one) plus the one-month response clock.
- **Data**: after stream deletion a rebuild simply never sees the customer's streams again — so
  every fold resolving a `customerId` must be total over a dangling reference (the same
  discipline your disappearance proposal just established for restaurants, applied
  symmetrically). The account case adds two NON-rebuildable stores (files registry, third-party
  identity): the erasure PM's verification duty widens to a per-store completion receipt — an
  unverifiable leg is the half-happened erasure the PM exists to prevent. The retention windows
  must live in ONE source — DSL-declared, generation feeding both the sweep and the DPIA — or
  the DPIA drifts into describing a system that no longer exists.

**Recommended: extend the tombstone+deletion pattern with the per-store receipts; windows
DSL-declared; counsel validates the schedule.** (Largely a schedule-and-sequencing decision now.)

---

## Chapter 3 — Admin acting on behalf of a restaurant (PROP-171500 D4)

**Question**: when support must fix a tenant's problem, does the admin use an explicit, logged
bypass — or time-boxed impersonation via the tenant's own graph? **Why yours: the standing
recommendation (bypass) reverses your own recorded ADR-0037 impersonation-only stance — only
you reverse you.**

- **API**: impersonation is the schema-boundary-clean answer (the admin reaches
  `/restaurant/graphql` with a scoped token; the admin graph never grows every tenant operation
  — exactly the over-responsible-graph growth the gateway design exists to prevent; audit
  concentrates at token issuance). The bypass is honest at the *event* layer: the envelope
  records the ADMIN as actor, where naive impersonation writes the tenant's identity onto an
  immutable log for an action a Captain employee performed — forged provenance you cannot
  amend. **The requirement that is orthogonal to the choice: whichever wins, the envelope must
  carry BOTH the real and the assumed principal (an act-as chain), or the audit trail lies.**
  Deciding question: which path exists and is fast when the incident is a paid order nobody is
  acting on at Friday peak — a logged, envelope-honest bypass is an acceptable V0 only if
  explicitly temporary and never composed into the admin schema as ordinary fields.
- **Legal** (from the objection pass): a *logged* bypass is the accountability artifact GDPR
  Art. 5(2) wants — no objection to either shape, provided the log is real.

**Recommended: keep ADR-0037's impersonation as the target architecture; allow the explicit
logged bypass as a temporary V0 mechanism with the act-as chain mandatory from day one.**

---

## Chapter 4 — The operating entity (PROP-032306 D7)

**Question**: which legal person operates Captain.Food? **Why yours: only a legal person can
own a licence, a tax posture, and a cooperative's structure — this is ownership itself.**

- **Legal**: three artifacts already name entities inconsistently — the Uber agreement (Caring
  Hope Foundation, a loi-1901 association), the development identity (TheCaptainCompany), and
  the cooperative ambition. If the association holds the licence while another entity operates
  and earns commission, the access sits outside the licence (termination right); the same
  mismatch replicates into every launch artifact that must name a legal person (GDPR
  controller, mentions légales, the consumer mediator, the Stripe platform agreement, the ACPR
  posture). And an association running a commissioned marketplace faces the lucrativity
  analysis — likely taxable, non-profit posture at risk (grade b). **Choose the entity before
  launch artifacts multiply; novate the Uber agreement to it.**
- **Business**: a SCIC-family cooperative is Meyer's stakeholder ranking made legally binding —
  multi-college membership (restaurants, riders, customers), one member one vote, mandatory
  reserves, an asset lock that forecloses the sell-to-the-incumbent ending. That structure IS
  retention economics: a restaurant that holds a share does not churn on a coupon. The
  sequencing is what makes this urgent: Connect onboarding papers every restaurant relationship
  to whatever entity exists at activation — re-papering later costs each restaurant a
  signature, and every signature request is a churn touchpoint. **Decide before Connect
  onboarding opens.**

**Recommended: decide the entity now (the lenses' analysis favors the SCIC direction, but the
choice of legal form is precisely the kind of decision this brief cannot make for you); book
the counsel hour — entity + payments posture are questions 1–2 of the packet.**

---

## Chapter 5 — Transparency levels (§19 D1)

**Question**: how much of the platform's operational reality is public (status page, incidents,
metrics, the build itself)? **Why yours: a genuine mission/values call — what "build in public"
means for YOUR cooperative.**

- **Business**: radical transparency attacks churn reason #4 (ranking and fee opacity) directly
  and is the one claim incumbents structurally cannot copy — moat, not gesture; also your
  cheapest senior-recruitment instrument pre-PMF. Two real exposures: partner-level data (hold
  the platform-aggregates-only line absolutely) and pre-PMF optics of small absolute numbers —
  prefer week-over-week presentation until absolutes are creditable.
- **UX**: L2 (public incident status) is the incident-time extension of the tracking screen's
  honesty contract — the harm of an incident is always the silence. For restaurants, verifiable
  uptime/deploys/post-incidents is an adoption argument no sales deck matches. A stale "all
  green" page is a false signifier — the freshness timestamp (now decided) is what makes L4
  honest.
- **Legal**: publishing converts every number into a representation — mechanical generation
  over narration is the right instinct; post-incident writeups are scrubbed and never front-run
  Art. 33/34 notification decisions; per-restaurant/per-rider figures stay off the table
  (sole-trader metrics are personal data; published rider metrics feed the reclassification
  file).

**Recommended: adopt the levels as proposed (information, never control; aggregates only) —
with L3's go-live gated on real volume and the funnel signal that #400 must first make exist.**

---

## Chapter 6 — Escalated by the team: who funds promotions? (PROP-165500 D5)

The one row the sweep sent BACK to you. The standing recommendation ("promo codes first,
loyalty later") never names who funds a redeemed code — **and on a 0%-commission platform that
is the entire question**: Captain has no commission margin to fund discounts from; restaurant-
funded-by-default is commission by another name. The business lens's reshape: (a)
restaurant-initiated, restaurant-funded promotions first (the restaurant's own lever, named in
the payment breakdown); (b) platform-funded acquisition codes deferred until a funder and a
cohort-repeat-rate signal exist; (c) loyalty on the existing #158 credit machinery sequenced
immediately behind. Incentive-spend doctrine: promotions train discount-waiting, and a
discount war against deeper pockets is lost by winning — the coop's moat is loyalty economics.
**Why yours: the reshape is a resequencing of the merchandising roadmap, and sequencing is the
product owner's** (the funder analysis, the lens notes, is a viability fact and is its own).

**Recommended: adopt the reshape (a)/(b)/(c).**

---

## Also surfaced by the sweep, riding other tracks

- **A launch precondition on no register row until today**: the mandatory **consumer mediator**
  registration — now recorded (register overhaul) and in the counsel packet.
- The counsel packet now stands at ~17 questions across two briefs
  (`docs/legal/BRIEF-20260808-listing-opt-out-objections.md` + the additions from this sweep) —
  one avocat morning, pre-structured. Entity (ch. 4) and payments (ch. 1.1) are its first two
  questions.
- The 30 team decisions and their folded cures:
  [ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md). Your
  veto window is open on all of them.
