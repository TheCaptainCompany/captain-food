## 1. Event store

The store DDL — the `domain_events` log (+ indexes), the `ce_events` / `et_events` / `all_events` helper
functions, the `domain_stream` retention table and the `$maxCount` trigger — is **GENERATED** to
[`specs/generated/schema.generated.sql`](generated/schema.generated.sql) from
[`database/tables/`](database/tables/) and [`database/functions/`](database/functions/) (run
`make generate`). Enum-typed columns hold the `scalars.yaml` TEXT value verbatim (ADR-20260728 — no
`ref_<enum>` lookup tables). **This section is the
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
  (matches the aggregates in `actors.yaml`); the `<id>` suffix is the instance id.
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

### The write-path journal & adapter staging (ADR-20260731-122500)

Two table categories sit BESIDE the event store, never inside it:

- **`inbound_messages`** ([`tables/journals.yaml`](database/tables/journals.yaml)) — THE ACTOR
  MAILBOX (ADR-20260731-122500 "the mailbox is the only door"), and since #242 Runtime D the
  **only** write-path journal. One row per message to one actor, whatever the channel: a GraphQL
  or worker command (kind `COMMAND`, persisted **before** handling — pk `message_id` is the
  write-path idempotency key, same payload hash = replayed acceptance, different = Conflict, and
  the row records **rejections too**, backing the `operationStatus` query/subscription), an
  adapted inbound business event enqueued by an adapter ACL (kind `EVENT`, events.yaml vocabulary
  only), or a reminder (kind `MESSAGE`). The partitioned workers deliver every one of them
  through the fenced completion transaction, so events appended by a delivery carry
  `message_id` as `domain_events.cause_id`, chaining request → mailbox → facts. Both
  predecessors are gone: `inbound_events` backfilled and dropped by the `20260731143000`
  migration, `command_journal` dropped by `20260812000000`.
- **`external_*` staging** ([`tables/integration_staging.yaml`](database/tables/integration_staging.yaml),
  ADR-0045 generalized) — adapter-OWNED verbatim mirrors (`external_sirene_restaurants`,
  `external_stripe_events`, `external_hubrise_callbacks`): verify → UPSERT → ACK, with
  `processed_at` as the translation high-water mark for replay/backfill.

Journals **never write `domain_events`** and are **never replayed as state** — the event log stays
the single source of truth. None of these are projected or a GraphQL `reads` target.

### Journal & mirror retention — `sweep_retention()` (ADR-20260721-025159)

Unlike the forever event log, journals and webhook mirrors have a **usefulness window**. The
windows live in **one place** — the [`sweep_retention()`](database/functions/sweep_retention.sql)
function (part of the generated schema): `inbound_messages` terminal rows 90 days from
`completed_at`; the webhook mirrors' processed rows 90 days from `processed_at`
(also the PII cap on verbatim third-party payloads). **Never swept**: `domain_events` /
`domain_stream` (this function does not reference them — the log's only trimming stays the
opt-in `$maxAge`/`$maxCount` above), `RECEIVED` mailbox rows (pending work — the mailbox's own
attempt cap flips a wedged delivery to `FAILED` first), `SCHEDULED` mailbox rows
(future work), unprocessed mirror
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
intrinsic to the read model: its **source aggregate + events** (`events.yaml` /
`actors.yaml`), its **business filters/rules**, and its **columns**. The consumer mapping
— which GraphQL query reads it — is declared in `api.yaml` via `@reads`
(rendered in the generated documentation). Money is stored as integer minor units (`*_cents`
+ `currency`), matching `Money`; `JSONB` is used where a whole sub-tree is fetched at once. The SQL
DDL for these tables is generated to `specs/generated/views.generated.sql`.

<!-- GENERATED:views START — source: specs/database/projection_views.yaml; run `make generate`. Do not edit between the markers. -->

### `View_DeliveryJob` · 🛶 V0 · source aggregate `DeliveryJob`

