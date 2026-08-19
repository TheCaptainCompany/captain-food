# 🚦 Captain.Food — Development & Deployment Status

> Hand-maintained snapshot (NOT generated, outside `specs/` so it never affects the DSL).

> **This file is CURRENT STATE.** The running journal — 223 entries, 2026-07-20 → 2026-08-19 — moved
> to [`docs/status/`](status/) on 2026-08-19, one file per ISO week, entries byte-identical. This page
> now opens with what the system IS; the last two weeks are indexed at the bottom, and the archive is
> linked from there. Rationale and the measurement that forced it:
> [ADR-20260819-174300](adr/ADR-20260819-174300-status-md-is-current-state-the-journal-moves-to-iso-week-files.md) · the boot reading order was 1.32 MB, of which this file was 632 KB.

> **Writing a new entry**: append it to the CURRENT week file, `docs/status/journal-<ISO-week>.md`,
> newest first, and add its one-line row to *Recent changes* below. Update the durable sections above
> in place when they change. One file per week means concurrent sessions do not conflict on a shared
> head — the same reason `.claude/loop-budget/<ISO-week>/` is shaped that way (ADR-20260812-011057).

## 🌐 Deployment

| Piece | Status | Notes |
|---|---|---|
| Render web service (Docker, Frankfurt) | ✅ | Blueprint IaC (`render.yaml`), cargo-chef cached build, verified live |
| Supabase Postgres (Frankfurt, eu-central-1) | ✅ | Session pooler; Data API off (intentional) |
| CI workflow `ci` (build+test+validate+drift; ex `codegen-consistency`) | ✅ | Gates deploys (`autoDeployTrigger: checksPass`) |
| CI `db-migrate` (sqlx-cli, gated on green build) | ✅ | Applies `migrations/*.sql` out-of-band (ADR-0043) |
| `/health` (schema-version readiness), `/ping`, `/projector` | ✅ | `>=` version gate; in-process projector |
| GraphQL `/{role}/graphql` + `/{role}/voyager` | ✅ | Role-as-path; per-role filtered schema |
| Custom domains `*.captain.food` (Dynadot wildcard → Render) + Host router | ✅ | Wildcard TLS issued; apex+`www` 301→`join` (GitHub Pages); `hosts.rs` dispatches audiences (`live`/`restos`/`riders`/`system`) + `{slug}` tenants; onrender URL disabled. Recorded in **ADR-0036 amendment (2026-07-18) + ADR-0042** |

## 📖 Read side (queries)

| Query | Status | Notes |
|---|---|---|
| `restaurants` / `restaurant` | ✅ | Real data once SIRENE runs |
| `prospectionPipeline` | ✅ | Admin; fed by SIRENE registrations |
| `pricingPolicy` / `uberEstimationPolicy` / `uberSplitPolicy` | ✅ | **Real seeded data** |
| `catalog` / `categories` | ✅ | **Real nested data** — catalog `tree` projector (categories→products→offers/option-lists + derived `stockStatus`) |
| `carts` / `cart` / `orders` / `order` | ✅ wired | Populated as carts/orders are placed |
| `me` / `favoriteRestaurants` | ✅ | `me` resolves the verified ADR-0047 `Principal` → Customer read model; `favoriteRestaurants` joins the customer's favourites |
| Projection worker → registry (per-aggregate checkpoints) | ✅ | In-process; **no batch cap** (drains all pending per tick, loops 1.5s); hardened to **log-skip a poison event** so one bad record can't wedge projection. ⚠️ Free-tier **spin-down** pauses it when the app is idle >15 min → kept warm via **uptimerobot `/ping` every 5 min** |

## ✍️ Write side (mutations)

| Piece | Status | Notes |
|---|---|---|
| `MutationRoot` (all api.yaml mutations generated) | ✅ | |
| Restaurant aggregate (13 commands) | ✅ | Spec invariants (event-stream rehydration) + 25 behaviour tests |
| Cart (3) · Order (11) · DeliveryJob (4) | ✅ | Round 2a — real invariants + tests; **Cart line-checks now enforced** (OfferUnavailable/InsufficientStock/InvalidOptionSelection) via the catalog offer read port |
| Catalog (12) · Prospect (3) · RestaurantAccount (3) | ✅ | Round 2b — real invariants + behaviour tests |
| Customer (14) | ✅ | Wired end-to-end: `customer` read model + Pg repo, fail-closed `AuthProviderGateway` stand-in (real Supabase ACL deferred), injected at the composition root |
| `placeOrder` + process managers (4 sagas) | ✅ wired | `placeOrder` live (fail-closed `PaymentGateway` stand-in); in-process PM runtime (`/saga`) — PlaceOrder/Refund/CartBinding/DeliveryDispatch react to payment/delivery facts → `OrderPlaced`/`OrderDelivered`/… **Real Stripe create-intent = 🅑**; ✅ **checkout-snapshot DSL closed** (ADR-20260719-014434): `PaymentIntentCreated` now carries `checkout` (`CheckoutSnapshot`), frozen by `place_order`, so `OrderPlaced` rebuilds from the log — priced `items`/`breakdown` + retiring the fail-closed `CheckoutSnapshotSource` ride on server-side pricing |
| Structured typed errors | ✅ | `DomainError::Rejected{code,context}` → GraphQL `extensions.code` + interpolated en/fr message (ADR-20260719-120000) |
| GraphQL **subscriptions** | ✅ | `SubscriptionRoot` + in-process event bus + WS transport + per-role ACL (`orderStatusChanged`/`operationStatusChanged`); works while the app is warm |

