# ADR-0041 — The acting user is envelope metadata, not event/command payload

## Status

Accepted (CTPO, 2026-07-07). Reinforces the "event payloads = business only" convention (CLAUDE.md) and
complements ADR-0037 (the `domain_events` store schema).

## Context

Several events carried the acting user in their payload — `RestaurantAccountRegistered.createdBy`,
`*Updated.updatedBy`, `RestaurantAcceptanceModeChanged.changedBy`, `RestaurantListingClaimed.claimedBy`,
`OrderAcceptedByRestaurant.acceptedBy`, `DeliveryCancelled.cancelledBy` (all typed `UserId`), and the
`AcceptOrder` command's `acceptedBy`. But **who** performed an action is the same kind of technical,
always-present, cross-cutting metadata as **when** it happened (`occurredAt`): it is known for every write
from the authenticated context, and the store already records it — `domain_events` has `user_id` +
`user_type` columns (ADR-0037). Duplicating it into each business payload makes every event heavier for no
business gain and invites drift between the payload copy and the envelope truth.

## Decision

The acting user is **envelope metadata**, recorded once on `domain_events.user_id` / `user_type` by
infrastructure at append time — **never** a field of a business event or command payload. Removed the
`UserId`-typed actor fields (`createdBy`/`updatedBy`/`changedBy`/`claimedBy`/`acceptedBy`/`cancelledBy`)
from `events.yaml` and `commands.yaml` (and the matching test fixtures).

**Distinction — a business ROLE stays.** A field that changes business *semantics* by naming a *party/role*
(not identifying the acting user) remains a payload field. Example kept: `OrderTipped.tippedBy` typed
`Tipper` (`CUSTOMER | RESTAURANT`) — which side tipped affects the split, so it is business data.

**Aggregates may still expose `createdBy`.** The `RestaurantAccount` / `Restaurant` aggregate entities keep
`createdBy` (+ `createdAt`): the aggregate legitimately exposes its creator, **reconstructed from the
creation event's envelope** (`user_id` / `occurredAt`) during the fold — not read from a payload field.

Rule of thumb: if it answers *who did this / when* and is uniform across all events → envelope. If it is a
*business fact of what happened* (including a role that changes outcomes) → payload.

## Consequences
### Positive
- Lighter, purely-business event payloads; one source of truth for the actor (`domain_events.user_id`).
- No payload/envelope drift; authz/audit stay a cross-cutting concern, out of the domain model.
### Negative / risks
- Reconstructing an aggregate's `createdBy` requires the fold to read the envelope (`user_id`), not just
  the payload — the projector/rebuild must thread envelope metadata (it already receives the `Envelope`).
- Inbound/system events (Stripe webhooks, Sirene/Google sync) have no human user — the envelope's actor is
  a system/integration principal (nullable / typed by `user_type`), which is correct and expected.

## References
CLAUDE.md event-payload convention; ADR-0037 (`domain_events` schema, `user_id`/`user_type`). The projector
`Envelope` (ADR-0040) already carries the metadata a fold needs.
