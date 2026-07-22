# Design Proposal — Delivery-delay satisfaction check + post-delivery tip/reward prompt (#62)

> **Status:** ✅ Approved & implemented (2026-07-22) — landed with **ADR-20260722-101500**. The recorded
> decision is the ADR; this proposal is kept as the design narrative. One follow-up remains: the
> `restaurantDeliverySatisfaction` read resolver (stubbed until its read repo lands — see the ADR).
>
> **Depends on:** **#60** (delivery dispatch strategy foundation, ✅ ADR-20260721-161939) — it emits the
> terminal delivery signals (`delivered` / `late` facts across both Captain- and restaurant-dispatch)
> this survey consumes. #60 explicitly deferred *"the delivery-delay **satisfaction** survey flow"* to
> this issue (proposal §3.10, decision 7).
>
> **Owns:** the customer-facing **delay-satisfaction** survey, the **restaurant-facing timeliness
> insight** (the self-dispatch-vs-Captain decision signal), and — per the product directive on #62 —
> the **post-delivery tip / reward prompt** for the delivery person, mirroring Uber Eats & Deliveroo.

---

## TL;DR

After a **delivered** order, ask the customer one lightweight question — *was the delivery delay
acceptable?* — and, at the **same** post-delivery moment, offer to **tip / reward the person who
delivered it**. This is exactly the Uber Eats / Deliveroo post-delivery pattern: one screen that
combines *"how did it go?"* with *"add something for your courier."*

Two things are genuinely new; a third already exists and is only **re-surfaced**:

1. **New — the delay-satisfaction signal.** A one-shot customer answer captured as a command → event →
   read model. Purpose (from the issue): *prove V0 delivery is good enough on the one thing it lives or
   dies on (timeliness)*, and *feed the restaurant a data-backed self-dispatch-vs-Captain decision*.
2. **New — the restaurant-facing timeliness insight.** A `View_*` read model that folds the survey
   answers together with #60's objective `delivered`/`late` facts, split by dispatch mode.
3. **Reused — the tip.** Tipping is **already fully modelled** (`TipOrder` → `OrderTipped`, `Tip`
   entity, `TipRecipient`, `Tipper`; ADR-012). "Tip the delivery person" is simply a **`RIDER`** tip
   (Captain-dispatch) or a **`RESTAURANT`** tip (restaurant self-dispatch), sent from the same
   post-delivery prompt. **No new tip command/event** — the work is surfacing it at the right moment.

The net V0 addition is **one command, one event, one read model, one restaurant query, and one enriched
SDUI prompt** — plus the ADR-0032 completeness (rules/tests/stories/translations/observability).

---

## 1. What Uber Eats & Deliveroo do today (the pattern we copy)

Recorded so the design is anchored to the reference the product directive named:

| | Uber Eats | Deliveroo |
|---|---|---|
| **When** | On delivery completion, a post-delivery card in the order screen | On delivery completion, an "order delivered — rate it" prompt |
| **Rating** | Thumbs up/down on the **order** and separately the **courier**; tag chips ("food was great", "delivery was on time") | Star/thumb rating of the **order** and the **rider** |
| **Timeliness** | Surfaced via tags/"on time" prompts and delivery ETA vs actual | Surfaced in the rider rating + "was it on time?" prompts |
| **Tip** | Same flow: **"Add a tip"** for the courier — preset amounts (or %) + custom; **100% to the courier**; editable for a window after delivery | Same flow: **"Tip your rider"** — presets (£1/£2/£3) + custom; **100% to the rider** |
| **Reward (non-money)** | "Compliments" / kudos badges to the courier | Rider thumbs-up |

**Takeaways we adopt:**
- **One post-delivery moment** carries rating **and** tip — we already have `rateOrder` /
  `rateRestaurant` / `tipOrder` there; we add the **timeliness** question beside them.
- **Tips are separate from price, 100% pass-through** — already true in our model (ADR-012, Captain
  skims 0%).
- **Preset + custom amounts** — a UI concern (SDUI + translations), not a domain change.
- **"Reward"** = the money tip in V0 (the Uber/Deliveroo behaviour); a **non-monetary compliment** to
  the rider is a small, clearly-scoped **future** extension (§7), not V0.

---

## 2. Guiding principle — new signal in the domain, tip reused, aggregation in a view

