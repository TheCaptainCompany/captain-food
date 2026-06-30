## 1. Event store table

PostgreSQL table definition (conceptual):

```sql
CREATE TABLE domain_events (
  position        BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, // $all order: total order across every stream (projection checkpoint)
  id              UUID NOT NULL UNIQUE,        // event id — idempotency key, deduped on append
  stream_name     TEXT NOT NULL,               // '<CatalogCategory>-<id>', e.g. 'Catalog-12345'; category = prefix before the first '-'
  version         INT  NOT NULL,               // 0-based event number within the stream (expected-version concurrency)
  user_id         UUID NOT NULL,
  user_type       INT NOT NULL,
  correlation_id  UUID NOT NULL,
  cause_id        UUID NULL,
  event_type      TEXT NOT NULL,               // event type ($et-<type> projection)
  payload         JSONB NOT NULL,              // event data
  metadata        JSONB,
  occurred_at     TIMESTAMPTZ NOT NULL,        // could be interesting if we use integer
  expired_at      TIMESTAMPTZ NULL,            // per-event TTL (cf. per-stream $maxAge / $maxCount metadata)
  UNIQUE (stream_name, version)                // optimistic concurrency: one event per (stream, expected version)
);

CREATE INDEX ON domain_events (stream_name, version);  // read one stream in order; prefix-scannable per category
CREATE INDEX ON domain_events (event_type);            // $et-<type>
CREATE INDEX ON domain_events (occurred_at);
```

This mirrors **EventStoreDB / SqlStreamStore** in plain SQL. A **stream** is the ordered event
sequence of one aggregate instance; it maps 1:1 to a domain aggregate (`actors.yaml`). The mapping:

| EventStore concept | Column / mechanism here |
|---|---|
| Stream name (`<CatalogCategory>-<id>`, e.g. `Catalog-12345`) | `stream_name` — category = prefix, so **no `stream_type` column** |
| Event number / stream revision (0-based) | `version` — `UNIQUE (stream_name, version)` gives expected-version concurrency |
| `$all` global position | `position` (identity) — total order; projections track a checkpoint on it |
| Event id (idempotent append) | `id` — `UNIQUE` |
| Event type | `event_type` |
| `$ce-<category>` projection | `ce_events(category)` (below) |
| `$et-<type>` projection | `et_events(event_type)` (below) |
| Stream `$maxAge` / `$maxCount` | `expired_at` (simplified to a per-event TTL) |

- The category prefix is one of `Restaurant | Catalog | Customer | Cart | Order | DeliveryJob`
  (matches the aggregates in [actors.yaml](actors.yaml)); the `<id>` suffix is the instance id.
- `metadata`: optional. To stay faithful to EventStore, `correlation_id` / `cause_id` / user could be
  folded in here (as `$correlationId` / `$causationId`) rather than kept as columns — left as columns
  for now for query convenience.

### Helper — events for a category (`$ce-<category>`)

`ce_events(category)` returns every event whose stream belongs to one **category**, in chronological
order — the SQL equivalent of EventStoreDB's `$ce-<category>` projection. This is for
inspection/replay over the log only — read paths still go through the `View_*` projections, never
`domain_events` directly.

```sql
-- ce_events('Catalog')  ==  SELECT * FROM domain_events WHERE stream_name LIKE 'Catalog-%'
CREATE FUNCTION ce_events(category TEXT)
RETURNS SETOF domain_events
LANGUAGE sql STABLE AS $$
  SELECT *
  FROM domain_events
  WHERE split_part(stream_name, '-', 1) = category
  ORDER BY stream_name, version;
$$;
```

- `category` is a stream-name prefix: `Restaurant | Catalog | Customer | Cart | Order | DeliveryJob`.
- The category is derived from `stream_name` (prefix before the first `-`), so no `stream_type`
  column is stored.
- Ordered by `(stream_name, version)` so each stream stays contiguous and replay-ordered.

### Helper — events for an event type (`$et-<type>`)

`et_events(event_type)` returns every event of one **event type** across all streams, in global
order — the SQL equivalent of EventStoreDB's `$et-<type>` projection. Same caveat: inspection/replay
only, never a read path.

```sql
-- et_events('RestaurantRegistered')  ==  SELECT * FROM domain_events WHERE event_type = 'RestaurantRegistered'
CREATE FUNCTION et_events(event_type TEXT)
RETURNS SETOF domain_events
LANGUAGE sql STABLE AS $$
  SELECT *
  FROM domain_events
  WHERE domain_events.event_type = et_events.event_type
  ORDER BY position;
$$;
```

