## 1. Event store

The store DDL — the `domain_events` log (+ indexes), the `ce_events` / `et_events` / `all_events` helper
functions, the `domain_stream` retention table and the `$maxCount` trigger — is **GENERATED** to
[`specs/generated/schema.generated.sql`](generated/schema.generated.sql) from
[`database/tables/`](database/tables/) and [`database/functions/`](database/functions/) (run
`make generate`). Plus one `ref_<enum>` lookup table per `scalars.yaml` enum. **This section is the
rationale — the generated SQL is the source of truth; do not hand-write DDL here.**

`domain_events` mirrors **EventStoreDB / SqlStreamStore** in plain SQL. A **stream** is the ordered event
sequence of one aggregate instance; it maps 1:1 to a domain aggregate (`actors.yaml`). Key columns:
`position` (the `$all` total order, identity PK, projection checkpoint), `id` (idempotent append,
unique), `stream_name` (`<Category>-<id>`, e.g. `Catalog-12345`; category = prefix before the first `-`),
`version` (0-based; `UNIQUE (stream_name, version)` gives expected-version concurrency), `event_type`,
`payload` (JSONB), `occurred_at`, and `expired_at` (per-event TTL). The mapping:

| EventStore concept | Column / mechanism here |
|---|---|
| Stream name (`<CatalogCategory>-<id>`, e.g. `Catalog-12345`) | `stream_name` — category = prefix, so **no `stream_type` column** |
| Event number / stream revision (0-based) | `version` — `UNIQUE (stream_name, version)` gives expected-version concurrency |
| `$all` global position | `position` (identity) — total order; projections track a checkpoint on it |
| Event id (idempotent append) | `id` — `UNIQUE` |
| Event type | `event_type` |
| `$ce-<category>` projection | `ce_events(category)` |
| `$et-<type>` projection | `et_events(event_type)` |
| `$all` global stream | `all_events()` — `ORDER BY position` |
| Stream `$maxAge` / `$maxCount` | `domain_stream(stream_name, max_age, max_count)` policy + `expired_at`; a trigger enforces `$maxCount`, a scheduled sweep enforces `$maxAge` |

- The category prefix is one of `Restaurant | Catalog | Customer | Cart | Order | DeliveryJob`
  (matches the aggregates in [actors.yaml](actors.yaml)); the `<id>` suffix is the instance id.
- `metadata`: optional. `correlation_id` / `cause_id` / user are kept as columns for query convenience
  (an EventStore-faithful alternative would fold them into `metadata` as `$correlationId` / `$causationId`).

### Log helpers (inspection / replay only — read paths use `View_*`, never `domain_events`)

- `ce_events(category)` — `$ce-<category>`: every event of one category, ordered `(stream_name, version)`.
- `et_events(event_type)` — `$et-<type>`: every event of one type across all streams, ordered `position`.
- `all_events()` — `$all`: the whole log, ordered `position` (projections track a checkpoint on it).

Bodies live in [`database/functions/*.sql`](database/functions/) and are assembled into the generated schema.

### Stream retention — `$maxAge` / `$maxCount`

The log is **append-only by default** — full history is what makes the `View_*` projections rebuildable, so
most streams keep everything. Retention is **opt-in per stream** (keyed by `stream_name`) via
`domain_stream` and meant only for **ephemeral** streams (e.g. a `Cart-<id>`). `$maxCount` is enforced
synchronously by the `trg_domain_events_max_count` trigger (`enforce_max_count`), trimming a stream to its
last N versions. **Only streams with a policy row are ever trimmed** — everything else keeps full history,
staying rebuildable (ADR-0005). `expired_at` is the per-event escape hatch.

`$maxAge` is enforced by a scheduled sweep — **not part of the generated schema** (environment-specific): a
`pg_cron` job, or a dedicated retention worker where `pg_cron` is unavailable (e.g. the managed tier):

```sql
SELECT cron.schedule('domain_events_retention', '0 * * * *', $$
  DELETE FROM domain_events e USING domain_stream s
  WHERE e.stream_name = s.stream_name
    AND (   (e.expired_at IS NOT NULL AND e.expired_at < now())          -- explicit per-event TTL
         OR (s.max_age    IS NOT NULL AND e.occurred_at < now() - s.max_age) );  -- stream $maxAge
$$);
```

