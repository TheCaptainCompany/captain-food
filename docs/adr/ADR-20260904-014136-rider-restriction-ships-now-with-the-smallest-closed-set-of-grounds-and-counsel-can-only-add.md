# ADR-20260904-014136 — Rider restriction ships now with the smallest closed set of grounds, and counsel can only add

<!-- Filename: docs/adr/ADR-20260904-014136-rider-restriction-ships-now-with-the-smallest-closed-set-of-grounds-and-counsel-can-only-add.md -->

## Status

Accepted (founder answer 2026-09-04, form question 3, on the `revocation-grounds` Concern of
[PROP-20260831-180622](../proposals/PROP-20260831-180622-staff-authentication-the-roster-the-invitation-and-the-door.md)).
Records a founder decision; `Consulted:` block below. **Realized by part C step 4** (not built by
this record); the Concern is rewritten to this ruling in the same change and stays unchecked until
step 4 lands the set.

**Note (2026-09-04, [ADR-20260904-081527](ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md) §7)**: the scalar this record cites as `RevocationGround` (the
proposal's name at the time) is built as **`RiderRestrictionGround`** — a naming refinement before
anything is stored; the four values and every rule below are unchanged.

**Relates**: [PUBLISH-PRECONDITIONS](../decisions/PUBLISH-PRECONDITIONS.yaml) (open, counsel —
carries the founder's timing *"After product on production workibg"*), [RIDER-REVOCATION-TTL](../decisions/RIDER-REVOCATION-TTL.yaml)
(decided: a restriction bites on the next request),
[ADR-20260810-194548](ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md)
(riders get revocation with a reason code, a log **and human review** — the review path is owed,
§Decision 6), [REVOKED-COLLEAGUE-NOTICE](../decisions/REVOKED-COLLEAGUE-NOTICE.yaml) (restaurant
staff, a different instrument, unaffected).

## Enforced by

n/a — no behavioral guarantee **yet**; step 4 writes the `rules.yaml` entries for §Decision 2, 4
and 5 with their tests (ADR-20260813-233418, ADR-0032).

## Context

PROP §6.3 stores the ground of a rider restriction as a closed scalar `RevocationGround` in
`RiderRestricted { riderId, ground, decidedAt, effectiveAt }` — a NEW event type, immutable once
appended. The Concern said the vocabulary is *"reviewed before it is stored, not after"*; the
founder's timing (2026-08-31) engages counsel only after the product works in production. Those
two could not both hold for step 4, so the form put it to him:

> **"A — build step 4 now with the smallest closed set naming no work-performance ground; counsel
> can only add"**

## Decision

1. **Step 4 builds now.** The Concern's "reviewed before stored" yields to the founder's timing;
   what replaces it is the smallest set, the additive-only rule, and the review path.
2. **The set names the FACT the platform observed, never a verdict about the rider**, and ships
   with four values — the legal lens's proposal, graded by that lens, **not clearance**:
   `RIDER_REQUESTED` (the rider's own act; Art. 17 GDPR where it precedes erasure — a restriction
   and not availability because the platform records it on the rider's behalf and `ReinstateRider`
   undoes it), `ELIGIBILITY_DOCUMENT_LAPSED` (the platform's own vigilance duty, Code du travail
   L.8222-1 / L.8221-6, work authorisation L.8251-1 — reach graded (b)), `IDENTITY_MISMATCH` (the
   authenticated subject is not the verified person; the reservation table is the proof artifact)
   and `ACCOUNT_COMPROMISE` (GDPR Art. 32, protective, `ReinstateRider` the mandatory exit).
   `LEGAL_ORDER` is the optional fifth counsel may add now.
3. **Refused before counsel, and recorded as a trade-off**: every performance or behaviour ground
   (`DECLINED_JOBS`, `LATE_DELIVERIES`, `LOW_RATING`, `INACTIVITY`, `CUSTOMER_COMPLAINT` — the right
   to refuse a proposal without penalty and the subordination criterion make a stored decline-keyed
   ground the strongest requalification exhibit obtainable; the business lens's "no lever for peak
   decline" is an ACCEPTED cost), `SAFETY_INCIDENT_REPORTED` and `FRAUD_SUSPECTED` (the shape of a
   *mise à pied conservatoire*; need the review path first and a time bound — counsel adds them
   with the procedure), and **`TERMS_BREACH` / `OTHER` never** (a catch-all is free text by the back
   door).
4. **Counsel can only add — and cannot subtract either** (young). Adding a variant is additive on
   the spec, its cost is **deploy order, readers first** (PROP §2.2 extended verbatim to this
   scalar). A variant is never removed or renamed: a ground counsel later objects to is made
   **unspellable at the command door**, the `SUSPENDED` move, while stored rows keep it. A regretted
   stored value is retired by a new fact — `RiderReinstated`, then if warranted `RiderRestricted`
   with the correct ground — never a rewrite.
5. **The fold keys on the FACT, never on the ground's value.** Dispatchability derives from
   `RiderRestricted` without a later `RiderReinstated`; the ground is attribution for counsel and
   for the notice, not a term in the derivation. An unknown ground therefore contributes nothing to
   a grant and the rider is not dispatchable **by construction** (PROP §6.4: a read predicate is a
   GRANT test only). For decoding, step 4 lands a **read-only catch-all variant the command door
   cannot spell** (tolerant reader — a codegen feature the tree does not have, built in step 4),
   because with strict decoding an unknown ground fails the whole stream load
   (`event_store.rs`, `map_err → Err`) and blocks `ReinstateRider` too. Unverified and to be
   verified in step 4: the projector's posture on the same row.
6. **What the event and the notice must carry — Directive (EU) 2024/2831 Chapter III**, article
   numbers graded (b), transposition date VERIFY-FIRST (~2 Dec 2026; already flagged as moving in
   ADR-20260810-194548). (i) **The decision is taken by a human** (Art. 11(5)): the human is the
   envelope `domain_events.user_id` (ADR-0041), so `RestrictRider` is **unspellable for a system or
   process-manager principal** — a human ops role only, at the command door. (ii) **Written reasons
   and a contact** (Art. 11(1)–(2)): the `fr` translation of the ground IS the reasons text and
   becomes a counsel-reviewed artifact; the rider screen shows the ground, `decidedAt`,
   `effectiveAt`, `SUPPORT_CONTACT` and **how to contest** — a screen showing the ground with no
   route to contest is the "control that does nothing" failure in legal dress. (iii) **Review**
   (Art. 11(3)–(4)): `RiderRestrictionReviewRequested` / `…Reviewed` (or equivalent) is **owed
   before the transposition date**; `ReinstateRider` is the rectification path meanwhile; filed as
   its own issue in this change. (iv) The four grounds are human-observed; no automated signal
   suggests a restriction (Art. 9 / GDPR Art. 22 not engaged), and the record says so.
   (v) Retention: the restriction event is the Art. 11 log; access logs and the Art. 30 entry per
   the existing erasure ADRs.
7. **The Concern text is rewritten to this ruling** and stays unchecked until step 4 lands the
   set; counsel's review is owed under the PUBLISH-PRECONDITIONS timing and, when it comes, may
   only add.

## Alternatives considered

- **B — hold steps 4 and 5 until counsel has looked; build 3 and 6 first.** Nothing stored that
  counsel has not read; cost: a signed-in rider cannot be restricted except through the legacy
  `SUSPENDED` status step 4 exists to retire, for as long as production takes. Rejected by the
  founder.
- **C — a ground-less event with the reason held outside the log.** Loses the stated reason the
  platform-work notice duties require. Not offered as a pick.

## Consequences

### Positive
- The rider slice completes in the founder's order; the smallest set is the one page counsel
  reviews instead of a code audit, and it can only grow.
- Fail-closed by construction: the fold never reads the ground.

### Negative
- A value counsel dislikes stays parseable forever (retired at the door, not from the log).
- Ops cannot restrict for a reason outside the set until counsel widens it; the business lens's
  peak-decline lever stays absent, on purpose.
- Step 4 carries a codegen feature (the unspellable catch-all variant) it did not carry before.

### Follow-up actions
- [ ] Step 4 dispatch card carries §Decision 2–6 verbatim as scope; the `fr` strings of the four
      grounds are written as counsel-reviewable copy.
- [x] [#858 "Rider restriction review path (Directive (EU) 2024/2831 Art. 11(3)–(4)): RiderRestrictionReviewRequested / Reviewed before the transposition date"](https://github.com/TheCaptainCompany/captain-food/issues/858).
- [x] Concern `revocation-grounds` and PROP §11 row 4 rewritten, same change.

## Consulted (ADR-20260812-143619 — one line per lens)

Never to relitigate; **no lens output is legal advice or clearance**.

- **legal-specialist** — the four-value set and its instruments, the refused set and why, the
  Directive 2024/2831 duties on the event, the notice and the review path, and the consistency
  condition against ADR-20260810-194548 (§Decision 2, 3, 6).
- **young** — additive-only with reader-first deploy order, never remove (unspellable at the door
  instead), retire by new fact, the fold keys on the fact, and the strict-decoding stream-load
  failure that motivates the catch-all variant (§Decision 4, 5).
- **holub** — asked on the same form's Q1/Q4/Q5; on Q5 recorded the cost of building restriction
  before the rider population exists (journal line); nothing further in that lens here.
- **business-specialist** — not asked today; its earlier position (no lever for peak decline,
  ADR-20260830-234532) is carried as the accepted cost in §Decision 3.
- **architect, beck, dba, evans, farley, graphql-architect, observability-agent, ux-designer,
  vernon** — not asked: step 4's briefing invites the roster; recorded so a lens never asked is
  distinguishable from one with nothing to say.