- `event_type` is an event name from [events.yaml](events.yaml), e.g. `'RestaurantRegistered'`.
- Backed by the `(event_type)` index.
- Ordered by `position` (the `$all` global order), since the result spans many streams.

## 2. Read models — projection views (`View_*`)

Queries **never** read `domain_events`; they read dedicated read tables fed by projections that
consume events. These read tables are **"fake" tables** (denormalized, query-shaped, rebuildable
from the log) — to avoid any confusion with a real/normalized table, every one is prefixed
**`View_`** (`View_{TableName}`).

The read models below are the **source of truth in [views.yaml](views.yaml)** and the per-view detail
is GENERATED from it (run `npm run generate` in `tools/codegen`). Each view declares only what is
intrinsic to the read model: its **source aggregate + events** ([events.yaml](events.yaml) /
[actors.yaml](actors.yaml)), its **business filters/rules**, and its **columns**. The consumer mapping
— which GraphQL query reads it — is declared in [api.yaml](api.yaml) via `@reads` and
surfaced in [traceability.md](traceability.md) §2. Money is stored as integer minor units (`*_cents`
+ `currency`), matching `Money`; `JSONB` is used where a whole sub-tree is fetched at once. The SQL
DDL for these tables is generated to `specs/generated/views.generated.sql`.

<!-- GENERATED:views START — source: specs/views.yaml; run `npm run generate`. Do not edit between the markers. -->

### `View_RestaurantAccount` · 🛶 V0 · 🔒 internal · source aggregate `RestaurantAccount`

- **Consumed by**: command handlers / auth resolution (no GraphQL query).
- **Fed by**: `RestaurantAccountRegistered`, `RestaurantAccountUpdated`, `RestaurantAccountDeleted`
- **Note**: Account read model (HubRise restaurant). Holds account-level facts shared by its locations; locations denormalize default_currency from here.

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `restaurant_account_id` | `RestaurantAccountId` | `UUID` | PK |  |
| `ref` | `ExternalReference` | `TEXT` | nullable |  |
| `legal_name` | `RestaurantLegalName` | `TEXT` | — |  |
| `default_currency` | `CurrencyCode` | `TEXT` | — |  |
| `timezone` | `TimeZone` | `TEXT` | nullable |  |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | Row write time, stamped on each event. |

### `View_Restaurant` · 🛶 V0 · source aggregate `Restaurant`

- **Fed by**: `RestaurantRegistered`, `RestaurantUpdated`, `RestaurantActivated`, `RestaurantDeactivated`, `RestaurantAcceptanceModeChanged`, `RestaurantRemoved`, `RestaurantGoogleBusinessProfileUpdated`, `RestaurantListingClaimed`, `RestaurantListingOptedOut`, `RestaurantMarkedClosed`, `RestaurantListingStatusChanged`, `RestaurantGoogleBusinessProfileOrderLinkConfigured`, `RestaurantGoogleBusinessProfileOrderLinkVerified`, `RestaurantAccountRegistered`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `restaurant_id` | `RestaurantId` | `UUID` | PK |  |
| `restaurant_account_id` | `RestaurantAccountId` | `UUID` | index, nullable | NULL for a non-partner public listing; set on claim/conversion. |
| `listing_status` | `RestaurantListingStatus` | `TEXT` | index |  |
| `external_identifiers` | `jsonb` | `JSONB` | nullable | Source-agnostic [{key,value}] (siret/naf/google_place_id…); not unique. |
| `google_place_id` | `GooglePlaceId` | `TEXT` | nullable |  |
| `slug` | `Slug` | `TEXT` | unique |  |
| `display_name` | `RestaurantDisplayName` | `TEXT` | — |  |
| `description` | `text` | `TEXT` | nullable | ⚠️ HOLE: no event carries a restaurant description — nothing populates this column yet. |
| `tags` | `jsonb` | `JSONB` | nullable | Cuisine/attribute tags, sourced from Google Business Profile enrichment. |
| `rating` | `GoogleRating` | `TEXT` | nullable |  |
| `reviews_count` | `integer` | `INTEGER` | nullable |  |
| `website` | `WebUrl` | `TEXT` | nullable |  |
| `phone` | `PhoneNumber` | `TEXT` | nullable |  |
| `gbp_order_url` | `WebUrl` | `TEXT` | nullable |  |
| `gbp_link_status` | `GbpLinkStatus` | `TEXT` | nullable |  |
| `address` | `jsonb` | `JSONB` | — |  |
| `opening_hours` | `jsonb` | `JSONB` | — |  |
| `status` | `RestaurantStatus` | `TEXT` | — | Derived from the lifecycle event type: DRAFT on register, ACTIVE/INACTIVE on (de)activation, INACTIVE on closure. |
| `order_acceptance` | `OrderAcceptanceMode` | `TEXT` | — |  |
| `default_currency` | `CurrencyCode` | `TEXT` | — |  |
| `timezone` | `TimeZone` | `TEXT` | nullable | Location timezone; falls back to the account's when null. |
| `preparation_time_minutes` | `integer` | `INTEGER` | nullable |  |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | Row write time, stamped on each event. |

