# ADR-20260804-041227 — Populate the two dead read-model columns, and address the refund facts by payment intent

## Status

Accepted

## Context

An audit of the 31 standing validator warnings found that **none were lint noise** — each was an unbuilt
feature, a tracked deferral, or a real hole. Five were actionable, in two groups:

- **`view-column-no-source` ×2** — `Restaurant.description` and `Catalog.slug` were columns no event fed.
  Both were annotated `⚠️ HOLE` in the spec, materialized in hand-written Rust, and **exposed through
  GraphQL**: `Catalog.slug` is declared NON-NULL over a column the projector could only ever fill with
  the empty string, so the API promised a value it never had.
- **`id-not-in-payload` ×3 (Payment)** — `RefundOpened`, `RefundApproved` and `RefundDenied` did not carry
  `paymentIntentId`, yet actors.yaml delivers all three **as messages to the `Payment` aggregate, whose
  identity is `paymentIntentId`**. The facts were addressable only because `View_PendingRefunds` folds by
  `stream_name`; nothing in the payload said which payment they belonged to.

The product owner directed both groups be fixed by **populating**, not by dropping the columns.

## Decision

**Give the two columns a real source.** `RestaurantUpdated` (the "editable LOCATION fields" event) and
`UpdateRestaurant` gain a nullable `description`, typed by a new dedicated `RestaurantDescription`
scalar rather than a bare `string` or a reuse of `ProductDescription` (one name = one scalar).
`CatalogCreated` and `CreateCatalog` gain a **required** `slug`, which is safe because `CatalogCreated`
is emitted only by `CreateCatalog` — the HubRise import path emits `CatalogImported` and never this event.

**Carry the payment identity on the refund facts.** All three refund events gain a required
`paymentIntentId`, and the `RefundProcess` legs supply it: `from_read: order.payment_intent_id` on the
four opening legs (which already read `OrderTracking`), and `from_state: payment_intent_id` on the two
decision legs (which already load the run's state row).

**Tighten `refund_process_manager.payment_intent_id` to NOT NULL.** This fell out of the above: the
generator refused a read field typed both `PaymentIntentId` (the new required event property) and
`Option<PaymentIntentId>` (the nullable state column). The column *cannot* be null for a run that
exists — every leg that opens one guards on `payment_status = CAPTURED` and skips otherwise, and the
column it reads is fed by `PaymentCaptured`.

## Alternatives considered

- **Drop the two columns and their API fields instead of populating them** — the smaller change, and this
  session's recommendation, since the spec note itself said "drop it or add slug to the event". Rejected
  by the product owner: a restaurant description is a real storefront feature and a catalog slug is a
  real route segment; deleting them would have to be undone.
- **Make `paymentIntentId` nullable on the refund events** — would have silenced the warning without the
  PM-wiring and NOT NULL consequences. Rejected: a nullable identity property cannot address an actor,
  which is the entire point of carrying it.
- **Leave the nullable state column and unwrap at the boundary** — rejected in favour of making the
  invariant structural. The one place the nullable *source* column is genuinely narrowed is
  `read_order`, which now skips an order with no payment intent, next to the existing "nothing captured
  to refund" skip.

## Consequences

### Positive
- **Warnings 31 → 26**, no new kind. `Catalog.slug` stops being a non-null GraphQL field over an empty
  string.
- Two hand-written projector shims **deleted**, not rewritten: `CatalogCompute::slug` and
  `RestaurantCompute::description` existed only to preserve values nothing produced. The generator now
  maps both columns from event lineage.
- One runtime gate **deleted because the compiler subsumes it** (ADR-20260803-234035): the
  `RefundNotPending` rejection in `input_payment_refund` guarded a `None` that the NOT NULL column makes
  unspellable.
- `slugify` moved from `infrastructure::integrations::sirene` to **`domain::shared::text`**. It had no
  callers outside its own tests and encodes a domain rule (the `Slug` pattern); the HubRise catalog
  import is its second consumer, and the ACL's stated "no dependency on `infrastructure` for its logic"
  survives. It is re-exported from `sirene` so that path keeps resolving.

### Negative
- Three business event payloads changed. The events are pre-production, so no migration is written, but
  any `domain_events` row already recorded under the old shape lacks `paymentIntentId` / `slug`.
- `refund_process_manager.payment_intent_id` going NOT NULL is a schema tightening on a PM state table;
  the generated DDL changes and an existing row with a NULL would block it.
- HubRise-imported catalogs now get a slug derived from the imported name, falling back to
  `catalog-{uuid}` when the name slugifies to nothing. That is a derived value crossing the ACL, so a
  HubRise rename does **not** move the slug (the id is stable; the slug is set once at creation).
- `RestaurantState` deliberately does **not** fold `description`, which makes its "exactly the fields
  `RestaurantUpdated` can carry" invariant no longer literally true. The comment now states the
  exception and why: no registry reports a description, so there is nothing to compare against.

### Follow-up actions
- No command sets a restaurant description through a screen yet — `UpdateRestaurant` carries the field,
  but the back-office form does not expose it. Wiring the input is a screen change, not covered here.
- The remaining 26 warnings are unbuilt features (delivery/rider ×18, credit/cart/replacement ×6),
  [#341](https://github.com/TheCaptainCompany/captain-food/issues/341) (the listing opt-out that does
  nothing — the `view-fedby-unused` symptom), and one `identity-property-not-on-command` that is correct
  as-is (the server legitimately mints the id for `RequestPhoneVerification`).
