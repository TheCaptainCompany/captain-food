# HubRise Integration

HubRise is the **interoperability standard** chosen for Captain.Food (order aggregation, POS,
delivery platforms). The Captain.Food domain model is aligned with the HubRise structure, but
**more strongly typed** where it helps (see [Refinements](#refinements-vs-hubrise)).

> 🛡️ **Anti-Corruption Layer (ACL)**: HubRise → domain translation happens at the integration
> boundary. HubRise-only concepts (`SKU`, `option_list`, `"9.80 EUR"` string prices) must **never**
> leak into the domain. The `ExternalReference` scalar (= HubRise `ref`) is the idempotent import key.

Sources: [API Catalogs](https://www.hubrise.com/developers/api/catalogs) ·
[API Locations & Accounts](https://www.hubrise.com/developers/api/accounts) ·
[Catalog concepts](https://www.hubrise.com/docs/catalog)

---

## 1. Data exposed by HubRise

### Location (= a point of sale → our `Restaurant`)
`name`, `address`, `postal_code`, `city`, `country`, `timezone`, `opening_hours` (slots per day,
`HH:mm`), `cutoff_time`, `preparation_time` (min), `order_acceptance` (`normal`/`busy`/`paused`),
attached to an **Account** that carries the `currency`.
❌ **No `phone`/`email`** at Location level.

### Catalog (= our `Catalog`)
- **Categories**: tree (`ref`, `parent_ref`, `name`, `description`, `tags`, `image_ids`)
- **Products**: `name`, `description`, `tags`, `image_ids`, `nutrition`, `tax_rate` (triplet
  `delivery`/`collection`/`eat_in`), array of **SKUs**
- **SKUs** (offers): `price` (`"9.80 EUR"`), `price_overrides`, `restrictions`,
  `option_list_ids`, `barcodes`…
- **Option lists / Options**: modifiers (`min/max_selections`, `multiple_selection`,
  `price`, `default`…)
- **Deals**: promotional bundles
- **Inventories**: stock per location (`stock`, `expires_at`)

Continuous sync is possible via **Callbacks** (webhooks) — not only a one-shot import.

---

## 2. HubRise → Captain.Food domain mapping

| HubRise | Domain ([entities.yaml](../entities.yaml) / [scalars.yaml](../scalars.yaml)) | Note |
|---|---|---|
| Location `name` | `Restaurant.displayName` | direct |
| Location `address/postal_code/city/country` | `Restaurant.address` (`Address`) | direct |
| Location `id` | `Restaurant.ref` | idempotent import key |
| Account `currency` | `Restaurant.defaultCurrency` | direct |
| Location `opening_hours` | `Restaurant.openingHours` (`OpeningHoursSlot[]`) | `HH:mm` → `TimeOfDay` |
| Location `timezone` | `Restaurant.timezone` (`TimeZone`) | direct |
| Location `preparation_time` | `Restaurant.preparationTimeMinutes` | direct |
| Location `order_acceptance` | `Restaurant.orderAcceptance` (`OrderAcceptanceMode`) | `normal/busy/paused` → `NORMAL/BUSY/PAUSED` |
| **(none)** `phone`/`email` | `RestaurantContact` (optional) | 🔧 filled manually by the admin |
| CatalogCategory (`ref`, `parent_ref`, `name`…) | `CatalogCategory` (`parentRef`) | tree preserved |
| Product (`name`, `description`, `tax_rate`…) | `Product` | `tax_rate` triplet → `TaxRate` |
| Product → SKUs | `Product.offers` (`Offer[]`, min 1) | 1 SKU = 1 `Offer` |
| SKU `price` `"9.80 EUR"` | `Offer.price` (`Money`) | parse + ×100, currency extracted |
| SKU `option_list_ids` | `Offer.optionListIds` | direct |
| Option list / Option | `OptionList` / `Option` | direct |
| Inventory `stock` / `expires_at` | `Offer.stock` (`Stock`) | `stock` → `quantity`, `expires_at` → `expiresAt` |
| SKU `restrictions.enabled` | `Offer.availability` (`CatalogItemAvailability`) | `enabled` → `AVAILABLE`/`UNAVAILABLE` |
| Deals | *not modelled* | out of V0 scope |

---

## 3. Refinements vs HubRise

Where Captain.Food is more precise than HubRise, we **keep our model**:

- **`Money`** value object (`amountCents` int + `currency`) instead of the `"9.80 EUR"` string.
  Conversion only at the ACL boundary.
- **`Stock`** explicit + derived **`StockStatus`** (`IN_STOCK`/`LOW_STOCK`/`OUT_OF_STOCK`).
  `LOW_STOCK` = `quantity <= lowStockThreshold` (risk threshold, absent from HubRise).
- **Availability ≠ stock**: `CatalogItemAvailability` (manual UI flag) distinct from derived stock status.
- **Strong typing** throughout (one name = one dedicated scalar), `$ref` everywhere.

---

## 4. Gaps / decisions to be aware of

1. **Restaurant contact**: HubRise exposes neither email nor phone at Location level.
   `RestaurantContact` is therefore **optional**; to be completed manually after import.
2. **`ServiceType`**: HubRise = `delivery`/`collection`/`eat_in`. Captain.Food = `DELIVERY`/`COLLECTION`
   (`collection` = pickup); `eat_in` not offered but kept in `TaxRate` for catalog fidelity.
3. **Deals** and advanced **price_overrides**/**restrictions**: not modelled in V0.
4. **Offers**: we adopt the `Product → Offer[]` structure (a simple product = 1 offer).
5. **Uber Eats real-price comparison** (ADR-0023/0030): when a restaurant is on HubRise, HubRise carries its
   Uber Eats menu prices, and it has **explicitly opted in** (`Restaurant.uberPricesOptIn`), the ACL may feed
   those prices so the comparison shows `basis: REAL` instead of the coefficient estimate. The per-offer
   real-price ingestion is **deferred to runtime**; V0 shows the labelled ESTIMATED comparison only.

---

## 5. Connect flow (OAuth) — provisioning & token

Connecting a HubRise **Account** (`GET /adapters/hubrise/connect` → OAuth → callback; issue #20,
ADR-20260721-100601) provisions the domain side with the ACL's derived UUIDv5 identities —
`RegisterRestaurantAccount` (Account), `RegisterRestaurant` per Location (`listingStatus:
PASSIVE_PARTNER`, `ref` = location id), `CreateCatalog` per catalog — then stores the
**account-scoped, non-expiring** access token in the adapter-owned
[`hubrise_connections`](../database/tables/integration_connections.yaml) table keyed by
`RestaurantAccount` (never in `domain_events`, never exposed through api.yaml), and runs an initial
`ImportCatalog`. Callbacks resolve their pull token via the connection's location snapshot; a
re-connect refreshes token, locations and catalog content idempotently. Runtime process:
[docs/integrations/hubrise-process.md §0](../../docs/integrations/hubrise-process.md).

## 6. Import path (events)

Two modes, both going through the ACL:

- **Full import / sync** → event `CatalogImported` (`source: HUBRISE`) carrying
  `categories[]`, `products[]`, `optionLists[]` and **replacing** the catalog content.
- **Inventory sync** (HubRise callback) → targeted `OfferStockUpdated` event, without rewriting the product.

For the restaurant itself, the import feeds the `RegisterRestaurant` command
(then manual contact completion). The import use case is classified **V1**.