## 🔐 Authorization

| Piece | Status | Notes |
|---|---|---|
| Per-role ACL — execution guard + per-role introspection/Voyager | ✅ | Spec-derived from api.yaml `roles` (ADR-0006); role now **verified** by JWT (ADR-0047), so Voyager filtering is trustworthy |
| Per-field ACL on FK-derived nav edges | 📋 | api.yaml has **op-level** `roles` only; needs a DSL extension → **plan mode** |
| EXTERNAL machine callers | ✅ | Pre-shared `X-External-Api-Key` (`EXTERNAL_API_TOKENS`, constant-time) or Supabase JWT w/ captain_role EXTERNAL (ADR-0047) |
| **OTP send guards — country allowlist + per-number caps + global daily ceiling** | ✅ | **#516**, [ADR-20260813-021500](adr/ADR-20260813-021500-the-allowlist-is-the-economic-control-and-only-a-global-ceiling-bounds-the-bill.md). The OTP request endpoint is anonymous BY DESIGN and every send spends money on our **own OVH account** — it previously had **no limit of any kind**. Now: a fail-closed **exact-membership** allowlist (default `+33,+32,+41,+44,+49,+34,+39` — the served-country decision made executable; bare `+1` was dropped 2026-08-13, #535, because a calling code is not a destination: `+1` reaches every NANP territory, premium Caribbean ranges included, all billed like a Boston number), 3/hour + 5/day per **canonical** number with a 30s→2min→10min cooldown, and a **global daily ceiling with a no-deploy kill switch** (`UPDATE sms_send_quota SET sent_count=999999 WHERE quota_key='global:day'`) — the only guard that bounds the total bill, since an attacker rotates numbers. The counter is **shared** (`sms_send_quota`; one atomic `INSERT…ON CONFLICT…WHERE`, because there is no per-phone actor lane and that statement is the only serialisation). **The wall is `/auth/sms-hook`**, where the euro is actually spent; the identity ACL only *sheds*. Compiler-enforced: `OvhSmsClient::send` takes an unforgeable `AuthorizedSmsRecipient` **by value**, so one claim buys exactly one send (a `&`-borrow would let a loop spend the whole budget on a single claim). Refusals are four **typed** states a client can tell apart, rendered from the server's own `errors.yaml` `messages.{en,fr}` — the single source; there is deliberately no second client-side copy of that string, and the client render path itself is #518/#521. Liveness is an **observable** gauge (`otp_send_guard_enforcing`) re-asserted on every export cycle and re-declared where enforcement is decided, not stamped once at boot. **The 200/day ceiling is derivable, not a guess**: OVH SMS France is €0.06 HT/SMS ([PROP-20260724-233605](proposals/PROP-20260724-233605-ovh-sms-hook.md), founder-approved 2026-07-24, screenshot-confirmed) → €12/day worst case France-rated; still unknown are OVH's per-destination multipliers and which pack was purchased. The account is a **prepaid pack**, so the real failure mode is a drained pack = a founder-gated phone-login outage, not an invoice (#535 corrections in [ADR-20260813-021500](adr/ADR-20260813-021500-the-allowlist-is-the-economic-control-and-only-a-global-ceiling-bounds-the-bill.md); owed there: an observed-but-not-served telemetry label set, and a credit-balance gauge). |
| Authentication / identity (Supabase JWT) | ✅ | **First cut shipped (ADR-0047)**: verify Supabase JWT via JWKS at `/{role}/graphql` (public keys, no shared secret; ~1h cache, serve-stale-on-refresh-failure — no per-request Supabase call); `app_metadata.captain_role` gates the path (`/public` open, else 401/403), fail-closed on cold cache, asymmetric-only. Verified role + `Principal` injected. **EXTERNAL service tokens** via `X-External-Api-Key` (constant-time, `EXTERNAL_API_TOKENS`) shipped. Per-field `@auth` on FK-nav edges = DSL/plan-mode follow-up |

## 🔎 SIRENE prospection (ADR-0019/0020/0027/0045)