- **Domain declares:** a new post-delivery feedback action `RecordDeliverySatisfaction` on the **Order**
  aggregate — sitting alongside the existing triad (`RateOrder` rider-thumb, `RateRestaurant`
  stars+comment, `TipOrder`), all three of which are already Order-aggregate actions (commands.yaml §10).
- **The tip is not re-modelled.** The prompt calls the existing `tipOrder` mutation with recipient
  `RIDER` (Captain-dispatch) or `RESTAURANT` (self-dispatch). `TipRecipient` / `Tipper` already cover
  both; `InvalidTipRecipient` already guards the illegal cases.
- **Aggregation lives in a read model,** not the write path: `View_DeliveryTimelinessInsight` folds the
  survey answers **plus** #60's `delivered`/`late` facts into the restaurant-facing decision signal.
- **Objective vs subjective are kept distinct.** #60 emits the **objective** late fact (promise vs
  actual). This survey captures the **subjective** customer verdict. The restaurant view shows **both**,
  because "10 min late but the customer didn't mind" and "on time but flagged" are different decisions.

---

## 3. The model

### 3.1 The post-delivery moment — one prompt, three asks

On a **DELIVERED**, **DELIVERY**-type order (COLLECTION/pickup has no courier → no survey, no rider
tip), the customer sees a single sheet (the existing `rating_sheet`, extended — §6) offering:

1. **Delivery timeliness** — *"Was the delivery on time?"* → `recordDeliverySatisfaction` **(new)**
2. **Order / restaurant rating** — the existing `rateOrder` (rider thumb) + `rateRestaurant` (stars)
3. **Tip your courier** — presets + custom → `tipOrder` with recipient `RIDER` / `RESTAURANT`
   **(reused)**

All three are **independent and optional** — the customer can answer timeliness without tipping, tip
without rating, etc. This mirrors Uber Eats / Deliveroo, where the tip is never gated behind a rating.

### 3.2 Delay-satisfaction capture — new command + event

- **Scalar `DeliveryTimeliness`** (enum) — the answer. Proposed 3 levels so the restaurant gets a
  gradient, not just a binary:

  ```
  DeliveryTimeliness = { ON_TIME, ACCEPTABLE_DELAY, TOO_LATE }
  ```

  (Owner decision in §9 — a 2-level thumb `{ ACCEPTABLE, TOO_LATE }` is the leaner alternative.)

- **Command `RecordDeliverySatisfaction`** (persona: customer; Order aggregate):

  ```yaml
  RecordDeliverySatisfaction:
    properties:
      orderId:        OrderId
      restaurantId:   RestaurantId
      timeliness:     DeliveryTimeliness
      reason:         DeliveryDissatisfactionReason?   # optional; only meaningful for TOO_LATE
    required: [orderId, restaurantId, timeliness]
  ```

- **Event `DeliverySatisfactionRecorded`** — mirrors `OrderRated` (business payload only; the acting
  customer is envelope metadata per ADR-0041, never a payload field):

  ```yaml
  DeliverySatisfactionRecorded:
    properties: { orderId, restaurantId, timeliness, reason? }
    required: [orderId, timeliness]
  ```

- **Guards (`actors.yaml` `throws`)** — reuse the existing rating guards exactly:
  - `OrderNotFound` — no such order.
  - `InvalidOrderStatus` — only a **DELIVERED** order can be surveyed (same guard `RateOrder` uses).
  - `DeliverySatisfactionAlreadyRecorded` **(new error)** — one answer per order, final; mirrors
    `OrderAlreadyRated` / `RestaurantAlreadyRated` (record-once semantics, §9 alt: last-wins).

### 3.3 Tip / reward — reuse, don't rebuild

- **Money tip** = existing `TipOrder` / `OrderTipped`. The prompt passes recipient **`RIDER`** for
  Captain-dispatch and **`RESTAURANT`** for restaurant self-dispatch (whose "courier" is the
  restaurant's own). `TipRecipient` already enumerates both; `Tipper = CUSTOMER` here (the
  restaurant-tips-rider path already exists separately). **Nothing new in the write model.**
- **"Reward" (the product wording)** = in V0, the money tip above — the exact Uber Eats / Deliveroo
  behaviour. A **non-monetary rider compliment** (kudos badges) is deliberately **out of V0 scope**
  (§7) to keep the change to one signal + the tip re-surface.
- **Which recipient is the "delivery person"?** Resolved from the order's dispatch mode (#60):
  `CAPTAIN` → `RIDER`; `RESTAURANT` (self-dispatch) → `RESTAURANT`. The SDUI reads this off the order
  read model so the prompt labels the tip correctly ("Tip your rider" vs "Tip the restaurant's driver").

