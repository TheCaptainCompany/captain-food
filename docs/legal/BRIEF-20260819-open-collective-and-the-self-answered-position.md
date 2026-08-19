# BRIEF-20260819 — Open Collective's fiscal host, and what a self-answered position may say

**Date**: 2026-08-19 · **Lens**: `legal-specialist`, returning on the founder's six queue answers ·
**Occasion**: **Q2** — *"Open collective but not yet configured"* — colliding with **Q3**, the
reaffirmed pre-filled contribution; and **Q6**, *"answer it ourselves from the lenses' analysis and
proceed"* ·
**Record**: [ADR-20260819-103112](../adr/ADR-20260819-103112-the-six-queue-answers-a-fiscal-host-in-the-money-path-and-a-refund-bearer-with-no-field.md) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

> **NOTHING IN THIS FILE IS LEGAL ADVICE OR CLEARANCE.** It is one agent lens reading public
> instruments and a platform's published terms. No aggregation of lens outputs becomes clearance
> either (CLAUDE.md, ADR-20260808-144738). Where the honest answer is *"this stays open, and here is
> what it risks"*, that is what is written. **Nothing here authorises configuring Open Collective,
> choosing a fiscal host, or shipping the pre-filled contribution.**

> **Attribution notice.** Everything in §1, §2 and §4 below is **transcribed** from the lens's return
> in the round-4 aggregation (`MOB-RETURNS-4-queue`), not composed by the executor. Blockquotes and
> the enumerated lists in those sections are the lens's own words. Prose outside them is the
> executor's framing and is not attributed to the lens.
>
> ⚠️ **§5 is deliberately empty.** The dispatch that commissioned this brief named counsel questions
> **G8–G11** as carried verbatim in the aggregation. **They are not in it.** They are therefore
> **not written here**, because the executor composing counsel questions from a summary is exactly
> the correctness defect banked in
> [ADR-20260818-233000](../adr/ADR-20260818-233000-the-ten-answers-per-head-monthly-invoice-and-a-cagnotte-that-exists-only-in-prose.md)
> §10 and corrected in revision 2 of
> [BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice](BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice.md).
> In a document whose purpose is to be handed to an avocat, an invented question is worse than a
> missing one.

Companion briefs: the G1–G7 packet and the CRD Art. 22 analysis live in
[BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice](BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice.md);
the retirable / reducible / irreducible triage lives in
[BRIEF-20260818-counsel-packet-and-self-answer-triage](BRIEF-20260818-counsel-packet-and-self-answer-triage.md).

---

## 0. Source discipline

The scale used in this file, unchanged from the companion briefs:

- **FETCHED** — retrieved this session, source named, with its retrieval date.
- **PRIMARY-READABLE** — a free named instrument.
- **VERIFY-FIRST** — training-based, cutoff 2026-05, unchecked.

**No French article number appears in this file.** Légifrance and economie.gouv.fr have been
returning 403 through the session proxy, so any such number would be VERIFY-FIRST at best and
memory-sourced at worst. A wrong article number in a repo record is worse than no number: it gets
copied. The CRD Art. 22 analysis and its open transposition question (**G2**) stay where they were
established, in the companion brief.

The Open Collective material below is **FETCHED, 2026-08-19**. It is a platform's own terms and its
own help pages — a contract and a product description, not law. Reading them tells you what the
platform says it will do; it does not tell you how a French authority would characterise the
resulting arrangement.

---

## 1. The Open Collective terms, FETCHED verbatim (retrieved 2026-08-19)

The lens quoted three clauses. They are the whole of the finding:

> §1(a): *"**'Host'** — an Organization that **receives and holds funds** contributed by Financial
> Contributors via the Platform **on behalf of** one or more Collectives."*

> §4(b): *"Each contribution you make is paid directly to an Organization that receives and holds
> funds…"*

> §4(e): *"**all contributions are final, and there are no refunds**… the Collective Admin or
> **Host** are solely responsible for providing any refunds."*

Other facts fetched in the same pass:

- The platform operator is **OFi Technologies LLC** (Delaware), owned by a 501(c)(6).
- Host fee class: **6 % of incoming contributions**, taken **before anything reaches the collective**.
- **Public contributor recognition by default** (§4(f)).
- **Changing host later** requires a **zero balance**, both hosts' cooperation, and **cancels
  non-Stripe recurring contributions**.

---

## 2. The collision: Q3 × §4(e)

