# ADR-20260904-015903 — The custody doors are a NEW fact: a rider hands a job back with the food's whereabouts, and the read models fold it

<!-- Filename: docs/adr/ADR-20260904-015903-the-custody-doors-are-a-new-fact-a-rider-hands-a-job-back-with-the-food-s-whereabouts-and-the-read-models-fold-it.md -->

## Status

Accepted — a **team decision by consent** under
[TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml)
([ADR-20260904-013834](ADR-20260904-013834-the-team-decides-option-spaces-and-spec-diffs-external-legal-and-admin-gated-actions-stay-with-the-founder.md)):
the whole roster was briefed before any code (ADR-20260809-013142), thirteen lenses answered, none
named a concrete harm in the option taken, and the founder reads this record. Realized by
[PROP-20260831-180622](../proposals/PROP-20260831-180622-staff-authentication-the-roster-the-invitation-and-the-door.md)
§11 rows **3-i** and **3-ii** (rewritten in this change).

**Relates**: [ADR-20260830-234532](ADR-20260830-234532-the-second-sitting-publish-france-wide-revocation-is-immediate-and-the-objection-chain-was-decided-22-days-ago.md)
§"the slice's real deliverable" (revocation of ACCESS is not release of CUSTODY),
[ADR-20260904-014136](ADR-20260904-014136-rider-restriction-ships-now-with-the-smallest-closed-set-of-grounds-and-counsel-can-only-add.md)
(step 4 names these two doors in its carve-out), [ADR-20260808-171056](ADR-20260808-171056-register-sweep-consent-decisions.md)
D2 (the house pattern for a free-text field: controlled enum + optional bounded note, declared in
the erasure scope), [ADR-20260731-160000](ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md)
(the `DeliveryJob` stream dies with the order), [ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md)
(why option (a) below was refused as shape staging).

## Enforced by

`rules.yaml#/DeliveryIssueLifecycle` (existing), and — written by steps 3-i/3-ii with their tests
(ADR-0032) — `DeliveryHandBackKeepsCustodyHonest` (a handback from a held job lands PENDING when the
food is not with the rider and FAILED when it is; a rider who does not hold the job is refused) and
`HandBackIsNeverALever` (no dispatchability, ranking or restriction fold reads a handback fact).

## Context

PROP §7.2: an immediately-restricted rider holding a customer's paid, cooked food has no way to hand
it back while the board still shows the job in flight and the customer's ETA counts down — *a paid
order nobody is told about, arriving through the security feature*. Step 4's restriction predicate
is per-operation and its allowed set is exactly `{report the issue, hand the job back}`; those doors
do not exist. PROP §11 row 3 called step 3 *"GREEN once the operations are additive"*.

At HEAD (`49211dfd`): `ReportDeliveryIssue`, `ResolveDeliveryIssue` and `DeclineDelivery` are
declared commands with handlers, inbox arms, behaviour tests and rules, and **no `api.yaml`
operation**; `DeclineDelivery` is PENDING-only by handler and by rule; `UnassignDeliveryFromPartner`
REFUSES a rider-held job (`partner_ref.is_none()`), has no `riderId`, and the lifecycle admits it
from ASSIGNED only; PICKED_UP has no exit but DELIVERED/CANCELLED. The briefing card offered three
options for "hand the job back": (a) reuse the partner unassign under a rider name, (b) widen
`DeclineDelivery`, (c) a new command and event. Every lens that took a position took **(c)**; six
independently found the card's grading of (a) as GREEN **false** (card defect, banked): reuse means
deleting the partner guard, adding a PICKED_UP edge and rewriting the rule — the same stored-meaning
change as (b), in the wrong vocabulary, with no rider identity and no custody fact.

The finding that dominated: **in every option, nobody is told.** `View_DeliveryJob` folds none of
`DeliveryUnassignedFromPartner`, `DeliveryDeclinedByRider`, `DeliveryIssueReported`,
`DeliveryIssueResolved`; `OrderTracking.delivery_status` has no release arm; `rider_id` is
"latest acceptance" and never clears; `DeliveryDispatchProcess` reacts to none of them. A door
opened on the write side alone re-creates §7.2 with a nicer button. The fold is slice content.

