# 🔌 HubRise — catalog & inventory process (end-to-end, by architecture layer)

> Hand-maintained process view. Data **mapping** (fields, refinements) is the source-of-truth spec in
> [specs/integrations/hubrise.md](../../specs/integrations/hubrise.md); this doc is the **runtime process**
> (who calls whom, across which crate). Behaviour source = `crates/adapters/hubrise/*`
> (`http.rs`, `acl.rs`, `enrich.rs`, `api.rs`) + the `import_catalog` / `update_offer_stock` handlers in
> `crates/application`. Complements ADR-20260718-145856 (inbound webhook ACL), ADR-20260718-213352
> (partner-adapter crates + OAuth2 pull), ADR-0041 (event envelope).

HubRise is a **stateless-callback** integration: a webhook only says *"catalog/inventory X changed"* — it
carries no content. So the flow is always **callback → verify → PULL the resource → map (ACL) → domain
write**. Two directions, per CLAUDE.md's request/report split:

- **Catalog** is an **orchestrated import we can reject** (ACL validation, `CatalogNotFound`, `MissingRef`)
  → it goes through the **`ImportCatalog` command** → `CatalogImported`.
- **Inventory** is a **reported fact** (stock already changed on the POS) → routed through the
  `update_offer_stock` handler **only** to reuse its `Catalog-<id>` stream/version + `StockStatus`
  derivation; its lone `OfferNotFound` rejection is the "SKU not imported yet" case, which we **skip** — so
  **no inbound fact is ever rejected**.

## The architecture layers (legend for every diagram below)

| Box | Crate | Role in the HubRise flow |
|---|---|---|
| **HubRise** | — | the external POS/aggregator (pushes callbacks, serves the pull API) |
| **server** | `crates/server` | Axum BFF: composition root (`lib.rs`), mounts `POST /adapters/hubrise/webhooks`, builds the `Enricher` |
| **adapter** | `crates/adapters/hubrise` | the HubRise vertical slice: `http.rs` (endpoint) · `acl.rs` (verify + callback shape) · `api.rs` (OAuth2 outbound pull) · `enrich.rs` (map + drive commands) |
| **application** | `crates/application` | `commands::{import_catalog, update_offer_stock}` handlers |
| **domain** | `crates/domain` | Catalog aggregate: validates refs, folds, emits `CatalogImported` / `OfferStockUpdated` |
| **infrastructure** | `crates/infrastructure` | `PgEventStore` |
| **domain_events** | Postgres | append-only log (`Catalog-<id>` streams) |

Dependency rule (ADR-0035): the adapter depends inward on `application`/`domain` (+ `infrastructure` only
in its standalone `main.rs`); it is **not** part of `infrastructure`. The ACL is framework-free and
unit-tested without axum.

---

## 0. Precondition — the Connect flow (✅ built, issue #20 / ADR-20260721-100601)

### Entity alignment — HubRise ↔ Captain.Food (1:1 by design)