- **Fed by**: `DeliveryRequested`, `DeliveryAcceptedByPartner`, `DeliveryRejectedByPartner`, `DeliveryStatusUpdated`, `DeliveryAcceptedByRider`, `DeliveryPickedUp`, `DeliveryCompleted`, `DeliveryCancelled`, `DeliveryDispatchFailed`
- **Rules**: `status` is derived from the lifecycle events: PENDING on DeliveryRequested → ASSIGNED on DeliveryAcceptedByRider/DeliveryAcceptedByPartner → PICKED_UP on DeliveryPickedUp → then partner DeliveryStatusUpdated (OUT_FOR_DELIVERY/DELIVERED/FAILED) or DeliveryCompleted (DELIVERED) / DeliveryCancelled (CANCELLED) / DeliveryDispatchFailed (FAILED — offer cap exhausted, ADR-20260720-004556). `provider` is INDEPENDENT once a rider accepts, PARTNER once a partner accepts.
- **Indexes**: `(restaurant_id, status)`, `(rider_id, status)`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `delivery_job_id` | `DeliveryJobId` | `UUID` | PK |  |
| `order_id` | `OrderId` | `UUID` | index |  |
| `restaurant_id` | `RestaurantId` | `UUID` | — |  |
| `status` | `DeliveryStatus` | `TEXT` | — | Derived from the lifecycle event type / DeliveryStatusUpdated.status (DeliveryDispatchFailed → FAILED, the offer-cap exhaustion). |
| `provider` | `DeliveryProvider` | `TEXT` | nullable | INDEPENDENT (rider accepted) or PARTNER (partner accepted); null while PENDING. |
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
| `timeliness` | `DeliveryTimeliness` | `TEXT` | — |  |
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
| `status` | `CityAvailabilityStatus` | `TEXT` | — | Derived from the latest lifecycle event type. |
| `requested_at` | `timestamptz` | `TIMESTAMPTZ` | — | occurrence: max(occurred_at) of the birth fact. |
| `decided_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | occurrence: max(occurred_at) of the latest decision (approve/revoke); null while PENDING. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `View_Reclamation` · 🔭 V1 · source aggregate `Reclamation`

- **Fed by**: `ReclamationOpened`, `ReclamationResolved`, `ReclamationRejected`, `ReclamationReopened`
- **Rules**: `status` is derived from the latest lifecycle event type: OPEN on Opened → RESOLVED on Resolved / REJECTED on Rejected → OPEN again on Reopened. Set-once identity fields (order_id, customer_id, restaurant_id, category, description, requested_resolution) are carried only by the ReclamationOpened birth fact. The decision fields (resolution, refund_amount_cents/currency, reject_reason, decided_at) fill in on the ReclamationResolved / ReclamationRejected fact and are null while OPEN.
- **Indexes**: `(customer_id, status)`, `(restaurant_id, status)`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `reclamation_id` | `ReclamationId` | `UUID` | PK |  |
| `order_id` | `OrderId` | `UUID` | index |  |
| `customer_id` | `CustomerId` | `UUID` | index |  |
| `restaurant_id` | `RestaurantId` | `UUID` | index |  |
| `category` | `ReclamationCategory` | `TEXT` | — |  |
| `description` | `ReclamationDescription` | `TEXT` | — |  |
| `requested_resolution` | `ReclamationResolution` | `TEXT` | nullable | The resolution the customer asked for at open time, if any. |
| `status` | `ReclamationStatus` | `TEXT` | — | Derived from the latest lifecycle event type. |
| `resolution` | `ReclamationResolution` | `TEXT` | nullable | The decided resolution once resolved; null while OPEN or if rejected. |
| `refund_amount_cents` | `MoneyCents` | `BIGINT` | nullable | amountCents of ReclamationResolved.refundAmount (Money — a PARTIAL_REFUND amount); null otherwise. |
| `currency` | `CurrencyCode` | `TEXT` | nullable | currency of ReclamationResolved.refundAmount (Money); null unless a refund amount was recorded. |
| `reject_reason` | `ReclamationReason` | `TEXT` | nullable | The reason recorded on rejection; null unless rejected. |
| `opened_at` | `timestamptz` | `TIMESTAMPTZ` | — | occurrence: max(occurred_at) of the birth fact. |
| `decided_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | occurrence: max(occurred_at) of the latest decision (resolve/reject); null while OPEN. |
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
| `status` | `RefundStatus` | `TEXT` | — | Derived from the latest lifecycle event type. |
| `amount_cents` | `MoneyCents` | `BIGINT` | — | amountCents of RefundOpened.amount (Money) — the captured total eligible for refund. |
| `currency` | `CurrencyCode` | `TEXT` | — | currency of RefundOpened.amount (Money). |
| `approved_amount_cents` | `MoneyCents` | `BIGINT` | nullable | amountCents of RefundApproved.amount (Money — may be partial); null until approved. |
| `reason` | `text` | `TEXT` | nullable | The latest recorded reason: the opening fact's, then the decision's. |
| `refund_id` | `RefundId` | `TEXT` | nullable | The Stripe Refund id once settled; null before PaymentRefunded. |
| `requested_at` | `timestamptz` | `TIMESTAMPTZ` | — | RefundOpened occurrence time. |
| `decided_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | The decision's occurrence time (approval or denial); null while REQUESTED. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `View_CustomerErasure` · 🔭 V1 · source aggregate `Customer`

- **Fed by**: `CustomerErasureRequested`, `CustomerErasureConfirmed`, `CustomerErasureCancelled`, `CustomerErasureDue`, `CustomerIdentityUnlinked`, `CustomerErased`
- **Rules**: `status` is derived from the latest recorded fact: REQUESTED -> CONFIRMED -> EXECUTING (the first destructive leg reported) -> ERASED. A CustomerErasureCancelled stamps cancelled_at and ends the row's life as a request; nothing further folds onto it. PARKED is ABSENT on purpose: parking is process-row state — an internal scheduling fact about OUR execution, never a recorded event — and the subject is owed the state of their RIGHT, not of our scheduler. The row-state enum that carries it lands with the orchestrator. Every column is pseudonymous. The row must survive the deletion of the Customer stream, which is exactly why it may never carry personal data.
- **Indexes**: `(customer_id, status)`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `customer_id` | `CustomerId` | `UUID` | PK | The accountability reference the query scopes on — resolved from the authenticated principal, never from a client argument. |
| `erasure_request_id` | `ErasureRequestId` | `UUID` | — | The subject's pseudonymous reference for the right they exercised; quotable back to us after everything else is gone. |
| `status` | `ErasureStatus` | `TEXT` | — | Derived from the latest fact. The grace window elapsing is what turns the answer to EXECUTING — the subject can no longer cancel from that moment, so telling them otherwise would be false. The unlink leg keeps it there. |
| `cancelled_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | occurrence: when the subject withdrew the request inside the window. A separate TIMESTAMP rather than a member of ErasureStatus: a cancelled request is not a phase of an erasure, it is the absence of one — and the moment they changed their mind is the fact worth keeping, not merely that they did.
 |
