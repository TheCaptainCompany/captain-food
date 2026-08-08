# BRIEF-20260808 — Obligation brief: listing opt-out objections

- **Date**: 2026-08-08
- **Author**: legal-specialist agent (internal obligation map — NOT legal advice; grades
  (a) established / (b) interpretation / (c) unknown; all (b)/(c) and cite-currency to be
  confirmed by licensed French counsel)
- **Session**: https://claude.ai/code/session_01AKgDqRbCcCxtUePWPRfxtp
- **Subjects**:
  [#347 "Decide the last annotated read-model hole: Restaurant fed by RestaurantListingOptedOut"](https://github.com/TheCaptainCompany/captain-food/issues/347) ·
  [#398 "Decide the API contract for tombstoned rows before the #194 projection sweep"](https://github.com/TheCaptainCompany/captain-food/issues/398) ·
  [#401 "Legal exposures from the opt-out obligation brief"](https://github.com/TheCaptainCompany/captain-food/issues/401)

---

## Obligation brief — listing opt-out (PROP-20260808-142532 §B, D3/D4)

### Q1 — Is the opt-out an Art. 21 GDPR objection?

**Yes, for a substantial subset of the population — grade (a) on the underlying law, (b) on the mapping to this event.**

- **The subset.** GDPR protects natural persons only (Recital 14), so pure legal-entity rows (SARL, SAS) are outside it as such. But SIRENE contains *entrepreneurs individuels* — sole traders whose registry row (name, address, SIRET, NAF) identifies a natural person. That business-register data of natural persons IS personal data is settled: CJEU C-398/15 *Manni* (2017), GDPR Art. 4(1). **Grade (a).** Note the aggravator: the pipeline's own scoring gives +3 to food trucks (NAF 56.10C), the micro-entrepreneur-heavy segment. The EI share of the ~200k rows is likely large in restauration. Named contact persons inside legal-entity rows are also natural persons. **Grade (b)** on proportions (unquantified).
- **Two legally distinct objections hide in one event.**
  - *Objection to direct marketing* — Art. 21(2)-(3): **absolute**, no balancing, processing for that purpose "shall no longer" occur. For email prospection, L.34-5 CPCE + CNIL B2B doctrine additionally apply: cold B2B email to a professional about their function is lawful **on an opt-out basis** — meaning the entire lawfulness of the ProspectionPipeline rests on objections actually working. **Grade (a)** for the regime; **(b)** for its exact application to sole traders' addresses (CNIL doctrine, verify currency).
  - *Objection to being listed/scored* — Art. 21(1): subject to a balancing test ("compelling legitimate grounds"), not absolute. For an EI who proved ownership and asked off, it is hard to argue compelling grounds to keep displaying them. **Grade (b).**
- **What "honored" requires** (Art. 21(3), Art. 12(2)-(3), Art. 5(2) accountability): cessation without undue delay across the marketing purpose — ALL channels, not just the one used, unless the subject narrowed it (grade a). Permanence: an objection stands until the subject themselves withdraws it — no expiry, no admin override (grade a). Record-keeping: the standard mechanism is the *liste repoussoir* / suppression list — retaining the **minimal identifier needed to keep suppressing** (SIREN/SIRET + the refusal + timestamp) is itself lawful and expected; deleting it would be the violation, because re-import would re-contact. This is exactly why D3's "row persists to remember the refusal" is the legally correct shape. Grade (a) on the doctrine, (b) on field-set minimality.
- **One structural gap the proposal doesn't see**: an Art. 21 objection is valid **through any channel** (reply email, phone call), and Art. 12(2) forbids making its exercise burdensome. Requiring a Google Business Profile ownership proof to stop receiving cold emails is defensible for *delisting* (identity matters) but **disproportionate as the only door for do-not-contact** — especially while the verifier is fail-closed, i.e. today the proven-opt-out channel cannot even succeed. An admin-recordable objection path (objection received by email → suppression) is needed; `ProspectMarkedCold` is not it (COLD reads as re-contactable pipeline stage, not a terminal legal objection). **Grade (b), exposure-level.**

### Q2 — Is the current state a live violation?

**Design-level: yes, a violation waiting for its first fact; act-level: probably not yet — grade (b), fact-dependent.** The elements of an actual Art. 21(3)/L.34-5 breach are (i) a recorded objection from a natural-person subject and (ii) subsequent prospection contact. Per `docs/STATUS.md`, SIRENE sync is paused and the opt-out command is fail-closed, so recorded opt-outs likely number zero and outreach is manual/admin-recorded — whether (i)+(ii) has ever occurred is a factual question I cannot ground **(c)**. But the moment the pipeline runs against real owners, `ProspectionPipeline` not folding the opt-out makes the breach mechanical, not hypothetical.

**Exposure if it fires**: Art. 83(5)(b) tier — violations of data-subject rights (Arts. 12–22) — up to €20M or 4% of turnover **(a)**. Realistic CNIL trajectory for a company this size: complaint → *mise en demeure* → injunction with *astreinte* → ordinary or simplified-procedure sanction (simplified capped at €20k) **(a)** on the procedure, **(b)** on likelihood. Commercial prospection has been a recurring CNIL enforcement priority with repeated published sanctions **(b — verify current enforcement posture)**. Classification: **BLOCKER for running prospection at scale; EXPOSURE today while paused.**

### Q3 — Which D4 shape is more defensible under audit?

**The orthogonal boolean, by a real but not decisive margin — grade (b) throughout (this is compliance-engineering judgement, not black-letter law).**

What a CNIL auditor asks for: (1) the register of objections (who/when/channel/scope); (2) the mechanism guaranteeing suppression across channels; (3) evidence it held continuously; (4) the documented procedure for lifting. The event log answers (1) and largely (3) under **either** shape — any re-listing would itself be an event, and the absence of a status-change event between the objection and today *is* the proof of continuity. That is a genuine compliance asset of the architecture; say so in the DPIA.

The shapes differ on **failure mode**, and the legal weighting is asymmetric:

- **Enum + dual guard**: the fatal path is write-side — one weakened/bypassed guard and an admin flip *clears the objection*. The event log would then record the violation, which under audit is evidence **against** you, not for you. Prevention beats provability: Art. 24/25 (data protection by design) rewards making the unlawful state unreachable, and a guard is a promise renewed at every code change.
- **Orthogonal boolean**: the fatal path is read-side — a forgotten filter re-exposes or re-contacts the owner. That is an incident, but the refusal record **survives** and the fix is a query change; the objection is never lost. Recoverable beats irreversible.

Also note: the enum conflates the two legally distinct objections of Q1 (delist vs do-not-contact) into one funnel value. The boolean conflates them too (one bit), but at least outside the funnel. **Regardless of D4's winner**, the do-not-contact fact should be auditable as its own artifact keyed on the *stable external identifier* (SIREN/SIRET), because the objection attaches to the person, not to your internal row — the SIRENE-ACL skip delivers this only if keyed externally. Either D4 option is *defensible* if the artifact trail exists; the boolean needs less ongoing proof.

### Q4 — Lifting the objection on genuine return

**A re-claim with fresh GBP ownership proof is a sufficient lifting artifact — grade (b).** An objection is withdrawn by the subject's own unambiguous act; no separate "consent" instrument is required for re-listing. Requirements for defensibility: (i) the lifting is **subject-initiated and identity-proven** (the claim command), never an admin flip; (ii) it is a timestamped event with the proof reference; (iii) the historical `RestaurantListingOptedOut` event is **retained** — it is the register, not stale data. One trap: **re-claiming the listing is not permission for renewed cold marketing** — the two objections of Q1 lift separately; once they are an actual partner, relationship communications rest on contract/legitimate interest anyway. Both D4 shapes can encode "left only via the claim path"; the boolean does it structurally (only the claim handler writes the column), the enum via a guard whitelist — same asymmetry as Q3.

### Q5 — What in §B is legally WRONG

1. **§B.3 reason 1 and the D3 tombstone-cons cell: "the population is legal-entity open data, not personal data under an erasure duty; #194's frame does not apply." Wrong as stated — grade (a) on the error.** The EI subset of SIRENE is personal data (*Manni*, Art. 4(1)); #194's GDPR frame — lawful basis, transparency, objection, DPIA question — **does** apply to storing, publishing, scoring (profiling, Art. 4(4)) and prospecting those rows. What legitimately does *not* carry over is the **erasure-by-tombstone remedy**: post-objection retention of the minimal suppression identifier is justified. So the **recommendation survives, the ratio does not** — and the wrong ratio is dangerous, because "not personal data" would also (incorrectly) waive Art. 14 notice duties and retention limits for the whole pipeline. §B.3 also contradicts §B.2's own admin row, which correctly calls the opt-out "Art. 21-shaped".
2. **Adjacent obligations the "not personal data" framing would have buried** (all grade (b), verify with counsel): (i) **Art. 14** — data not obtained from the subject: EI prospects are owed an information notice at latest at first contact; (ii) **CNIL référentiel "gestion commerciale"** — prospect data retention benchmark of **3 years from last contact**: an indefinite ProspectionPipeline row for a never-responding prospect conflicts with it; (iii) **SIRENE diffusion status** — EIs can restrict public diffusion at INSEE ("statut de diffusion" P); re-users must respect it — (confirmed by code check 2026-08-08: the ACL has no diffusion-status handling at all — zero grep hits in crates/infrastructure/src/integrations/sirene.rs and crates/sirene_ingest/src/; tracked in #401); (iv) the GBP-only objection door (Q1's Art. 12(2) point).

**Triage**: BLOCKER — ProspectionPipeline folding the opt-out before any real outreach runs (proposal step 5 is correctly scoped). EXPOSURE — objection channel beyond GBP proof; Art. 14 notice; retention benchmark; diffusion-status filtering. HYGIENE — correcting §B.3's legal ratio so the record does not carry a false premise into #194.

## Counsel packet (for a French avocat, ~1 hour)

1. **Personal-data perimeter**: we ingest ~200k SIRENE rows (legal entities + entrepreneurs individuels) and score them 0–10 for cold outreach. Confirm the EI subset is personal data end-to-end (storage, marketplace display, scoring, outreach), and whether the scoring reaches Art. 4(4) profiling with DPIA implications.
2. **Objection scope**: an owner's "remove my listing" (identity-proven) — must we treat it as an Art. 21(2) objection to ALL marketing channels by default, or may the UI offer separate "delist" and "do not contact" choices?
3. **Suppression-list retention**: confirm the minimal field set we may retain post-objection (SIREN/SIRET + refusal + timestamp?) against Art. 17(1)(c)/(3), and whether keying it on the state identifier rather than our internal ID is required or merely prudent.
4. **Objection channel friction**: our proven opt-out requires Google Business Profile ownership proof. Is that proportionate under Art. 12(2) for do-not-contact requests, and what lighter admin-recorded path must exist for objections received by email/phone?
5. **B2B prospection basis**: confirm the current CNIL position on L.34-5 CPCE for cold email to sole traders' professional addresses (opt-out regime, per-message mentions, Art. 14 notice timing at first contact).
6. **Retention**: does the CNIL référentiel gestion commerciale's 3-years-since-last-contact benchmark bind our prospect rows, and what deletion/anonymization behaviour satisfies it given an append-only event log?
7. **SIRENE re-use**: what exactly do the diffusion-status rules require of us as re-users — exclude non-diffusible EIs from import entirely, or only from public display?
8. **Lifting**: confirm a fresh ownership-proven re-claim suffices to lift the objection for re-listing, and that renewed marketing to a returned owner needs no separate consent once a contractual relationship exists.

### Account-erasure additions (E-series, 2026-08-08 — context: [BRIEF-20260808-account-erasure-two-path.md](BRIEF-20260808-account-erasure-two-path.md))

- **E1 — Two-path model**: we propose deactivate (recoverable, data kept, disclosed as not-deletion) + delete (Art. 17). Confirm the model, and set the dormant-account sunset for deactivated accounts that never return (CNIL inactivity benchmark applicable? what N?).
- **E2 — Grace window**: is a ≤30-day recoverable window before executing a deletion request compatible with Art. 17(1)/12(3)? Must immediate execution be available on explicit demand? From when does the one-month clock run?
- **E3 — Backup horizon**: acceptable lag for purging backups/replicas after production erasure (we would document a Meta-style ≤90-day backstop in the retention schedule); conditions on restore procedures.
- **E4 — Retention schedule sign-off**: validate the per-category carve-out table — in particular which of our artifacts count as *pièces justificatives* under L123-22, and whether the event-stream financial skeleton or an exported bookkeeping record is the right 10-year carrier.
- **E5 — Unspent credit**: a customer with wallet credit ([#158 "Customer credit-balance ledger (GOODWILL_CREDIT resolution) — own ADR"](https://github.com/TheCaptainCompany/captain-food/issues/158) machinery) requests deletion — must we refund before erasing, may the credit extinguish, and what does the accounting record of extinguishment look like?
- **E6 — Processor erasure proof**: Supabase holds identity as a processor — what instruction/receipt artifact satisfies Art. 28(3)(g); a deletion attestation per request or per audit period?
- **E7 — Erasure-receipt minimality**: the completion record keeps pseudonymous references (`customerId` etc.) as the accountability proof — compatible with minimization, or does it need its own retention?
- **E8 — Interface duties**: confirm the equal-prominence requirement between deactivate and delete under Art. 12(2) + DSA Art. 25 (dark patterns), so the UX spec can encode it as a hard rule.

### Funding-model additions (F-series, 2026-08-08 — context: [ADR-20260808-203443](../adr/ADR-20260808-203443-tips-voluntary-contributions-funding-model.md))

- **F1 — Voluntary contributions to a commercial SASU**: HelloAsso-style voluntary contributions collected by the SASU during and outside the order process — VAT treatment (taxable turnover vs outside-scope voluntary payment?), invoicing/receipt obligations, and whether any consumer-law framing constraints apply to the in-checkout ask (pre-ticked prohibition, DGCCRF drip-pricing rules).
- **F2 — Cascade pricing clause**: the declared fallback (monthly `fixed platform cost ÷ restaurant count`, 0 € when contributions cover costs) as a term in restaurant contracts — P2B 2019/1150 transparency requirements, notice periods for price changes, and what cost-baseline evidence must be publishable to make "in advance, in total transparency" contractually safe.
- **F3 — Public cagnotte**: displaying contributor names/amounts on per-contribution consent — confirm the consent artifact and any DSA/consumer-information duties on the public bet narrative ("le pari") so the claim "contributions cover the platform" is substantiated by the published accounting.

*This brief maps the landscape; none of it is legal advice, and items graded (b)/(c) plus all VERIFY-FIRST cites must be confirmed by licensed French counsel before launch decisions rest on them.*
