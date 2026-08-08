# ADR-20260808-230800 — Rider/delivery slices 1–2 approved by the customer; slice 1 applied by the run; quick wins pulled forward; slice order adopted

**Status**: Accepted · **Date**: 2026-08-08 · **Decider**: the customer (product owner), live in
session, answering the full #348 decision batch via `AskUserQuestion` · **Tracking**:
[#348 "Epic: the rider/delivery write surface does not exist (24 of main's 32 validator warnings)"](https://github.com/TheCaptainCompany/captain-food/issues/348)

## The five answers (2026-08-08, ~23:05 UTC, each the recommended option)

1. **Slice 1 spec diff — "Approve as written."** The vocabulary-retirement diff prepared in
   [PROP-20260808-221424](../proposals/PROP-20260808-221424-rider-delivery-slices-1-2-spec-diff.md)
   §2 is approved exactly as prepared (retire the `AssignDeliveryToPartner`/
   `DeliveryAssignedToPartner` and `UpdateDeliveryPartnerStatus`/`DeliveryPartnerStatusUpdated`
   families across 6 spec files incl. the forced `TestDeliveryUnassignedFromPartner` rewire and
   two prose rewords; declare `PaymentFailed` + `CustomerIdentified` `nonProjectedEvents`).
2. **Slice 2's D6 `sends:` YAML (§3.2) — "Approve."** It lands only TOGETHER with the D6
   validator mechanism (checkable both ways), never before. §3.1 (crediting
   `BindCartToCustomer`/`GrantCustomerCredit` via their existing PM send steps) is validator code
   only and needed no spec approval.
3. **Customer-anxiety quick wins — "Prepare both now."** The `DeliveryPickedUp` → OrderTracking
   `fedBy` addition (+ `delivery_status` derive) and the checkout FAILED state for
   `PaymentFailed` are prepared as a spec diff ahead of the rider slices, per the parent
   proposal's §7 urgency; the prepared diff comes back for approval like slice 1.
4. **Slices 3–8 ordering — "Adopt value order."** The parent proposal's §6 order becomes the
   backlog order: 3 `rider-identity` → 4 `rider-decline` → 5 `delivery-issue-lifecycle` →
   6 `assignment-failure-recovery` → 7 `ops-delivery-surface` →
   8 `customer-delivery-reassurance`; one claimable issue per slice; every spec-touching slice
   still returns as a prepared diff for approval before application. (9–11 stay V1.)
5. **Application vehicle — "Apply now, this run."** Asked explicitly because
   PROP-20260808-221424 §6 named a plan-mode session as the applying vehicle: the customer chose
   immediate application by this run, the exact-text approval standing as the authorization. The
   spec-freeze rule's substance (no autonomous spec change without explicit customer approval of
   the exact diff) is satisfied; this ADR records the authorization verbatim so the exception is
   never read as precedent for UNapproved spec edits.

## Consequences

- Slice 1 is applied to `main` by this run per §2 exactly, gated by full `make rust`
  (0 errors; warning histogram diffed against a re-measured pristine baseline; expected
  43 → 37: `command-no-mutation` 13 → 11, `event-not-projected` 11 → 7).
- The D6 `sends:` block ships with the slice-2 validator slice (after PR #414
  "fix(#388): purge the test env-mutation race, gate the class, make CI failures diagnosable"
  merges — same validator territory).
- The quick-wins spec diff is prepared next and returns for approval; slice issues 3–8 are filed
  in the adopted order in the Prioritized backlog.
- PROP-20260808-221424's Status flips to `Approved`, naming this ADR.
