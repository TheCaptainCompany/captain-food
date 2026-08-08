# ADR-20260808-155656 — First consent-based ensemble decisions (rider + disappearance proposals)

## Status

Accepted by ensemble consent — **customer veto window OPEN** (any veto in the customer's next
responses reverts the decision; all six are unbuilt spec vocabulary or doctrine, so reverting is
an edit, not a migration). Recorded under ADR-20260808-144738 decision 3.

## Context

The customer asked whether the team had dealt with the decision package on
[PROP-20260808-141817](../proposals/PROP-20260808-141817-rider-delivery-write-surface.md) and
[PROP-20260808-142532](../proposals/PROP-20260808-142532-disappearance-terminal-states.md) — and
the honest answer was no: the coordinator had routed all eleven decisions to the customer,
PM-funnel style, despite ADR-20260808-144738 decision 3 classifying reversible, evidence-settled,
gated decisions as the TEAM's to make with an asynchronous customer veto. This ADR applies the
classification and records the team's decisions. The consent evidence is the three-lens
independent review of 2026-08-08 (reviewer/Beck: fidelity — every sampled citation confirmed;
architect/Young-Vernon-Evans: doctrine; graphql-architect/Byron: API), whose reports explicitly
endorsed or raised no named harm against each decision below; corrections they demanded were
folded in (`ba0d389`) before this record.

## Decisions — decided by the ensemble, veto window open

1. **Rider D1 — retire the `AssignDeliveryToPartner`/`DeliveryAssignedToPartner` family.**
   An un-accepted assignment is the oversell failure mode as an event type; the architect
   verified the events have never been emitted anywhere (no mutation, no PM, no ACL, no
   migration seed; production starts from an empty schema) — a pure deletion.
2. **Rider D2 — retire the `UpdateDeliveryPartnerStatus`/`DeliveryPartnerStatusUpdated`
   family.** A rejectable command wrapping an external fact (the ADR-0004 violation); the ACL
   already records the same fact as inbound `DeliveryStatusUpdated`.
3. **Rider D4 — one open issue per delivery job in V0.** The commands carry no issueId; the
   honest minimal model. Reversible: issue entities with history are an additive later change.
4. **Rider D6 — `PlaceReplacementOrder` gains spec-checkable dispatch coverage via a declared
   `sends:` on the wrapper-seam receive** (option b), parallel to the existing declared `emits:`
   precedent and checkable both ways (ref resolves AND the target inbox accepts the command).
   The architect authored this option and flagged the choice "PO-visible": it is hereby visible —
   veto reverts to option (a) DSL extension or (c) documented baseline warning.
5. **Disappearance D1 — the scoped mix**: projector-/event-carried composition for every field a
   surface must render after its FK target may vanish, plus a thin pinned dangling policy (null
   with pinned meaning; silent drop and join hard-errors banned), enforced by the `Option<_>`
   type flip rather than any scanner.
6. **Disappearance D5 — an erased restaurant's storefront host serves a parked "closed" page**,
   never the claim-your-subdomain landing (never invite resurrection of a dead business's
   address) and never a bare 404. Copy remains a customer-taste item for later.

## Deliberately NOT decided here — the customer's five

Per the same classification (money-path, legal-shaded, explicitly reserved, or genuinely
contested): **Rider D3** (release-event rename — the proposal's Concern reserves event-vocabulary
naming to the customer), **Rider D5** (credit consumption at checkout — money path), **Disp. D2**
(widening `OrderPlaced` + `CheckoutSnapshot`/`PaymentIntentCreated` — money-path event
vocabulary, reserved by Concern), **Disp. D3** (`OPTED_OUT` fold — legal-shaded: Art. 21 posture
and an owner promise), **Disp. D4** (enum+extended-guard vs orthogonal boolean — the second-door
finding left this genuinely contested between lenses, so it fails the evidence-settled test).

## Consequences

- `docs/proposals/DECISIONS.md` §20/§21 mark the six as ensemble-decided (veto open) and the five
  as customer-open; the proposals' Concerns tied to customer items stay unchecked, so both
  proposals mechanically remain `Proposed` until the customer's five land.
- Realization stays gated regardless: every one of the six passes through plan-mode spec changes
  with the normal gates; this ADR settles direction, not implementation.
- Process precedent: this is the first use of ADR-20260808-144738's consent mechanism; if the
  veto window pattern proves noisy or unsafe, amending that ADR is the correct fix, not silently
  reverting to the funnel.

## Refs

ADR-20260808-144738 · the three review reports (session
https://claude.ai/code/session_01AKgDqRbCcCxtUePWPRfxtp, 2026-08-08) · correction commit
`ba0d389` · [#348](https://github.com/TheCaptainCompany/captain-food/issues/348) ·
[#398](https://github.com/TheCaptainCompany/captain-food/issues/398) ·
[#347](https://github.com/TheCaptainCompany/captain-food/issues/347)
