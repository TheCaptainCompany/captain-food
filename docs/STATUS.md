# 🚦 Captain.Food — Development & Deployment Status

> Hand-maintained snapshot (NOT generated, outside `specs/` so it never affects the DSL).

## 🌐 Deployment

| Piece | Status | Notes |
|---|---|---|
| Render web service (Docker, Frankfurt) | ⏸️ SUSPENDED | Billing-suspended since ~2026-08-04 (`suspenders: ["billing"]`); `captain-food.onrender.com` returns 404. **This is a decided state, not an open incident** — see "Production is DELIBERATELY SUSPENDED" below (ADR-20260817-105844). Blueprint IaC (`render.yaml`) still describes what was live before suspension |
| Supabase Postgres (Frankfurt, eu-central-1) | ⏸️ idle | No live traffic while the Render app is suspended; the team develops/walks against a **local** Postgres stack instead (ADR-20260813-004634) |
| Hosting target — OVH Managed Kubernetes + in-cluster CloudNativePG, GitOps-reconciled | 📋 decided, not built | [ADR-20260807-002705](adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md): MKS (Paris), CNPG ≥3 nodes + WAL archiving + restore drills, manifests GENERATED from specs, GitOps-only ops. Realization backlog tracked under [#271](https://github.com/TheCaptainCompany/captain-food/issues/271); the cluster does not exist yet — production cutover is a separate decision to re-take, not a task in flight |
| CI workflow `ci` (build+test+validate+drift; ex `codegen-consistency`) | ✅ | Gates deploys (`autoDeployTrigger: checksPass`); `changes` also runs the decision-lookup stub suite (#679) |
| CI `Claude Code Review` | ✅ | Fires on `opened`/`ready_for_review`/`reopened` — **one pass per presentation, never per push** (ADR-20260826-084500). Re-request = draft → ready |
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
| **OTP send guards — country allowlist + per-number caps + global daily ceiling** | ✅ | **#516**, [ADR-20260813-021500](adr/ADR-20260813-021500-the-allowlist-is-the-economic-control-and-only-a-global-ceiling-bounds-the-bill.md). The OTP request endpoint is anonymous BY DESIGN and every send spends money on our **own OVH account** — it previously had **no limit of any kind**. Now: a fail-closed **exact-membership** allowlist (default `+33,+32,+41,+44,+49,+34,+39` — the served-country decision made executable; bare `+1` was dropped 2026-08-13, #535, because a calling code is not a destination: `+1` reaches every NANP territory, premium Caribbean ranges included, all billed like a Boston number), 3/hour + 5/day per **canonical** number with a 30s→2min→10min cooldown, and a **global daily ceiling with a no-deploy kill switch** (`UPDATE sms_send_quota SET sent_count=999999 WHERE quota_key='global:day'`) — the only guard that bounds the total bill, since an attacker rotates numbers. The counter is **shared** (`sms_send_quota`; one atomic `INSERT…ON CONFLICT…WHERE`, because there is no per-phone actor lane and that statement is the only serialisation). **The wall is `/auth/sms-hook`**, where the euro is actually spent; the identity ACL only *sheds*. Compiler-enforced: `OvhSmsClient::send` takes an unforgeable `AuthorizedSmsRecipient` **by value**, so one claim buys exactly one send (a `&`-borrow would let a loop spend the whole budget on a single claim). Refusals are four **typed** states a client can tell apart, rendered from the server's own `errors.yaml` `messages.{en,fr}` — the single source; there is deliberately no second client-side copy of that string, and the client render path itself is #518/#521. Liveness is an **observable** gauge (`otp_send_guard_enforcing`) re-asserted on every export cycle and re-declared where enforcement is decided, not stamped once at boot. **The 200/day ceiling is derivable, not a guess**: OVH SMS France is €0.06 HT/SMS ([PROP-20260724-233605](proposals/PROP-20260724-233605-ovh-sms-hook.md), founder-approved 2026-07-24, screenshot-confirmed) → €12/day worst case France-rated; still unknown are OVH's per-destination multipliers and which pack was purchased. The account is a **prepaid pack**, so the real failure mode is a drained pack = a founder-gated phone-login outage, not an invoice (#535 corrections in [ADR-20260813-021500](adr/ADR-20260813-021500-the-allowlist-is-the-economic-control-and-only-a-global-ceiling-bounds-the-bill.md)). **The observed-but-not-served telemetry gap is closed (#696)**: `otp_send_refused_total` now carries a closed, hand-declared `region` attribute (`north_america`/`non_eu_europe`/`rest_of_world`), so a refused `+1` is `north_america` instead of collapsing into the same bucket as every other unserved code, with no widening of the attacker-mintable label set. **Still owed: the credit-balance gauge** — `ovh_sms_credit_balance` is DECLARED in `specs/observability.yaml` but not yet emitted, tracked by [#699](https://github.com/TheCaptainCompany/captain-food/issues/699). |
| Authentication / identity (Supabase JWT) | ✅ | **First cut shipped (ADR-0047)**: verify Supabase JWT via JWKS at `/{role}/graphql` (public keys, no shared secret; ~1h cache, serve-stale-on-refresh-failure — no per-request Supabase call); `app_metadata.captain_role` gates the path (`/public` open, else 401/403), fail-closed on cold cache, asymmetric-only. Verified role + `Principal` injected. **EXTERNAL service tokens** via `X-External-Api-Key` (constant-time, `EXTERNAL_API_TOKENS`) shipped. Per-field `@auth` on FK-nav edges = DSL/plan-mode follow-up |
| **IDENT-1 Phase A — CUSTOMER identity resolved from Postgres instead of the JWT claim** | 🅑 gated OFF | [#641](https://github.com/TheCaptainCompany/captain-food/issues/641), [ADR-20260818-004646](adr/ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md). `resolve_read_scope`'s CUSTOMER arm can now resolve the caller's domain id from Postgres via the existing `CustomerReadRepository::by_auth_ref` bridge instead of trusting `captain_food.customer_id` — behind `RESOLVE_CUSTOMER_IDENTITY_FROM_POSTGRES` (`specs/customer/configuration.yaml`, **default `false`**), selected ONCE at startup, never a per-request fallback. A `NoMapping` row and a `LookupFailed` lookup both fail closed to `Public` identically, distinguishably in telemetry (`customer-identity` observability contract: span `customer.identity.resolve`, histogram `customer_identity_resolve_ms`, counters `customer_identity_not_found_total` / `customer_identity_lookup_failed_total{reason}` / `customer_identity_lookup_source_total{source}`). Scope: **CUSTOMER only** — RESTAURANT/RESTAURANT_ACCOUNT/RIDER have no sign-in operation in the DSL at all (STAFF-AUTH, DECISIONS §46). **No GDPR erasure flow exists for Customer at all** (no `deletion:` block on the Customer actor anywhere in `specs/customer/actors.yaml`) — flagged, not fixed here, out of this PR's scope. Flipping the default is a separate recorded decision after the gated form is smoked. |

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

## ✅ `claude-review` is no longer a required check — REV-1 executed (2026-08-26)

> **If you are here because a PR is stuck on `claude-review`: it is no longer required, so that is
> not what is blocking you.** [REV-1](decisions/REV-1.yaml) decided on 2026-08-17 to remove it and
> was finally **executed by the founder on 2026-08-26**, nine days later, in the GitHub UI. Ruleset
> `19179892` now requires `codegen`, `build-test` and `db-test` only.
>
> **The exposure this section used to carry is CLOSED**: a 429, a model outage, a permission denial
> or the action's self-skip on a PR editing its own workflow is no longer a repo-wide merge stop.
>
> `claude-review` still RUNS on every PR and still posts findings — it just does not gate merges.
> The compensating control is unchanged and is a process obligation, not a mechanism: the
> independent reviewer pass stays MANDATORY before ready-for-review (founder directive 2026-08-01),
> now at one pass per PRESENTATION rather than per push
> (`ADR-20260826-084500`, [`review-triage`](../.claude/skills/review-triage/SKILL.md)).
>
> **A red on `claude-review` still means NO VERDICT WAS PRODUCED**, not that the reviewer found a
> problem — #680 hardened it to fail rather than self-clear, which is why that distinction matters.

## 🗂️ Decision register & ask gate (2026-08-21)

| Piece | Status | Notes |
|---|---|---|
| Machine-readable decision rows — **`docs/decisions/<KEY>.yaml` is the authority** | ✅ | One file per globally unique key, closed status vocabulary (`open\|decided\|deferred\|superseded\|withdrawn`), resolvable `decided_by`/`superseded_by`, `reconsiders` challenge chains ([ADR-20260821-095957](adr/ADR-20260821-095957-decision-register-rows-are-machine-readable-files.md)). Generated index injected into `DECISIONS.md` (§22b keeps it in sync); `_legacy.yaml` = 102 prose-only keys, a **migration boundary, never authority**; `_exempt.yaml` = self-pruning held-record citation exemptions |
| Ask gate — founder decision questions carry `Decision row: <KEY>` on an OPEN row | ✅ | Fail-closed PreToolUse hook on `AskUserQuestion` (`.claude/hooks/register-check.sh`; envelope/trail/passive lanes, exit 0 allow / 2 block only) + selftest run by the stop-gate every turn **and by CI's always-run `gate-scripts` job on every push, docs-only included** ([ADR-20260821-010543](adr/ADR-20260821-010543-agents-check-the-register-before-asking.md), [ADR-20260821-103403](adr/ADR-20260821-103403-decision-ask-unregistered-and-the-citation-ratchet.md)). Boundary stated honestly: only the structured envelope is mechanically gated; free text is not. **A SECOND always-run gate step joined 2026-08-26** — the decision-lookup hermetic stub suite (row `RETRIEVAL-QMD-CI`, [ADR-20260824-205911](adr/ADR-20260824-205911-the-decision-lookup-stub-suite-runs-in-ci.md)) — and both steps compare all four gate scripts against their committed blobs before reporting. That comparison is **mostly pre-merge**: `make hooks-test`/`make stub-tests` opt out unconditionally, and the stop-gate opts out only when a gate script is dirty. So an ordinary overwrite is still caught at push (it makes the tree dirty, which is what opts the turn out); what the in-session armed path catches is the tamper that HIDES from `git status` (`--assume-unchanged`, `--skip-worktree`), i.e. the stealthier class. CI is the only caller that cannot be talked out of it. Every job the `codegen` aggregator consumes now carries a `timeout-minutes`, because `always()` still waits. **The locus is DECIDED** — `GATE-STEP-LOCUS` option (a), founder 2026-08-27 ([ADR-20260827-081500](adr/ADR-20260827-081500-the-call-sheet-answers-gate-steps-move-and-the-citation-rule-hardens.md)): both steps live in the sibling always-run `gate-scripts` job, so a gate red still reds the required check but no longer skips every other job |
| Citation ratchet + docs-only CI enforcement | ✅ | Validator §22/§23 on `make validate` (every full-form ADR/PROP citation across `docs/**` + `CLAUDE.md` resolves); the docs-only CI path runs the canonical validator (`docs-validate` job + by-name `codegen` aggregator assertion — the pre-2026-08-21 bypass is closed and pinned by shape tests) |

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
See [`docs/adr/`](adr/) for the full chronological log (247 records as of 2026-08-28) — **latest:
ADR-20260828-120500** (an answered question is never asked again — the register-check rule closing
the ask-gate). The picks below are the **load-bearing** ones a new session needs first, not the
newest filenames:

- **[ADR-20260807-002705](adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md)** — hosting
  target is OVH Managed Kubernetes + in-cluster CloudNativePG, GitOps-only ops. Decided, **not yet
  built** (see the Deployment table above).
- **[ADR-20260817-105844](adr/ADR-20260817-105844-the-walk-goes-first-on-one-database-and-production-stays-suspended.md)**
  — production stays deliberately suspended; the team develops against a local walk, not a live
  cluster.
- **[ADR-20260802-224532](adr/20260802-224532-push-driven-mailbox-approved.md)** — the CQRS/actor
  posture: the mailbox door notifies via Postgres `pg_notify`, workers wake cross-process instead of
  polling.
- **[ADR-20260719-031136](adr/20260719-031136-write-side-repository-event-sourced-actor.md)** —
  write-side `Repository` / event-sourced actors: handlers and the saga runner route through it,
  never the raw `EventStore`.
- **[ADR-20260821-095957](adr/ADR-20260821-095957-the-register-row-gets-machine-identity-reg2-reg4-and-the-ask-gate-reads-it.md)**
  — decision register rows are machine-readable files; this is the governance spine the ask-gate
  (register-check) reads before any founder question.

**ADR ids are date-time** to avoid concurrent-session collisions (ADR-20260718-135417).

> Convention: keep this file current with every substantive change, and record cross-cutting decisions as an ADR in the same change.

## Journal

**This page is durable state and a journal index. Dated status-journal entries do not go here.**

Write a new dated entry at the **TOP** of the applicable weekly file under `docs/status/` — the
journal is newest-first, so never append at the end of a week file. If the entry falls in an ISO
week that has no file yet, create `docs/status/journal-YYYY-Www.md` with the established header:

    # Status journal — YYYY-Www

    Journal entries for ISO week YYYY-Www, newest first, in the order they were written.
    Current state: [`../STATUS.md`](../STATUS.md).

then add it to the list below and place the entry at the top of the new file.

- [`journal-2026-W30.md`](status/journal-2026-W30.md)
- [`journal-2026-W31.md`](status/journal-2026-W31.md)
- [`journal-2026-W32.md`](status/journal-2026-W32.md)
- [`journal-2026-W33.md`](status/journal-2026-W33.md)
- [`journal-2026-W34.md`](status/journal-2026-W34.md)
- [`journal-2026-W35.md`](status/journal-2026-W35.md)