## Decision

1. **Option (c) — a new fact.** `HandBackDelivery { deliveryJobId, riderId, foodLocation }` →
   `DeliveryHandedBackByRider { deliveryJobId, orderId, riderId, foodLocation }` — `orderId`
   REQUIRED, folded from the aggregate's own state, never client input (the D-QW1 convention,
   ADR-20260808-234907; without it the customer's tracking mirror is unreachable — amended
   2026-09-04 at 3-ii's checkpoint, young). `foodLocation` is a
   dedicated kernel-or-delivery scalar **`FoodCustody`** (one name = one scalar):
   `NOT_COLLECTED | RETURNED_TO_RESTAURANT | WITH_RIDER`. **No free-text `reason`** on the handback
   (legal: the narrative the restriction ADR made unspellable would re-enter by the side door).
   `riderId` is asserted equal to the job's `rider_id` (as `confirmPickup` does); a partner-held
   job is refused (that is `UnassignDeliveryFromPartner`'s door). `DeclineDelivery` and
   `DeliveryDeclinedByRider` keep their meaning: an offer refused, a PENDING job, a fold no-op.
2. **Transitions keyed on custody, not on courage** (vernon): `from [ASSIGNED]` → PENDING with
   `NOT_COLLECTED` (derived, never asked — the rider is not at the restaurant); `from [PICKED_UP,
   OUT_FOR_DELIVERY]` with `RETURNED_TO_RESTAURANT` → PENDING (re-offerable, the restaurant has
   the bag); `from [PICKED_UP, OUT_FOR_DELIVERY]` with `WITH_RIDER` → **FAILED**, never PENDING — a
   PENDING job whose food is in a restricted rider's bag would be re-offered and is an oversell;
   FAILED already means "surface for manual handling, still cancellable". OUT_FOR_DELIVERY is in
   the set: the restriction scenario is mid-route. No `DeliveryStatus` value is added. The
   dynamic-target form (`via: foodLocation`, the `via: status` precedent) is used if the grammar
   admits a payload field other than `status`; if not, the grammar is extended in the same change,
   never a second event.
3. **The read models fold it, in the same slice.** `View_DeliveryJob`: the handback in `fedBy`
   and `status.derive`, `rider_id` and `provider` RESET (the `derive:` grammar gains an explicit
   `null` value — `DeriveVal::Null` — because today a YAML `null` is silently skipped, a latent
   defect closed here), new nullable columns `food_location`, `open_issue_kind`, `handed_back_at`;
   `OrderTracking.delivery_status` gains the arm and `courier` resets. **The generated
   `views.generated.sql` is applied by nothing** (farley): the view change ships as a hand-written
   `CREATE OR REPLACE VIEW` migration plus the `include_str!` chain entry, readers first, and a
   drift gate between the emitted SQL and the applied DDL is filed.
4. **The issue door follows the D2 pattern** (ADR-20260808-171056, controlling): `ReportDeliveryIssue`
   gains a closed **`DeliveryIssueKind`** (`ADDRESS_NOT_FOUND | CUSTOMER_UNREACHABLE |
   RESTAURANT_NOT_READY | FOOD_DAMAGED | VEHICLE_OR_INJURY | OTHER`), required on the command,
   nullable on the event (old rows carry none); `issue` becomes an optional note bounded to 300
   characters on the command (the event keeps its 1000 bound so stored rows parse), prompted on the
   rider screen as *facts only, no description of persons* (**amended 2026-09-05 at 4-iii-A's
   round 3, ux + reviewer + legal**: the rider sheet no longer carries the note — `text_area` has no
   renderer arm, so an unconditional note would have been an inert box; the field stays nullable and
   reachable by support callers, and the prompt returns with the arm,
   [#888 "Renderer: `text_area` and `tip_amount_selector` have no arm"](https://github.com/TheCaptainCompany/captain-food/issues/888));
   `ResolveDeliveryIssue.resolution`
   likewise: a closed **`DeliveryIssueResolution`** (`REASSIGNED | DELIVERED_BY_RESTAURANT |
   CANCELLED | OTHER`) plus a 300-character note. Both notes are personal data of the customer
   inside another aggregate's stream: covered by the order's tombstone
   (ADR-20260731-160000 names the `DeliveryJob` stream), and the spec-declared related-stream list
   that ADR promised is filed as owed before the first real order.
5. **API, additive**: `reportDeliveryIssue` `roles: [RIDER, ADMIN]`; `resolveDeliveryIssue`
   `roles: [RESTAURANT, RESTAURANT_ACCOUNT, ADMIN]` (whoever is told acts; the reporter never
   closes their own issue); `declineDelivery` `roles: [RIDER]`; `handBackDelivery` `roles: [RIDER]`.
   `DeliveryJob.openIssue` and `foodLocation` exposed nullable. Narrowing a role later is a break; widening is
   additive, so the sets start narrow.
6. **The step-4 seam is a closed key set, not a comment**: the api loader silently drops unknown
   operation keys today; step 3-i adds the validator rule `api-operation-key` (closed set = the
   keys in use); step 4 adds `whileRestricted: [ROLE]` to that set, a SUBSET of `roles:`, emitted
   into the SDL beside `@auth`, fail-closed by absence. The restricted rider's set is
   `{delivery (by orderId), reportDeliveryIssue, handBackDelivery}` — the query is in it, or the
   only live control on the page has no data.
   **Amended 2026-09-04 by [ADR-20260904-081527](ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md) §4 (team consent, step-4 briefing)**: the set is
   `{ myStanding, delivery, reportDeliveryIssue, handBackDelivery }` — `myStanding` added (the
   held job's `orderId` is otherwise unreachable), `myDeliveries` refused (it returns the PENDING
   pool), and operations with `roles:` omitted are unaffected by restriction by construction.
7. **One control, two exits** (ux-designer): `job_detail` gets a secondary *"Un problème"* beside
   the primary; one sheet that ROUTES — two buttons, *"Je continue, mais…"* opening the report
   sheet (3-i's kind chips, note, confirm — **the note removed 2026-09-05 at 4-iii-A's round 3**,
   see §4; six kind chips and confirm remain) and *"Je ne peux pas continuer"* opening the handback
   sheet (**amended 2026-09-04 at 3-ii's checkpoint, ux**: the SDUI has no client-side
   re-evaluation of conditions on form fields, so an exit chosen by a chip can gate nothing —
   the one runtime toggle is `open_bottom_sheet`; #872 carries the gap); the handback sheet asks
   nothing about the kind (the "report + hand back" two Tells of the briefing need an `on_success`
   chaining primitive the driver lacks — #872 — so the handback exit is one Tell, narrower not
   misleading); the food question asked only at PICKED_UP or later, as two cards; confirm
   *"Prévenir le restaurant"* on both exits; after a handback the screen becomes an instruction
   (`WITH_RIDER` → *"Rapportez la commande"* + address). In-app only, no call button
   (SUPPORT-CONTACT). The restaurant board renders a handed-back job as a pinned card headlined by
   where the food is, acknowledged through `resolveDeliveryIssue` (a sound has no DSL primitive;
   the board's read is `skipped_reads`, #745, so today the fold and the dead-man gauge are what
   tell the restaurant); the customer tracking replaces the ETA with a facts-only banner —
   *"La livraison n'arrivera pas à l'heure indiquée. Le restaurant est prévenu. Nous vous
   tiendrons informé ici."* — keyed on `Order.deliveryHandedBack`, a custody flag folded onto the
   order mirror the pushed frame carries, with NO order-status term (**amended 2026-09-04, legal +
   ux**: the original predicate keyed on `OrderStatus::OUT_FOR_DELIVERY`, which no projector
   produces, and the original copy promised a re-offer nobody performs; the from-ASSIGNED
   `NOT_COLLECTED` case leaves the order at READY) — never a counting ETA, never blank, never a
   promised remedy (#862 owns the remedy).
8. **Who is told, beyond the fold**: the re-offer is `DeliveryDispatchProcess`'s job and the
   process manager is fenced — filed as a follow-on with a `deferred:` block; a **dead-man gauge**
   `delivery_handed_back_unreassigned_age_seconds` (operational, OTLP, the #608 shape, threshold
   derived from `DELIVERY_OFFER_MAX_TTL_SECONDS`, emitted by a non-fenced timer worker beside the
   offer-timeout worker) is declared and emitted in 3-ii — a counter that fires on the handback and
   goes quiet during the stranding is the ADR-20260810-231300 defect class; the business fold
   **`DeliveryHandback`** (question: *when a rider hands a job back, does the customer still get
   that food, and how often at the dinner peak?*) is declared on the rider's `Deliver` activity,
   which carries no metric today, and 3-i declares `DeliveryIssueRate` on the same activity.
9. **A handback is never a lever** (legal): no dispatchability, ranking or restriction fold reads
   `DeliveryHandedBackByRider`; the rule carries a test.
10. **The fence is opened for exactly one additive arm.** `crates/infrastructure/src/inbox.rs`
    gains `DeliveryJobInbox::HandBackDelivery(cmd) => run(...)` and nothing else — the compiler
    demands it (E0004 on the regenerated enum), the fence's own rule working as designed.
    Antecedent for lifting it: the isolation programme's issue #780 closed on 2026-08-30
    (PR #783); the last commit on any fenced path is `c1a70a6f` (2026-08-30); no open issue carries
    `status/in-progress` except #639; no open PR touches a fenced path. The fence otherwise stands.
    The executor's self-check: `git diff --name-only origin/main -- <fence globs>` returns only
    `inbox.rs`, with one added line.
    **Amended 2026-09-04 by [ADR-20260904-081527](ADR-20260904-081527-rider-standing-is-a-grant-on-the-identity-row-the-doors-are-human-only-and-step-4-lands-in-three-slices.md) §8**: "exactly one" was the count standing in for the
    rule — the fence admits ONE additive arm PER NEW `receives:` entry, E0004-forced, and the fence
    globs are named there in one place.
11. **Two slices, both `HOLD: human`, both on the lower executor tier**: **3-i** the issue doors
    (D2 pattern, the three additive mutations, the issue fold + migration + `DeriveVal::Null`,
    the sheet's report-only path, the board's issue card, the closed-key rule, the story steps,
    the `DeliveryIssueRate` fold); **3-ii** the handback (command, event, scalar, transitions,
    rules, the one arm, the custody fold + `OrderTracking` arm, the sheet's second exit, the board's
    handback card, the tracking banner, the dead-man contract and worker, the `DeliveryHandback`
    fold). Deploy order inside each: view migration → command/event/handler dark → the screen
    binding is the release flip.

## Alternatives considered

- **(a) Reuse `UnassignDeliveryFromPartner` under `handBackDelivery`.** Refused by six lenses on
  the same fact: not additive at HEAD (guard, lifecycle edge, rule), a stored lie ("unassigned from
  its partner" on rider streams, unreadable on replay), no `riderId` (the mutant cannot be killed),
  no custody fact; and (a)-now-(c)-later is shape staging forced by an internal fence, which
  ADR-20260808-235113 does not license.
- **(b) Widen `DeclineDelivery` to held jobs.** One stored type with two fold effects (no-op vs
  state change) needs a discriminator forever; makes every clean decline look like an abandonment
  in the record (the freedom-to-refuse exhibit that helps the independent posture); records nothing
  about the food. Refused.
- **Ask the rider nothing about the food** (holub): derive `NOT_COLLECTED` from ASSIGNED — taken;
  derive `WITH_RIDER` from PICKED_UP — not taken, because a rider standing at the restaurant hands
  the bag back and that present fact decides re-offerability (business, ux, vernon).
- **Keep OUT_FOR_DELIVERY out** (evans: that is a delivery failure, not a handback). Not taken as a
  door restriction — the outcome honours it: `WITH_RIDER` lands FAILED.
- **Split 3-i/3-ii only if the fence holder refuses** (holub). The split is taken anyway for the
  environment's reason (restarts, run length), not for the fence.

## Consequences

### Positive
- The log says what happened, in the rider's word, with the one fact the kitchen decides on.
- The failure mode the step exists for is closed on the read side, where it actually lived.
- Step 4's carve-out binds two operations whose commands carry the principal.

### Negative
- Step 3 is `HOLD: human` twice where the proposal said GREEN once; the row is corrected.
- A hand-written view migration and a new emitter value (`DeriveVal::Null`) ride with 3-i.
- The re-offer stays a follow-on behind the process-manager fence; until it lands a handed-back job
  is PENDING with the board saying why and the dead-man gauge counting.

### Follow-up actions
- [x] Issues filed in this change: [#860](https://github.com/TheCaptainCompany/captain-food/issues/860)
      the re-offer PM step (fenced, `deferred:`); [#861](https://github.com/TheCaptainCompany/captain-food/issues/861)
      the `views.generated.sql` ↔ applied-DDL drift gate; [#862](https://github.com/TheCaptainCompany/captain-food/issues/862)
      no refund route from a delivery outcome (legal: L.216 non-delivery remedy, refund bearer per
      REFUND-BEARER); [#863](https://github.com/TheCaptainCompany/captain-food/issues/863) the
      spec-declared related-stream erasure list for `DeliveryJob` (owed before the first real order).
- [x] PROP §11 row 3 rewritten as 3-i / 3-ii, this change.

## Consulted (ADR-20260812-143619 — one line per lens)

Briefing before any code; **no lens output is legal advice or clearance**.

- **vernon** — one aggregate; transitions keyed on custody (`WITH_RIDER` → FAILED); Tell not Ask;
  the PM re-offer is fenced and owed; the one-arm carve-out is the compiler enforcing the fence's
  own rule.
- **young** — (a) is a change of meaning of a stored type; (b) needs an upcaster forever; the read
  models fold no release today; a new EVENT compiles fenced code untouched, only a new command
  variant needs the arm; the frozen fixture test.
- **evans** — the four terms; "hand back" has no word; *rendre la course* / *ramener la commande
  au restaurant*; one verb with a custody fact, `FoodCustody` a dedicated scalar; (a) is (b) with a
  cheaper label.
- **ux-designer** — one control, two exits; derive the food question when possible, two cards
  otherwise; the board's pinned card and the tracking banner; the mockup lines; `DeliveryIssueKind`
  chips; the `rider.restricted` predicate named for step 4.
- **graphql-architect** — the exact additive operations and role sets; the silently-dropped
  operation keys and the `api-operation-key` rule as step 3's half of the seam; a versionless name
  must never be rebound.
- **beck** — the projection test that is red today with no runtime change; the mutant (handler
  never compares `riderId`); the expected-red list per option; one `thrown:` per negative; the
  gate incantation with the pre-flight line.
- **legal-specialist** — the D2 pattern for `issue`/`resolution`; risk stays with the professional
  until physical possession, the customer must be told and refunded or re-dispatched; no free-text
  `reason` on a handback and a handback is never a lever; the counsel packet of five questions.
- **business-specialist** — capture-on-delivered makes the customer side a void, the restaurant
  make-whole has no flow and no field; `foodLocation` is the kitchen's input, `reason` is the
  platform's; the `DeliveryHandback` fold and its question.
- **observability-agent** — a counter is the dead-man defect; the gauge, its derived threshold, its
  non-fenced emitter home; free text never a span attribute.
- **farley** — (a) is broken at runtime, not GREEN; `views.generated.sql` is applied by nothing; the
  arm is E0004 by design; deploy order and the release flip; the hand-back template with per-gate
  wall-clock; there is no deployed monolith to prove it on.
- **holub** — the outcome in one sentence; (a)→(c) is shape staging; derive `foodLocation` where
  possible; defer `resolveDeliveryIssue` for ops and the restaurant reporter (taken in part: resolve
  stays, for the board's acknowledgement); WIP = 1; six stale drafts named.
- **dba** — the fold is the load-bearing storage work and is identical under (a) and (c);
  `DeriveVal::Null`; `DeliveryJob` streams declare no `deletion:`; storage cost negligible.
- **architect** — the register-check misattribution corrected (the carve-out's records are the
  PROP and ADR-20260830-234532, not ADR-20260904-014136); SUPPORT-CONTACT's note names the
  restaurant as the recorded destination; the business-metric rule has no gate and goes on the
  expected-red list by hand.