| `policy` | `text` | `TEXT` | nullable | The window the erasure ran under; null until the receipt exists. |
| `retained_under` | `jsonb` | `JSONB` | nullable | The approved retention windows under which data about this subject survives, by catalog name — the disclosure limb of §3.6, generated from the same declarations that enforce it, so the screen cannot drift from the behaviour. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `Restaurant` · 🛶 V0 · source aggregate `Restaurant`

- **Fed by**: `RestaurantRegistered`, `RestaurantUpdated`, `RestaurantActivated`, `RestaurantDeactivated`, `RestaurantAcceptanceModeChanged`, `RestaurantRemoved`, `RestaurantGoogleBusinessProfileUpdated`, `RestaurantListingClaimed`, `RestaurantListingOptedOut`, `RestaurantMarkedClosed`, `RestaurantListingStatusChanged`, `RestaurantGoogleBusinessProfileOrderLinkConfigured`, `RestaurantGoogleBusinessProfileOrderLinkVerified`, `RestaurantSlugConfigured`, `RestaurantSlugReconfigured`, `RestaurantAccountRegistered`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `restaurant_id` | `RestaurantId` | `UUID` | PK |  |
| `restaurant_account_id` | `RestaurantAccountId` | `UUID` | index, nullable | NULL for a non-partner public listing; set on claim/conversion. |
| `listing_status` | `RestaurantListingStatus` | `TEXT` | index |  |
| `external_identifiers` | `jsonb` | `JSONB` | nullable | Source-agnostic [{key,value}] (siret/naf/google_place_id…); not unique. |
| `google_place_id` | `GooglePlaceId` | `TEXT` | nullable |  |
| `slug` | `Slug` | `TEXT` | unique, nullable |  |
| `display_name` | `RestaurantDisplayName` | `TEXT` | — |  |
| `description` | `RestaurantDescription` | `TEXT` | nullable |  |
| `tags` | `jsonb` | `JSONB` | nullable | Cuisine/attribute tags — general restaurant info (source-agnostic), not from the GBP event. |
| `margin_rate` | `MarginPercent` | `TEXT` | nullable | Food margin %, input to the Captain service-fee split (ADR-0017); back-office only. |
| `cuisine_category` | `CuisineCategory` | `TEXT` | nullable | Selects the Uber Eats price-estimate coefficient in UberEstimationPolicy (ADR-0024). |
| `uber_prices_opt_in` | `boolean` | `BOOLEAN` | nullable | Restaurant authorized showing its real Uber prices via HubRise (ADR-0023). Gates REAL vs ESTIMATED basis. |
| `website` | `WebUrl` | `TEXT` | nullable |  |
| `rating` | `GoogleRating` | `TEXT` | nullable | GBP-specific metric (Google listing), independent of the restaurant's own info. |
| `reviews_count` | `integer` | `INTEGER` | nullable |  |
| `gbp_order_url` | `WebUrl` | `TEXT` | nullable |  |
| `gbp_link_status` | `GbpLinkStatus` | `TEXT` | nullable |  |
| `address` | `jsonb` | `JSONB` | — |  |
| `location` | `jsonb` | `JSONB` | nullable | Geo coordinates {latitude, longitude}; typically from the Google Maps sync. |
| `opening_hours` | `jsonb` | `JSONB` | — |  |
| `status` | `RestaurantStatus` | `TEXT` | — | Derived from the lifecycle event type: DRAFT on register, ACTIVE/INACTIVE on (de)activation, INACTIVE on closure. |
| `order_acceptance` | `OrderAcceptanceMode` | `TEXT` | — |  |
| `default_currency` | `CurrencyCode` | `TEXT` | — |  |
| `timezone` | `TimeZone` | `TEXT` | nullable | Location timezone. NULL means NO timezone — the account-level fallback this note used to claim has NO materialized source (View_RestaurantAccount was deleted; corrected under RSO-1, DECISIONS §43): service-window evaluation treats NULL, or a zone that does not parse while hours are declared, as HOURS_UNDECLARED, never as "closed".
 |
| `preparation_time_minutes` | `integer` | `INTEGER` | nullable |  |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `SlugAlias` · 🛶 V0 · source aggregate `Restaurant`

