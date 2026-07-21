# Design Proposal — Delivery Dispatch Strategy (multi-partner foundation)

> **Status:** Proposed — **decisions locked** (v2). Plan-mode proposal (DSL + saga + runtime).
> **No `specs/**` or code changed yet.** On approval it becomes an ADR that lands with the
> implementation.
>
> **Owns:** the shared "how does a delivery get routed" foundation that unblocks **#57 (Uber Direct)**
> and **#58 (CoopCycle)** — built **once**, so both adapters plug in without re-touching the saga.
>
> **Supersedes on landing:** ADR-20260720-004556 (single-partner cap-3 re-offer; timeout §5 deferral).

---

## TL;DR

Delivery routing is decided at **runtime by configuration**, not enumerated in the spec. The split is
**two layers**:

- **Spec (`specs/**`, referential):** the **catalog** of known delivery channels/partners, each with
  **spec-level defaults** (e.g. a default offer timeout). This is declared data, like `PhoneCountry`.
- **Runtime config (city / restaurant):** **how** those channels are *used* — which restaurant
  self-dispatches, each city's **ranked** channel list, and TTL overrides. This is what we "can't
  anticipate," so it never lives in the spec. Later, partners can **self-register** their city-level
  availability via a dedicated web app + our EXTERNAL GraphQL API.

Two runtime dimensions:

| Dimension | Scope | Question | Values |
|---|---|---|---|
| **Dispatch responsibility** | **restaurant** | *Who* delivers? | `RESTAURANT` (self / own deal) · `CAPTAIN` (we route) |
| **Routing rank** | **city** | *How* does Captain route? | an **ordered list of catalog channels**, e.g. Tours = `[INDEPENDENT, UBER_DIRECT]` |

When Captain routes, the saga **walks the city's ranked list**, advancing on a partner **decline**, an
offer **timeout**, or a **manual escalate**; exhausting the list fails closed. A config reorder promotes
Avelo37 to rank 1 once Tours has volume — **zero code**.

---

## 1. Product decisions (locked)

From our conversation — recorded so nobody re-litigates them:

1. **Start ranking (Tours):** `INDEPENDENT → UBER_DIRECT`. **Post-volume:** `AVELO37 → INDEPENDENT →
   UBER_DIRECT` — a config reorder, not code.
2. **Channels are declared in the spec** as referential partners/channels (a known catalog + spec-level
   defaults). **Usage per city/restaurant is runtime config.** *(decision a — two-layer)*
3. **Bound = the declared list.** Walk each ranked channel **once**, then fail closed. Supersedes the
   ADR-004556 numeric cap-3. *(decision b)*
