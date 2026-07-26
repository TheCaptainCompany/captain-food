# Architecture review — probe checklist

Each probe records the **state as of the 2026-07-26 baseline**. A probe whose result differs from the
baseline is either progress (something landed) or a regression (something got worse). Both are worth
reporting; neither should be re-filed if already tracked.

Always confirm in code with `file:line`. Never report from this file alone — it is a starting point,
not evidence.

---

## Order lifecycle

| Probe | Command / location | Baseline 2026-07-26 |
|---|---|---|
| Acceptance timeout | `specs/processmanager.yaml` — any PM triggered by `OrderPlaced`? | none (#167) |
| Opening hours enforced | `crates/application/src/commands.rs` `place_order` guards | not enforced; no `RestaurantClosed` error (#180) |
| Capacity | `grep -rn "BUSY" crates/application/src` | `BUSY` unenforced; `PAUSED` untimed (#186) |
| ETA | `grep -rn "preparationTimeMinutes\|estimatedReadyAt" crates/` | stored, consumed by nothing (#182) |
| Scheduling | `grep -rn "scheduledFor\|requestedFor" specs/` | zero hits (#197) |
| Order modification | `grep -n "ModifyOrder\|ChangeDeliveryAddress" specs/commands.yaml` | zero hits (#197) |
| Restaurant notification | `grep -rn "subscription:" specs/screens/restaurant_backoffice.yaml` | none (#166) |

## Money

| Probe | Command / location | Baseline |
|---|---|---|
| Fee legs | `sed -n '100,115p' crates/application/src/pricing.rs` | all zeroed (#172) |
| Connect / payouts | `grep -rn "transfer_data\|application_fee\|acct_\|/v1/transfers" crates/` | zero hits (#173) |
| Payout destination | `grep -rni "iban\|connectAccount" specs/ crates/` | zero hits (#173) |
| VAT computed | `grep -n "tax" crates/application/src/pricing.rs` | never read (#174) |
| Invoice / receipt | `grep -rni "invoice\|facture" specs/ crates/` | none (#174) |
| Capture method | `grep -n "capture_method" crates/adapters/stripe/src/outbound.rs` | absent ⇒ automatic capture (#175) |
| Idempotency key | `sed -n '45,75p' crates/adapters/stripe/src/outbound.rs` | not sent (#176) |
| Refund run key | `grep -n "order_id" specs/database/tables/process_managers.yaml` | `order_id` is PK ⇒ one run per order (#177) |

## Authorization — **check every run; this is the highest-value regression class**

| Probe | Command / location | Baseline |
|---|---|---|
| Unscoped read ports | `grep -n "async fn by_id" crates/application/src/queries.rs` | take no principal (#144) |
| Optional filters that leak everything | `orders`, `pendingRefunds`, `restaurantReclamations` in `crates/server/src/graphql/generated/query.rs` | omit the filter ⇒ whole platform (#144) |
| Write-side scope | `grep -n "restaurant_id" crates/application/src/commands.rs` (accept/reject/cancel) | caller-supplied, unchecked (#178) |
| `riderId` forgeable | `accept_delivery` in `crates/application/src/commands.rs` | never compared to the actor (#178) |
| `Principal` shape | `crates/server/src/auth.rs:45-48` | `user_id` + `role` only (#178) |
| GraphQL limits | `crates/server/src/graphql/schema.rs:92-125` | no depth/complexity/introspection limits (#179) |
| Router middleware | `crates/server/src/lib.rs:575-661` | `response_timing` only (#179) |

**New-query check:** for every query or mutation added since the last review, confirm it either scopes
by the verified principal or has a *declared* exemption. An unscoped new operation is a regression even
if #144 is still open.

## Runtime correctness

| Probe | Command / location | Baseline |
|---|---|---|
| Drain visibility guard | `crates/infrastructure/src/projection/worker.rs:298-306` | none — position gap (#189) |
| Same for sagas | `crates/infrastructure/src/process_manager/runner.rs:222-261` | none (#189) |
| Lag computation | `worker.rs:274-283` | returns `(head, head)` ⇒ lag always 0 (#190) |
| Poison handling | `worker.rs:310-324` | logged, skipped, checkpoint advances (#190) |
| Reprojection tooling | `grep -rn "reproject" Makefile crates/` | none (#190) |
| Event versioning | `grep -rni "upcast\|event_version" specs/ crates/` | zero hits (#192) |
| Leader election | `grep -rn "pg_advisory\|SKIP LOCKED" crates/` | zero hits (#193) |
| Stream index | `grep -n "CREATE INDEX" specs/generated/schema.generated.sql` | `(stream_name, version)` — wrong column for the fold views (#193) |

## Observability

| Probe | Command / location | Baseline |
|---|---|---|
| Telemetry dependency | `grep -n "opentelemetry\|tracing" Cargo.toml` | absent (#191) |
| Logging style | `grep -rc "println!\|eprintln!" crates/server/src crates/infrastructure/src` | 69 calls (#191) |
| Contracts | `specs/observability.yaml` | 755 lines, 0 emitted (#191) |

## Catalog & compliance

| Probe | Command / location | Baseline |
|---|---|---|
| Allergens | `grep -rni "allergen" specs/ crates/` | **zero hits** (#184) |
| Product images | `grep -rn "upload\|Upload" specs/ crates/` | zero hits; `image_ids: vec![]` hardcoded (#185) |
| Catalog UI | `grep -rn "addProduct\|updateOfferStock" specs/screens/` | no screen binds them (#171) |
| `updateOfferStock` roles | `specs/api.yaml` | `[ADMIN, RESTAURANT_ACCOUNT]` — excludes `RESTAURANT` (#171) |
| Stock decrement | `projection_tables.yaml#/Catalog` `fedBy` | `OrderPlaced` absent (#183) |
| Checkout re-validation | `crates/application/src/commands.rs:2066-2069` | TODO (#183) |
| GDPR erasure | `grep -rni "erase\|anonymi\|DeleteAccount" specs/commands.yaml specs/events.yaml` | zero hits (#194) |
| Privacy / terms | `grep -rli "privacy policy\|CGV\|DPIA" docs/ specs/` | none (#194) |

## Delivery

| Probe | Command / location | Baseline |
|---|---|---|
| Delivery area | `crates/application/src/commands.rs:2065` | TODO; no zone model (#181) |
| Missing mutations | check `RegisterRider`, `DeclineDelivery`, `ReportDeliveryIssue` in `specs/api.yaml` | 9 commands unreachable (#187) |
| Job pool filter | `crates/infrastructure/src/persistence/delivery.rs:85` | global, unfiltered by city/zone/status (#188) |
| Proof of delivery | `grep -rni "proofOfDelivery\|handoverCode\|contactless" specs/ crates/` | zero hits (#188) |
| Abandonment path | `specs/actors.yaml` DeliveryJob lifecycle | no `ASSIGNED → PENDING` rider edge (#188) |

## Gates

```
make validate     # must be 0 errors (known view design-hole warnings excepted)
make rust         # build + test + validate + generate
make check-drift  # spec ↔ generation drift
```

For any screen touched since the last review, hand-check that each action's `variables` satisfy the
bound mutation's `required` list — the validator does not do this yet (#169).
