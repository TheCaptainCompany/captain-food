# ADR-20260719-014434 — Freeze the checkout snapshot on `PaymentIntentCreated`

## Status

Accepted. Realizes the checkout-snapshot half of the PlaceOrder saga (STATUS item 1a); the runtime
population of priced `items`/`breakdown` and retiring the `CheckoutSnapshotSource` port ride on the
separate server-side pricing work (see Follow-up).

## Context

`PlaceOrderProcess` reacts to the inbound Stripe fact `PaymentCaptured` by emitting `OrderPlaced`
(`Order-{id}`) + `CartCheckedOut` (`Cart-{id}`). But `events.yaml#/PaymentIntentCreated` — the saga's
first, command-initiated fact — carried only the payment handle (`paymentIntentId`, `restaurantId`,
`customerId?`, `amount`), **not** the checkout content `OrderPlaced` needs (cartId, contact, serviceType,
delivery address, priced items, breakdown). So the saga could **not reconstruct the order from the event
log alone**; it resolved the missing data through a `CheckoutSnapshotSource` seam whose fail-closed
stand-in returns `None`, making the reaction `Skip` (fail-closed — an order fact is never guessed).

Two fixes were possible (documented in `docs/sagas.md`): **(1)** enrich `PaymentIntentCreated` to carry the
frozen checkout, so the order rebuilds purely from the log (event-sourcing-pure); or **(2)** a durable
out-of-log pending-checkout store keyed by `payment_intent_id`, written at create-intent time.

## Decision

**Option 1** — freeze the checkout onto the event. Add an `entities.yaml#/CheckoutSnapshot` value object
(the `OrderPlaced` content + `cartId`, mirroring the application `CheckoutSnapshot` port type) and a
required `checkout` field on `PaymentIntentCreated`. `PaymentIntentCreated` is a `nonProjectedEvents`
entry, so this has **zero read-model/view impact**. `commands::place_order` builds and freezes the
snapshot when it creates the PaymentIntent (`rules.yaml#/CheckoutSnapshotFrozenAtIntent`), so the order is
reconstructable from the `Order-{id}` stream alone — **no external store to operate**.

Invariant recorded on the event: `checkout.totalAmount == amount == checkout.breakdown.total`.

**Scope of this change (DSL now, runtime rides pricing):** the DSL + regeneration + tests + docs land now;
`place_order` freezes the snapshot from currently-available data. `items` and the `breakdown` split are
**best-available until server-side line pricing lands** (the Cart projector does not price lines yet, and
the `PaymentBreakdown` fee/split of ADR-0016/0017 is unwired). We deliberately do **not** retire the
`CheckoutSnapshotSource` seam or materialize `OrderPlaced` from the frozen snapshot yet — that waits until
pricing makes the frozen `items`/`breakdown` correct, so no approximate breakdown is ever consumed.

## Alternatives considered

- **Option 2 — durable pending-checkout store** keyed by `payment_intent_id`: keeps the event lean but
  adds an out-of-log store to write, read, and operate, against the repo's log-is-source-of-truth bias
  (ADR-0035 read side folds `View_*` from `domain_events`; no external state stores in V0). Rejected.
- **Make `checkout` nullable**: would leave `OrderPlaced` unreconstructable when absent, defeating the
  purpose. Rejected — the field is required (the correct end-state).

## Consequences

### Positive
- `OrderPlaced` + `CartCheckedOut` are reconstructable from the `Order-{id}` stream alone — no external
  store; the future `CheckoutSnapshotSource` becomes a trivial read of `PaymentIntentCreated.checkout`.
- Zero view/GraphQL drift (`PaymentIntentCreated` is non-projected); the Rust `CheckoutSnapshot` port type
  is de-duplicated onto the generated `entities.yaml#/CheckoutSnapshot`.

### Negative
- `PaymentIntentCreated` now carries a self-contained snapshot that duplicates `restaurantId`/`customerId`/
  `totalAmount` (documented, equal by construction — same pattern as `OrderPlaced`'s `totalAmount` +
  `breakdown.total`).
- Until pricing lands, the frozen `items`/`breakdown` are best-available (empty items, articles-only
  breakdown); nothing reads them yet, so no wrong order is materialized.

### Follow-up actions
- **Server-side pricing** (prerequisite to full population): Cart projector prices lines (priced
  `OrderLineItem[]`) + the ADR-0016/0017 fee/split `PaymentBreakdown`.
- **Retire `CheckoutSnapshotSource`**: have `on_payment_captured` read `PaymentIntentCreated.checkout` from
  the `Order-{id}` stream (drop the port + `UnavailableCheckoutSnapshotSource`) — trivial once the frozen
  data is correct.
- Real Stripe **outbound** create-intent with `metadata.{restaurantId,orderId}` + the split (STATUS 1b).

## References
`specs/{entities,events,tests,rules,actors}.yaml`, `docs/sagas.md` (saga runtime + the now-closed gap),
ADR-0046 (write side), ADR-0035 (read side / event log), ADR-0016/0017 (fee + Connect split),
ADR-0041 (event envelope). Behaviour: `crates/application/.../place_order.rs` +
`tests/place_order_behaviour.rs`.
