# Cross-tenant IDOR — obligation map and counsel packet

**Date-stamped 2026-08-16** · **Status**: Counsel questions IDOR-L1–IDOR-L9 pending a French avocat ·
**Prepared by**: the `legal-specialist` lens of the 2026-08-16 IDOR posture consult ·
**Defect record**: [DECISIONS §39](../proposals/DECISIONS.md) (IDOR-1), scope-corrected 2026-08-17 ·
**Work items**: [#178 "Write-side per-instance authorization"](https://github.com/TheCaptainCompany/captain-food/issues/178)
(write half) · [#618 "Seven read surfaces have no `ReadScope` — and two return the whole platform when called with no arguments"](https://github.com/TheCaptainCompany/captain-food/issues/618)
(read half) ·
[PROP-20260726-171500](../proposals/PROP-20260726-171500-write-side-per-instance-authorization.md)

> **Caveat — verbatim, and it does not shrink anywhere in this document.**
>
> **This brief was produced by an AI legal lens. It is not legal advice and it is not clearance. No
> aggregation of lenses upgrades a hedged finding to a settled one. Every claim in it is date-stamped
> 2026-08-16 and speaks only to the state of the system on that date.**
>
> Governed by [ADR-20260812-143619](../adr/ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md):
> no lens output, and no agreement between lenses, is legal advice or clearance. This document maps
> obligations and formulates questions; it never substitutes for licensed French counsel.

**Grades**: **(a)** established obligation · **(b)** interpretation for counsel to confirm ·
**(c)** unknown.

**Provenance note (2026-08-17).** The lens ran audit-only on 2026-08-16 and correctly wrote nothing.
This file lands its output in the house format one day later. The **substance** is the lens's; the
**arrangement and the IDOR-L numbering** are the recording executor's. Facts marked *verified* were
re-checked against `origin/main` on 2026-08-17 and carry a `file:line`; everything else is the lens's
finding as relayed and should be read as such. Where counsel needs a primary source, the tree — not
this brief — is the source.

## The finding in one line

Captain's API grants authorization by **role**, not by **relationship to the record** — so on
2026-08-16 **83 of 118 GraphQL operations** could be driven against another tenant's data by a caller
holding an ordinary, legitimately-issued credential. Two of the read surfaces return other tenants'
rows **when called with no arguments at all**. The exposure is a data-protection question and not only
a security one, because the records reachable this way include **unbounded customer free text in a
food business**, which predictably carries Art. 9(1) special-category data.

## What the defect is, factually (the basis for everything below)

- **Both sides, not one.** 76 of 86 mutations carry a role with no proven domain binding; 7 read
  surfaces carry no `ReadScope`. Detail, class split and the `file:line` trace live in
  [DECISIONS §39](../proposals/DECISIONS.md) and are not restated here.
- *Verified 2026-08-17* — two read surfaces (`restaurantReclamations`, `deliveryPartnerAvailabilities`)
  take an **optional** filter, fall back to a default filter on `None`, and issue a list query with **no
  tenant predicate**. The specification prose asserts a control that does not exist
  (`specs/ordering/api.yaml:207` *"Restaurant/ownership scoping is enforced server-side"*;
  `specs/comms/api.yaml:58` *"Ownership enforced server-side"*).
- *Verified 2026-08-17* — `EXTERNAL` partner callers authenticate against a **flat shared list** of
  pre-shared secrets (`crates/server/src/auth.rs:442,480-483`). There is no per-partner identity, so
  **a partner action cannot be attributed to a partner**. This matters below (IDOR-L7) independently of
  the IDOR itself.
- **No real personal data has passed through the system yet** — [DECISIONS §35 Q-L3](../proposals/DECISIONS.md)
  closed *"is there a real phone-verified end user?"* with **no**, on 2026-08-12. The obligations below
  are therefore mostly **forward-looking**, and that is precisely the window in which they are cheap.

## The two findings that SURVIVE the code fix

Scoping the reads and binding the writes closes the access path. It does **not** close either of
these, and both need design work that no `WriteScope` witness performs.

### 1. Free-text special-category data — Art. 9(1)

**Reclamation descriptions and order conversation threads are unbounded customer prose in a *food*
business.** That is not a hypothetical: the predictable content of a customer complaining about a meal
or messaging a kitchen includes **allergy statements, illness statements, and dietary-religious
statements** — health data and religious-belief data, both Art. 9(1) special categories. Nothing in
the schema constrains it and nothing filters it.

Three consequences, each of which outlives the access fix:

- **Individual notification becomes close to automatic.** Under Art. 34, notification to data subjects
  is owed where a breach is *likely to result in a high risk*. A breach touching special-category data
  is the paradigm case; the usual mitigating arguments (pseudonymisation, low sensitivity) are not
  available. **(a/b)** — the obligation is established, its application to these specific fields is for
  counsel.
- **It is a mandatory-DPIA trigger in its own right**, independent of scale, profiling, or the first
  real order. **(b)** — Art. 35(3)(b) plus the CNIL's list of processing requiring a DPIA.
- **It needs an Art. 9(2) basis for the ORDINARY case, and that is unsolved design work.** This is the
  part most easily missed: the question is not what happens in a breach, it is *on what lawful basis
  Captain processes health data at all* when a customer volunteers it into a complaint box. Art. 6 does
  not carry Art. 9 data; contract performance is not an Art. 9(2) condition. The candidate bases
  (explicit consent under 9(2)(a); the data being manifestly made public by the subject) each imply a
  **product change** — a consent moment, a field redesign, structured allergen capture instead of prose,
  or a documented no-store posture. **(b/c)** — see IDOR-L4.

### 2. Blast-radius unboundability

With **no tenant predicate, no pagination and no returned-row-count logging**, the team could not, after
the fact, bound how many records a breach touched. There is no artefact from which to reconstruct
"this caller saw N rows belonging to M tenants".

The legal consequence is asymmetric and expensive: **notification would have to assume the maximum.**
An organisation that cannot bound the scope of a breach cannot argue the breach was narrow, so both the
Art. 33 supervisory notification and any Art. 34 individual notification default to the whole
population. Row-count and result-scope logging on these surfaces is **cheap**, and it belongs on the
remediation artifact list alongside the authorization fix rather than after it. **(b)** for the precise
retention and content — see IDOR-L8, which is where minimisation cuts the other way.

## Counsel packet — IDOR-L1 … IDOR-L9

- **IDOR-L1 — Art. 32, is there a present failure?** Does an unremediated cross-tenant authorization
  defect of this breadth constitute a failure of Art. 32 "appropriate technical and organisational
  measures" **now**, on a system with no real users and an effectively empty log, or does the
  obligation attach only once personal data is present? **(b)** The team's working assumption is that
  Art. 32 is assessed against the processing actually carried out, so a pre-production system with no
  data has little exposure — but *shipping* in this state would be the failure, and we need to know
  which date is the one that counts.
- **IDOR-L2 — Art. 33/34, the notification threshold and the unbounded-scope problem.** If exploitation
  occurred, or could not be excluded, what triggers the 72-hour CNIL notification and what triggers
  individual notification? Specifically: **where the blast radius cannot be bounded (finding 2), must
  notification assume the maximum population?** **(a/b)** The 72-hour clock is established; the
  worst-case-assumption rule is the interpretation we need.
- **IDOR-L3 — Art. 35, DPIA timing and trigger.** Does the free-text special-category exposure
  (finding 1) independently make a DPIA **mandatory**, separately from the triggers already recorded
  against the first real order? And must it be **complete** before the first real order, or before
  first *processing* of such a field in any environment? **(b)**
- **IDOR-L4 — Art. 9(2), the ordinary case.** What lawful basis carries the **routine, non-breach**
  processing of health, illness and dietary-religious statements volunteered by customers into
  free-text reclamation descriptions and order conversation threads? Is explicit consent (9(2)(a))
  workable at the point the text box is presented, or does compliance require **redesigning the fields**
  — structured allergen capture, a no-free-text posture on the customer surface, or documented
  non-retention? **(b/c) This is unsolved design work and the access fix does not touch it.**
- **IDOR-L5 — Art. 5(2)/24, accountability and the public register.** What contemporaneous
  documentation of the defect — discovery date, assessed scope, remediation timeline, interim measures
  — must exist to demonstrate accountability? And a second-order question the team needs answered
  before it writes anything more: **does maintaining this register publicly help or harm that showing?**
  **(b)** The team's position is that an accurate register is an accountability asset and an
  *understated* one is a liability; confirm that publishing it does not create a distinct exposure.
- **IDOR-L6 — Art. 25, by design and by default.** Does shipping an API in which per-instance
  authorization is absent on 83 of 118 operations constitute an Art. 25 failure **independent of any
  breach**, given that authorization-by-relationship is the textbook "by default" control? **(b)** If
  yes, remediation is owed on the Art. 25 clock and not on the breach clock.
- **IDOR-L7 — Art. 28/30, partner non-attribution.** Delivery and ordering partners authenticate
  against a shared secret list with no per-partner identity, so a partner action cannot be attributed
  to a partner. Does that defeat the audit and demonstration obligations owed under Art. 28(3)(h) in
  partner processing agreements, and the Art. 30 record-keeping obligations toward those transfers?
  **(b)** Note this is **present tense** and is not gated on the first order.
- **IDOR-L8 — logging vs minimisation, the two-sided constraint.** Finding 2 says log returned-row
  counts and result scope so a breach can be bounded. Art. 5(1)(c) minimisation and Art. 5(1)(e)
  storage limitation cut the other way. **What may be logged, at what granularity, and for how long,
  such that the logs bound a breach without themselves becoming a second exposure?** **(b/c)**
- **IDOR-L9 — publication of an exploitable description.** *Verified 2026-08-17: the
  `TheCaptainCompany/captain-food` repository is **public**.* [DECISIONS §39](../proposals/DECISIONS.md)
  and `docs/STATUS.md` therefore publish, to anyone, a `file:line` description of a **live,
  unremediated** cross-tenant authorization defect. What are the obligations and risks of doing so —
  under Art. 32 (does publishing a working recipe defeat the "appropriate measures" showing?) and under
  French criminal law on providing means of unauthorized access to an automated data-processing system
  (Code pénal art. 323-3-1)? **(b/c)** The mitigating fact is that no production instance currently
  serves real users; see the publication split below, which is the team's interim answer and needs
  confirming or replacing.

## Publication split — recommended by the lens, followed by the team, with a condition

The lens recommended, and the team adopted, a **split between what may be published and what must
not**:

- **Publishable**: that the defect class exists, its breadth, its status, who owns it, and when it will
  be fixed — i.e. the **posture**. This is the Art. 5(2) accountability material and suppressing it is
  the failure mode this whole record exists to avoid.
- **Not publishable once a live instance serves real users**: the exploitable specifics — the operation
  names, the payload shapes, the no-argument call that returns another tenant's rows.

**The condition attached, and it is the operative half:**

> **If production is restored before the fix lands, the public posture page comes down until it does.**

The split holds today only because **no production instance serves real users** — Render is suspended,
no OVH spend has been authorized ([DECISIONS §35 INV-1](../proposals/DECISIONS.md)), and Q-L3 is *no*.
Those are the facts carrying the publication, and **the moment any of them changes, the publication
decision has to be re-taken, not inherited.** This is a standing trigger, not a note: whoever restores a
live instance owns executing it.

## Triage

- **BLOCKER (pre-launch)** — per-instance authorization on both sides (#178 + #618); an Art. 9(2) basis
  or a field redesign for free-text customer prose (IDOR-L4); the DPIA if IDOR-L3 confirms it is
  mandatory.
- **BLOCKER (conditional, immediate)** — taking the public posture page down if production is restored
  before the fix lands. Cheap, and it has an owner only if it is written down.
- **EXPOSURE** — blast-radius unboundability (finding 2 / IDOR-L8); partner non-attribution via the
  shared `external_tokens` list (IDOR-L7), which is present tense; the Art. 25 by-design question
  (IDOR-L6).
- **HYGIENE** — the two specification descriptions that assert a server-side ownership control the code
  does not implement (`specs/ordering/api.yaml:207`, `specs/comms/api.yaml:58`). A false control claim
  in the source of truth is how a reviewer stops looking; it owes a `specs/**` edit and a SPEC-LOG row.