### `View_Customer` · 🛶 V0 · source aggregate `Customer`

- **Fed by**: `CustomerRegistered`, `RestaurantRated`, `RestaurantFavorited`, `RestaurantUnfavorited`, `CustomerInfoUpdated`, `CustomerEmailVerified`, `CustomerPhoneChanged`, `CustomerLanguageChanged`, `CustomerPreferencesSet`, `CustomerAddressSet`, `CustomerAddressRemoved`, `CustomerPaymentMethodSet`
- **Rules**: `ratings` accumulates the customer's own restaurant ratings (from RestaurantRated) so they can see how they rated each restaurant. `favorite_restaurant_ids` is maintained from RestaurantFavorited/RestaurantUnfavorited; the favoriteRestaurants query joins it to View_Restaurant.
- **Note**: Identity/lookup read model: resolves a returning phone (or auth_ref) to an existing Customer, backs VerifyPhone idempotency + auth resolution, and serves the `me` query (CustomerProfile). Also bound when CustomerIdentified stamps carts. The stored `locale` localizes authenticated SMS/email sends.

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `customer_id` | `CustomerId` | `UUID` | PK |  |
| `phone` | `PhoneNumber` | `TEXT` | unique |  |
| `auth_ref` | `ExternalReference` | `TEXT` | index, nullable | Auth provider user id (Supabase Auth) → Customer. |
| `display_name` | `CustomerDisplayName` | `TEXT` | nullable |  |
| `email` | `EmailAddress` | `TEXT` | nullable |  |
| `email_verified` | `boolean` | `BOOLEAN` | — | True once an email magic link has been confirmed (CustomerEmailVerified). |
| `locale` | `Locale` | `TEXT` | nullable | i18n culture; set at registration or via ChangeLanguage. Localizes authenticated SMS/email sends. |
| `timezone` | `TimeZone` | `TEXT` | nullable |  |
| `ratings` | `jsonb` | `JSONB` | — | The customer's own submitted ratings (assembled from RestaurantRated): [{ order_id, restaurant_id, stars, comment, rated_at }]. |
| `favorite_restaurant_ids` | `jsonb` | `JSONB` | — | [restaurant_id] the customer favorited. |
| `preferences` | `jsonb` | `JSONB` | nullable | { dietary_tags: [...], favorite_cuisines: [...] } from CustomerPreferencesSet. |
| `addresses` | `jsonb` | `JSONB` | — | Saved address book: [{ address_id, label, address }] from CustomerAddressSet/Removed. |
| `payment_method_id` | `PaymentMethodId` | `TEXT` | nullable |  |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | Row write time, stamped on each event. |

### `View_PhoneCountry` · 🛶 V0 · 📦 reference (static seed)

- **Reference data**: seeded at deploy time (not event-fed).
- **Note**: Phone-country reference for the dialing-code picker. The picker emits the `dialing_code` ('+33').

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `country` | `CountryCode` | `TEXT` | PK |  |
| `dialing_code` | `DialingCode` | `TEXT` | index |  |
| `name` | `text` | `TEXT` | — |  |
| `default_locale` | `Locale` | `TEXT` | — |  |

### `View_Catalog` · 🛶 V0 · source aggregate `Catalog`

- **Fed by**: `CatalogCreated`, `CatalogCategoryAdded`, `CatalogCategoryUpdated`, `CatalogCategoryRemoved`, `ProductAdded`, `ProductUpdated`, `ProductRemoved`, `OptionListAdded`, `OptionListUpdated`, `OptionListRemoved`, `OfferStockUpdated`, `CatalogImported`
- **Rules**: `stock_status` is derived (quantity vs lowStockThreshold); orderable = AVAILABLE and stock > 0. Could be normalized (one row per offer) if per-item querying is needed later.

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `catalog_id` | `CatalogId` | `UUID` | PK |  |
| `restaurant_id` | `RestaurantId` | `UUID` | index |  |
| `slug` | `Slug` | `TEXT` | — | ⚠️ HOLE: CatalogCreated carries no slug — nothing populates this column (drop it or add slug to the event). |
| `name` | `CatalogName` | `TEXT` | — |  |
| `catalog` | `jsonb` | `JSONB` | — | Assembled tree: categories -> products -> offers { price_cents, currency, availability, stock_status } + option lists. |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | Row write time, stamped on each event. |