### 3.4 Read models

- **Customer prompt eligibility** — **no new view.** The client already reads `order` /
  `OrderTracking`; we add a `deliveryTimeliness` field (nullable — null = not yet answered) to the order
  read model so the sheet knows whether to show/hide the question (same way `rateOrder` idempotency is
  surfaced). The tip's recipient hint comes from the existing dispatch/delivery read model (#60).
- **Restaurant-facing insight — `View_DeliveryTimelinessInsight` (new).** Per `(restaurantId,
  dispatchMode)`, a fold over `DeliverySatisfactionRecorded` **and** #60's `delivered`/`late` facts:

  | column | from |
  |---|---|
  | `restaurantId` | order lineage |
  | `dispatchMode` | #60 `RestaurantDispatchConfig` / dispatch facts (`CAPTAIN` vs `RESTAURANT`) |
  | `deliveriesCount` | count of delivered orders |
  | `onTimeCount` / `acceptableCount` / `tooLateCount` | fold of `timeliness` |
  | `objectiveLateCount` | fold of #60 `late` facts (promise vs actual) |
  | `satisfactionRate` | derived (`ON_TIME`+`ACCEPTABLE_DELAY`) / surveyed |

  This is the payload behind *"is self-dispatch still worth it?"* — the restaurant sees, side by side,
  its **self-dispatch** timeliness vs what **Captain** routing achieves.

### 3.5 Restaurant-facing query

