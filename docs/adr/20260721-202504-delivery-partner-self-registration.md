# ADR-20260721-202504 — Delivery partner self-registration: EXTERNAL write-path + admin approval (#61, slice 1)

## Status

Accepted (builds on ADR-20260721-161939; issue #61, first slice of an L "likely split")

## Context

The #60 dispatch foundation (ADR-20260721-161939) split delivery routing into a **spec-side channel
catalog** and **runtime usage config** — `City`, `DeliveryChannelCatalog`, `CityDeliveryRanking`,
`RestaurantDispatchConfig`, all **seeded referential tables** whose header explicitly earmarks them as
"later API-writable via partner self-registration #61". Today adding a partner to a city is a hand-wired
config/seed edit with an engineer in the loop.

Issue #61 wants delivery partners to **onboard themselves at the city level** through a partner-facing
surface over the EXTERNAL GraphQL role, for three partner shapes (already-integrated / willing-to-be-
integrated / self-integrate) plus a partner web app. It is sized **L (likely split)**.

Forces:
- The config tables are **referential (seeded), not event-sourced** — a runtime write path collides with
  the CQRS model (commands → events → `View_*`), so the write path needs its own event-sourced home.
- Self-registration must be **reviewable** — an unverified partner must not enter live routing unchecked.
- The scope is large; a first slice should be the **riskiest, highest-value core** (the write-path +
  approval + read model) without destabilising #60's dispatch saga (220+ tests).

## Decision

**Slice 1 = the event-sourced self-registration write-path + admin approval gate + a queryable read
model, API-only over the EXTERNAL role.** Dispatch *consumption* of the approved set is a deferred
follow-up.

- New aggregate **`DeliveryPartnerRegistration`** (id = client-generated `registrationId`): one
  registration per (partner, city, channel). Commands
  `RegisterDeliveryPartnerAvailability` (EXTERNAL/ADMIN, birth → PENDING),
  `ApproveDeliveryPartnerAvailability` (ADMIN, PENDING → APPROVED),
  `RevokeDeliveryPartnerAvailability` (EXTERNAL/ADMIN → REVOKED). All invariants are **self-contained**
  (derived from the aggregate's own stream): already-requested / not-found / not-pending. Referential FK
  integrity (channel-in-catalog, city-exists) is a **boundary concern deferred** with the consumption
  wiring — the pattern for known channels is a picker fed by the catalog, not a domain throw.
- New read model **`View_DeliveryPartnerAvailability`** (fold view over `domain_events`, status derived)
  backs the first **EXTERNAL query** `deliveryPartnerAvailabilities` (partner tracks its submissions;
  admin works the review queue). EXTERNAL is a trusted partner-ACL role, so slice 1 does no per-owner
  narrowing (a recorded gap).
- The **APPROVED subset is the substrate** the #60 `CityDeliveryRanking` walk will consume; that wiring
  (and the referential FK checks) is the immediate next slice — #60's dispatch saga is untouched here.

## Alternatives considered

- **Command writes referential `CityDeliveryRanking` rows directly** — rejected: breaks the
  "referential = seeded once, not written at runtime" model and gives no review/audit trail.
- **One PR for the whole issue** (all three partner shapes + an SDUI partner web app) — rejected: an L
  issue explicitly marked "likely split"; a single large PR risks a long review/merge cycle and couples
  the risky saga-consumption change to the write-path.
- **Auto-approve (no gate)** — rejected: an unverified partner would enter live routing without review;
  the issue calls for an approval/verification workflow.
- **Collapse partner + availability into per-city aggregates vs a separate partner aggregate** — chose a
  single per-(partner, city) `DeliveryPartnerRegistration` aggregate: one row per fold (fits the view
  generator), leanest coherent write-path.

## Consequences

### Positive
- Delivery partners self-register city availability and admins review it with **no code change per
  partner/city** for the common case — the core value of #61.
- Additive: no change to #60's dispatch saga or its tests; the new read model is exactly what the
  consumption follow-up will read.
- Establishes the **EXTERNAL partner-portal query surface** and the reviewable write-path both later
  slices build on.

### Negative / deferred (recorded gaps)
- Approved availability is **not yet consumed by dispatch** — an approval is currently informational
  until the consumption slice wires `View_DeliveryPartnerAvailability` (APPROVED) into the city ranking.
- **No referential FK enforcement** on channel/city in the domain layer yet (boundary concern deferred).
- **No per-owner scoping** on the EXTERNAL query (trusted partner-ACL assumption) and **no partner SDUI
  web app** — API-only.
- The "willing-to-be-integrated" onboarding-request and "self-integrate" partner shapes are **later
  slices / follow-up issues**.

### Follow-up actions
- Consume the APPROVED set in the #60 `CityDeliveryRanking` walk (+ enforce channel-in-catalog /
  city-exists at that boundary).
- Per-owner (contact/JWT) scoping for the EXTERNAL query; the onboarding-request flow; the partner SDUI
  app (a new `specs/screens/*.yaml`).
