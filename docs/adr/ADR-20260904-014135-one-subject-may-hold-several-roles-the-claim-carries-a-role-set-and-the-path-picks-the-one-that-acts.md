# ADR-20260904-014135 — One subject may hold several roles: the claim carries a role SET, and the path picks the one that acts

<!-- Filename: docs/adr/ADR-20260904-014135-one-subject-may-hold-several-roles-the-claim-carries-a-role-set-and-the-path-picks-the-one-that-acts.md -->

## Status

Accepted (founder answer 2026-09-04, form question 2, on the `one-subject-one-role` Concern of
[PROP-20260831-180622](../proposals/PROP-20260831-180622-staff-authentication-the-roster-the-invitation-and-the-door.md)).
Records a founder decision; `Consulted:` block below
([ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)).
**Not built by this record**: the build is its own issue, sequenced AFTER part C step 6
(founder), and **the refusal `AuthSubjectHoldsAnotherRole` stands until it lands**. The Concern is
rewritten to this decision in the same change (LIVING proposal,
[ADR-20260801-020000](ADR-20260801-020000-proposals-are-living-documents.md)).

**Relates**: [ADR-20260818-004646](ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md)
(no business identifier in the provider — this record is READ IN ITS DIRECTION, see §Decision 1),
[ADR-0041](0041-acting-user-is-envelope-not-payload.md) (`domain_events.user_type` is envelope),
[#851](https://github.com/TheCaptainCompany/captain-food/issues/851) (the CUSTOMER/RIDER seam
asymmetry is the interim, not the design).

## Enforced by

n/a — no behavioral guarantee **yet**. When the issue lands, the tests named in §Decision 6 pin
it; the `rules.yaml` entry is written with that change (ADR-20260813-233418).

## Context