- **Fed by**: `RestaurantSlugReconfigured`
- **Rules**: One row per superseded label, keyed by that label. A restaurant renamed N times leaves N rows -- each recording the address that superseded it AT THAT MOMENT, which is a historical fact and therefore never stale. `current_slug` is NOT how a redirect is resolved: after A->B->C, row A still says B. `hosts.rs` resolves the alias to `restaurant_id` and reads that restaurant's CURRENT slug from the Restaurant projection, so every superseded label lands on the live address in ONE hop rather than walking a 301 chain. Rows are never deleted: the reservation table bars reuse of a released label, so an alias can never start pointing at a different business.
- **Note**: Superseded storefront labels, so a renamed restaurant's OLD host keeps resolving (ADR-20260728-011344). `hosts.rs` resolves an incoming Host header against `Restaurant.slug` first and falls back here, answering 301 -> the current address. Without this, a rename instantly 404s every printed menu, QR code, inbound link and search result pointing at the old label -- which is why `RestaurantSlugReconfigured` carries `previousSlug` as business data rather than leaving it to be re-derived by folding history. Host resolution runs on EVERY request and must never fold.


| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `previous_slug` | `Slug` | `TEXT` | PK |  |
| `restaurant_id` | `RestaurantId` | `UUID` | index |  |
| `current_slug` | `Slug` | `TEXT` | — | The address that superseded `previous_slug` at the time of the rename. Historical, not authoritative -- see the rules above: resolution goes through `restaurant_id`. |
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
| `pipeline_status` | `ProspectPipelineStatus` | `TEXT` | index | Derived from the prospect events + listingStatus (see rules). |
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
| `auth_ref` | `AuthSubject` | `TEXT` | index, nullable | Auth provider user id (Supabase Auth) → Customer. |
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

### `Rider` · 🛶 V0 · 🔒 internal · source aggregate `Rider`

- **Consumed by**: command handlers / auth resolution (no GraphQL query).
- **Fed by**: `RiderRegistered`, `RiderInfoUpdated`, `RiderStatusChanged`
- **Rules**: This table answers WHO this connection is. It must never become the anchor for WHAT that connection may see -- the distinction the mapping exists to preserve. Concretely: the resolver reads `auth_ref -> rider_id` and NOTHING else. `status` sits in the same row and will tempt a `SUSPENDED => deny` check onto the auth path; that check belongs in the handler folding the `Rider-{id}` stream, which owns the transition table (delivery/actors.yaml) and reads it anyway. A read model is not an authorization oracle, and revocation is separately recorded as unrepresentable today -- there is no unbinding fact for anyone in the model (ADR-20260818-094500 finding 5). `auth_ref` is UNIQUE, not merely indexed, and the difference is a security property rather than a performance one: the repository lookup is `fetch_optional`, which on multiplicity returns an ARBITRARY row -- plan-dependent, no error -- and `ScopeMembership` keys its grants on `member_id = rider_id`, so a duplicate would hand one rider another rider's order scope. The constraint converts a silent breach into a visible denial. It does NOT create the invariant: nothing on the write side prevents two `RiderRegistered` with the same `authRef` and different `riderId`s (`register_rider` guards `riderId` existence only), and the write-side reservation that would is designed but unbuilt -- see the Rider sign-in door, tracked on #639 part C. Uniqueness over a POPULATION is not an aggregate's to enforce. `phone` carries NO unique constraint and NO index, deliberately, and this is where copying Customer would inject a defect: Customer.phone is UNIQUE because it is that aggregate's identity key, whereas a rider is keyed by `authRef` precisely so the phone never becomes a domain key. French mobile numbers are recycled, so a unique phone here is a scheduled future projector fault on a number's second owner. Rebuild by RESETTING THE CHECKPOINT, never by TRUNCATE. The fold is an upsert keyed on `rider_id` and `RiderRegistered` is the only creating arm, so a from-zero replay rewrites every row in place and no rider is ever denied mid-rebuild. TRUNCATE + replay does the opposite: every rider fails closed to Public for the length of the drain, and the fleet cannot sign in. ERASURE, named here so the sweep cannot miss an app-projected table the generated dispatch skips (the ScopeMembership precedent): `display_name` and `phone` are a natural person's data held OUTSIDE the stream, and the deletion engine deletes streams, not projection rows. Delivery declares no `deletion:` block for Rider today, so there is nothing to build yet -- but a rider tombstone fold is OWED the moment one is declared.
- **Note**: The rider identity read model: the auth subject -> riderId mapping the authenticated request path resolves against (ADR-20260818-004646 -- no business identifier lives in the identity provider, so the mapping resolves in OUR Postgres), plus the rider's profile and availability. `RiderRegistered` has always carried `authRef` as required; until #639 nothing projected it, so `auth_ref` occurred exactly once in the whole projection set, on Customer, and the RIDER role had no sign-in-capable identity at all. A TABLE and not a `View_*` for the reason SlugAlias states three declarations up: this is read on EVERY authenticated request and must never fold on read -- peak is Friday/Saturday 19:00-21:30. Recovery is REPLAY, so there is no backup of it and there must not be one.


| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `rider_id` | `RiderId` | `UUID` | PK |  |
| `auth_ref` | `AuthSubject` | `TEXT` | unique | Auth provider user id (Supabase Auth) -> Rider. The lookup key of the identity bridge; see the `auth_ref` rule above for why the constraint is UNIQUE and what it does and does not guarantee. |
| `display_name` | `text` | `TEXT` | — | Rider's display name; RiderInfoUpdated overwrites it only when it carries one. |
| `phone` | `PhoneNumber` | `TEXT` | — | Contact number, a profile attribute and never a lookup key -- see the `phone` rule above. NOT NULL for the same partial-update reason as display_name. |
| `status` | `RiderStatus` | `TEXT` | — | Availability/lifecycle status, straight off the payload of whichever event wrote last. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `Catalog` · 🛶 V0 · source aggregate `Catalog`

- **Fed by**: `CatalogCreated`, `CatalogCategoryAdded`, `CatalogCategoryUpdated`, `CatalogCategoryRemoved`, `ProductAdded`, `ProductUpdated`, `ProductRemoved`, `OptionListAdded`, `OptionListUpdated`, `OptionListRemoved`, `OfferStockUpdated`, `CatalogImported`, `CatalogSlugConfigured`
- **Rules**: `stock_status` is derived (quantity vs lowStockThreshold); orderable = AVAILABLE and stock > 0. Could be normalized (one row per offer) if per-item querying is needed later. Each offer carries a derived `uberPrice` { amountCents, currency } + `uberPriceBasis` for the product-level comparison (ADR-0022): ESTIMATED = UberEstimationPolicy[restaurant.cuisine_category].price_coefficient × offer price (null when the restaurant has no cuisine_category); REAL = the restaurant's own Uber price when uber_prices_opt_in and a HubRise Uber menu is present (ingestion deferred — runtime). Always labelled.

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `catalog_id` | `CatalogId` | `UUID` | PK |  |
| `restaurant_id` | `RestaurantId` | `UUID` | index |  |
| `slug` | `Slug` | `TEXT` | nullable | Null until the owner configures it (ConfigureCatalogSlug) -- the unset case is first-class, not an empty string, exactly like Restaurant.slug. |
| `name` | `CatalogName` | `TEXT` | — |  |
| `tree` | `jsonb` | `JSONB` | — | Assembled tree: categories -> products -> offers { price_cents, currency, availability, stock_status, uberPrice?, uberPriceBasis? } + option lists. See rules for how uberPrice is derived (ADR-0022/0024). |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `Cart` · 🛶 V0 · source aggregate `Cart`

- **Fed by**: `CartStarted`, `CartLineAdded`, `CartLineQuantityChanged`, `CartLineRemoved`, `CartCheckedOut`, `CartBoundToCustomer`
- **Rules**: The fold is PURE and money-free: no price, currency, breakdown or comparison is stored — the read side prices the lines fresh on every read via price_cart (LIVE catalog), fail-closed on an unresolvable line (PriceUnresolvable → the honest no-price state, never a stale or client number). `customer_id` is NULL while the cart is owned by a guest; bound when CartBindingProcess reacts to CustomerIdentified by sending BindCartToCustomer to each OPEN cart of the session (same-stream CartBoundToCustomer), or at checkout. The estimated PaymentBreakdown and UberComparison shown on the cart (ADR-0018/0022/0025) are READ-TIME computations from the freshly priced food total + PricingPolicy / Uber*Policy — same formulas as before, no longer materialized. Recomputed authoritatively on OrderPlaced.breakdown.
- **Note**: MONEY-FREE fold (PROP-20260810-231500 Option B, ADR-20260810-112836): the row stores only identity, status and the REPRICING INPUTS (per line: offer_id, quantity, selected_option_ids). A replay reproduces these rows exactly — the fold reads nothing outside the event stream. Prices are computed AT READ TIME by `application::pricing::price_cart` against the live catalog (the same authority the checkout write path uses); the authoritative price freeze happens once, on PaymentIntentCreated.CheckoutSnapshot.

- **Indexes**: `(customer_id, updated_at)`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `cart_id` | `CartId` | `UUID` | PK |  |
| `restaurant_id` | `RestaurantId` | `UUID` | — |  |
| `session_id` | `SessionId` | `UUID` | index | The visitor session that started the cart; CartBindingProcess binds all OPEN carts of a session on CustomerIdentified. |
| `customer_id` | `CustomerId` | `UUID` | nullable | NULL while guest; bound by CartBoundToCustomer (CartBindingProcess sends BindCartToCustomer per open cart of the identified session) or at checkout. |
| `status` | `CartStatus` | `TEXT` | — | Derived from event type: OPEN on CartStarted, CHECKED_OUT on CartCheckedOut. |
| `lines` | `jsonb` | `JSONB` | — | MONEY-FREE repricing inputs, folded verbatim from the cart events: [{ cart_line_id, offer_id, quantity, selected_option_ids }]. NO stored price/name fields — the read side resolves names and prices from the live catalog via price_cart. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `OrderTracking` · 🛶 V0 · source aggregate `Order`