> **The collision**: Q3's Art. 22 exposure is *reimbursement of every contribution ever collected*.
> Q2 routes contributions through an entity where **Captain does not hold the refund lever**. He
> would owe a remedy he cannot execute.

Stated plainly, and without grading the underlying question (which is unchanged and open, and is
**G2** in the companion packet): the remedy attached to the pre-filled-default shape is *give the
money back, per contribution, across the whole population*. Under §4(e) the party who can give it
back is the **Host** — not Captain, not the Collective. A remedy Captain owes and a lever Captain
does not hold is the specific failure mode this brief exists to name.

**A correction the lens made to the coordinator's own framing, carried here because it changes the
conclusion**: the relay had told the founder that Open Collective's public ledger partially rescues
the Q1 handover problem.

> Half true and the wrong half — the ledger evidences **receipt**, not **attribution**, because the
> counterparty on every row is the Host, before and after incorporation alike. Pre- and
> post-incorporation contributions look identical.

So Open Collective does **not** mitigate Q1. It is neutral on it at best.

---

## 3. The fork — four options, and which put a third legal person in the money path

**FETCHED, 2026-08-19**, from Open Collective's *"Choosing a Fiscal Host"*. Four options:

| Option | What it is | Third legal person in the money path? |
|---|---|---|
| **No one** | no host configured | **No** — and no money can be received |
| **Organization** | *"manage money using your own bank account and legal status"* | **No** — Open Collective becomes a **public ledger only** |
| **Our Own Fiscal Host** | you operate the host | **Yes** |
| **Apply to a Fiscal Host** | a third-party host receives and holds | **Yes** |

> Only the last two put a third legal person in the money path. **Organization keeps OC as a public
> ledger only** — but requires a legal entity that does not yet exist. **One configuration screen
> decides it.**

That last clause is the operative one. The option that avoids the §4(e) collision — *Organization* —
is precisely the option that **cannot be selected today**, because it needs the company that Q1's
sequence says will not exist until real money arrives.

### 3.1 What configuring before incorporation commits him to, and what is reversible

Read only from the fetched terms and help pages; nothing here is a view on French law.

**Effectively committing** (hard to undo):

- **Choosing a host at all puts contributions in that host's hands under §1(a) and §4(b).** The money
  is received and held by the Host, on behalf of the collective — not by Captain.
- **Changing host afterwards requires a zero balance**, the cooperation of **both** hosts, and it
  **cancels non-Stripe recurring contributions.** A zero-balance precondition means the exit is
  gated on having spent or transferred everything first, which is not a decision one takes on a
  Friday.
- **The 6 % host fee is taken before anything reaches the collective**, so the arithmetic of "what
  the cagnotte covers" is set by this screen too.