### `View_Cart` · 🛶 V0 · source aggregate `Cart`

- **Fed by**: `CartStarted`, `CartLineAdded`, `CartLineQuantityChanged`, `CartLineRemoved`, `CartCheckedOut`, `CustomerIdentified`
- **Rules**: Prices are computed by the projection from the current catalog, never trusted from the client. `customer_id` is NULL while the cart is owned by a guest; bound when CustomerIdentified resolves authRef → customerId, or at checkout.
- **Note**: Joined with the catalog for pricing (secondary source).

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `cart_id` | `CartId` | `UUID` | PK |  |
| `restaurant_id` | `RestaurantId` | `UUID` | — |  |
| `customer_id` | `CustomerId` | `UUID` | nullable | NULL while guest; bound by CustomerIdentified or at checkout. |
| `status` | `CartStatus` | `TEXT` | — | Derived from event type: OPEN on CartStarted, CHECKED_OUT on CartCheckedOut. |
| `lines` | `jsonb` | `JSONB` | — | Priced by the projection from the live catalog: [{ cart_line_id, offer_id, product_id, name, offer_name, quantity, unit_price_cents, selected_options, line_total_cents }]. |
| `total_amount_cents` | `MoneyCents` | `BIGINT` | — | COMPUTED by the projection from the live catalog (never trusted from the client). |
| `currency` | `CurrencyCode` | `TEXT` | — | From the catalog currency at pricing time (the restaurant's default_currency). |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | Row write time, stamped on each event. |

### `View_OrderTracking` · 🛶 V0 · source aggregate `Order`

- **Fed by**: `OrderPlaced`, `OrderAcceptedByRestaurant`, `OrderPreparationStarted`, `OrderMarkedReady`, `OrderDelivered`, `OrderRejectedByRestaurant`, `OrderCancelledByCustomer`, `OrderCancelledByRestaurant`, `PaymentCaptured`, `PaymentRefunded`, `OrderRated`, `RestaurantRated`, `RiderTipped`
- **Rules**: `payment_status` is folded from the Stripe payment facts. Rating columns are populated from OrderRated (rider_thumb), RestaurantRated (restaurant_stars + comment) and RiderTipped (rider_tip_cents); null until the customer acts. The restaurant reads restaurant_stars/comment to see its rating.
- **Note**: The single canonical Order read model. Folds the Order lifecycle + Stripe payment facts (secondary source). Serves every order query — by id (`order`), by customer (history) and by restaurant+status (back-office queue) — via the indexes below; there is no separate per-persona order projection.

- **Indexes**: `(restaurant_id, status, placed_at)`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `order_id` | `OrderId` | `UUID` | PK |  |
| `ref` | `ExternalReference` | `TEXT` | — |  |
| `restaurant_id` | `RestaurantId` | `UUID` | — |  |
| `customer_id` | `CustomerId` | `UUID` | index, nullable |  |
| `status` | `OrderStatus` | `TEXT` | — | Derived from the lifecycle event type. |
| `service_type` | `ServiceType` | `TEXT` | — |  |
| `items` | `jsonb` | `JSONB` | — |  |
| `total_amount_cents` | `MoneyCents` | `BIGINT` | — | amountCents of OrderPlaced.totalAmount (Money). |
| `currency` | `CurrencyCode` | `TEXT` | — | currency of OrderPlaced.totalAmount (Money). |
| `delivery_address` | `jsonb` | `JSONB` | nullable |  |
| `estimated_ready_at` | `timestamptz` | `TIMESTAMPTZ` | nullable |  |
| `placed_at` | `timestamptz` | `TIMESTAMPTZ` | — | OrderPlaced occurrence time. |
| `status_changed_at` | `timestamptz` | `TIMESTAMPTZ` | — | Occurrence time of the latest status-changing event. |
| `payment_status` | `text` | `TEXT` | — | Folded from Stripe facts; candidate for a PaymentStatus enum. |
| `restaurant_stars` | `StarRating` | `INTEGER` | nullable | Customer's 0–5 rating of the restaurant; null until rated. |
| `rating_comment` | `RatingComment` | `TEXT` | nullable |  |
| `rider_thumb` | `ThumbRating` | `TEXT` | nullable |  |
| `rider_tip_cents` | `MoneyCents` | `BIGINT` | nullable | amountCents of RiderTipped.amount (Money); null if no tip. |
| `rated_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | Occurrence time of the latest rating/tip event. |

<!-- GENERATED:views END -->