- **Fed by**: `OrderPlaced`, `OrderAcceptedByRestaurant`, `OrderPreparationStarted`, `OrderMarkedReady`, `OrderDelivered`, `OrderRejectedByRestaurant`, `OrderCancelledByCustomer`, `OrderCancelledByRestaurant`, `OrderAcceptanceTimedOut`, `PaymentCaptured`, `PaymentReleased`, `PaymentRefunded`, `OrderRated`, `RestaurantRated`, `DeliverySatisfactionRecorded`, `OrderTipped`, `DeliveryAcceptedByPartner`, `DeliveryAcceptedByRider`, `DeliveryPickedUp`, `DeliveryStatusUpdated`, `DeliveryCompleted`, `DeliveryDispatchFailed`
- **Rules**: `payment_status` is folded from the Stripe payment facts. `delivery_status`/`courier`/`estimated_dropoff_at` mirror the order's DeliveryJob (correlated by order_id) so the customer's order view shows live delivery progress (ADR-0031); the full operational board is View_DeliveryJob. Rating columns are populated from OrderRated (rider_thumb), RestaurantRated (restaurant_stars + comment); null until the customer acts. The restaurant reads restaurant_stars/comment to see its rating. `delivery_timeliness` is the customer's post-delivery delay verdict (DeliverySatisfactionRecorded; #62); null until answered — the client hides the survey once set. The restaurant-facing aggregate is View_DeliverySatisfaction. `*_tip_cents` sum OrderTipped.tips by recipient (customer AND restaurant tippers combined; ADR-012); separate from the core split, Captain 0% skim; feed per-recipient Open-Collective totals. `uber_*` columns are the estimated Uber Eats comparison for the pedagogical receipt (ADR-0025), COMPUTED by the projection from breakdown.articles + the restaurant's cuisine_category → UberEstimationPolicy.price_coefficient + UberSplitPolicy. uber_total = coefficient·articles + avg_delivery_fee + platform fee; uber_restaurant = coefficient·articles·(1−uber_commission_pct/100); uber_rider ≈ rider_base_cents (per-km omitted, distance not modelled); uber_platform = uber_total − uber_restaurant − uber_rider. All null when the restaurant has no cuisine_category. uber_basis is ESTIMATED in V0 (REAL when opted-in + HubRise Uber prices — deferred). Contrast against the exact Captain split (restaurant_payout/rider_payout/captain_net).
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
| `uber_basis` | `ComparisonBasis` | `TEXT` | nullable | ESTIMATED (V0) or REAL (opted-in + HubRise Uber prices; deferred). Null if no comparison. |
| `delivery_address` | `jsonb` | `JSONB` | nullable |  |
| `estimated_ready_at` | `timestamptz` | `TIMESTAMPTZ` | nullable |  |
| `placed_at` | `timestamptz` | `TIMESTAMPTZ` | — | OrderPlaced occurrence time. |
| `status_changed_at` | `timestamptz` | `TIMESTAMPTZ` | — | Occurrence time of the latest status-changing event. |
| `payment_intent_id` | `PaymentIntentId` | `TEXT` | nullable | The order's Stripe PaymentIntent, SEEDED by OrderPlaced (the birth fact carries it — a charging order is born with its authorized intent; a $0 replacement carries none) and re-confirmed by PaymentCaptured. PaymentSettlementProcess reads it on fulfilment to capture/release the hold, and RefundProcess reads it to open a pending refund. NOT fed by PaymentAuthorized: that fact precedes the row's birth, so the OrderPlaced seed carries it.
 |
| `payment_status` | `text` | `TEXT` | — | Folded from Stripe facts; candidate for a PaymentStatus enum. OrderPlaced seeds AUTHORIZED for a charging order (authorize-then-capture, ADR-20260808-195315 §1.2): PlaceOrderProcess emits it only in reaction to PaymentAuthorized, and that authorization sits earlier in the log than the row it would fold into ($0 replacements — no intent — keep the historical CAPTURED = "nothing owed"). PaymentCaptured flips it on fulfilment, PaymentReleased on a voided/expired hold, PaymentRefunded on settlement of a post-capture abort.
 |