### Write-path journals & adapter staging (ADR-20260720-015300 / -015400)

Two table categories sit BESIDE the event store, never inside it:

- **`command_journal`** ([`tables/journals.yaml`](database/tables/journals.yaml)) — one row per
  command submission (any channel), persisted **before** handling. pk `message_id` is the write-path
  idempotency key (same payload hash = replayed acceptance; different = Conflict); the row records
  **rejections too** and backs the `operationStatus` query/subscription. Events appended by the
  command carry `message_id` as `domain_events.cause_id`, chaining request → journal → facts.
- **`inbound_events`** (same file) — adapted inbound **business** events (events.yaml vocabulary
  only) staged by adapter ACLs, drained by the `InboundEventsDrainWorker` through the normal write
  path (`cause_id = inbound_event_id`; the aggregate's fold stays the authoritative dedupe).
- **`external_*` staging** ([`tables/integration_staging.yaml`](database/tables/integration_staging.yaml),
  ADR-0045 generalized) — adapter-OWNED verbatim mirrors (`external_sirene_restaurants`,
  `external_stripe_events`, `external_hubrise_callbacks`): verify → UPSERT → ACK, with
  `processed_at` as the translation high-water mark for replay/backfill.

Journals **never write `domain_events`** and are **never replayed as state** — the event log stays
the single source of truth. None of these are projected or a GraphQL `reads` target.

### Journal & mirror retention — `sweep_retention()` (ADR-20260721-025159)

Unlike the forever event log, journals and webhook mirrors have a **usefulness window**. The
windows live in **one place** — the [`sweep_retention()`](database/functions/sweep_retention.sql)
function (part of the generated schema): `command_journal` terminal rows 90 days from
`completed_at`; `inbound_events` `DELIVERED` rows 30 days from `delivered_at`;
`external_stripe_events`/`external_hubrise_callbacks` processed rows 90 days from `processed_at`
(also the PII cap on verbatim third-party payloads). **Never swept**: `domain_events` /
`domain_stream` (this function does not reference them — the log's only trimming stays the
opt-in `$maxAge`/`$maxCount` above), `RECEIVED` journal rows (the stale-`RECEIVED` sweep marks
crashed runs `FAILED` first), `FAILED` inbound rows (kept until resolved), unprocessed mirror
rows, and `external_sirene_restaurants` (full mirror — detect-by-absence needs every row).
Scheduling is environment-side, like the `$maxAge` sweep: the in-process `RetentionSweepWorker`
calls it every 6 h (`RUN_RETENTION_SWEEP`, default on), or a `pg_cron` job
(`SELECT * FROM sweep_retention();`) where DB-side scheduling is preferred.

## 2. Read models — projection views (`View_*`)

Queries **never** read `domain_events`; they read dedicated read tables fed by projections that
consume events. These read tables are **"fake" tables** (denormalized, query-shaped, rebuildable
from the log) — to avoid any confusion with a real/normalized table, every one is prefixed
**`View_`** (`View_{TableName}`).

The read models below are the **source of truth in [projection_views.yaml](database/projection_views.yaml)** and the per-view detail
is GENERATED from it (run `make generate`). Each view declares only what is
intrinsic to the read model: its **source aggregate + events** ([events.yaml](events.yaml) /
[actors.yaml](actors.yaml)), its **business filters/rules**, and its **columns**. The consumer mapping
— which GraphQL query reads it — is declared in [api.yaml](api.yaml) via `@reads`
(rendered in the generated documentation). Money is stored as integer minor units (`*_cents`
+ `currency`), matching `Money`; `JSONB` is used where a whole sub-tree is fetched at once. The SQL
DDL for these tables is generated to `specs/generated/views.generated.sql`.

<!-- GENERATED:views START — source: specs/database/projection_views.yaml; run `make generate`. Do not edit between the markers. -->

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
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `View_DeliveryJob` · 🛶 V0 · source aggregate `DeliveryJob`

- **Fed by**: `DeliveryRequested`, `DeliveryAcceptedByPartner`, `DeliveryRejectedByPartner`, `DeliveryStatusUpdated`, `DeliveryAcceptedByRider`, `DeliveryPickedUp`, `DeliveryCompleted`, `DeliveryCancelled`, `DeliveryDispatchFailed`
- **Rules**: `status` is derived from the lifecycle events: PENDING on DeliveryRequested → ASSIGNED on DeliveryAcceptedByRider/DeliveryAcceptedByPartner → PICKED_UP on DeliveryPickedUp → then partner DeliveryStatusUpdated (OUT_FOR_DELIVERY/DELIVERED/FAILED) or DeliveryCompleted (DELIVERED) / DeliveryCancelled (CANCELLED) / DeliveryDispatchFailed (FAILED — offer cap exhausted, ADR-20260720-004556). `provider` is INDEPENDENT once a rider accepts, PARTNER once a partner accepts.
- **Indexes**: `(restaurant_id, status)`, `(rider_id, status)`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `delivery_job_id` | `DeliveryJobId` | `UUID` | PK |  |
| `order_id` | `OrderId` | `UUID` | index |  |
| `restaurant_id` | `RestaurantId` | `UUID` | — |  |
| `status` | `DeliveryStatus` | `INTEGER` | — | Derived from the lifecycle event type / DeliveryStatusUpdated.status (DeliveryDispatchFailed → FAILED, the offer-cap exhaustion). |
| `provider` | `DeliveryProvider` | `INTEGER` | nullable | INDEPENDENT (rider accepted) or PARTNER (partner accepted); null while PENDING. |
| `rider_id` | `RiderId` | `UUID` | nullable | Set for an independent-rider delivery; null for a partner delivery. |
| `courier` | `jsonb` | `JSONB` | nullable | Courier { displayName, phone?, riderId? }; from the partner on acceptance (independent rider is in rider_id). |
| `partner_ref` | `ExternalReference` | `TEXT` | nullable | Partner-side delivery id; idempotent key for inbound updates. |
| `pickup_address` | `jsonb` | `JSONB` | — |  |
| `dropoff_address` | `jsonb` | `JSONB` | — |  |
| `estimated_pickup_at` | `timestamptz` | `TIMESTAMPTZ` | nullable |  |
| `estimated_dropoff_at` | `timestamptz` | `TIMESTAMPTZ` | nullable |  |
| `requested_at` | `timestamptz` | `TIMESTAMPTZ` | — | DeliveryRequested occurrence time. |
| `picked_up_at` | `timestamptz` | `TIMESTAMPTZ` | nullable |  |
| `delivered_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | Set on DeliveryCompleted or DeliveryStatusUpdated=DELIVERED (conditional occurrence). |
| `last_partner_rejection` | `text` | `TEXT` | nullable | Reason of the latest partner decline (the job stays PENDING and is re-offered, up to the 3-offer cap — ADR-20260720-004556); null if never rejected. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `View_DeliverySatisfaction` · 🔭 V1 · source aggregate `Order`

- **Fed by**: `DeliverySatisfactionRecorded`
- **Rules**: One row per order, present only once the customer has answered the survey (DeliverySatisfactionRecorded); record-once, so the fold never sees a second answer. `timeliness` is the customer's verdict (ON_TIME / ACCEPTABLE_DELAY / TOO_LATE); the restaurant reads it, filtered by restaurant_id, to weigh self-dispatch vs Captain routing.
- **Indexes**: `(restaurant_id, timeliness)`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `order_id` | `OrderId` | `UUID` | PK |  |
| `restaurant_id` | `RestaurantId` | `UUID` | index |  |
| `timeliness` | `DeliveryTimeliness` | `INTEGER` | — |  |
| `reason` | `DeliveryDissatisfactionReason` | `TEXT` | nullable |  |
| `recorded_at` | `timestamptz` | `TIMESTAMPTZ` | — | DeliverySatisfactionRecorded occurrence time. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `View_DeliveryPartnerAvailability` · 🔭 V1 · source aggregate `DeliveryPartnerRegistration`

- **Fed by**: `DeliveryPartnerAvailabilityRequested`, `DeliveryPartnerAvailabilityApproved`, `DeliveryPartnerAvailabilityRevoked`
- **Rules**: `status` is derived from the latest lifecycle event type: PENDING on Requested → APPROVED on Approved → REVOKED on Revoked. Set-once identity fields (channel, city_id, partner_name, contact_email) are carried only by the Requested birth fact.
- **Indexes**: `(city_id, status)`, `(channel)`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `registration_id` | `DeliveryPartnerRegistrationId` | `UUID` | PK |  |
| `channel` | `DeliveryChannelKey` | `TEXT` | — |  |
| `city_id` | `CityId` | `UUID` | index |  |
| `partner_name` | `DeliveryPartnerName` | `TEXT` | — |  |
| `contact_email` | `EmailAddress` | `TEXT` | — |  |
| `status` | `CityAvailabilityStatus` | `INTEGER` | — | Derived from the latest lifecycle event type. |
| `requested_at` | `timestamptz` | `TIMESTAMPTZ` | — | occurrence: max(occurred_at) of the birth fact. |
| `decided_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | occurrence: max(occurred_at) of the latest decision (approve/revoke); null while PENDING. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `View_PendingRefunds` · 🛶 V0 · source aggregate `Payment`

- **Fed by**: `RefundOpened`, `RefundApproved`, `RefundDenied`, `PaymentRefunded`
- **Rules**: A row exists only for a refund actually opened for decision: RefundOpened is delivered by RefundProcess ONLY when the order's payment is CAPTURED (the guard lives in the saga, so the fold needs no payment-status filter). `status` is derived from the lifecycle events: REQUESTED on RefundOpened → APPROVED on RefundApproved (Stripe refund requested) or DENIED on RefundDenied → REFUNDED on PaymentRefunded (Stripe settled). `amount_cents` is the captured order total eligible for refund; `approved_amount_cents` is the (possibly partial) approved amount, null until approved.
- **Indexes**: `(restaurant_id, status)`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `order_id` | `OrderId` | `UUID` | PK |  |
| `restaurant_id` | `RestaurantId` | `UUID` | index |  |
| `status` | `RefundStatus` | `INTEGER` | — | Derived from the latest lifecycle event type. |
| `amount_cents` | `MoneyCents` | `BIGINT` | — | amountCents of RefundOpened.amount (Money) — the captured total eligible for refund. |
| `currency` | `CurrencyCode` | `TEXT` | — | currency of RefundOpened.amount (Money). |
| `approved_amount_cents` | `MoneyCents` | `BIGINT` | nullable | amountCents of RefundApproved.amount (Money — may be partial); null until approved. |
| `reason` | `text` | `TEXT` | nullable | The latest recorded reason: the opening fact's, then the decision's. |
| `refund_id` | `RefundId` | `TEXT` | nullable | The Stripe Refund id once settled; null before PaymentRefunded. |
| `requested_at` | `timestamptz` | `TIMESTAMPTZ` | — | RefundOpened occurrence time. |
| `decided_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | The decision's occurrence time (approval or denial); null while REQUESTED. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `Restaurant` · 🛶 V0 · source aggregate `Restaurant`

- **Fed by**: `RestaurantRegistered`, `RestaurantUpdated`, `RestaurantActivated`, `RestaurantDeactivated`, `RestaurantAcceptanceModeChanged`, `RestaurantRemoved`, `RestaurantGoogleBusinessProfileUpdated`, `RestaurantListingClaimed`, `RestaurantListingOptedOut`, `RestaurantMarkedClosed`, `RestaurantListingStatusChanged`, `RestaurantGoogleBusinessProfileOrderLinkConfigured`, `RestaurantGoogleBusinessProfileOrderLinkVerified`, `RestaurantAccountRegistered`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `restaurant_id` | `RestaurantId` | `UUID` | PK |  |
| `restaurant_account_id` | `RestaurantAccountId` | `UUID` | index, nullable | NULL for a non-partner public listing; set on claim/conversion. |
| `listing_status` | `RestaurantListingStatus` | `INTEGER` | index |  |
| `external_identifiers` | `jsonb` | `JSONB` | nullable | Source-agnostic [{key,value}] (siret/naf/google_place_id…); not unique. |
| `google_place_id` | `GooglePlaceId` | `TEXT` | nullable |  |
| `slug` | `Slug` | `TEXT` | unique |  |
| `display_name` | `RestaurantDisplayName` | `TEXT` | — |  |
| `description` | `text` | `TEXT` | nullable | ⚠️ HOLE: no event carries a restaurant description — nothing populates this column yet. |
| `tags` | `jsonb` | `JSONB` | nullable | Cuisine/attribute tags — general restaurant info (source-agnostic), not from the GBP event. |
| `margin_rate` | `MarginPercent` | `TEXT` | nullable | Food margin %, input to the Captain service-fee split (ADR-0017); back-office only. |
| `cuisine_category` | `CuisineCategory` | `INTEGER` | nullable | Selects the Uber Eats price-estimate coefficient in UberEstimationPolicy (ADR-0024). |
| `uber_prices_opt_in` | `boolean` | `BOOLEAN` | nullable | Restaurant authorized showing its real Uber prices via HubRise (ADR-0023). Gates REAL vs ESTIMATED basis. |
| `website` | `WebUrl` | `TEXT` | nullable |  |
| `rating` | `GoogleRating` | `TEXT` | nullable | GBP-specific metric (Google listing), independent of the restaurant's own info. |
| `reviews_count` | `integer` | `INTEGER` | nullable |  |
| `gbp_order_url` | `WebUrl` | `TEXT` | nullable |  |
| `gbp_link_status` | `GbpLinkStatus` | `INTEGER` | nullable |  |
| `address` | `jsonb` | `JSONB` | — |  |
| `location` | `jsonb` | `JSONB` | nullable | Geo coordinates {latitude, longitude}; typically from the Google Maps sync. |
| `opening_hours` | `jsonb` | `JSONB` | — |  |
| `status` | `RestaurantStatus` | `INTEGER` | — | Derived from the lifecycle event type: DRAFT on register, ACTIVE/INACTIVE on (de)activation, INACTIVE on closure. |
| `order_acceptance` | `OrderAcceptanceMode` | `INTEGER` | — |  |
| `default_currency` | `CurrencyCode` | `TEXT` | — |  |
| `timezone` | `TimeZone` | `TEXT` | nullable | Location timezone; falls back to the account's when null. |
| `preparation_time_minutes` | `integer` | `INTEGER` | nullable |  |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `ProspectionPipeline` · 🔭 V1 · source aggregate `Prospect`

- **Fed by**: `RestaurantRegistered`, `RestaurantGoogleBusinessProfileUpdated`, `RestaurantListingStatusChanged`, `ProspectContacted`, `ProspectMarkedCold`, `ProspectReplied`
- **Filters**: Rows for NON_PARTNER / PASSIVE_PARTNER listings (active prospects); CONVERTED once ACTIVE_PARTNER.
- **Rules**: `score` (0–10) is COMPUTED by the projection from listing facts, NEVER stored in an event: food-truck NAF 56.10C +3, Google rating ≥4.0 +2, reviews <20 +2, created <12mo +2, no website +1, already on Uber/Deliveroo −2, national franchise −3; clamped to 0–10. Inputs not yet captured as fields (Sirene creation date, on-aggregator, national franchise) are best-effort/V1; the formula degrades gracefully to the available signals. `pipeline_status` is derived: NEW (no contact) → CONTACTED → COLD (ProspectMarkedCold) / REPLIED (ProspectReplied); CONVERTED when RestaurantListingStatusChanged reaches ACTIVE_PARTNER.
- **Note**: B2B prospection pipeline (ADR-0020): one row per worked listing, with the COMPUTED score and outreach state. Read by the admin prospectionPipeline query.

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `restaurant_id` | `RestaurantId` | `UUID` | PK |  |
| `score` | `ProspectionScore` | `INTEGER` | index | Derived (see rules); not an event field. |
| `pipeline_status` | `ProspectPipelineStatus` | `INTEGER` | index | Derived from the prospect events + listingStatus (see rules). |
| `contacts_count` | `integer` | `INTEGER` | — | Count of ProspectContacted; drives the anti-spam ≤3 rule. |
| `last_contacted_at` | `timestamptz` | `TIMESTAMPTZ` | nullable |  |
| `replied_at` | `timestamptz` | `TIMESTAMPTZ` | nullable |  |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `Customer` · 🛶 V0 · source aggregate `Customer`

- **Fed by**: `CustomerRegistered`, `RestaurantRated`, `RestaurantFavorited`, `RestaurantUnfavorited`, `CustomerInfoUpdated`, `CustomerEmailVerified`, `CustomerPhoneChanged`, `CustomerLanguageChanged`, `CustomerPreferencesSet`, `CustomerAddressSet`, `CustomerAddressRemoved`, `CustomerPaymentMethodSet`
- **Rules**: `ratings` accumulates the customer's own restaurant ratings (from RestaurantRated) so they can see how they rated each restaurant. `favorite_restaurant_ids` is maintained from RestaurantFavorited/RestaurantUnfavorited; the favoriteRestaurants query joins it to Restaurant.
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
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `Catalog` · 🛶 V0 · source aggregate `Catalog`

- **Fed by**: `CatalogCreated`, `CatalogCategoryAdded`, `CatalogCategoryUpdated`, `CatalogCategoryRemoved`, `ProductAdded`, `ProductUpdated`, `ProductRemoved`, `OptionListAdded`, `OptionListUpdated`, `OptionListRemoved`, `OfferStockUpdated`, `CatalogImported`
- **Rules**: `stock_status` is derived (quantity vs lowStockThreshold); orderable = AVAILABLE and stock > 0. Could be normalized (one row per offer) if per-item querying is needed later. Each offer carries a derived `uberPrice` { amountCents, currency } + `uberPriceBasis` for the product-level comparison (ADR-0022): ESTIMATED = UberEstimationPolicy[restaurant.cuisine_category].price_coefficient × offer price (null when the restaurant has no cuisine_category); REAL = the restaurant's own Uber price when uber_prices_opt_in and a HubRise Uber menu is present (ingestion deferred — runtime). Always labelled.

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `catalog_id` | `CatalogId` | `UUID` | PK |  |
| `restaurant_id` | `RestaurantId` | `UUID` | index |  |
| `slug` | `Slug` | `TEXT` | — | ⚠️ HOLE: CatalogCreated carries no slug — nothing populates this column (drop it or add slug to the event). |
| `name` | `CatalogName` | `TEXT` | — |  |
| `tree` | `jsonb` | `JSONB` | — | Assembled tree: categories -> products -> offers { price_cents, currency, availability, stock_status, uberPrice?, uberPriceBasis? } + option lists. See rules for how uberPrice is derived (ADR-0022/0024). |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `Cart` · 🛶 V0 · source aggregate `Cart`

- **Fed by**: `CartStarted`, `CartLineAdded`, `CartLineQuantityChanged`, `CartLineRemoved`, `CartCheckedOut`, `CartBoundToCustomer`
- **Rules**: Prices are computed by the projection from the current catalog, never trusted from the client. `customer_id` is NULL while the cart is owned by a guest; bound when CartBindingProcess reacts to CustomerIdentified by sending BindCartToCustomer to each OPEN cart of the session (same-stream CartBoundToCustomer), or at checkout. `estimated_breakdown` applies PricingPolicy (fee_rate/buyer_share/margin band) + the restaurant's margin_rate to the food total: serviceFee_buyer = buyer_share·fee_rate·articles; restaurantContribution = (1−buyer_share)·clamp((margin−margin_low)/(margin_high−margin_low),0,1)·fee_rate·articles; total = articles + delivery + serviceFee_buyer. Recomputed authoritatively on OrderPlaced.breakdown. `uber_comparison` is the UberComparison (ADR-0022/0025), COMPUTED by the projection from the cart food total + UberEstimationPolicy[restaurant.cuisine_category] + UberSplitPolicy. Null when the restaurant has no cuisine_category. Basis ESTIMATED in V0 (REAL when opted-in + HubRise Uber prices — deferred).
- **Note**: Joined with the catalog for pricing (secondary source).

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `cart_id` | `CartId` | `UUID` | PK |  |
| `restaurant_id` | `RestaurantId` | `UUID` | — |  |
| `session_id` | `SessionId` | `UUID` | index | The visitor session that started the cart; CartBindingProcess binds all OPEN carts of a session on CustomerIdentified. |
| `customer_id` | `CustomerId` | `UUID` | nullable | NULL while guest; bound by CartBoundToCustomer (CartBindingProcess sends BindCartToCustomer per open cart of the identified session) or at checkout. |
| `status` | `CartStatus` | `INTEGER` | — | Derived from event type: OPEN on CartStarted, CHECKED_OUT on CartCheckedOut. |
| `lines` | `jsonb` | `JSONB` | — | Priced by the projection from the live catalog: [{ cart_line_id, offer_id, product_id, name, offer_name, quantity, unit_price_cents, selected_options, line_total_cents }]. |
| `total_amount_cents` | `MoneyCents` | `BIGINT` | — | COMPUTED by the projection from the live catalog (never trusted from the client). |
| `currency` | `CurrencyCode` | `TEXT` | — | From the catalog currency at pricing time (the restaurant's default_currency). |
| `estimated_breakdown` | `jsonb` | `JSONB` | nullable | ESTIMATED PaymentBreakdown for the checkout display (ADR-0018), COMPUTED by the projection from the cart food total + PricingPolicy + the restaurant margin_rate. Same shape as OrderPlaced.breakdown; recomputed on the final order. |
| `uber_comparison` | `jsonb` | `JSONB` | nullable | UberComparison for the cart-level comparison (ADR-0022/0025), COMPUTED by the projection (see rules). Null when the restaurant has no cuisine_category. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `OrderTracking` · 🛶 V0 · source aggregate `Order`

- **Fed by**: `OrderPlaced`, `OrderAcceptedByRestaurant`, `OrderPreparationStarted`, `OrderMarkedReady`, `OrderDelivered`, `OrderRejectedByRestaurant`, `OrderCancelledByCustomer`, `OrderCancelledByRestaurant`, `PaymentCaptured`, `PaymentRefunded`, `OrderRated`, `RestaurantRated`, `DeliverySatisfactionRecorded`, `OrderTipped`, `DeliveryAcceptedByPartner`, `DeliveryAcceptedByRider`, `DeliveryStatusUpdated`, `DeliveryCompleted`, `DeliveryDispatchFailed`
- **Rules**: `payment_status` is folded from the Stripe payment facts. `delivery_status`/`courier`/`estimated_dropoff_at` mirror the order's DeliveryJob (correlated by order_id) so the customer's order view shows live delivery progress (ADR-0031); the full operational board is View_DeliveryJob. Rating columns are populated from OrderRated (rider_thumb), RestaurantRated (restaurant_stars + comment); null until the customer acts. The restaurant reads restaurant_stars/comment to see its rating. `delivery_timeliness` is the customer's post-delivery delay verdict (DeliverySatisfactionRecorded; #62); null until answered — the client hides the survey once set. The restaurant-facing aggregate is View_DeliverySatisfaction. `*_tip_cents` sum OrderTipped.tips by recipient (customer AND restaurant tippers combined; ADR-012); separate from the core split, Captain 0% skim; feed per-recipient Open-Collective totals. `uber_*` columns are the estimated Uber Eats comparison for the pedagogical receipt (ADR-0025), COMPUTED by the projection from breakdown.articles + the restaurant's cuisine_category → UberEstimationPolicy.price_coefficient + UberSplitPolicy. uber_total = coefficient·articles + avg_delivery_fee + platform fee; uber_restaurant = coefficient·articles·(1−uber_commission_pct/100); uber_rider ≈ rider_base_cents (per-km omitted, distance not modelled); uber_platform = uber_total − uber_restaurant − uber_rider. All null when the restaurant has no cuisine_category. uber_basis is ESTIMATED in V0 (REAL when opted-in + HubRise Uber prices — deferred). Contrast against the exact Captain split (restaurant_payout/rider_payout/captain_net).
- **Note**: The single canonical Order read model. Folds the Order lifecycle + Stripe payment facts (secondary source). Serves every order query — by id (`order`), by customer (history) and by restaurant+status (back-office queue) — via the indexes below; there is no separate per-persona order projection.

- **Indexes**: `(restaurant_id, status, placed_at)`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `order_id` | `OrderId` | `UUID` | PK |  |
| `ref` | `ExternalReference` | `TEXT` | — |  |
| `restaurant_id` | `RestaurantId` | `UUID` | — |  |
| `customer_id` | `CustomerId` | `UUID` | index, nullable |  |
| `status` | `OrderStatus` | `INTEGER` | — | Derived from the lifecycle event type. |
| `service_type` | `ServiceType` | `INTEGER` | — |  |
| `items` | `jsonb` | `JSONB` | — |  |
| `total_amount_cents` | `MoneyCents` | `BIGINT` | — | amountCents of OrderPlaced.totalAmount (Money). |
| `currency` | `CurrencyCode` | `TEXT` | — | currency of OrderPlaced.totalAmount (Money). |
| `articles_cents` | `MoneyCents` | `BIGINT` | — | breakdown.articles.amountCents (food TTC; ADR-0016/0018). |
| `delivery_cents` | `MoneyCents` | `BIGINT` | — | breakdown.delivery.amountCents (→ rider; 0 for collection). |
| `service_fee_cents` | `MoneyCents` | `BIGINT` | — | breakdown.serviceFee.amountCents (Captain buyer service fee). |
| `restaurant_payout_cents` | `MoneyCents` | `BIGINT` | — | breakdown.restaurantPayout.amountCents (3-way split → restaurant). |
| `rider_payout_cents` | `MoneyCents` | `BIGINT` | — | breakdown.riderPayout.amountCents (3-way split → rider). |
| `captain_net_cents` | `MoneyCents` | `BIGINT` | — | breakdown.captainNet.amountCents (kept by Captain; feeds Open-Collective totals). |
| `uber_total_cents` | `MoneyCents` | `BIGINT` | nullable | DERIVED estimated Uber Eats all-in total for the same order (ADR-0025; see rules). Null if no cuisine_category. |
| `uber_restaurant_cents` | `MoneyCents` | `BIGINT` | nullable | DERIVED estimated Uber restaurant net (after ~30% commission; see rules). |
| `uber_rider_cents` | `MoneyCents` | `BIGINT` | nullable | DERIVED estimated Uber courier earning (base; per-km not modelled in V0; see rules). |
| `uber_platform_cents` | `MoneyCents` | `BIGINT` | nullable | DERIVED estimated Uber platform take = uber_total − uber_restaurant − uber_rider. |
| `uber_basis` | `ComparisonBasis` | `INTEGER` | nullable | ESTIMATED (V0) or REAL (opted-in + HubRise Uber prices; deferred). Null if no comparison. |
| `delivery_address` | `jsonb` | `JSONB` | nullable |  |
| `estimated_ready_at` | `timestamptz` | `TIMESTAMPTZ` | nullable |  |
| `placed_at` | `timestamptz` | `TIMESTAMPTZ` | — | OrderPlaced occurrence time. |
| `status_changed_at` | `timestamptz` | `TIMESTAMPTZ` | — | Occurrence time of the latest status-changing event. |
| `payment_intent_id` | `PaymentIntentId` | `TEXT` | nullable | The captured Stripe PaymentIntent; RefundProcess reads it to open a pending refund. |
| `payment_status` | `text` | `TEXT` | — | Folded from Stripe facts; candidate for a PaymentStatus enum. OrderPlaced seeds CAPTURED: PlaceOrderProcess emits it only in reaction to PaymentCaptured (V0 prepaid-online), and that capture sits earlier in the log than the row it would fold into.
 |
| `restaurant_stars` | `StarRating` | `INTEGER` | nullable | Customer's 0–5 rating of the restaurant; null until rated. |
| `rating_comment` | `RatingComment` | `TEXT` | nullable |  |
| `rider_thumb` | `ThumbRating` | `INTEGER` | nullable |  |
| `delivery_timeliness` | `DeliveryTimeliness` | `INTEGER` | nullable | Customer's post-delivery delay verdict (#62); null until answered. |
| `rider_tip_cents` | `MoneyCents` | `BIGINT` | nullable | Σ OrderTipped.tips[recipient==RIDER].amount (all tippers); null if none. |
| `restaurant_tip_cents` | `MoneyCents` | `BIGINT` | nullable | Σ OrderTipped.tips[recipient==RESTAURANT].amount; null if none. |
| `captain_tip_cents` | `MoneyCents` | `BIGINT` | nullable | Σ OrderTipped.tips[recipient==CAPTAIN].amount; null if none. |
| `rated_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | Occurrence time of the latest rating/tip/survey event. |
| `delivery_status` | `DeliveryStatus` | `INTEGER` | nullable | Mirror of the order's DeliveryJob status (correlated by order_id); null for COLLECTION / before dispatch. DeliveryDispatchFailed (offer cap exhausted) mirrors FAILED (ADR-20260720-004556). |
| `courier` | `jsonb` | `JSONB` | nullable | Assigned Courier { displayName, phone?, riderId? } once accepted; null before. |
| `estimated_dropoff_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | Partner-reported ETA to the customer; null when unknown. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

<!-- GENERATED:views END -->
