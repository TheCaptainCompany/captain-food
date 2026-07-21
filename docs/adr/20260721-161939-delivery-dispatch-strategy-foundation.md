# ADR-20260721-161939 — Delivery dispatch strategy: restaurant/city-scoped multi-partner routing

## Status

Accepted (supersedes ADR-20260720-004556)

## Context

ADR-20260720-004556 fixed V0 delivery dispatch at a **single** partner with a numeric cap-3 re-offer,
and named the re-offer step as the future multi-partner extension point. Issue #60 is that extension —
the shared foundation both #57 (Uber Direct) and #58 (CoopCycle) plug into, so we build the routing
mechanism **once** instead of hand-building it per partner.

The product constraints (decided with the product owner, captured in
`docs/proposals/delivery-dispatch-strategy-foundation.md`):

- Routing cannot be enumerated in the spec — *which* partner is used, *where*, and *for whom* is
  runtime data the spec cannot anticipate (restaurants may even bring their own direct partner deal, or
  deliver with their own staff).
- Two orthogonal dimensions: **who dispatches** (a restaurant may self-dispatch, or delegate to Captain)
  and, when Captain dispatches, **how it routes** (an ordered list of channels, configured per city).
- Escalation between channels must fire on a partner **decline**, an offer **timeout**, or a **manual
  operator escalate**.
- The start ranking for Tours is `INDEPENDENT → UBER_DIRECT`; a later config reorder makes it
  `AVELO37 → INDEPENDENT → UBER_DIRECT` with no code change.

## Decision

**Catalog + mechanism in the spec; usage content in runtime config.**

1. **Channel catalog (spec, referential).** `DeliveryChannelCatalog` declares the known channels with
   spec-level defaults (`kind` POOL|PARTNER, `default_offer_ttl_seconds`). Channels are keyed by a
   data-driven `DeliveryChannelKey` slug (not a fixed enum), so a new partner is a catalog row + an
   adapter, no scalar edit. Every PARTNER channel must have a wired `services.yaml` `delivery`
   implementation.
2. **Usage config (runtime).** `CityDeliveryRanking` (`city_id, rank, channel, ttl_override, …`) is the
   per-city ordered walk list; a `city_id IS NULL` row is the platform default. `RestaurantDispatchConfig`
   (`restaurant_id, city_id, mode, self_dispatch_ttl`) carries the restaurant's city + dispatch mode.
   `City` is a first-class entity (`CityId`). All seeded/managed, later API-writable (partner
   self-registration, #61).
3. **`DeliveryDispatchProcess` = resolve → walk.** The birth leg resolves the plan
   (`RestaurantDispatchMode`: `RESTAURANT` → `SELF_DISPATCHED`, Captain tracks but never offers;
   `CAPTAIN` → offer the rank-1 channel). One shared **advance** behaviour is reached by three legs —
   `DeliveryRejectedByPartner`, `DeliveryOfferTimedOut`, `DeliveryEscalationRequested` — each offering
   the next-ranked channel, or recording the existing terminal `DeliveryDispatchFailed` when the ranked
   list is exhausted (**the list length is the bound; fail-closed**, `rules.yaml#/DispatchExhaustionFailsClosed`).
4. **`offer_job` carries a `channel` target**, routed by a **composite `DeliveryService`** (channel→adapter
   registry) in the composition root. An unwired/unconfigured channel falls through via the offer-timeout
   escalation (see Consequences), so unconfigured deployments (V0 Tours without Uber Direct) are unchanged.
5. **Timeout is a dedicated worker.** `DeliveryOfferTimeoutWorker` (mirrors the retention sweep;
   env-gated) escalates `OFFERED` runs stale past the resolved TTL = `min(global env max, city override ??
   channel default)`, recording `DeliveryOfferTimedOut`. This implements the ADR-004556 §5 deferred timeout.
6. **Manual escalate** is the `EscalateDelivery` command (roles RESTAURANT/ADMIN) → `DeliveryEscalationRequested`.

### Codegen note

A new PM step value form `{ from_hook: <name> }` was added: an async, rowless, orchestrator-resolved
value usable on any leg (including a birth leg with no state row). It emits a hook the runtime implements
(strategy/channel resolution reads the config tables) — the DSL carries the skeleton, the hook carries the
config-reading logic, mirroring how the old cap arithmetic lived in the `compute_*`/`branch` hooks.

## Alternatives considered

- **Fixed `DeliveryChannel` enum** — rejected: a new partner (or a restaurant's own deal) can't be
  anticipated in the spec; a data-driven catalog + slug key keeps additions to config + an adapter.
- **Per-restaurant ranking** — rejected: ranking is a city concern; the restaurant dimension is only
  *who dispatches* (CAPTAIN vs RESTAURANT). Keeps the config small.
- **Timeout inside the PM runner poll loop** — rejected: the runner is event-drain-only; a dedicated,
  env-gated worker (the established retention-sweep pattern) is cleaner and independently testable.
- **Unwired channel throws a synchronous decline** — rejected: `offer_job` returning a hard `Err` would
  poison the saga leg rather than advance it; the offer-timeout fall-through is fail-closed and uniform.

## Consequences

### Positive

- One routing foundation; #57 and #58 become an adapter crate + a catalog row + a `services.yaml`
  implementation, with **no** further saga change.
- The volume switch (promote Avelo37 to rank 1) is a config reorder, zero code.
- Restaurant self-dispatch is first-class and keeps customer order tracking; every dispatch run still
  terminates (`ACCEPTED`/`COMPLETED`/`FAILED`/`SELF_DISPATCHED`).

### Negative

- An **unconfigured** partner channel (e.g. `uber_direct` in Tours today) falls through only **after its
  TTL**, not instantly — a future adapter can decline synchronously via its webhook.
- The V0 seed duplicates the Tours ranking under a `city_id IS NULL` platform-default row so that
  restaurants with no `RestaurantDispatchConfig` still route.

### Follow-up actions

- #57 Uber Direct adapter and #58 CoopCycle adapter register their channels on this seam.
- #61 partner self-registration (city-level availability via GraphQL) writes the usage config.
- #62 delivery-delay satisfaction check consumes the delivered/late signals.
- A synchronous decline path for unconfigured channels (avoid the TTL fall-through) when an adapter can.