| `restaurant_stars` | `StarRating` | `INTEGER` | nullable | Customer's 0–5 rating of the restaurant; null until rated. |
| `rating_comment` | `RatingComment` | `TEXT` | nullable |  |
| `rider_thumb` | `ThumbRating` | `TEXT` | nullable |  |
| `delivery_timeliness` | `DeliveryTimeliness` | `TEXT` | nullable | Customer's post-delivery delay verdict (#62); null until answered. |
| `rider_tip_cents` | `MoneyCents` | `BIGINT` | nullable | Σ OrderTipped.tips[recipient==RIDER].amount (all tippers); null if none. |
| `restaurant_tip_cents` | `MoneyCents` | `BIGINT` | nullable | Σ OrderTipped.tips[recipient==RESTAURANT].amount; null if none. |
| `captain_tip_cents` | `MoneyCents` | `BIGINT` | nullable | Σ OrderTipped.tips[recipient==CAPTAIN].amount; null if none. |
| `rated_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | Occurrence time of the latest rating/tip/survey event. |
| `delivery_status` | `DeliveryStatus` | `TEXT` | nullable | Mirror of the order's DeliveryJob status (correlated by order_id); null for COLLECTION / before dispatch. DeliveryPickedUp mirrors PICKED_UP on the rider path (the partner path reports it via DeliveryStatusUpdated); DeliveryDispatchFailed (offer cap exhausted) mirrors FAILED (ADR-20260720-004556). |
| `courier` | `jsonb` | `JSONB` | nullable | Assigned Courier { displayName, phone?, riderId? } once accepted; null before. |
| `estimated_dropoff_at` | `timestamptz` | `TIMESTAMPTZ` | nullable | Partner-reported ETA to the customer; null when unknown. |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `OrderConversation` · 🛶 V0 · source aggregate `Conversation`

- **Fed by**: `ConversationOpened`, `MessagePosted`, `MessageTranslationAdded`, `AdminInvitedToConversation`, `ParticipantMuted`, `ParticipantUnmuted`, `OrderPlaced`, `OrderAcceptedByRestaurant`, `OrderPreparationStarted`, `OrderMarkedReady`, `OrderDelivered`, `OrderRejectedByRestaurant`, `OrderCancelledByCustomer`, `OrderCancelledByRestaurant`, `ReclamationOpened`, `ReclamationResolved`, `ReclamationRejected`, `ReclamationReopened`, `ReclamationEvidenceAttached`
- **Note**: The per-order conversation read model (#129). Folds the conversation's own messages AND the order's status lifecycle events (cross-aggregate, correlated by order_id) into one timeline, so order status participates in the thread with no status copied into a message. The projector appends each MessagePosted, splitting PUBLIC (messages) from INTERNAL (internal_notes).


| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `order_id` | `OrderId` | `UUID` | PK |  |
| `restaurant_id` | `RestaurantId` | `UUID` | index |  |
| `customer_chat_enabled` | `boolean` | `BOOLEAN` | — |  |
| `status` | `OrderStatus` | `TEXT` | — | Derived from the latest order lifecycle event type (cross-aggregate fold, correlated by order_id). |
| `messages` | `jsonb` | `JSONB` | — | PUBLIC ConversationMessage[] (entities.yaml#/ConversationMessage), appended per MessagePosted (visibility=PUBLIC) by the projector; MessageTranslationAdded is folded into the targeted message's per-message `translations` array (translate once, reuse; #129). |
| `internal_notes` | `jsonb` | `JSONB` | — | INTERNAL ConversationMessage[] staff notes, appended per MessagePosted (visibility=INTERNAL) by the projector; MessageTranslationAdded is folded into the targeted note's per-message `translations` array (#129). |
| `opened_at` | `timestamptz` | `TIMESTAMPTZ` | — |  |
| `admin_invited` | `boolean` | `BOOLEAN` | — | True once an admin was pulled in by a reasoned escalation (#129). |
| `escalation_reason` | `EscalationReason` | `TEXT` | nullable | The reason recorded on the latest escalation; null until an admin is invited. |
| `muted` | `jsonb` | `JSONB` | — | current MutedParticipant[] (entities.yaml#/MutedParticipant), applied per mute/unmute by the projector. |
| `claim_events` | `jsonb` | `JSONB` | — | ClaimTimelineEntry[] (entities.yaml#/ClaimTimelineEntry) — weaves the Reclamation lifecycle into the order thread: the projector appends one entry per Reclamation* event (kind OPENED/RESOLVED/REJECTED/REOPENED/EVIDENCE_ATTACHED), keyed onto the order row by the event's orderId (cross-aggregate, correlated by order_id), so a claim's status and evidence show inline in the per-order conversation (§2.5, #155, #156). |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `CustomerCreditBalance` · 🛶 V0 · source aggregate `CustomerCredit`

- **Fed by**: `CustomerCreditGranted`, `CustomerCreditConsumed`
- **Rules**: `balance_cents` is COMPUTED by the projector as Σ granted − Σ consumed (a SUM, not a fold-view column); the row is born on the first CustomerCreditGranted (a consume before any grant never materializes a ledger, mirroring the domain fold). `currency` is set from the first grant's amount and stays fixed (a customer's ledger is single-currency — their local currency, EUR for Tours V0).
- **Note**: The per-customer store-credit BALANCE read model (#158, Part B of #207). One row per customer with a ledger; the projector keeps `balance_cents` as the running SUM over the ledger stream (`CustomerCredit-{customerId}`): += on CustomerCreditGranted, −= on CustomerCreditConsumed. Serves the `customerCredit` query (the customer sees their spendable goodwill balance) and mirrors the pure write-side fold in `crates/domain/src/customer_credit.rs`. The balance is never negative (the write side rejects an overspend, errors.yaml#/InsufficientCustomerCredit).


| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `customer_id` | `CustomerId` | `UUID` | PK |  |
| `balance_cents` | `MoneyCents` | `BIGINT` | — | Σ granted − Σ consumed (minor units), never negative. amountCents of the grant/consume Money, summed by the projector. |
| `currency` | `CurrencyCode` | `TEXT` | — | The ledger currency, from the first grant's amount (single-currency per customer). |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

### `ScopeMembership` · 🛶 V0 · 🔒 internal · source aggregate `Order`

- **Consumed by**: command handlers / auth resolution (no GraphQL query).
- **Fed by**: `OrderPlaced`, `DeliveryAcceptedByRider`, `DeliveryCancelled`, `DeliveryDispatchFailed`, `RestaurantRegistered`, `RestaurantListingClaimed`
- **Rules**: GRANT on OrderPlaced: the order's customer, its restaurant, and that restaurant's account. These are PERMANENT — order history must stay readable forever (product-owner decision, 2026-07-25). GRANT on DeliveryAcceptedByRider: the accepting rider, on the `orderId` the event carries (D-QW1 option b, ADR-20260808-234907). REVOKE on DeliveryCancelled / DeliveryDispatchFailed: the RIDER role rows for that job's order. The ONLY revoke in V0 — riders are the only membership that ends. DeliveryCancelled carries no orderId, so the worker resolves it via View_DeliveryJob before folding; an unresolvable job yields NO change (allow-stale on an orphan stream — acceptable only because a birth fact always precedes its cancel in position order). GRANT on RestaurantRegistered: the restaurant itself and its owning account, scope_type RESTAURANT. GRANT on RestaurantListingClaimed: the claiming account, scope_type RESTAURANT — the post-registration attachment path. A Sirene-seeded listing registers with NO accountId; without this fold its account would never gain membership and resolve_restaurant_account would find nothing for every subsequent OrderPlaced (review finding, ADR-20260809-160000 addendum). ADMIN holds NO rows — the guard short-circuits on the role. Storing them would mean a row per admin per instance, unbounded and pointless. membership_id is UUIDv5 over (scope_type, scope_id, member_type, member_id), so re-projecting a grant is an idempotent upsert and revoking needs no lookup — the same inputs always derive the same key (the hubrise_connections.restaurant_account_id pattern). ERASURE (#194, ADR-20260731-160000): this is an Order-fed read model holding a customer-to-order link — it OWES an OrderExpired tombstone fold (delete the order scope's rows) when the deletion engine lands. Named here so the #194 sweep cannot miss an app-projected table the generated dispatch skips.
- **Note**: WHO may see WHICH protected instance (#144). One row per (scope, member): the single index every read-side authorization question resolves against, for every role and every surface — `SELECT EXISTS(... WHERE membership_id = $1)`. The guard never learns what an order is, so a new ScopeType is a projector rule rather than new code in the guard.
Adopting this index replaced four separate mechanisms (PROP-20260725-185140 §3.4): per-role table/column resolution, the restaurant -> account hop, an `active` predicate on rider membership, and multi-rider-per-order special cases. Reassignment needs no special handling: it is a REVOKE followed by a GRANT, so the previous rider loses access the moment the new one is recorded.
SAFETY: this is an ACL cache with asymmetric failure modes. A MISSING row denies (visible, safe); a STALE row grants (a silent breach). The revoke rules are therefore more safety-critical than the grants, and the projector errs toward deleting. Drift is repairable by replay, which is the property that makes the cache acceptable at all.

- **Indexes**: `(member_type, member_id, scope_type)`, `(scope_type, scope_id)`

| Column | Type | SQL | Constraints | Notes |
| --- | --- | --- | --- | --- |
| `membership_id` | `uuid` | `UUID` | PK | UUIDv5(scope_type|scope_id|member_type|member_id) — derived, never random, so a replayed grant upserts onto itself. |
| `scope_type` | `ScopeType` | `TEXT` | — | Which KIND of instance — ORDER or RESTAURANT. Constant per grant/revoke rule, not read from a payload. The revoke events appear in this lineage because the revoke's DELETE predicate reads exactly (scope_type, scope_id, member_type) — they write no value, they key a deletion. |
| `scope_id` | `uuid` | `UUID` | — | The protected instance: an OrderId or a RestaurantId. DeliveryCancelled carries no orderId — the worker resolves the job's order via View_DeliveryJob before folding. |
| `member_type` | `UserType` | `TEXT` | — | In the key deliberately: a rider who is ALSO a customer must hold two distinct memberships, or their customer row would let them fetch rider-audience data. The revoke events key their deletion by this column (the RIDER role), which is why they appear in its lineage.
 |
| `member_id` | `uuid` | `UUID` | — | The DOMAIN id (customerId / restaurantId / restaurantAccountId / riderId), never the auth subject — the sub->domain bridge happens once per request at the edge. |
| `granted_at` | `timestamptz` | `TIMESTAMPTZ` | — | When the membership was recorded (the event's occurred_at) — deterministic under replay; preserved on a replayed grant (ON CONFLICT DO NOTHING). |
| `created_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |
| `updated_at` | `timestamptz` | `TIMESTAMPTZ` | — | technical — stamped from event.occurred_at (implicit on every read model) |

<!-- GENERATED:views END -->
