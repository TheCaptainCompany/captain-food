# Design Proposal — Delivery Dispatch Strategy (multi-partner foundation)

> **Status:** Proposed — awaiting product-owner approval. This is a *plan-mode* proposal (DSL + saga
> + runtime). **No `specs/**` or code has been changed.** On approval it becomes an ADR that lands
> with the implementation.
>
> **Owns:** the shared "how does a delivery get routed" foundation that unblocks **#57 (Uber Direct)**
> and **#58 (CoopCycle)** — built **once**, so both adapters plug in without re-touching the saga.
>
> **Supersedes on landing:** ADR-20260720-004556 (single-partner cap-3 re-offer; timeout §5 deferral).

---

## TL;DR

Delivery routing is **not** something the spec can enumerate ("which partner, which city, which
restaurant") — it is **runtime configuration**. So the DSL encodes only the **mechanism**: *a delivery
is dispatched under a resolved strategy*. **Config carries the content.**

Two independent dimensions, both resolved at runtime:

| Dimension | Scope | Question | Values |
|---|---|---|---|
| **Dispatch responsibility** | **restaurant** | *Who* delivers? | `RESTAURANT` (self / their own deal) · `CAPTAIN` (we route) |
| **Routing rank** | **city** | *How* does Captain route? | an **ordered list of channels**, e.g. Tours = `[INDEPENDENT, UBER_DIRECT]` |

When Captain routes, the saga **walks the city's ranked channel list**, advancing to the next channel
on any of three triggers: a partner **decline**, an offer **timeout**, or a **manual operator escalate**.
Exhausting the list fails the dispatch closed (as today). Config reorders the rank with **zero code**
(a new `effective_from` row promotes Avelo37 to rank 1 once Tours has volume).

**Decisions still needed from you:** see [§9](#9-decisions-i-need-from-you) (6 quick calls).

---

## 1. Product decisions captured

From our conversation (recorded here so nobody re-litigates them):

1. **Start ranking (Tours, pre-volume):** `INDEPENDENT → UBER_DIRECT`.
2. **Post-volume ranking:** `AVELO37 → INDEPENDENT → UBER_DIRECT` — a **config reorder**, not code.
3. **Escalation between tiers:** *timeout* **and** *manual/operator escalate* (plus the partner decline
   that already exists).
4. **Strategy is resolved per city and per restaurant at runtime** — the DSL must **not** anticipate
   *which / when / where / for whom*.
5. **Restaurants may deliver themselves** — own employees, or their own direct deal with
   Avelo37 / CoopCycle / Uber. Modeled as a dispatch **strategy**: *restaurant dispatches* vs
   *Captain dispatches*. When the restaurant dispatches, Captain does not route.
6. **City** is the routing scope, and it is **not modeled yet** (today it is only a string on `Address`).

---

## 2. Guiding principle — mechanism in the DSL, content in config

- **DSL (spec) declares:** a `DeliveryJob` is dispatched under a **resolved dispatch strategy**; the
  saga *resolves* the strategy, then *walks* it. The spec references *the mechanism and its triggers*,
  never a concrete partner-per-city or restaurant list.
- **Config (data) carries:** which restaurants self-dispatch; each city's ranked channel list; per-channel
  timeouts; which channels are wired. All runtime-editable, all outside `specs/**`.
- **Code (adapters) provides:** one crate per partner channel (`avelo37`, `uber_direct`, `coopcycle`),
  each behind the generated `DeliveryService` port. `INDEPENDENT` is not an adapter (it's the rider pool).

This is the same split the repo already uses for `PricingPolicy`/`UberSplitPolicy`
(`referential.yaml`, time-versioned via `effective_from`).

---

## 3. The model

### 3.1 New scope — City

Today `city` is a `CityName` **string** on `Address` (`entities.yaml`), with no identity. We introduce a
**lightweight city scope** without over-modeling:

- A restaurant's city is derived from its address `city` (already present), **normalized to a `CityKey`**
  (slug of the city name, e.g. `tours`). No new mandatory data entry for V0.
- Ranking config is keyed by `CityKey`, with a **platform-default** row (`city = null`) as the fallback.

*(Alternative — a full `City` entity with an id — deferred; see §9(e).)*

### 3.2 Dispatch responsibility — restaurant-scoped

- New scalar `RestaurantDispatchMode = { CAPTAIN, RESTAURANT }`. **Default `CAPTAIN`** (unconfigured
  restaurants behave exactly as today).
- `RESTAURANT` = the restaurant handles delivery itself (own couriers **or** their own partner deal).
  Captain **does not** call `offer_job`; it creates the `DeliveryJob` for **tracking** only, and the
  restaurant drives status via existing commands (`UpdateDeliveryStatus` / `MarkOrderDelivered`).
- Config surface: a `restaurant_dispatch_config` referential row (or a `SetRestaurantDispatchMode`
  command — see §9(d)). Runtime-editable; no spec change to switch a restaurant.

### 3.3 Routing rank — city-scoped

- **Channels** are the routable targets. Two kinds: the **pool** (`INDEPENDENT`) and **partner adapters**
  (`AVELO37`, `UBER_DIRECT`, `COOPCYCLE`, …).
- Because partners **cannot be fully anticipated**, channels are **data-driven**: a `delivery_channel`
  registry (`referential.yaml`) names them, rather than a frozen scalar `enum` that every new partner
  must edit. A validator asserts every *partner* channel has a wired adapter (`services.yaml`
  implementation); `INDEPENDENT` is the one non-adapter channel. *(vs a fixed enum — see §9(a).)*
- **City ranking config:** `city_delivery_ranking` referential table —
  `(city_key, rank, channel, effective_from)`. Resolution = the latest `effective_from` set for the
  city, ordered by `rank`; fall back to the `city = null` platform default. Tours seed today:
  `(tours, 1, INDEPENDENT), (tours, 2, UBER_DIRECT)`.

### 3.4 Strategy resolution (runtime, per order)

Given an order's restaurant + city:

1. **Restaurant mode** `RESTAURANT` → **self-dispatch**: no routing, no timeout, no `offer_job`.
2. Else `CAPTAIN` → load the city's ranked channels (latest `effective_from`, by `rank`; else default).
3. The saga **walks** that list.

### 3.5 The saga — `DeliveryDispatchProcess` becomes *resolve → walk*

```
OrderMarkedReady (DELIVERY)
  └─ deliver DeliveryRequested (birth, UUIDv5 job id)      [unchanged]
  └─ resolve strategy
       ├─ RESTAURANT  → status SELF_DISPATCHED (no offer_job); restaurant drives progress
       └─ CAPTAIN     → offer rank-1 channel; state{ current_rank=1, current_channel, offer_attempts=1, OFFERED }
                         · INDEPENDENT → publish to pool + arm timeout
                         · PARTNER     → offer_job(target=channel) + arm timeout

advance-to-next-channel  ⇐ three triggers:
  ├─ DeliveryRejectedByPartner (inbound decline)     [exists]
  ├─ DeliveryOfferTimedOut     (sweep, §3.7)          [new]
  └─ DeliveryEscalationRequested (EscalateDelivery)   [new, §3.8]
      → next rank exists?  yes → offer it (rank+1); no → DeliveryDispatchFailed + FAILED   [terminal, exists]

DeliveryAcceptedByPartner | AcceptDelivery (rider)  → ACCEPTED (cancel timeout)
DeliveryStatusUpdated=DELIVERED | DeliveryCompleted → MarkOrderDelivered → COMPLETED
```

- `offer_attempts` keeps its meaning (total offers made). The **numeric cap (3) is replaced** by
  "walk each ranked channel in order, then fail closed" — the list length is the bound (still
  fail-closed, no unbounded loop). *(This supersedes ADR-004556; see §9(b).)*
- The three triggers all converge on **one** "advance" leg, so the escalation logic is written once.

### 3.6 `offer_job` grows a target + a composite gateway

- The generated port `offer_job` gains a **`channel` target** (which partner to offer). Today it takes
  none (single bound impl) — this is the missing seam the runtime map flagged.
- The composition root's hard-coded *Avelo-vs-Noop* choice is replaced by a **composite
  `DeliveryService`**: a registry of configured adapters keyed by channel. `offer_job(target)` routes
  to that adapter. **A channel with no wired adapter behaves as an immediate decline → escalate**, so
  an unconfigured `UBER_DIRECT` in Tours simply falls through to the next rank — preserving fail-closed
  and keeping today's unconfigured deployments working unchanged.

### 3.7 Offer timeout — a new sweep worker

- The PM runner is **event-drain-only** (no timer). So timeouts get a **dedicated worker**
  (`DeliveryOfferTimeoutWorker`), mirroring the existing `retention_sweep_worker.rs` pattern
  (periodic, env-gated).
- It finds `delivery_dispatch_process_manager` rows `OFFERED` with `last_update_utc` older than the
  channel's TTL and records a **PM-authored `DeliveryOfferTimedOut`** fact on the job stream (exactly
  like the PM authors `DeliveryDispatchFailed`). The saga reacts → advance leg. Event-sourced, no
  hidden state.
- Adds a `(process_status, last_update_utc)` index (migration). **This implements ADR-004556 §5's
  deferred timeout** — see §9(c).

### 3.8 Manual operator escalate — a new command

- `EscalateDelivery` (roles `RESTAURANT`/`ADMIN`): "skip the current channel, try the next now."
  Emits `DeliveryEscalationRequested` → the same advance leg. Guarded: only when `CAPTAIN`-dispatched,
  job `OFFERED`, and channels remain (else a typed error).
- Overlaps loosely with the existing ad-hoc `UnassignDeliveryFromPartner` lever; this one is
  **saga-integrated** (drives the ranked walk) rather than a direct aggregate poke.

### 3.9 Restaurant self-dispatch mechanics

- `RESTAURANT` mode → job created with a distinct status (proposal: `SELF_DISPATCHED`), no partner, no
  routing, no timeout. Restaurant progresses it via the existing `UpdateDeliveryStatus` /
  `MarkOrderDelivered`. This is exactly how "the restaurant deals directly with Avelo37/CoopCycle/Uber
  themselves" is handled — **out-of-band** from Captain's routing. *(status modeling — §9(d).)*

---

## 4. What lands in `specs/**` (DSL change list)

| File | Change |
|---|---|
| `scalars.yaml` | `RestaurantDispatchMode`; `CityKey`; `DeliveryChannel` (registry-backed — §9a); +`SELF_DISPATCHED` on `DeliveryDispatchProcessStatus` |
| `entities.yaml` | derive restaurant `CityKey` from address; dispatch-mode reference |
| `database/tables/referential.yaml` | `delivery_channel` registry; `city_delivery_ranking`; `restaurant_dispatch_config` |
| `database/tables/process_managers.yaml` | `delivery_dispatch_process_manager` += `current_rank`, `current_channel` (+ sweep index) |
| `services.yaml` | `offer_job` input += `target`; consume ranked `implementations`; (+ `uber_direct`/`coopcycle` land with **their** issues) |
| `events.yaml` | `DeliveryOfferTimedOut`, `DeliveryEscalationRequested` |
| `commands.yaml` | `EscalateDelivery` (+ optional `SetRestaurantDispatchMode`) |
| `errors.yaml` | `NotCaptainDispatched`, `NoDeliveryChannelsRemaining` |
| `processmanager.yaml` | rewrite `DeliveryDispatchProcess` → resolve→walk |
| `actors.yaml` | wire the new messages |
| `stories.yaml` | steps for `EscalateDelivery` (+ dispatch-mode config) |
| `rules.yaml` | `RestaurantDispatchBypassesRouting`, `CityRankingWalkedInOrder`, `TimeoutEscalatesToNextChannel`, `ManualEscalateSkipsChannel`, `DispatchExhaustionFailsClosed` (supersedes `DispatchRetriesAreBounded`) |
| `tests.yaml` | one behaviour test per rule above |
| `observability.yaml` | `delivery-dispatch-strategy` contract |

Every one of these is required for `make validate` to stay at **0 errors** (ADR-0032 completeness).

## 5. Codegen changes (`tools/codegen-rs`)

- `parse_services` reads `implementations` + order; new emitter produces the **composite/registry**
  `DeliveryService` binding instead of the single `local()` passthrough.
- `offer_job` trait + the saga `call` step carry the `target` channel.
- Saga emitter: the resolve→walk leg (select next channel; self-dispatch short-circuit).

## 6. Runtime changes (`crates`)

- Composite `DeliveryService` (adapter registry by channel) + composition-root wiring.
- `DeliveryOfferTimeoutWorker` (new, env-gated).
- Saga hooks: `resolve_strategy` (read city ranking + restaurant mode), `select_next_channel`,
  self-dispatch short-circuit.
- Migrations: config tables + the `(process_status, last_update_utc)` index.

## 7. Prior decisions superseded / extended

- **ADR-20260720-004556** — the cap-3 same-partner re-offer is replaced by the ranked-list walk;
  the §5 timeout deferral is implemented. The new ADR supersedes it.
- **ADR-0031** (delivery bounded context) — extended with the strategy/city dimensions.

## 8. Sequencing & coordination (with the #58 CoopCycle session)

1. **This foundation** lands first, as its own issue + PR (the multi-strategy dispatch mechanism,
   city scope, channel registry, composite gateway, timeout worker, manual escalate — **but no
   Uber/CoopCycle adapter code**).
2. **#57 (Uber Direct)** lands on top: `crates/adapters/uber_direct` (OAuth2 token manager,
   `external_uber_direct_events` staging, `X-Uber-Signature` verify) registered as the `UBER_DIRECT`
   channel. Fail-closed until `UBER_DIRECT_*` configured (so it simply escalates past in Tours today).
3. **#58 (CoopCycle)** lands the `coopcycle` channel + per-instance config on the **same** foundation.

> ⚠️ **Coordination:** #57 and #58 must **not** each invent the saga/ranking. The **channel registry +
> composite `DeliveryService`** is the shared plug-in seam. Recommend the #58 session **hold** any
> saga/`services.yaml`-ranking edits and rebase onto this foundation once it merges; their PR then
> only adds an adapter crate + a `delivery_channel` row + a `services.yaml` implementation entry.

## 9. Decisions I need from you

Defaults in **bold** — I'll proceed on these unless you say otherwise.

- **(a) Channel type:** **data-driven `delivery_channel` registry** (add a partner = config row + adapter
  crate, no scalar edit) vs a fixed `enum`. *Registry matches "we can't anticipate the partners."*
- **(b) Bound:** **replace the numeric cap-3 with "walk each ranked channel once, then fail closed"**
  (list length is the bound). Confirms superseding ADR-004556's cap.
- **(c) Timeout home:** **a dedicated `DeliveryOfferTimeoutWorker`** (mirrors the retention sweep) vs a
  new timer branch inside the PM runner. *Dedicated worker is the established pattern.*
- **(d) Restaurant self-dispatch:** new **`SELF_DISPATCHED`** dispatch status + a
  `SetRestaurantDispatchMode` config command, vs pure seed config. Also: does a self-dispatching
  restaurant still need Captain **order tracking** (job created, status manual) — **yes** assumed?
- **(e) City modeling:** **lightweight `CityKey` derived from the address** vs a first-class `City`
  entity with its own id. *Lightweight keeps V0 small.*
- **(f) Per-channel timeout TTL:** where configured — **on the `delivery_channel` registry row**
  (per-channel) vs a single global env TTL.

## 10. Scope guardrails (what this is NOT)

- **Not** building the #57 Uber Direct or #58 CoopCycle adapters — those are follow-ups on this seam.
- **Not** a restaurant-facing UI for dispatch-mode / ranking config — config is seeded/command-set first.
- **Not** a general scheduler — the timeout worker does exactly one job (stale-offer escalation).