4. **Escalation triggers:** partner **decline** (exists) + **timeout** + **manual/operator escalate**.
5. **Timeout is scoped, layered** *(decisions c + f)*:
   - a **single global env TTL = hard max** (ceiling over everything);
   - **per-channel spec-level default** TTL in the catalog;
   - **override at city level** (and INDEPENDENT's TTL is defined **by city**);
   - a **self-dispatching restaurant defines its own** delay threshold.
6. **Restaurant self-dispatch is event-sourced** and **still gets customer order tracking** — *very
   important* so the customer stays informed. *(decision d)*
7. **A delivery-delay satisfaction check** is asked of the customer — to prove we're good enough and to
   help the restaurant decide whether self-dispatch is still worth it. *(new; scoping in §3.10)*
8. **City is a first-class entity with its own id** (not a derived string). *(decision e)*
9. **Future:** delivery partners **register themselves at city level** via a dedicated web app + our
   GraphQL API — whether already-integrated, willing-to-be, or self-integrated against the EXTERNAL
   API we expose. *(new; scoping in §8/§10)*

---

## 2. Guiding principle — catalog in the spec, usage in config

- **Spec declares:** (i) the **channel catalog** (referential partners + spec defaults), and (ii) the
  **mechanism** — a `DeliveryJob` is dispatched under a *resolved strategy*; the saga *resolves* then
  *walks* it. The spec never says which city ranks which partner, or which restaurant self-dispatches.
- **Config carries:** per-city ranking + TTL overrides; per-restaurant dispatch mode + self-dispatch
  timeout. Runtime-editable; API-writable later (partner self-registration).
- **Code provides:** one adapter crate per partner channel (`avelo37`, `uber_direct`, `coopcycle`),
  each behind the generated `DeliveryService` port. `INDEPENDENT` is the rider pool (no adapter).

---

## 3. The model

### 3.1 City — first-class entity

- New **`City`** entity with its own `CityId` (aggregate/referential). Restaurants belong to a `City`
  (derived/backfilled from their address `city` string on introduction).
- `City` is the **anchor for routing config and partner availability** — a city's ranked channels, and
  (future) the partners that have registered to serve it.

### 3.2 Dispatch responsibility — restaurant-scoped

- Scalar `RestaurantDispatchMode = { CAPTAIN, RESTAURANT }`. **Default `CAPTAIN`** (today's behaviour).
- `RESTAURANT` = the restaurant delivers itself (own couriers **or** their own partner deal). Captain
  **does not** `offer_job`; it still creates an **event-sourced `DeliveryJob` for customer tracking**,
  progressed by the restaurant via existing commands.

### 3.3 Channel catalog — spec referential (+ usage config)

- **Spec (`referential.yaml`) — `delivery_channel` catalog:** one row per known channel/partner
  (`INDEPENDENT`, `AVELO37`, `UBER_DIRECT`, `COOPCYCLE`…), each with **spec-level defaults**:
  `kind` (`POOL` | `PARTNER`), `default_offer_ttl`, and (for partners) the adapter binding. A validator
  asserts every `PARTNER` channel has a wired `services.yaml` implementation.
- **Runtime config — usage per scope:**
  - `city_delivery_ranking` — `(city_id, rank, channel, ttl_override?, effective_from)`: the **ordered**
    channels a city uses, with optional per-city TTL (INDEPENDENT's TTL lives here). Latest
    `effective_from` set wins; a platform default (`city_id = null`) is the fallback.
  - `restaurant_dispatch_config` — `(restaurant_id, mode, self_dispatch_ttl?)`.

### 3.4 Strategy resolution (runtime, per order)

1. **Restaurant mode `RESTAURANT`** → self-dispatch: no routing, no `offer_job`; job tracked, restaurant
   drives status; the self-dispatch TTL feeds the *late/tracking* signal (§3.10), **not** an escalation.
2. Else `CAPTAIN` → load the city's ranked channels (latest `effective_from`, by `rank`; else default).
3. The saga **walks** that list.

### 3.5 The saga — `DeliveryDispatchProcess` becomes *resolve → walk*

```
OrderMarkedReady (DELIVERY)
  └─ deliver DeliveryRequested (birth, UUIDv5 job id)              [unchanged]
  └─ resolve strategy (restaurant mode → city ranking)
       ├─ RESTAURANT → status SELF_DISPATCHED (no offer_job); restaurant drives progress; arm SLA (§3.10)
       └─ CAPTAIN    → offer rank-1 channel; state{ current_rank, current_channel, offer_attempts, OFFERED }
                        · INDEPENDENT → publish to pool + arm timeout (city TTL)
                        · PARTNER     → offer_job(target=channel) + arm timeout (channel/city TTL)

advance-to-next-channel  ⇐ three triggers → the SAME leg:
  ├─ DeliveryRejectedByPartner  (inbound decline)        [exists]
  ├─ DeliveryOfferTimedOut      (sweep worker, §3.7)      [new]
  └─ DeliveryEscalationRequested (EscalateDelivery, §3.8) [new]
      → next rank exists? yes → offer it; no → DeliveryDispatchFailed + FAILED    [terminal, exists]

DeliveryAcceptedByPartner | AcceptDelivery (rider)  → ACCEPTED (cancel timeout)
DeliveryStatusUpdated=DELIVERED | DeliveryCompleted → MarkOrderDelivered → COMPLETED → satisfaction (§3.10)
```

- `offer_attempts` keeps meaning (total offers). The **bound is the list length** — walk each ranked
  channel once, then fail closed. No unbounded loop.

### 3.6 `offer_job` grows a target + a composite gateway

- The generated `offer_job` gains a **`channel` target**. The composition root's hard-coded
  *Avelo-vs-Noop* is replaced by a **composite `DeliveryService`** — a registry of configured adapters
  keyed by channel. A channel with **no wired adapter behaves as an immediate decline → escalate**, so
  an unconfigured `UBER_DIRECT` in Tours simply falls through to the next rank (fail-closed; today's
  unconfigured deployments unchanged).

### 3.7 Offer timeout — a new sweep worker, layered TTL

- Dedicated **`DeliveryOfferTimeoutWorker`** (mirrors `retention_sweep_worker.rs`; periodic, env-gated).
  Finds `OFFERED` rows with `last_update_utc` older than the **resolved TTL** and records a PM-authored
  **`DeliveryOfferTimedOut`** fact → the saga's advance leg. Adds a
  `(process_status, last_update_utc)` index.
- **Resolved TTL = min( global env max , city override ?? channel spec-default )**. INDEPENDENT's TTL
  comes from the city row; partners default from the catalog, overridable per city.

### 3.8 Manual operator escalate — a new command

- `EscalateDelivery` (roles `RESTAURANT`/`ADMIN`): "skip the current channel, try the next now" → emits
  `DeliveryEscalationRequested` → the advance leg. Guarded: only `CAPTAIN`-dispatched, `OFFERED`,
  channels remaining (else a typed error).

### 3.9 (removed — folded into 3.3/3.7)

### 3.10 Restaurant self-dispatch, tracking & satisfaction

- **Self-dispatch:** job created event-sourced with status `SELF_DISPATCHED`; restaurant progresses it
  (`UpdateDeliveryStatus` / `MarkOrderDelivered`). This is how "the restaurant deals directly with a
  partner themselves" is handled — out-of-band from Captain routing, **but the customer still gets order
  tracking**.
- **SLA / late signal:** the restaurant's `self_dispatch_ttl` (and, for Captain-dispatch, the delivered
  time vs promise) drives a **late** signal surfaced in tracking — informational, not an escalation.
- **Satisfaction check (scoping):** on delivery, ask the customer a delay-satisfaction question. The
  **foundation emits the terminal signals** a survey builds on (delivered/late facts); the **survey
  flow itself (prompt, capture, restaurant-facing insight) is a follow-up issue** so the foundation
  stays focused. Flagged here so it isn't lost.

---

## 4. What lands in `specs/**` (DSL change list)

| File | Change |
|---|---|
| `scalars.yaml` | `RestaurantDispatchMode`; `CityId`; channel-`kind`; +`SELF_DISPATCHED` on `DeliveryDispatchProcessStatus` |
| `entities.yaml` | `City` entity; restaurant → `City`; dispatch-mode reference |
| `database/tables/referential.yaml` | `city` catalog; `delivery_channel` catalog (+ spec-default TTL); `city_delivery_ranking`; `restaurant_dispatch_config` |
| `database/tables/process_managers.yaml` | `delivery_dispatch_process_manager` += `current_rank`, `current_channel` (+ sweep index) |
| `services.yaml` | `offer_job` input += `target`; consume ranked `implementations` (uber_direct/coopcycle land with their issues) |
| `events.yaml` | `DeliveryOfferTimedOut`, `DeliveryEscalationRequested` |
| `commands.yaml` | `EscalateDelivery` (+ `SetRestaurantDispatchMode`) |
| `errors.yaml` | `NotCaptainDispatched`, `NoDeliveryChannelsRemaining` |
| `processmanager.yaml` | rewrite `DeliveryDispatchProcess` → resolve→walk |
| `actors.yaml` · `stories.yaml` · `rules.yaml` · `tests.yaml` | wire messages; story steps; new rules + a behaviour test each (ADR-0032 completeness) |
| `observability.yaml` | `delivery-dispatch-strategy` contract |

New rules: `RestaurantDispatchBypassesRouting`, `CityRankingWalkedInOrder`, `TimeoutEscalatesToNextChannel`,
`ManualEscalateSkipsChannel`, `DispatchExhaustionFailsClosed` (supersedes `DispatchRetriesAreBounded`).

## 5. Codegen (`tools/codegen-rs`)

- `parse_services` reads `implementations` + order → emit a **composite** `DeliveryService` binding
  (replaces the single `local()` passthrough).
- `offer_job` trait + saga `call` step carry the `target`.
- Saga emitter: the resolve→walk leg (select next channel; self-dispatch short-circuit).

## 6. Runtime (`crates`)

- Composite `DeliveryService` (adapter registry by channel) + composition root.
- `DeliveryOfferTimeoutWorker` (new, env-gated).
- Saga hooks: `resolve_strategy` (city ranking + restaurant mode), `select_next_channel`,
  self-dispatch short-circuit, TTL resolution.
- Migrations: `city` + config tables + the `(process_status, last_update_utc)` index.

## 7. Superseded / extended

- **ADR-20260720-004556** — cap-3 same-partner re-offer → ranked-list walk; §5 timeout implemented.
- **ADR-0031** (delivery bounded context) — extended with strategy + city dimensions.

## 8. Sequencing & coordination

1. **This foundation** first (its own issue + PR): the mechanism, `City`, channel catalog, usage config,
   composite gateway, timeout worker, manual escalate, self-dispatch + tracking — **no partner adapter
   code**.
2. **#57 (Uber Direct)** on top: `crates/adapters/uber_direct` (OAuth2 token mgr,
   `external_uber_direct_events`, `X-Uber-Signature`) as the `UBER_DIRECT` channel; fail-closed until
   `UBER_DIRECT_*` set.
3. **#58 (CoopCycle)** on top: `coopcycle` channel + per-instance config.
4. **Follow-ups (own issues):** partner **self-registration** web app + GraphQL API (decision 9);
   delivery **satisfaction** survey (decision 7).

> ⚠️ **Coordination with the #58 session:** the **channel catalog + composite `DeliveryService`** is the
> shared plug-in seam. #58 should **hold** saga/`services.yaml`-ranking edits and rebase onto this
> foundation; their PR then only adds an adapter crate + a `delivery_channel` catalog row + a
> `services.yaml` implementation entry.

## 9. Decisions — resolved

| # | Decision | Resolution |
|---|---|---|
| a | Channel type | **Spec referential catalog** (partners declared + defaults); **usage** per city/restaurant is runtime config |
| b | Bound | **Walk the declared ranked list once**, then fail closed (supersedes cap-3) |
| c | Timeout home | **Dedicated worker**, scoped per channel/type; INDEPENDENT by city; restaurant self-dispatch by restaurant |
| d | Self-dispatch | **Event-sourced**, `SELF_DISPATCHED`; **customer tracking retained**; satisfaction check added |
| e | City | **First-class `City` entity with its own id** |
| f | TTL | **Global env max** (ceiling) + **per-channel spec default** + **city/restaurant override** |

## 10. Scope guardrails (what this is NOT)

- **Not** the #57/#58 adapters, the partner **self-registration** app/API, or the **satisfaction** survey
  flow — each a follow-up on this foundation (it emits the seams/signals they need).
- **Not** a restaurant-facing config UI — city ranking + dispatch mode are seeded/command-set first.
- **Not** a general scheduler — the timeout worker does one job (stale-offer escalation).
