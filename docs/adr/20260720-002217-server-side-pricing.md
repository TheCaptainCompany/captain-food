# ADR-20260720-002217 — Server-side pricing on the order path, fail-closed

## Status

Accepted

## Context

Money flows through the checkout path: cart line totals, the order total, and the amount sent to the
payment gateway. The specs already stated the intent ("client-supplied prices are NEVER trusted" on
`commands.yaml#/CartLine`; "prices it server-side" on `PlaceOrder`), and no command schema carried a
client Money on the cart/order path — but the runtime did not honour the intent: `place_order` read
the payment amount from the `Cart` projection row, whose `total_amount_cents` is a projection stub
that always returns 0, and froze a `CheckoutSnapshot` with EMPTY `items` and a degenerate breakdown.
There was no recomputation from the catalog, no typed rejection when a price could not be resolved,
and no way for the client to confirm the total it displayed.

## Decision

The server is the ONLY price authority on the write path.

1. **Recompute, never read from the client.** At checkout, every cart line is repriced from the LIVE
   catalog through the offer-level `CatalogReadRepository` read port
   (`application::pricing::price_cart`): `unitPrice` = the offer's live price, each selected option
   priced from its live option list, `lineTotal = (unitPrice + Σ optionPrices) × quantity`, the
   order total = Σ line totals. That recomputed total IS the payment-intent amount and is frozen —
   with the priced `items` and the `PaymentBreakdown` — onto `PaymentIntentCreated`'s
   `CheckoutSnapshot` (rules.yaml#/CheckoutSnapshotFrozenAtIntent). The Cart aggregate fold now
   carries the full lines (offer, quantity, selected options) so pricing works from the cart's own
   stream, race-free — cart events still record NO money.
2. **Confirmation, not authority.** No command on the order path carries an authoritative client
   amount. `PlaceOrder` gains one OPTIONAL field, `expectedTotal` — strictly a confirmation of the
   total the client displayed, validated for EQUALITY against the server computation. Divergence
   rejects with the new typed `errors.yaml#/PriceMismatch` (context: `cartId`,
   `expectedAmountCents` = the server total, `submittedAmountCents` = the client's, `currency`).
3. **Fail-closed.** If any line's price cannot be resolved from the live catalog — offer gone,
   selected option gone, or a currency clash across resolved prices — the checkout is rejected with
   the new typed `errors.yaml#/PriceUnresolvable` (context: `cartId`, `offerId`). The server NEVER
   falls back to a client-supplied number, and never charges a fabricated amount (an empty cart in
   pricing is also rejected rather than priced 0 in a guessed currency).

The guarantee is recorded as `rules.yaml#/ServerPriceAuthority`, wired as guards on the
PlaceOrderProcess command leg (`processmanager.yaml`), and asserted by three behaviour tests
(ADR-0032 completeness): `TestPlaceOrderRecomputesPriceServerSide`,
`TestPlaceOrderRejectsPriceMismatch`, `TestPlaceOrderRejectsUnresolvablePrice`.

## Alternatives considered

- **Price from the Cart projection row** (status quo seam) — rejected: the projection is eventually
  consistent and unpriced today; the write side must not trust a read model for money it can
  recompute authoritatively from the catalog at decision time.
- **Require `expectedTotal`** — rejected for V0: the confirmation is valuable but optional keeps the
  command backward-compatible; the server total is authoritative either way.
- **Tolerance window on the confirmation** — rejected: equality is the only defensible check for a
  displayed-vs-charged amount; a diverging price means the client must re-render and re-confirm.

## Consequences

### Positive
- The payment-intent amount can never be influenced by a client — recomputed server-side per checkout.
- `OrderPlaced`/`CheckoutSnapshot.items` are now real priced lines; the order is reconstructable
  from the log with its full pricing detail.
- Typed, translated rejections (`PriceMismatch`, `PriceUnresolvable`) instead of silent drift.

### Negative
- Checkout now hard-depends on the catalog read model being available and holding the cart's offers
  (fail-closed): a catalog outage blocks checkout — by design.
- The V0 breakdown is still degenerate (articles = total, fees 0); the ADR-0016/0017 fee/split
  policy plugs into `application::pricing` when it lands.

### Follow-up actions
- Land the ADR-0016/0017 fee/split policy inside `pricing::price_cart` (delivery fee, service fee,
  restaurant contribution/payout).
- Re-validate line ORDERABILITY at checkout (OfferUnavailable / InsufficientStock /
  InvalidOptionSelection) — pricing already fails closed on a line that left the catalog.
- Price the Cart READ projection (display totals) from the same catalog source so the UI total the
  customer confirms is computed by the same rules.