- **`restaurantDeliveryTimeliness`** query (roles `[RESTAURANT, RESTAURANT_ACCOUNT, ADMIN]`) →
  `View_DeliveryTimelinessInsight`, scoped to the caller's restaurant(s). Sits beside the existing
  `restaurantDeliveries` (#60/ADR-0031) in the restaurant "track deliveries" activity.

---

## 4. What lands in `specs/**` (DSL change list)

| File | Change |
|---|---|
| `scalars.yaml` | `DeliveryTimeliness` enum; `DeliveryDissatisfactionReason` (bounded free-text or small enum) |
| `commands.yaml` | `RecordDeliverySatisfaction` (§10 feedback block, beside `RateOrder`) |
| `events.yaml` | `DeliverySatisfactionRecorded` |
| `errors.yaml` | `DeliverySatisfactionAlreadyRecorded` |
| `actors.yaml` | Order aggregate inbox: `RecordDeliverySatisfaction → DeliverySatisfactionRecorded`, throws `OrderNotFound`/`InvalidOrderStatus`/`DeliverySatisfactionAlreadyRecorded` |
| `database/projection_views.yaml` | `View_DeliveryTimelinessInsight` (fold over `DeliverySatisfactionRecorded` + #60 delivered/late facts) |
| `api.yaml` | mutation `recordDeliverySatisfaction` `[CUSTOMER, ADMIN]`; query `restaurantDeliveryTimeliness` `[RESTAURANT, RESTAURANT_ACCOUNT, ADMIN]`; `deliveryTimeliness` field on the order read type. **`tipOrder` unchanged** (reused). |
| `stories.yaml` | customer activity 9 (Rate the order): `+RateDeliveryTimeliness → recordDeliverySatisfaction`; restaurant TrackDeliveries: `+ReviewDeliveryTimeliness → restaurantDeliveryTimeliness`. (`TipRiderRestaurantOrCaptain → tipOrder` already covers the tip step.) |
| `rules.yaml` | `DeliverySatisfactionRecordedOncePerDeliveredOrder`; `DeliveryTimelinessVisibleToRestaurant` |
| `tests.yaml` | a behaviour test per new command/event/error + the record-once rejection; each linked to a rule (ADR-0032 both-ways) |
| `translations.yaml` | survey keys (`delivery.timeliness.question` / `.on_time` / `.acceptable` / `.too_late` / `.reason_ph`), tip-your-courier CTA + presets, thanks toast |
| `screens/customer_screens.yaml` | extend `rating_sheet` (or a new `delivery_feedback_sheet`): timeliness question + tip section; `actions` allowlist `+record_delivery_satisfaction → recordDeliverySatisfaction`, `+tip_order → tipOrder`; registry `+tip_amount_selector` component |
| `observability.yaml` | `delivery-satisfaction` contract (survey shown → answered rate; tip uptake; TOO_LATE rate by dispatch mode) |

**New rules:** `DeliverySatisfactionRecordedOncePerDeliveredOrder`, `DeliveryTimelinessVisibleToRestaurant`.

> ADR-0032 completeness: every new command/event/error gets a behaviour test (+ rule link); the new
> mutation **and** query each get a story step; the new rules each get a test. `make validate` must stay
> **0 errors** — the change extends the specs, it does not weaken the gate.

## 5. Codegen (`tools/codegen-rs`) — no new emitter shapes

Everything here is expressible with existing DSL constructs, so no generator work beyond regeneration:
the enum scalar → `ref_delivery_timeliness` lookup + Rust enum; the command/event/error → the generated
handler + fold + behaviour test; `View_DeliveryTimelinessInsight` → a generated fold view over
`domain_events` (a cross-fact fold like the existing Order/Payment views); the mutation/query →
generated resolvers over the view. If the survey ever needs to read a #60 process/config value inline,
the `{ from_hook: … }` PM value form (added by #60) is available — but the view-fold path avoids it.

## 6. Runtime (`crates`) — deferred contract, thin when it lands

- Order-aggregate handler for `RecordDeliverySatisfaction` (generated require+guard+append, like the
  other feedback commands) + fold of `DeliverySatisfactionRecorded` into the order read state.
- One migration for `View_DeliveryTimelinessInsight` (V0 SQL view; a hot view can later become a
  materialized projector table with no API change — ADR-0035).
- SDUI: the enriched sheet renders through the generated Leptos registry (deferred until `crates/web`).
- **Business code stays telemetry-free** — the `delivery-satisfaction` contract is satisfied at the
  GraphQL/middleware boundary (`surface: graphql` binding, ADR-20260721-031127), not in the aggregate.

## 7. Scope guardrails (what this is NOT)

- **Not** a new tip mechanism — `TipOrder`/`OrderTipped` are reused verbatim (recipient `RIDER` /
  `RESTAURANT`). The only tip work is **surfacing** it in the post-delivery sheet.
- **Not** non-monetary rider "compliments"/kudos badges — a clearly-scoped **future** extension (a
  `RiderComplimented` fact on the delivery context); flagged here so it isn't lost, but out of V0 so the
  change stays one signal + the tip re-surface.
- **Not** a change to #60's objective late detection — this consumes those facts, it does not compute
  them.
- **Not** a survey for COLLECTION/pickup orders — no courier, no delay question, no rider tip.
- **Not** a restaurant-facing config/notification UI beyond the read query — surfacing the insight in a
  richer dashboard is a follow-up.

## 8. Sequencing & coordination

1. **This issue (#62):** the survey (command/event/error), the `View_DeliveryTimelinessInsight` read
   model + `restaurantDeliveryTimeliness` query, the enriched post-delivery SDUI sheet with the reused
   tip, and full ADR-0032 completeness.
2. Reuses #60's delivered/late seam and the existing tip/rating triad — **no coordination lock** with
   other in-flight delivery work; the only shared file touched by delivery issues is
   `projection_views.yaml` (additive here).

## 9. Open decisions for the product owner

| # | Decision | Options | Proposed default |
|---|---|---|---|
| 1 | Timeliness granularity | 3-level `{ON_TIME, ACCEPTABLE_DELAY, TOO_LATE}` vs 2-level thumb `{ACCEPTABLE, TOO_LATE}` | **3-level** (richer restaurant signal) |
| 2 | Answer mutability | **Record-once** (final, typed error on re-answer) vs **last-wins** | **Record-once** (matches `OrderAlreadyRated`) |
| 3 | Reason field | Free-text (`maxLength`) vs a small enum of causes vs omit | Optional **free-text**, `TOO_LATE` only |
| 4 | Aggregate home | **Order** (beside the feedback triad) vs the **DeliveryJob**/delivery context | **Order** (consistency with `RateOrder`) |
| 5 | Tip window | Tip only at the post-delivery moment vs an editable window after (Uber-style) | Post-delivery moment in V0; window = future |
| 6 | Non-monetary reward | Ship rider "compliments" now vs defer | **Defer** (§7) |

## 10. Superseded / extended

- **#60 proposal §3.10 / decision 7** — the deferred "delivery-delay satisfaction check" is realised
  here.
- **ADR-012 (tips)** — extended in *usage* only: the existing customer→rider tip is surfaced at the
  post-delivery moment; no change to the tip domain model.
- **ADR-0031 (delivery bounded context)** — consumes its terminal facts; the restaurant insight view is
  the new read-side surface.
