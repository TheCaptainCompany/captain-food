# ADR-20260722-181500 — Delivery-delay satisfaction survey + post-delivery tip/reward prompt (#62)

## Status
Accepted — realizes the deferred "delivery-delay **satisfaction** survey" of the #60 delivery-dispatch
foundation (ADR-20260721-161939, decision 7). Landed with the implementation; supersedes the proposal
`docs/proposals/delivery-delay-satisfaction-survey.md` (now approved).

## Context
#60 tracks deliveries end-to-end (Captain-dispatch and restaurant self-dispatch) and emits the terminal
`delivered`/`late` facts, but deliberately deferred the customer survey. V0 delivery lives or dies on
**timeliness**, and a restaurant that self-dispatches needs a data-backed signal to decide whether that's
still worth it vs. handing routing to Captain. The product directive on #62 also asked to **prompt a tip
or reward for the delivery person** at the post-delivery moment, mirroring Uber Eats & Deliveroo.

Tipping already exists in the domain (`TipOrder` → `OrderTipped`, `Tip`, `TipRecipient`, `Tipper`;
ADR-012, Captain skims 0%), so "tip the courier" is not new modelling — it is a `RIDER` tip
(Captain-dispatch) or `RESTAURANT` tip (self-dispatch) surfaced at the right moment.

## Decision
Add one post-delivery feedback action beside the existing rating/tip triad, plus a restaurant-facing
read model; reuse the tip for the "reward":

- **Scalars** `DeliveryTimeliness` = `{ ON_TIME, ACCEPTABLE_DELAY, TOO_LATE }` (3-level for a gradient the
  restaurant can act on) and `DeliveryDissatisfactionReason` (optional free-text, `TOO_LATE` only).
- **Command** `RecordDeliverySatisfaction` on the **Order** aggregate → **event**
  `DeliverySatisfactionRecorded`. Guards mirror `RateOrder`: `OrderNotFound`, `InvalidOrderStatus` (only a
  DELIVERED order), and **record-once** via the new `DeliverySatisfactionAlreadyRecorded`.
- **Read side.** The customer's answer folds into the canonical `OrderTracking` read model as
  `delivery_timeliness` (surfaced as `Order.deliveryTimeliness`, null until answered → the client hides the
  prompt once set). A new single-event fold view **`View_DeliverySatisfaction`** (one row per surveyed
  order) backs the restaurant query **`restaurantDeliverySatisfaction`** (RESTAURANT/RESTAURANT_ACCOUNT/
  ADMIN) — the self-dispatch-vs-Captain signal.
- **Tip/reward reused.** The post-delivery SDUI sheet gains the timeliness question and a "tip your
  courier" section (presets + custom) that calls the existing `tipOrder` (recipient `RIDER`, or
  `RESTAURANT` for self-dispatch). No new tip command/event.
- **Objective vs subjective kept distinct.** #60's objective `late` fact is not recomputed here; the
  survey captures the customer's subjective verdict, and the restaurant view shows the subjective signal
  (objective delivery timing stays on the delivery read models).

Non-monetary rider "compliments"/kudos are **out of scope** (a future `RiderComplimented` fact); the V0
"reward" is the money tip, i.e. the Uber Eats / Deliveroo behaviour.

## Consequences
- **Completeness (ADR-0032):** new rules `DeliverySatisfactionRecordedOncePerDeliveredOrder` /
  `DeliverySatisfactionVisibleToRestaurant`, three behaviour tests, a customer story step
  (`RateDeliveryTimeliness`) + a restaurant story step (`ReviewDeliverySatisfaction`), translations, and
  the enriched `rating_sheet`. `make validate` 0 errors, workspace green (222 application tests).
- **Runtime:** hand-written Order handler + fold flag + the `OrderTracking`/`ordertracking` column
  (migration `20260722000000`, `ref_delivery_timeliness`, `View_DeliverySatisfaction`;
  `REQUIRED_SCHEMA_VERSION` bumped). Codegen: the Order `From<OrderTrackingRow>` template gained the field.
- **Read resolver wired (no stub):** `restaurantDeliverySatisfaction` reads through
  `application::queries::DeliverySatisfactionReadRepository` + `infrastructure::PgDeliverySatisfactionRepository`
  (over `view_deliverysatisfaction`), injected at the composition root; the emitter now emits the wired
  resolver + `From<DeliverySatisfactionRow>`. End-to-end complete — write path, both projections, the fold
  view, and the restaurant query.

## Alternatives considered
- **2-level thumb** (`ACCEPTABLE`/`TOO_LATE`) — leaner, but loses the on-time vs tolerated-late gradient.
- **DeliveryJob aggregate home** — rejected for Order, to sit with the existing feedback triad.
- **A cross-aggregate GROUP-BY insight view** — rejected; the fold-view DSL is single-aggregate per-row, so
  the restaurant aggregate is computed over the flat per-order rows instead.
- **A dedicated observability contract** — omitted to match precedent (rating/tip actions have none); the
  generic `command-acceptance` surface contract already covers the mutation.
