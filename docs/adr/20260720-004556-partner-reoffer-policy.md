# ADR-20260720-004556 — Bounded partner re-offer policy for DeliveryDispatchProcess

## Status

Accepted

## Context

DeliveryDispatchProcess (specs/processmanager.yaml) offers a ready DELIVERY order's job to the
delivery partner. Until now a partner decline (`DeliveryRejectedByPartner`, inbound) flagged the run
`REOFFER_REQUIRED` and blindly called `offer_job` again with a `TODO(saga)` note — the last open
orchestration gap (docs/sagas.md "Partner re-offer policy"). There was no attempt bound, no terminal
outcome, and nothing a restaurant could see when dispatch kept failing.

Constraints:

- The PM never appends to `domain_events` itself — it delivers facts to the owning aggregate
  (`DeliveryJob`), which records them (typed-step DSL, ADR-20260719-172821).
- V0 has a **single** delivery partner (the Avelo37 ACL is still a no-op stand-in), so "re-offer"
  can only target the same partner.
- Fail-closed operating principle: no silent unbounded retry loops.

## Decision

1. **Bounded re-offer, cap = 3 total offers.** When the partner explicitly declines an offered job,
   the PM re-offers it — the birth offer counts as attempt 1, so at most 2 re-offers follow. The
   counter is the new `offer_attempts` column on the PM's private state row
   (`delivery_dispatch_process_manager`, specs/database/tables/process_managers.yaml). A decline on
   a run that is not `OFFERED` (already resolved/terminal) is a benign skip.
2. **V0 single partner; multi-partner ranking is the extension point.** Every re-offer goes to the
   same partner through the `delivery.offer_job` port. When more partners exist, the re-offer step
   is where a partner-ranking policy plugs in (choose the next partner instead of repeating the
   same one); `offer_attempts` keeps its meaning unchanged (total offers made, whatever the
   targets).
3. **Exhaustion is a terminal domain fact.** The 3rd decline emits **`DeliveryDispatchFailed`**
   (business payload: `deliveryJobId`, `orderId`, `restaurantId`, `attempts`, `lastReason`),
   delivered by the PM to the `DeliveryJob` aggregate's stream (the PM is authoritative for the
   dispatch outcome, exactly like the `DeliveryRequested` birth). The job's fold and the read
   models (`View_DeliveryJob.status`, `OrderTracking.delivery_status`) turn it into
   `DeliveryStatus.FAILED`, so the restaurant's delivery board surfaces the job for manual
   handling. The PM run closes `FAILED` — fail-closed, no retry beyond the cap
   (rules.yaml#/DispatchRetriesAreBounded).
4. **Enum slot reuse.** `DeliveryDispatchProcessStatus.REOFFER_REQUIRED` (the manual-handling flag
   this policy replaces) is repurposed **in place** as `FAILED` (ordinal 2 under the ADR-0037
   declaration-order INTEGER mapping). Stored rows keep a faithful meaning — both values flagged
   "needs manual handling" — and `COMPLETED` keeps ordinal 3, so no data migration is needed.
5. **Offer timeouts are explicitly OUT (deferred).** A partner that never answers leaves the run
   `OFFERED`. The repo has no time-based worker/sweep pattern — the PM runner and the projection
   worker both drain `domain_events` by type/stream, and building a scheduler for this one case is
   not justified. Intended mechanism when needed: a sweep hosted in the PM runner's existing poll
   loop that treats `OFFERED` rows with a stale `last_update_utc` (older than a configured offer
   TTL) as declines — feeding the same bounded re-offer counter — or, preferably, an expiry
   callback from the partner ACL once the real Avelo37 integration lands.

### DSL note

The cap comparison (`offer_attempts < 3`) is an integer bound, not expressible in the typed-step
guard `that` form (structural `const` enum matches only), and a leg's step list is linear (no
branching). The `DeliveryRejectedByPartner` leg therefore carries the two branches with explicit
step `note`s (re-offer branch: `call offer_job` + `state set`, ended by a `skip` guard; exhausted
branch: `deliver DeliveryDispatchFailed` + `state set FAILED` as the fall-through), and the
orchestrator implements the branch. A typed `bounded-retry` step (or numeric guard predicates) is
possible future DSL work if a second bounded loop appears.

## Alternatives considered

- **Unbounded re-offer with backoff** — rejected: violates fail-closed; with a single V0 partner it
  would hammer the same partner forever with no operator signal.
- **Counting rejections from the DeliveryJob stream fold** (no state column) — elegantly
  event-sourced, but the attempt count is dispatch-run state, not job state: a future
  re-dispatch (new run) must restart the count while the stream keeps the old declines. The PM
  state table is the correlation identity of the run, so the counter lives there.
- **Exhaustion as a thrown ops error** (guard `throws`) — rejected: an ops abort surfaces on
  `/saga`, not to the restaurant; the failure is a business outcome the restaurant must act on, so
  it must be a domain event feeding read models.
- **Appending `FAILED` as a new enum member and dropping `REOFFER_REQUIRED`** — would shift
  `COMPLETED`'s stored ordinal (3 → 2) and require remapping data in place; the in-place slot
  reuse is semantically faithful and migration-free.
- **Building an offer-timeout scheduler now** — rejected (see Decision 5): no existing sweep
  pattern to host it cleanly; deferred with the intended mechanism documented.

## Consequences

### Positive

- The last open DeliveryDispatchProcess orchestration gap is closed with a deterministic, bounded
  policy; every dispatch run terminates (`ACCEPTED`/`COMPLETED`/`FAILED`).
- Restaurants see failed dispatches on their existing delivery board (`View_DeliveryJob`
  status = FAILED with `last_partner_rejection`) — no new query surface.
- The multi-partner future has a named seam (the re-offer step + `offer_attempts`).

### Negative

- A partner that never answers still parks the run `OFFERED` until the deferred timeout mechanism
  lands (Decision 5).
- The DSL carries the cap branch in notes, not typed structure — the validator proves the wiring
  (steps/values/targets) but not the bound itself; the bound is asserted by the behaviour tests
  (`TestDispatchPartnerRejected`, `TestDispatchAcceptedAfterReoffer`, `TestDispatchFailsAfterOfferCap`,
  `TestDeliveryJobRecordsDispatchFailure` ↔ rules.yaml#/DispatchRetriesAreBounded).

### Follow-up actions

- Offer-timeout sweep (Decision 5) once the partner ACL or a scheduler pattern exists.
- Multi-partner ranking policy in the re-offer step when a second partner is integrated.