| Piece | Status | Notes |
|---|---|---|
| SIRENE ACL (INSEE → RegisterRestaurant mapping) | ✅ | Unit + DB verified |
| Interim direct-write `sirene_sync` binary | ✅ | **Retired** (ADR-0045) — replaced by the split below |
| `external_sirene_restaurants` staging table | ✅ | Migration applied by CI |
| Thin CI ingestion crate `sirene_ingest` (fetch → UPSERT raw rows, France-wide by department, active-only) | ✅ | No domain deps; scheduled workflow builds only this crate |
| On-app `sync_sirene_worker` (ACL on deployed version) + deletion reconciliation | ✅ | Per-row checkpoint; detect-by-absence (21d debounce) + explicit `F`/`C`; NON_PARTNER auto-close, partners flagged; `POST /internal/sirene/drain` (token-gated, fail-closed) |
| `INSEE_API_TOKEN` repo secret | ✅ | Added. **⏳ PAUSED 2026-07-28** — the scheduled ingestion → staging → worker chain is stopped at both ends until [#220](https://github.com/TheCaptainCompany/captain-food/issues/220) |
| `INTERNAL_TRIGGER_TOKEN` (Render env + repo secret) to enable the CI→worker ping | ⏳ | Optional; unset, so `POST /internal/sirene/drain` is fail-closed (503). `RUN_SIRENE_WORKER` now **defaults OFF** (paused, #220) |

## 🔌 External integrations — partner adapters & M2M (ADR-20260718-145856 / -213352)

**Partner webhook adapters are self-contained crates** under `crates/adapters/*` — each an ACL +
axum shell + standalone binary, mountable into the monolith **or** deployable as its own web service.
Two directions: partner-**push** webhooks (below) vs external-**drive** `/external/graphql` (M2M).

| Piece | Status | Notes |
|---|---|---|
| **Stripe** — `crates/adapters/stripe` (`POST /adapters/stripe/webhooks`, `stripe-webhook` bin) | ✅ | `Stripe-Signature` HMAC over raw body (constant-time, 300s replay, fail-closed); ACL → `PaymentCaptured`/`PaymentFailed`/`PaymentRefunded`; idempotent by Stripe event id. 12 tests |
| Checkout must set `metadata.restaurantId` (+`orderId`) on the PaymentIntent/charge | ✅ | `StripePaymentGateway` sends `metadata[orderId]`/`[restaurantId]`/`[cartId]` on create-intent — the webhook ACL maps `charge.refunded` from them; exercised by the green prod smoke |
| **HubRise** — `crates/adapters/hubrise` (`POST /adapters/hubrise/webhooks`, `hubrise-webhook` bin) | ✅ | **Ingress** ✅ (HMAC-SHA256 hex, fail-closed, envelope parse). **Outbound OAuth2 client** ✅ (`api.rs`: `X-Access-Token`, non-expiring token from `HUBRISE_ACCESS_TOKEN`, `exchange_code` connect helper, catalog/inventory pull). **Domain wiring** ✅ (`enrich.rs`): verified catalog/inventory callback → API pull → enrichment ACL → `ImportCatalog` / per-SKU `update_offer_stock` handlers. **Deterministic UUIDv5-of-HubRise-id** ids reconciled with the **Catalog aggregate** (offer seeded from the SKU `ref` = inventory's `sku_ref`, so a stock update hits the imported `OfferId`); `"9.80 EUR"`→`Money`, tax-rate strings→`TaxRate`, `data` envelope translated at the boundary; catalog = rejectable command (`CatalogNotFound`→skip), inventory = reported fact (`OfferNotFound`→skip, never rejected). 14 tests. Enricher wired at the server composition root + the standalone bin (needs only `DATABASE_URL`). ✅ **Connect flow landed (#20, ADR-20260721-100601)**: OAuth connect provisions account/locations/catalogs with the derived ids + stores the account-scoped token in `hubrise_connections` (env token retired) |
| **`/external/graphql`** — M2M standard | ✅ | External entities query/mutate via the `EXTERNAL` role path; API-key auth (`X-External-Api-Key`, ADR-0047); allowlist is per-op `roles: [EXTERNAL]`. **Subscribe** = future (needs `SubscriptionRoot` + WS + `api.yaml`); per-partner keys = future |

## 👤 Ops / user actions

- ✅ Keep the web service **warm via uptimerobot `/ping` every 5 min** (prevents free-tier spin-down so the in-process projector + SIRENE worker keep running).
- 🗑️ `INTERNAL_TRIGGER_TOKEN` / `POST /internal/sirene/drain` — agreed to **remove** (superseded by the `/ping` warmth approach); code removal deferred to avoid colliding with concurrent `routes.rs` edits — harmless meanwhile (fail-closed 503 when the secret is unset).

> **Claim protocol (2026-07-20, ADR-20260720-233000, #39; amended 2026-07-21 by
> ADR-20260721-042018):** before working an issue, add the `status/in-progress` label + a claim
> comment naming the `NN-slug` branch, **create the branch and open a draft PR (`Closes #NN`)
> immediately**; NEVER work a claimed issue; on completion mark ready + enable auto-merge and
> supervise checks until MERGED; the hourly stale-claim reaper releases claims silent for >24h.
> Method: `BACKLOG.md`.

## 📋 Remaining work — todo & session split

> **⚠️ TRACKING MOVED (2026-07-20, user-directed): remaining work now lives in
> [GitHub issues](https://github.com/TheCaptainCompany/captain-food/issues) (#12–#28, typed
> Task/Bug/Feature) managed on the **org-level GitHub Project**
> ([github.com/orgs/TheCaptainCompany/projects](https://github.com/orgs/TheCaptainCompany/projects),
> created 2026-07-20) — not in this file.** Issues carry `size/*` labels + org issue fields
> Priority/Effort (mapping recorded in ADR-20260720-143000); the project's views read those
> directly, so triage state lives on the issue, never in a board-only field. New work items get an
> issue, not a table row; this file stays the narrative deployment/architecture snapshot. The table
> below is the last pre-migration snapshot, kept for history.
>
> **Issue workflow (2026-07-20, ADR-20260720-143000):** every issue is sized once with a
> `size/XXXS`…`size/XXXL` label (AI-native scale: agent sessions + cost + review, see the ADR
> table) and carries standard pre-task sections — *Why now? / What & why? / Impact / Sequence
> diagram / Estimation* (with its rank in the simplest→largest queue). The issue is the pre-task
> contract; the PR is the post-task record — overlap is intentional, divergence is signal. No
> Scrum: flow-based queue, cheapest-impactful first; re-size only on scope change; XXXL must be
> split before starting.

Two sessions run in parallel — 🅐 = this (desktop) session, 🅑 = the iPhone/other session. Pull-rebase before every push.

| # | Item | Owner | Status |
|---|---|---|---|
| 1 | **Checkout saga** — `placeOrder` + `PlaceOrderProcess` + PM runtime | 🅐 | ✅ wired (real Stripe gateway; smoke-proven in prod) |
| 1a | **Checkout snapshot** on `PaymentIntentCreated` (ADR-20260719-014434) — DSL + `place_order` freeze + tests done | 🅐 | ✅ DSL · runtime population + port retirement ride pricing |
| 1b | Stripe **outbound** `PaymentGateway` (create PaymentIntent) in the Stripe adapter crate | 🅐 (landed here, not 🅑) | ✅ `stripe::outbound::StripePaymentGateway` (create-intent + refunds, env-gated by `STRIPE_SECRET_KEY`, fail-closed stand-in otherwise) — exercised by the green prod smoke |
| 2 | **HubRise** domain ACL — webhook → `ImportCatalog`/`OfferStockUpdated` (OAuth2 pull + deterministic ref-mapping) | 🅐 | ✅ landed (`enrich.rs`, 14 tests) |
| 2a | **Connect flow** — provision `RegisterRestaurantAccount` + `Restaurant`(s) + `CreateCatalog` with the enricher's derived UUIDv5 ids, and persist the HubRise **account-scoped** token in `hubrise_connections` keyed by `RestaurantAccount`. See `docs/integrations/hubrise-process.md` §0 | #20 | ✅ (ADR-20260721-100601) |
| 3 | **Process managers** — Refund/CartBinding/DeliveryDispatch + PM runtime (event-driven, `/saga`) | 🅐 | ✅ (outbound refund via the real gateway; bounded partner re-offer landed — offer timeouts deferred, ADR-20260720-004556) |
| 4 | **Cart line invariants** + catalog `tree` projector + offer read port | 🅐 | ✅ |
| 5 | **Frontend** — Leptos/WASM SDUI renderer (customer/restaurant/rider apps) | unassigned | 📋 |
| 6 | GraphQL **subscriptions** (`SubscriptionRoot` + bus + WS + ACL) | 🅐 | ✅ |
| 7 | **Structured typed errors** (ADR-20260719-120000) | 🅐 | ✅ |
| 8 | **Per-field nav-edge ACL** — optional `roles:` on nav fields (default public), same guard/visible as ops; design agreed | 🅐 | 📋 plan mode (after ACL emitter free) |
| 8b | Delivery/account read queries + catalog `tree` + `me`/favorites | 🅐 | ✅ (read surface complete except `operation`; `phoneCountries` deleted with #305) |
| 9 | Remove `INTERNAL_TRIGGER_TOKEN`/drain endpoint (use `/ping` warmth) | 🅐 | 🗑️ deferred |
| 10 | Projection worker robustness (poison-skip) + spin-down mitigation (uptimerobot `/ping`) | 🅐 | ✅ |
| 10a | **Push-driven drain loops** ([#300](https://github.com/TheCaptainCompany/captain-food/issues/300), ADR-20260802-200416) — `pg_notify` in the append transaction + one `LISTEN` connection wakes the projector AND the saga runner; safety-net drain kept (NOTIFY has no replay) and the fallback reverts to the 1.5 s poll whenever the listener is down; idle head-gate skips per-group queries when the log has not moved | 🅐 | ✅ idle DB round trips ~70,900/h → ~120/h, and sagas react on commit instead of up to 1.5 s later. **Requires a session-mode pooler** (Supabase 5432); `RUN_EVENT_PUSH=false` forces polling |
| 10b | **Mailbox keyspace width 100 → 5** (ADR-20260802-220402) — post-#301 audit found the mailbox out-polls what #301 removed: 16 actor types × 100 lanes × one per-lane SELECT per 10 s pass ≈ 580k idle queries/h, un-gated. Width 5 in `specs/actors.yaml` + migration `20260802220000` (exact remap: 5 divides 100, so `partition % 5` = the width-5 stamp; rows remapped BEFORE registry shrink) | 🅐 | ✅ idle mailbox queries ~580k/h → ~29k/h. Real fix is 10c |
| 10c | **Push-driven mailbox** ([#313](https://github.com/TheCaptainCompany/captain-food/issues/313), [PROP-20260802-223522](proposals/PROP-20260802-223522-push-driven-mailbox.md) approved D1–D5, ADR-20260802-224532) — `pg_notify` at the `PgMailbox` door (one channel, actor-type payload) wakes workers cross-process; lanes-with-work idle gate; attempts-cap poison policy (`FAILED` + error at the cap); gated `RUN_MAILBOX_PUSH` + `MAILBOX_MAX_DELIVERY_ATTEMPTS` | 🅐 | ✅ door notifies in the enqueue tx (`PgMailbox` + PM chain); listener per process feeds the nudge map cross-process; full pass 60 s under confirmed push (beat stays on heartbeat, degradation = pre-push cadence); poison cap default 5 (`0` = old behaviour); retries back off EXPONENTIALLY since #316 (base x 2^(N-1), ~5 min to terminal at cap 5); heartbeat/lease/cap wired from Config (MAILBOX_* keys were previously unread) |
| 11 | **CoopCycle** delivery partner (#58) — third `PARTNER` adapter; **federated** per-instance registry + OAuth2 (ADR-20260721-122910) | 🅐 | 🚧 PR #59: DSL surface (staging + services + obs + c4 + integration doc) landed; `crates/adapters/coopcycle` + server wiring in progress |

## Production is DELIBERATELY SUSPENDED — a decided state, not an open incident (decided 2026-08-17)

**Founder answer, 2026-08-17** ([DECISIONS §45 PROD-1](proposals/DECISIONS.md),
[ADR-20260817-105844](adr/ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)):
**production stays down and the team walks locally.** The team had recommended restoring with signup
closed at the auth provider; he declined. **The walk does not need production** — `stripe listen
--forward-to` is outbound-only, so the money path walks against a local stack with the CLI's own
signing secret and no cluster ingress (ADR-20260813-004634). Nothing here waits on an account action
any more: restoring is a decision to re-take, not a task to finish.

**The real defect was never the 503.** Verified against the Actions API on 2026-08-17: the nightly
`prod-smoke` has been **RED for 19 consecutive scheduled runs** — last green **2026-07-29**,
2026-07-30 through 2026-08-17 with no gaps — of which the billing suspension explains only **13**.
Six earlier red nights (2026-07-28, 2026-07-30 through 2026-08-04) have an **unrecorded cause**. A
scheduled gate whose red is expected trains everyone to skip the one signal that would say something
new. **Owed** (not filed by the records run that landed the answer): re-point the nightly at the
local walk target, or disable its schedule with the reason recorded in the workflow.

*History*: the Render web service `srv-d9ctcpgk1i2s73cj6820` went `suspenders: ["billing"]` at
~2026-08-04 12:26 UTC and `captain-food.onrender.com` has returned 404 since. `render-status` was
fixed in that run to report **red** on suspension (ADR-20260805-070138) — it previously read only the
last deploy's status and showed a false green while prod was down.

## 🧭 Architecture decisions
See [`docs/adr/`](adr/) — latest: **20260802-200416 (drain loops woken by Postgres NOTIFY, not a 1.5 s poll — background polling was 95% of outbound bandwidth)**, 0047 (API auth — Supabase JWT/JWKS), 20260719-120000 (structured domain rejections), **20260719-014434 (checkout snapshot on `PaymentIntentCreated`)**, **20260719-031136 (write-side `Repository` / event-sourced actors — handlers + saga runner route through it, never the raw `EventStore`)**, 20260718-145856 amendment (adapter webhook routes → `/adapters/{partner}/webhooks`). **ADR ids are now date-time** to avoid concurrent-session collisions (ADR-20260718-135417).

> Convention: keep this file current with every substantive change, and record cross-cutting decisions as an ADR in the same change.

## 📜 Recent changes

One line per entry for the current and preceding ISO week. Full entries — and everything older —
are in the week files linked under each heading. **These counts are hand-maintained**: nothing yet
gates that an appended entry also gets its row (see the ADR's follow-up).

### [2026-W34](status/journal-2026-W34.md) — 2026-08-17 → 2026-08-19 · 24 entries · current week

- `2026-08-19` 🛑 DESIGN APPROVAL ONLY: the register ruling is AMENDED, all four defects are CORRECTED, and NOTHING…
- `2026-08-19` ⚖️ THE REGISTER RULING: PROP-20260819-110442 IS APPROVED, AND THE RULING CARRIES FOUR DEFECTS OF ITS…
- `2026-08-19` 📌 THE TWO FOLLOW-UPS ARE FILED, AND NEITHER IS APPROVED TO BUILD
- `2026-08-19` 🗃️ STATUS.md IS CURRENT STATE: the journal moves to ISO-week files, 628 654 B → ~33 KB
- `2026-08-19` 🧹 `sessions.md` SPLIT: 134 KB → a 10 KB index + four topic files, no rule removed. Founder…
- `2026-08-19` 📄 THE DECISION REGISTER IS THE UNIT OF DECISION: proposal filed, `Proposed`, NOT dispatchable
- `2026-08-19` ✅ THE SIX QUEUE ANSWERS LANDED: four register rows close, two open
- `2026-08-19` 🗓️ COST-OF-DELAY ORDER REVERSED: the Stripe answer moves two windows
- `2026-08-18` 💶 THE TEN ANSWERS LANDED: per head, monthly invoice, stop checkout
- `2026-08-18` 🧭 THE TEAM ASKED, THE FOUNDER ANSWERED: no human maintains this Rust
- `2026-08-18` ↩️ CAPTURE ON DELIVERED DISSOLVES THE REFUND GAP
- `2026-08-18` 📸 TWO PHOTOS, and the labels are OPTIONAL
- `2026-08-18` 🧾 THE INVOICE CHAIN IS RULED: restaurant → customer, rider → RESTAURANT, Captain self-bills both
- `2026-08-18` ✅ DECISION QUEUE CLEARED: the restaurant signs in by EMAIL LINK, and #638 FREEZES at chunk 1
- `2026-08-18` ⚖️ RULINGS: staff sign-in has a mechanism, refund approval stays with the restaurant, and the…
- `2026-08-18` 🔐 GENERATED SECURITY SQL EXISTS, APPLIED TO NO DATABASE, SINCE 2026-08-18
- `2026-08-18` 🗂️ RECORDS: the founder's own rationale for database-level security, plus the two register rows owed…
- `2026-08-18` 🕸️ FOUNDER RULING: BUILD THE GRAPH ENGINEERING. Plan committed as PROP-20260818-013222
- `2026-08-18` 🔐 THREE FOUNDER RULINGS: THE TOKEN CARRIES NO BUSINESS IDENTIFIER, RLS LANDS AT THE CUTOVER ON THE…
- `2026-08-17` 🛒 THE SMOKE'S CART READ IS A PAIR ON TWO HOSTS, NOT ONE READ ON THE WRONG ONE
- `2026-08-17` 🔎 A FAILED CHECKOUT IS ATTRIBUTABLE AGAIN, AND THE JOURNAL ROW CAN NO LONGER CARRY A STRIPE KEY
- `2026-08-17` 🗳️ THE FOUNDER ANSWERED THE WHOLE DECISION QUEUE: THE WALK GOES FIRST ON ONE DATABASE, PRODUCTION…
- `2026-08-17` 🧾 FOUR RECORDS PUT RIGHT: THE IDOR COVERS 83 OF 118 OPERATIONS, NOT THE ORDER LIFECYCLE'S WRITES,…
- `2026-08-17` 🔒 THE PUBLIC-REPO CORRECTION: SIX FALSE CONTROL CLAIMS FIXED, ONE THEATRE CONTROL REPLACED, AND THE…

### [2026-W33](status/journal-2026-W33.md) — 2026-08-10 → 2026-08-16 · 77 entries · preceding week

- `2026-08-16` 🔒 THE LANE WIDTH IS NOW UNSPELLABLE, NOT MERELY UNSPELLED
- `2026-08-16` 🛟 A LANE IS ADDRESSED FROM THE DECLARATION, AND AN UNSEEDED LANE NOW WAITS INSTEAD OF POISONING A…
- `2026-08-16` 🧑‍🤝‍🧑 THE MOB'S CHECKPOINT IS NOW THE CONCERN-DECLARED SUBSET, AND REVIEW IS PRICED BY REVERSIBILITY
- `2026-08-16` 💸 "MONEY HELD, NO ORDER" IS NOW A SIGNAL THE SYSTEM EMITS
- `2026-08-16` 📡 THE ORDER LANE HAS A HEARTBEAT, AND THE CHECKOUT SUCCESS RULE STOPS LYING
- `2026-08-16` 💸 THE LOOP'S CONTEXT BUDGET IS NOW A RECORD, AND ONE HALF OF IT IS THE FOUNDER'S
- `2026-08-16` ✂️ THE TOKEN DIET IS LANDED
- `2026-08-16` 📮 `deliver:` IS RULED A LANE ENQUEUE, NOT A FOREIGN-STREAM APPEND
- `2026-08-16` 🔒 THE PLACEMENT COUNTER IS COMPILER-CARRIED; THE LANE CONSTRAINT IS HALF-CARRIED AND HALF-GUARDED,…
- `2026-08-16` 🛬 AND IT IS BUILT: THE ORDER BIRTH RIDES THE ORDER LANE, BEHIND A FLAG
- `2026-08-16` ⏱️ #167 ACCEPTANCE TIMEOUT IS CODE-COMPLETE ON THE BRANCH (PHASES 0–3 + the mob conditions): #167 "No…
- `2026-08-15` 🛠️ #582 ACTORS HALF IN FLIGHT (branch `582-actor-answers-dsl`, draft PR #583)
- `2026-08-15` ✅ PM DECISION-GRAMMAR PROPOSAL APPROVED
- `2026-08-15` 🧾 THE DECISION REGISTER RENDERS AS WRITTEN AGAIN, AND §13b IS AN ERROR
- `2026-08-15` ⚖️ THE TEAM MERGES ITS OWN WORK; NO PR WAITS ON FOUNDER REVIEW
- `2026-08-15` 🔶 RSO-1 IS CODE-COMPLETE ON THE BRANCH (PHASES 1–4): #180 "Opening hours are stored, displayed, and…
- `2026-08-15` ⚖️ MERGE POSTURE RULED: AUTO-MERGE-ON-GREEN IS THE DEFAULT; `HOLD: human` FOR THE NAMED CLASS
- `2026-08-15` 🧱 THE DECISION REGISTER'S TABLES ARE NOW GATED, AND THE GATE FOUND SEVEN BROKEN ROWS ON ARRIVAL
- `2026-08-15` ✅ RSO-1 IS NOW DISPATCHABLE: ITS THREE BLOCKING SUB-QUESTIONS ARE ANSWERED — AND THREE OF THE…
- `2026-08-15` 🛑 RSO-1 CANNOT BE BUILT AS RECORDED: A BOOLEAN "IS IT OPEN?" WOULD TAKE LIVE RESTAURANTS OFFLINE
- `2026-08-15` 🛑 RSO-2 CANNOT CLOSE OVERSELL, AND IT WAS ABOUT TO BE BUILT AS IF IT COULD
- `2026-08-15` 🧹 the autonomous-run brief no longer tells the run that `specs/**` is untouchable
- `2026-08-15` 🧠 THE ARCHITECT IS SPLIT INTO THREE NAMED DOCTRINE LENSES: `young`, `vernon`, `evans`
- `2026-08-15` ⚖️ OPENING HOURS AND STOCK ARE CHECKED SERVER-SIDE ON PLACE ORDER; A BIG CATALOG SNAPSHOTS EVERY 100…
- `2026-08-15` 📝 `specs/services.yaml`'s "V0: one deployable" line now names its destination
- `2026-08-15` ⚖️ AMENDED: ADR-20260815-030206 was WRONG on its central claim and now carries a…
- `2026-08-15` ⚖️ A PROCESS MANAGER IS A WRITE-SIDE COMPONENT AND NEVER READS THE READ SIDE
- `2026-08-15` 🧭 A PM `read:` STEP NOW DECLARES ITS SOURCE: THE DISTINCTION THE MECHANICAL DERIVATION WAS BLOCKED ON
- `2026-08-14` 🗄️ EVERY TABLE HAS A DECLARED HOME: STO-2 CLOSED, PLACEMENT IS NOW A VALIDATOR REQUIREMENT
- `2026-08-14` 💳 COLLECTION ORDERS WILL CAPTURE AT READY, NOT AT PICKUP
- `2026-08-14` 🎯 THE ACCEPTANCE KEYSTONE NOW PROMISES MORE: FULL ENFORCEMENT + FULL SPLIT ARE IN SCOPE
- `2026-08-14` 🔒 L5 acceptance-walk executor handed back on two real problems; the architect assessed, re-sequenced…
- `2026-08-13` 💳 CAPTURE ON DELIVERED IS IMPLEMENTED: the recorded posture (ADR-20260808-195315 §1.2/§1.3) and the…
- `2026-08-14` ✅ FOUNDER DELEGATED A DECISION BATCH TO THE TEAM
- `2026-08-14` 💳 the #544 five-lens review's carry-forwards (recorded)
- `2026-08-13` 🗄️ THE DATABASE PLACEMENT DECLARATION SITE EXISTS
- `2026-08-13` ⏱️ THE WEEKLY CAP IS NOT A STOP SIGN; billing continues
- `2026-08-13` 🔁 "I'M REPEATING MYSELF": RECORDED INTENT MUST EXECUTE ITSELF, AND THE UBER EATS ONBOARDING WEDGE
- `2026-08-13` 🎯 THE ACCEPTANCE CRITERION EXISTS: SIX CLAUSES WALKED ON THE LOCAL STACK, WITH THE FRONT DOOR…
- `2026-08-13` 🚪 ONE JOURNAL, ONE DOOR IS NOW LEVEL 4 ON ALL THREE SURFACES
- `2026-08-13` 🔐 A TOKEN MUST NOW PROVE THE PRODUCT, NOT ONLY THE PROVIDER
- `2026-08-13` 🔐 IDENTITY: SUPABASE AUTH IS RETAINED FOR V0, AND THE WINDOW TO OWN IDENTITY CLOSES AT THE FIRST…
- `2026-08-12` 📌 THE FOLLOW-UP REGISTER: nine findings from tonight's mob reads are now ISSUES, not paragraphs
- `2026-08-12` 🧭 THE FOUNDER ANSWER SHEET: THE FLIP IS TAKEN, THE REGISTRY IS DESTROYED, AND NOTHING IS PAID FOR…
- `2026-08-12` 🧾 THE FOUNDER IS THE FOUNDER, AND EVERY FOUNDER MESSAGE GOES TO THE WHOLE TEAM
- `2026-08-12` 🔒 EACH ADAPTER OWNS ITS OWN, COMPLETELY ISOLATED DATABASE — decided, then CORRECTED the same day
- `2026-08-12` 🗂️ THE APP INDEX IS GENERATED, AND IT SAYS THE SPLIT IS NOT CLEAN
- `2026-08-11` ✂️ THE API TIER IS THE WIDEST APP IN THE TOPOLOGY, AND `server` IS ONE EDGE AWAY FROM EIGHT PODS
- `2026-08-11` ✅ BND-1 IS CLOSED: THE BOUNDARY SET IS FIVE, AND THE REGISTER'S LONGEST-STANDING ROW IS ANSWERED
- `2026-08-12` 🧱 A READ TARGET IS DECLARED, NEVER INFERRED: the `reads:` ownership wall is a gate
- `2026-08-12` ✅ THE JOURNAL CONCERN IS CLOSED: `inbound_messages` is the only journal (#242 Runtime D,…
- `2026-08-11` ⏱️ THE ETA IS THE PRODUCT, AND NOTHING COMPUTES IT; PLUS: ONE EVENT LOG
- `2026-08-11` 📦 REPOSITORY CRATES: TWO OPEN ROWS CLOSE, AND THE COUPLING NOBODY HAD NAMED
- `2026-08-11` 🧮 THE WARNING BASELINE IS A GATE, NOT A NUMBER IN A DOC
- `2026-08-11` 🗄️ THE STORAGE SPLIT IS COSTED, AND IT FOUND TWO DEFECTS THAT ARE NOT ABOUT THE SPLIT
- `2026-08-11` 🗂️ THE 57-APP LIST, AND THE PER-APP KNOWLEDGE THAT LIVES IN RUST
- `2026-08-11` ⚖️ THE ERASURE-FREE ZONE, CORRECTLY FRAMED: THE STREAMS WERE ALREADY PERSONAL DATA, AND TWO FORWARD…
- `2026-08-11` 🧪 THE CUTOVER WAS REHEARSED, LOCALLY AND END TO END; THE MONOLITH NOW HAS A MANIFEST
- `2026-08-10` 🔓 THE `specs/**` FREEZE IS LIFTED: THE DSL IS THE TEAM'S WORK
- `2026-08-11` 🧾 PER-BIN SCOPE ISOLATION: THE MANIFESTS NOW SAY WHAT THE BUILD ENFORCES
- `2026-08-11` 🧭 BEHAVIOUR TRACKING IS ISOLATED END TO END, AND A FAULTED WORKER PRE-DIAGNOSES ITSELF — BUT "SAY IT…
- `2026-08-11` 🛑 A REJECTED FOLD NOW HALTS ITS GROUP — AND THE FLIP CANNOT LAND ALONE, BECAUSE A HALTED PROJECTOR…
- `2026-08-11` ✅ THREE MORE DECISIONS SETTLED; ONE IS WITH LEGAL
- `2026-08-11` ✅ THE REVERSAL IS CONFIRMED, AND THE SPEC GETS STRONGLY TYPED
- `2026-08-11` ♻️ A BUSINESS METRIC IS A PROJECTION, NOT A COUNTER — THE TEAM CHANGED ITS OWN RECOMMENDATION, AND…
- `2026-08-11` 🔍 BEHAVIOUR EVENT TRACKING GETS A DECLARATION SITE — AND THE ARTICLE 9 EXPOSURE IS ALREADY IN THE…
- `2026-08-11` 📏 BUSINESS METRICS BECOME A DECLARED, GATED OBLIGATION — AND 26 OF THE 29 WE ALREADY DECLARE EMIT…
- `2026-08-10` ✅ THE LOCAL TEST GATE IS HONEST: `make test-crates` RUNS FROM THE STOP HOOK, AND A MISSING DATABASE…
- `2026-08-11` ✅ #469: THE OPEN PATH READS CREDENTIALS, AND `current` IS TENANT-SCOPED BY HOST
- `2026-08-10` 🚧 #451 PHASE 2 LANDED (code): THE CART IS PRICED LIVE ON READ — BUT THE CUSTOMER STILL CANNOT SEE IT
- `2026-08-10` 🚧 #451 PHASE 1 LANDED: THE AMBER SPEC SLICE OF THE CART-PRICING KEYSTONE
- `2026-08-10` ✅ STRIPE PUBLISHABLE KEY BAKED: the #440 env-var-only follow-up is closed
- `2026-08-10` ✅ CART-PRICING KEYSTONE APPROVED (Option B / LIVE); BUILD STARTING
- `2026-08-10` 🚧 `orders_placed_total{status="PLACED"}` EMIT WIRED ON THE PM-MAILBOX PATH — ARMS WITH THE…
- `2026-08-10` ✅ SECRET-GATE EXTRACTED TO ITS OWN LEAN CRATE: the deploy-path cold-compile tail risk is gone
- `2026-08-10` ✅ PRE-DEPLOY SECRET-PRESENCE GATE: a declared secret missing/mis-named in the deploy target now…
- `2026-08-10` ✅ THE STRIPE PUBLISHABLE KEY REACHES /checkout AND THE PAYMENT ELEMENT CAN MOUNT

### Archive

| Week | Dates | Entries | File |
|---|---|---:|---|
| 2026-W34 | 2026-08-17 → 2026-08-19 | 24 | [`journal-2026-W34.md`](status/journal-2026-W34.md) |
| 2026-W33 | 2026-08-10 → 2026-08-16 | 77 | [`journal-2026-W33.md`](status/journal-2026-W33.md) |
| 2026-W32 | 2026-08-03 → 2026-08-09 | 35 | [`journal-2026-W32.md`](status/journal-2026-W32.md) |
| 2026-W31 | 2026-07-27 → 2026-08-02 | 27 | [`journal-2026-W31.md`](status/journal-2026-W31.md) |
| 2026-W30 | 2026-07-20 → 2026-07-26 | 60 | [`journal-2026-W30.md`](status/journal-2026-W30.md) |

**223 entries** across five week files (624,786 B, measured at the commit that lands this).
This page is what a session reads at boot; the archive is fetched on purpose.
