# The `RIDER_REQUESTED` restriction ground — what counts as the rider's message, and where it lives

**Date-stamped 2026-09-04** · Authority: [ADR-20260904-152807](../adr/ADR-20260904-152807-the-admin-s-hands-one-custody-truth-read-at-query-time-a-door-that-refuses-until-the-notice-exists-and-two-slices.md)
§6, realizing [ADR-20260904-014136](../adr/ADR-20260904-014136-rider-restriction-ships-now-with-the-smallest-closed-set-of-grounds-and-counsel-can-only-add.md).
Linked from `specs/screens/system.yaml`'s `restrict_rider_sheet` `gaps:` (the sheet's one sentence
under this ground: *"Conservez le message du rider (procédure)."*).

**No lens output on this page is legal advice or clearance** — it maps a procedure this product needs
so an admin knows what to do today; retention and evidentiary sufficiency are open counsel questions
(§6 below), not answered here.

## What this ground is

`RIDER_REQUESTED` is one of the four closed grounds an admin may cite when restricting a rider's
access (`scalars.yaml#/RiderRestrictionGround`). Unlike the other three (`ELIGIBILITY_DOCUMENT_LAPSED`,
`IDENTITY_MISMATCH`, `ACCOUNT_COMPROMISE`), which are the PLATFORM's own finding, this one exists
because the RIDER asked to be restricted — a request the platform is honouring, not a fact the
platform observed. The sheet carries no free text (ADR §6): the ground alone is what the Art. 11 log
stores, so the message that actually justifies the restriction has to be filed SOMEWHERE ELSE, and
this page is that somewhere.

## What counts as "the rider's message"

Any of the following, in the rider's own words or voice, unambiguously asking to be restricted:

- an SMS or WhatsApp message to the support number;
- an e-mail to the support address (`SUPPORT_CONTACT`);
- a signed note (paper or a photo of one) handed to an admin in person;
- a phone call, PROVIDED the admin who took it writes a short contemporaneous note of what was said
  (a call alone, unrecorded and unwritten, is not filable — nothing survives to file).

A message merely reporting a problem ("my bike is broken") is NOT a restriction request unless the
rider explicitly asks to stop receiving runs. When in doubt, the admin asks the rider to confirm in
writing before restricting on this ground.

## Who files it, and where

The ADMIN who restricts the rider files the message **the same day**, keyed on:

- `rider_id` (the restricted rider's id), and
- the `RiderRestricted` event's `decidedAt` (the exact instant the restriction sheet's confirm button
  fires — visible in the admin's own `restrictRider` operation).

Filed today as a dated entry in the team's shared incident/ops log (the same channel the platform
already uses for other admin-side records), named `RIDER_REQUESTED-<rider_id>-<decidedAt>` and
carrying: the message itself (or its photo/screenshot), the channel it arrived on, and the filing
admin's name. **No dedicated storage exists in the product yet** — this is a manual procedure, not an
automated one, until a dedicated evidence store is built (a recorded gap, not a decision to build one).

## Retention

The retention period for this filed message is **the Art. 11 log's own retention** — i.e., it must
survive at least as long as the `RiderRestricted` event it justifies survives in `domain_events`
(which is immutable and, absent a deletion engine reaching it, indefinite today). **The acceptable
FORM of the filed message and whether a shorter retention is defensible are counsel question 6**
(open, ADR-20260904-081527's counsel packet) — this page states the working procedure, not the
answer.

## Open counsel questions this page does not answer

- **Q6**: retention period and acceptable form of the `RIDER_REQUESTED` message.
- **Q7**: does the rider have a right to know WHICH human decided (Art. 15 GDPR recipients vs. the
  admin's own data)?
- **Q8**: is the admin's chip-selection + confirm sufficient evidence of a human decision, or is a
  logged acknowledgement (e.g. the admin typing their own name) needed?

Until counsel answers these, this procedure is the team's best-effort filing discipline, reviewable
and revisable, never a compliance guarantee.