The Supabase provider replaces the `captain_food` claim object wholesale on every stamp, and each
stamper writes the whole object: `stamp_put_body` → `{ role: CUSTOMER, customer_id }` (hardcoded
on purpose, #437, never parameterised) and `stamp_rider_put_body` → `{ role: RIDER }`. Stamping
RIDER on a phone that already carries the customer object would erase the customer claim, so a
rider who also orders dinner is unrepresentable. Step 2c-i
([PR #852](https://github.com/TheCaptainCompany/captain-food/pull/852)) made `confirmRiderSignIn`
REFUSE such a subject with the typed, translated `AuthSubjectHoldsAnotherRole` — fail closed,
counted as `rider_claim_stamp_failed_total{reason="claim_conflict"}` — and registered the option
space as a Concern for the founder. The form of 2026-09-04 put three options to him:

> **"A — final vision: one claim, one binding per role; own issue after step 6; refusal stands
> until then"**

Young's consult (below) found the word *binding* ambiguous in a way that matters: an **id in the
token per role** would reverse ADR-20260818-004646 (which records `customer_id` in the claim as
the ONE business identifier at the provider, with phases to stop writing and erase it); a **role in
the token with the binding resolved in our Postgres** is the RIDER shape already built in 2b and
the direction that ADR chose. Under [TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml)
the reading is the team's, and the team takes the one that contradicts no record.

## Decision

1. **A binding is a ROLE in the token; the identity behind it is resolved in our Postgres per
   request.** The final claim object is `captain_food: { roles: [CUSTOMER, RIDER] }` — a set of
   roles and **no id for any of them**. This is ADR-20260818-004646 read forward, not reversed:
   `customer_id` in the claim stays the recorded legacy exception until that ADR's phases B/C retire
   it, and the CUSTOMER seam's claim-read (#851) is the interim, not the design.
2. **Additive producer, tolerant reader, one write** (young). The claim is not history and gets no
   upcaster: the stamper that writes `roles` keeps writing `role` + `customer_id` singular **in the
   same PUT** for as long as any deployed verifier reads them; a token where the two forms disagree
   is **no grant**, counted, never "pick one". Deploy order is **reader first, writer second** —
   an old verifier given a token with no `role` singular does not elevate, it locks the customer out
   of checkout, and at Friday peak that is the product.
3. **Existing subjects converge on next sign-in.** `stamp_decision`'s `Noop` today fires on
   `customer_id == target && role == CUSTOMER` regardless of shape; the issue extends it so an
   old-shape-but-correct-id object is a `Put` of the new shape. The single-role form is **read with
   no sunset recorded** — a sunset is a later decision with its own row.
4. **Role-as-path stays a pure function of (path, token).** `/{role}/graphql` selects which role
   of the set acts; `role_permitted` becomes "the set contains the path role"; no lookup enters the
   verifier; one request acts exactly one role. **`domain_events.user_type` (envelope, ADR-0041, a
   stored single value) is the PATH role, never the subject's set** — the one STORED shape this
   decision touches, and it does not change.
5. **The two hardcoded stampers stay two functions.** Neither takes a role parameter; each writes
   its own whole object including the set it may add its role to, and the customer stamper's
   `"role": "CUSTOMER"` literal stays where #437 put it.
6. **The tests that prove it** (young): `an_old_single_role_token_still_verifies_after_the_bindings_stamper`
   — a FROZEN JSON literal `{"role":"CUSTOMER","customer_id":"<uuid>"}`, never a call to the
   stamper, asserting a CUSTOMER grant bound to that id; its mirror `{"role":"RIDER"}` →
   `Identity::Unbound` for RIDER; `an_absent_or_unrecognised_role_grants_nothing_rather_than_customer`
   kept as the fail-closed half; and a disagreeing-forms token → no grant.
7. **Erasure is a right of the PERSON, not of a role** (legal-specialist). The issue carries the
   `deletion:` block for `Rider` and `Member` that PROP §6.5 deferred — owed there, not later — one
   erasure journey covering both roles with per-role retention grounds; the processor-side identity
   deletion happens only after the LAST role's tombstone; and `auth_subject_reservations` ("never
   released", PROP §6.4) needs its own retention ground or a one-way hash, or erasure is incomplete
   by design. No lens output is legal advice or clearance.
8. **Until the issue lands, the refusal stands**: a rider whose phone already carries a customer
   claim is told to contact `SUPPORT_CONTACT`; nothing is overwritten; the counter stays.

## Alternatives considered

- **B — the refusal is permanent for V0; a rider orders with another phone.** Cheapest, nothing
  moves; rejected by the founder — a wall the platform built for a Tours rider who is also a
  customer.
- **C — one login per role, linked in a table for erasure and support.** Every stamper untouched;
  two logins and two OTP flows for one person, erasure has to find both, and the link table is a new
  stored shape. Rejected by the founder.
- **A' — an id in the token per role.** The other reading of "binding"; rejected by the team
  because it reverses ADR-20260818-004646 and re-creates the cache the platform cannot invalidate.

## Consequences

### Positive
- The final shape is built once, in the direction the identity ADR already chose; the ACL stays
  a function of the path.
- The stored shape (`user_type`) does not move; the stamped shape is additive.

### Negative
- Until the issue lands, riders who are customers are refused — recorded as a known Tours-facing
  limitation, not a bug.
- The stamper grows a second form it must write consistently; the disagreement rule is the guard.

### Follow-up actions
- [x] Tracking issue [#857 "#639 part C, after step 6: one subject may hold several roles — the claim carries a role SET, the path picks the one that acts"](https://github.com/TheCaptainCompany/captain-food/issues/857),
      sequenced after part C step 6, carrying §Decision 2–7 as its checklist.
- [x] Concern `one-subject-one-role` rewritten to this record and checked, same change.

## Consulted (ADR-20260812-143619 — one line per lens)

Never to relitigate; **no lens output is legal advice or clearance**.

- **young** — the claim is state + a wire reply, not history: additive producer + tolerant reader,
  no upcaster; reader-first deploy order; the `Noop` line decides "next sign-in vs re-stamp";
  role-as-path stays pure; `domain_events.user_type` is the one stored shape and it is the path
  role; the frozen-literal test; and the binding ambiguity against ADR-20260818-004646 (§Decision 1).
- **legal-specialist** — Art. 17 is the person's right, not a role's: one erasure journey, the
  `deletion:` block owed in the issue, processor-side deletion after the last tombstone, the
  never-released reservation needs a retention ground or a hash (§Decision 7).
- **holub** — asked on the same form's Q1/Q4/Q5; nothing in that lens on this.
- **architect, beck, business-specialist, dba, evans, farley, graphql-architect,
  observability-agent, ux-designer, vernon** — not asked: the build is a later issue whose
  briefing invites the roster then; recorded so a lens never asked is distinguishable from one with
  nothing to say.