Captain.Food's aggregates were deliberately shaped to HubRise's hierarchy, so the mapping is direct
(`specs/entities.yaml`: *"a `RestaurantAccount` groups one or more `Restaurant` locations … which links
to its account via `accountId`"*):

| HubRise | Captain.Food | Notes |
|---|---|---|
| **Account** (carries `currency`) | **`RestaurantAccount`** | the holding / brand — groups the locations |
| **Location** (a point of sale) | **`Restaurant`** | one physical location; `Restaurant.accountId → RestaurantAccount`, and `Restaurant.ref` = the HubRise location id |
| **Catalog** | **`Catalog`** | the menu, imported per catalog |

So a HubRise **Account with N Locations** is a Captain **`RestaurantAccount` with N `Restaurant`s** — and
that is exactly why the access token is **not** one global secret.

### The token is scoped to a HubRise **Account** → it lives on the `RestaurantAccount`

A HubRise OAuth **connection** is authorized against an **Account** and returns a **non-expiring** access
token covering that account (its locations + catalogs). So **one token per `RestaurantAccount`** (= per
HubRise Account) serves **all** of its `Restaurant` locations. The token response itself names the
connection scope (`account_id`, and `location_id`/`catalog_id` when location-scoped) — confirmed against
the HubRise auth docs. Tokens live in the adapter-owned **`hubrise_connections`** table keyed by
`RestaurantAccount`, with a **`hubrise_connection_locations`** snapshot so the enricher resolves a
callback's `location_id` → the owning account's token before pulling
(`specs/database/tables/integration_connections.yaml`). The former single global `HUBRISE_ACCESS_TOKEN`
is **retired**.

The flow is `crates/adapters/hubrise/src/connect.rs` (+ the two routes in `http.rs`), driven by the
operator/restaurant hitting `GET /adapters/hubrise/connect`:

```mermaid
sequenceDiagram
    actor Op as Operator / restaurant
    participant HR as HubRise (manager.hubrise.com)
    box adapter (crates/adapters/hubrise)
        participant EP as http.rs · GET /connect → /oauth/callback (signed state)
        participant CF as connect.rs · HubRiseConnectFlow
        participant CT as hubrise_connections (+ _locations)
    end
    box application / domain
        participant J as command_journal (WORKER channel)
        participant REG as RegisterRestaurantAccount / RegisterRestaurant / CreateCatalog / ImportCatalog
        participant ES as Repository (actor journal)
    end
    box infrastructure
        participant DB as PgEventStore adapter → domain_events
    end

    Op->>EP: GET /adapters/hubrise/connect
    EP-->>Op: 302 authorize (client_id, redirect, scope, HMAC state)
    Op->>HR: authorize the Captain HubRise app for an Account
    HR-->>EP: GET /oauth/callback?code&state (state verified, 15 min)
    EP->>CF: connect(code)
    CF->>HR: POST /oauth2/v1/token (Basic client_id:client_secret + code)
    HR-->>CF: access_token + account_id (account-scoped, NON-expiring)
    CF->>HR: GET /account · GET /locations · GET /catalogs (X-Access-Token)
    CF->>J: journaled sends (message_id = UUIDv5(attempt, command, entity))
    J->>REG: RegisterRestaurantAccount + RegisterRestaurant per location (PASSIVE_PARTNER)
    CF->>CT: UPSERT connection { restaurantAccountId, hubriseAccountId, token } + location snapshot
    J->>REG: CreateCatalog + initial ImportCatalog per catalog (same ACL mapping as §1)
    REG->>ES: save the aggregates' decided facts
    ES->>DB: append (behind the port)
    Note over CF,DB: every id is the ENRICHER'S derived UUIDv5 (account / location / catalog),<br/>so §1/§2 callbacks land on exactly these aggregates.
    Note over CT: re-connect = token refresh + location catch-up + catalog re-import<br/>(creations are idempotent on the derived ids; rejections are warnings, never retried).
```

**Why the derived ids matter:** every domain id is a **UUIDv5 of the HubRise identifier** under a fixed
namespace (`enrich.rs::derive`). The connect flow must create the `RestaurantAccount`/`Restaurant`/`Catalog`
with *those same* ids, so a later inventory update targets the exact `OfferId` the import assigned. The
reconciliation table:

| Domain id | Seed (`kind:value`) | Why that seed |
|---|---|---|
| `RestaurantAccountId` | `account:<hubrise account id>` | the connect flow creates the account that holds the token (`derive_restaurant_account_id`) |
| `CatalogId` | `catalog:<hubrise catalog id>` | the connect flow must `CreateCatalog` with this id |
| `RestaurantId` | `location:<hubrise location id>` | the location **is** the restaurant |
| category / product / option-list / option | `<kind>:<hubrise id>` | the tree re-joins by `ref` after translation |
| **`OfferId`** | `sku:<SKU ref, else SKU id>` | **inventory joins by `sku_ref`** — the SKU's *ref*, not its id |

---

## 1. Catalog callback — import (orchestrated, rejectable)

A verified `catalog` callback triggers an **outbound pull** of the full catalog, which the ACL maps to an
`ImportCatalog` command. The Catalog aggregate validates and emits `CatalogImported`.

```mermaid
sequenceDiagram
    participant HR as HubRise
    box server (crates/server)
        participant MNT as router (lib.rs mounts hubrise_adapter::routes)
    end
    box adapter (crates/adapters/hubrise)
        participant EP as http.rs · POST /adapters/hubrise/webhooks
        participant V as acl::verify_hubrise_signature
        participant API as api::get_catalog (X-Access-Token)
        participant MAP as enrich::map_catalog (ACL, pure)
    end
    box application / domain
        participant IMP as commands::import_catalog → Catalog aggregate (decides)
        participant ES as Repository (actor journal)
    end
    box infrastructure
        participant PG as PgEventStore (→ domain_events)
    end

    HR->>MNT: POST /adapters/hubrise/webhooks (raw body, X-HubRise-Hmac-SHA256)
    MNT->>EP: routed
    EP->>EP: HUBRISE_WEBHOOK_SECRET set? no → 503 (fail closed)
    EP->>V: verify(secret, header, RAW body)
    Note over V: hex HMAC-SHA256(client_secret, rawBody), constant-time,<br/>NO timestamp. Bad → 400, HubRise retries.
    V-->>EP: ok → parse HubRiseCallback { resource_type:"catalog", location_id, catalog_id }
    EP->>EP: needs_pull()? & Enricher wired? no → 202 verified_pending_enrichment
    EP->>API: pull_catalog(hubrise_catalog_id)
    API->>HR: GET /catalog/{id}
    HR-->>API: catalog JSON (categories/products/skus/option_lists)
    API-->>MAP: JSON
    MAP->>MAP: derive UUIDv5 ids, "9.80 EUR" → Money, tax strings → TaxRate,<br/>every entity gets a ref (so domain MissingRef never fires)
    MAP-->>IMP: ImportCatalog command
    IMP->>ES: save CatalogImported (decided by the Catalog aggregate) on Catalog-{id}
    ES->>PG: append (behind the port)
    alt catalog not created yet
        IMP-->>EP: Err(CatalogNotFound) → Skipped (definitive, logged) → 200
    else mapping failed
        MAP-->>EP: MapFailed → 200 (retry won't help)
    else pull failed
        API-->>EP: PullFailed → 502 (HubRise redelivers)
    else ok
        IMP-->>EP: CatalogImported → 200
    end
    EP-->>HR: 200 (ACK or 5xx only on event-store failure)
```

**Layer notes**
- **Two auth schemes, opposite directions:** inbound uses HubRise's **HMAC** (`x-hubrise-hmac-sha256`, hex,
  raw body, **no** timestamp — contrast Stripe's `t=`+replay window); outbound uses **our** token
  (`X-Access-Token`) on the pull. Both fail closed when their secret/token is unset.
- **ACL discipline:** `enrich::map_catalog` is the **only** place HubRise's `"9.80 EUR"` string, decimal
  tax-rate strings, and `data` envelope exist. Ids never leak — they become UUIDv5 (`§0`).
- **Rejectable by design:** `import_catalog` is a real command; `CatalogNotFound` (connect flow hasn't run)
  and `MissingRef` are **definitive skips** (logged, ACKed 200 — retrying the same payload won't help).

---

## 2. Inventory callback — stock update (reported fact, never rejected)

Same verify + pull shape, but the pulled inventory maps to **one `UpdateOfferStock` per SKU line**, routed
through the handler purely to reuse the `Catalog-<id>` stream + `StockStatus` derivation.

```mermaid
sequenceDiagram
    participant HR as HubRise
    box adapter (crates/adapters/hubrise)
        participant EP as http.rs · POST /adapters/hubrise/webhooks
        participant API as api::get_inventory
        participant MAP as enrich::map_inventory (ACL, pure)
    end
    box application / domain
        participant UPD as commands::update_offer_stock → Catalog aggregate (decides)
        participant ES as Repository (actor journal)
    end
    box infrastructure
        participant PG as PgEventStore adapter → domain_events
    end

    HR->>EP: POST /adapters/hubrise/webhooks (inventory callback, verified as §1)
    EP->>API: pull_inventory(hubrise_location_id)
    API->>HR: GET /location/{id}/inventory
    HR-->>MAP: inventory JSON (lines: sku_ref, stock, expires_at)
    MAP->>MAP: per SKU line → UpdateOfferStock{ offer_id = derive_offer_id(sku_ref), quantity, expires_at }<br/>option_ref lines skipped, unparseable quantity dropped (a fact is never rejected)
    loop each SKU update
        MAP-->>UPD: UpdateOfferStock
        alt offer known (join by sku_ref succeeds)
            UPD->>ES: save OfferStockUpdated (Catalog aggregate, StockStatus derived) on Catalog-{id} → applied++
            ES->>PG: append (behind the port)
        else OfferNotFound (SKU not imported yet)
            UPD-->>MAP: skip (not an error) → skipped++
        end
    end
    MAP-->>EP: InventoryApplied { applied, skipped }
    EP-->>HR: 200
```

**Layer notes**
- **The join is the whole point:** `derive_offer_id(sku_ref)` in §2 equals the `OfferId` §1 assigned
  (both seed on the SKU **ref**), so an inventory update lands on the exact imported offer — idempotent
  across re-syncs.
- **No fact is rejected:** `OfferNotFound` = "we don't know this SKU yet" ⇒ counted as `skipped`, never a
  domain error. Only an unreachable event store surfaces as `Err` → **5xx** (HubRise redelivers).
- `low_stock_threshold` is `None` from HubRise (it carries no threshold), so the handler derives
  `IN_STOCK`/`OUT_OF_STOCK` from quantity alone (`LOW_STOCK` needs the manual threshold — see
  [specs/integrations/hubrise.md](../../specs/integrations/hubrise.md#3-refinements-vs-hubrise)).

---

## 3. Envelope, idempotency & ingress-only fallback

- **Envelope (ADR-0041):** facts are stamped with `user_id = hubrise_system_user_id()` (UUIDv5),
  `user_type = EXTERNAL`, `correlation_id = UUIDv5("callback:<id>")` — same discipline as the Stripe ACL,
  so a catalog/inventory change is traceable end-to-end.
- **Idempotency:** the deterministic UUIDv5 ids make a re-sync map to the *same* command shape; combined
  with the Catalog aggregate's fold + `UNIQUE(stream, version)`, a redelivered callback converges (a
  re-import replaces catalog content; a repeated stock line re-derives the same `StockStatus`).
- **Ingress-only fallback:** when no `Enricher` is wired (no `DATABASE_URL`) or the callback needs no
  pull, a *verified* callback is ACKed **202 `verified_pending_enrichment`** — the signature is proven
  but nothing is written. With an enricher wired, a callback for a **location no connection covers**
  (the connect flow `§0` hasn't run, or the location joined HubRise after the last connect) is a
  definitive **skip** — re-connecting refreshes the location snapshot.

---

## 4. Configuration & operational contract

| Env | Used by | Effect when unset |
|---|---|---|
| `HUBRISE_WEBHOOK_SECRET` | `http.rs` (inbound HMAC; also the OAuth **client secret** — it IS the HubRise app client secret) | `POST /adapters/hubrise/webhooks` → **503**; connect routes → **503** (fail closed) |
| `HUBRISE_CLIENT_ID` | `http.rs`/`connect.rs` (OAuth authorize + token exchange) | connect routes → **503** (webhook enrichment unaffected) |
| `HUBRISE_CONNECT_REDIRECT_URL` | `http.rs` (the public `/adapters/hubrise/oauth/callback` URL) | connect routes → **503** |
| `HUBRISE_OAUTH_SCOPE` | `http.rs` | defaults to `account[catalog.read,inventory.read]` |
| `HUBRISE_API_BASE_URL` | `api.rs` | defaults to `https://api.hubrise.com/v1` |

Pull tokens are **not** env config: they live per connected account in `hubrise_connections`
(the retired `HUBRISE_ACCESS_TOKEN` has no fallback — reconnect the account through `§0` instead).

- **Endpoint:** `POST /adapters/hubrise/webhooks` — mounted by `crates/server/src/lib.rs`
  (`.merge(hubrise_adapter::routes(hubrise_enricher))`), **not** the GraphQL surface. The adapter ships a
  standalone `main.rs`, so HubRise can be **deployed as its own web service** (ADR-20260718-213352).
- **Import path (events):** full sync → `CatalogImported` (replace semantics); inventory sync →
  `OfferStockUpdated` — see [specs/integrations/hubrise.md §6](../../specs/integrations/hubrise.md#6-import-path-events).

## Open items (HubRise)
| Item | Where | Blocked on |
|---|---|---|
| Restaurant-facing connect UI + "connected" read surface | §0 | restaurant screens (SDUI) |
| Disconnect/revoke (`POST /oauth2/v1/revoke`) + token encryption at rest | §0 | follow-up (ADR-20260721-100601) |
| Confirm `GET /catalogs` + the location `opening_hours` wire shape (left empty on provisioning) | §0, `connect.rs` | check against the live API reference |
| Confirm HubRise API resource **paths** (`/catalog/{id}`, `/location/{id}/inventory`) | §1–§2, `api.rs` | check against the live API reference |
| Deals / advanced price_overrides / restrictions | mapping | out of V0 scope (spec §4) |