- **Public contributor recognition is on by default** (§4(f)), and a public contributor ledger is
  permanent by design. That collides with the erasure epic
  ([#194](https://github.com/TheCaptainCompany/captain-food/issues/194)) on a **US-operated
  platform** — noted as a collision, not adjudicated here.

**Still reversible today** (and only today):

- **Not configuring is free.** Q2's answer is *"not yet configured"*, so no host is selected and no
  money has moved through it. The whole fork is open.
- **Waiting until incorporation makes the collision-free option available** — *Organization* needs a
  legal entity, and the sequence already recorded (association now, company when real money arrives)
  produces one.
- **The choice of whether the cagnotte is inside the system at all** is untouched by anything on the
  Open Collective side. Both designs remain open and neither is recorded: an ACL ingesting the OC
  ledger as inbound integration events, or declaring the cagnotte out of the system and never folding
  it (register row **`OC-LEDGER`**).

**A register fact that changes the framing, and is not a legal point**: Open Collective is **not a
new door**. `docs/adr/HISTORY.md:106` marks ADR-009 **"✅ Active"** and `ADR-20260808-195315:79`
already records *"Accounting will be publicly visible on Open Collective"* — while
`docs/adr/HISTORY.md:207` records Open Collective as **post-PMF (M18+)**. The decision exists; it has
never been executed; and the two records disagree on when it should be.

---

## 4. Q6 — what a self-answered position may say, and how it must be labelled

The founder's Q6 answer hands the team ownership of a question this lens had graded **irreducible**.
The lens's position on what that ownership does and does not change:

> A question's bucket is a property of the question. His answer changes **who carries the residual
> risk**, not the grade.

### 4.1 What a self-answered position **MAY** do

- **Describe the mechanism factually.**
- **State which characterisation the team will build for**, choosing the **strictest plausible**
  reading as the build target.
- **Record analysis, alternatives, date, sources, grades and the quantified downside if wrong.**
- **Drive internal reversible artifacts.**

### 4.2 What it **MAY NOT** do — four hard lines

1. **It may not state a conclusion of law as settled.** The permitted form is: *"the team's working
   position is X; the underlying question remains grade (b) and open."*
2. **It may not leave the repo.** Never copied into the CGU, the restaurant terms, the mentions
   légales, the Stripe onboarding description, a *rescrit* (where an inaccurate description voids the
   guarantee), `/tarifs`, or a grant application — because externally the identical sentence becomes a
   **representation with liability attached**.
3. **It may not close a counsel question or count as diligence.** It is discoverable and it
   **evidences knowledge**, so it must record the **mitigations actually adopted**.
4. **Two carve-outs may not be self-answered at any labelling**:
   - **authorisation questions** — unauthorised payment services is criminal, and a self-formed view
     has **zero defensive value**;
   - **fiscal receipts**.

### 4.3 The label — fixed and greppable

> *A label does not travel by reference.*

Every self-answered position carries, inline:

```
WORKING POSITION (self-answered) — NOT legal advice, NOT clearance
Id: WP-<YYYYMMDD>-<slug>
```

and, with it:

- **the question**;
- **the position phrased as a build target**;
- **the unchanged grade of the underlying question**;
- **the counsel questions it does NOT close**;
- **the quantified downside**;
- **the basis**;
- **the review triggers**;
- **an expiry**.

The `WP-` id is **quoted inline wherever relied on**, and the register row stays **counsel-gated** —
never ✅ DECIDED.

### 4.4 Review triggers — seven, any one of which reopens the position

1. **First contact with counsel** — the position is **replaced, not amended**.
2. **Legal form chosen at incorporation.**
3. **The contribution stops being gratuitous** — any benefit, badge, priority or naming destroys the
   not-consideration limb on which **both** the Art. 22 counter-argument **and** *Tolsma* rest, in one
   product decision.
4. **Any external contact** — DGCCRF, CNIL, ACPR, URSSAF, a complaint, a médiateur, or a chargeback
   citing the contribution.
5. **A materiality threshold in euros, declared in advance.**
6. **A new instrument or new guidance.**
7. **6 months, or the first real payment — whichever is sooner.**

### 4.5 The executable form the lens asked for

> A gate that fails if a `WP-` id appears in any file in the declared **external-artifact** set.

CLAUDE.md prefers executable over prose, and *"may not leave the repo"* is exactly a rule prose cannot
enforce. Filed as a candidate card, class REVERSIBLE INTERNAL
([ADR-20260819-103112](../adr/ADR-20260819-103112-the-six-queue-answers-a-fiscal-host-in-the-money-path-and-a-refund-bearer-with-no-field.md)
§13). **Not built by this brief.**

---

## 5. Counsel questions G8–G11 — NOT TRANSCRIBABLE, and deliberately not composed

**Empty.** See the attribution notice at the head of this file.

The dispatch commissioning this brief listed counsel questions **G8–G11** as carried verbatim in the
round-4 aggregation. The aggregation contains **no G8, G9, G10 or G11** — the only mentions of
counsel questions in it are generic (*"close a counsel question"*, *"the counsel questions it does NOT
close"*), inside the §4 labelling spec. Nothing was transcribable, and nothing was invented.

**What is owed**, so the gap is actionable rather than merely noted: the four questions must come from
the `legal` lens directly, in its own words, numbered to continue the **G1–G7** packet in
[BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice](BRIEF-20260818-pre-filled-contribution-and-the-monthly-invoice.md)
§6. Until they exist, **no G-number above G7 may be cited anywhere in this repo** — a cited number
with no question behind it is the failure mode this section is preventing.

---

## 6. What this brief does not say

- It does **not** say that any Open Collective configuration is lawful, unlawful, safe or unsafe.
- It does **not** state, or imply, the French transposition of CRD Art. 22 — that is **G2**, and it is
  open.
- It does **not** grade the *appel public à la générosité* question raised against the association's
  publishing capacity — that is **G1**, and it is open.
- It does **not** clear the pre-filled contribution. The Art. 22 exposure recorded on 2026-08-18 is
  **unchanged** by the founder's reaffirmation; what changed is that the decision is now dated,
  knowing and taken twice.
- It is **not** legal advice and it is **not** clearance, and no aggregation of it becomes either.
