# ADR-20260810-112836 — Cart priced LIVE on read; the Cart projection is a pure money-free fold

## Status

Accepted (product-owner decision 2026-08-10, recorded in
[PROP-20260810-231500 "cart.current: the authenticated customer's PRICED cart"](../proposals/PROP-20260810-231500-cart-current-priced.md)
and DECISIONS.md §1 row G). Realizes Option B / LIVE for
[#451 "cart.current returns the authenticated customer's priced cart (#429 keystone: total computes + no route params)"](https://github.com/TheCaptainCompany/captain-food/issues/451).

## Context

The `Cart` read model in `specs/database/tables/projection_tables.yaml` declared
`total_amount_cents` / `currency` / `estimated_breakdown` / `uber_comparison` as fold columns fed
by the cart events, with notes saying the projection computes them "from the live catalog", and
ADR-0028 §5 said the checkout estimate is "derived by the projection". That is an **impure fold**:
a projector that reads today's catalog while folding yesterday's events produces rows a replay
cannot reproduce. In practice the projector stubbed every money column to zero, so `/checkout`
rendered `0,00 €` — one of the two recorded #429 blockers (the other being `/checkout` needing a
route param).

PROP-20260810-231500 laid out the option space (A: price frozen into cart events at add-time; B:
price computed at read; C — rejected outright: the impure fold the spec accidentally committed).
The product owner decided **B — LIVE**.

## Decision

1. **The cart price is LIVE.** The `Cart` projection is a **pure, money-free fold** of the cart
   events: identity (`cart_id`, `restaurant_id`, `session_id`, `customer_id`), `status`,
   timestamps, and per line exactly the repricing inputs (`cart_line_id`, `offer_id`, `quantity`,
   `selected_option_ids`). Every price a customer sees on a cart is computed **on read** from the
   live catalog by `application::pricing::price_cart` — the SAME authority the checkout write path
   uses (one code path, no second pricer). Fail-closed: an unresolvable line yields the honest
   no-price state, never a stale or client-supplied number.
2. **Live upstream, frozen at commitment (business + legal posture).** Upstream of the commit
   moment the displayed price TRACKS the live catalog (a restaurant price change shows on the next
   read). The ONE authoritative, replayable, audit-grade freeze happens where the money moves:
   `PlaceOrder → price_cart → CheckoutSnapshot` on `PaymentIntentCreated`
   (`rules.yaml#/CheckoutSnapshotFrozenAtIntent`). The legal display guarantee (Code de la
   consommation L112-1 price display / L221-5 pre-contractual information) is carried by the
   `expectedTotal` equality check: the total the consumer agreed to is the total charged, or the
   checkout rejects with `PriceMismatch` (now explicit in `rules.yaml#/ServerPriceAuthority`).
3. **`cart.current`, zero-arg and claim-resolved.** The new customer query returns the caller's
   most-recently-updated OPEN cart (or null), identity from the verified claim
   (`ReadScope::Customer`), never a client argument — so `/checkout` carries no route params.
4. **The `cart` by-id IDOR is retired.** The pre-existing `cart(id)` query had NO role guard —
   any cart was fetchable by id (found in the #451 mob briefing). It is now `roles: [CUSTOMER,
   ADMIN]` with claim-ownership for CUSTOMER (a non-owned id resolves to null — no existence
   oracle). Claim-ownership was chosen over ADMIN-only because the customer story steps
   (checkout breakdown, Uber comparison) legitimately read a specific per-restaurant cart by id.
   Guests keep NO by-id cart read (ADR-20260720-213000 §3 posture); the guest mini-cart is a
   recorded gap.
5. **ADR-0028 §5 is corrected** (addendum in that file): the estimate is computed at read time,
   not materialized by the projection. Formulas and `PaymentBreakdown` shape unchanged.
6. **Read-side pricing observability is a contract** (`specs/observability.yaml#/cart-price`):
   span `cart.price` at the GraphQL resolver seam (the pricer stays SDK-free), histogram
   `cart_price_ms` (initial budgets p95 300ms / p99 600ms, tunable after first peak), defect
   counter `cart_price_unresolvable_total{reason: offer_gone | policy_missing | stock_unknown}`,
   alert on any sustained non-zero. Classification: unresolvable at read = `technical_error`
   (the customer sees no price — a defect, not a domain rejection); the write-path twin stays a
   `business_rejected` under `place-order`.

## Alternatives considered

Recorded in full in PROP-20260810-231500 §2 (living option-space record): **A** (price frozen
into cart events at add-time) reverses ADR-20260720-002217, needs an event-versioning story, and
freezes a price the customer has not committed to; **C** (projection reads the live catalog while
folding) is not a fold at all and was rejected explicitly.

## Consequences

### Positive
- `/checkout` and the mini-cart can render a REAL total (the #429 keystone unblocks).
- Replay-honest read model: a rebuild reproduces the rows exactly; no money in cart events.
- One pricer: `price_cart` serves reads and checkout; display-vs-charged drift is structurally
  rejected, which is also the L112-1/L221-5 posture.

### Negative
- Every cart read reprices against the catalog on the checkout hot path. Mitigations, in order:
  the `cart-price` contract sees it before it hurts; the recorded lever is a per-request memoized
  catalog read (the N+1 shape — each line resolving offers/options — is a Phase-2 note on the
  resolver seam, PROP §8).
- The transient price a guest once saw is not in the log. Accepted: the audit-grade price is the
  ORDER price, which is (CheckoutSnapshot).

### Follow-up actions
- **Phase 2 (GREEN, same issue #451)**: the `current` resolver + claim-resolved repo lookup
  (`[customer_id, updated_at]` index shipped with this change), the projector fold of money-free
  `lines` (today it folds nothing), `price_cart` at the read seam + the contract's metrics proved
  firing via the `checkout_degraded_metric.rs`-style spy, and the N+1 memoization decision.
- **Counsel packet (verify-first, external legal action)**: delivery-fee VAT treatment and the
  3-way rate map (food 10%/5.5%, delivery 20%, service fee 20% — indicative, NOT verified) must
  be confirmed by counsel before real receipts; the V0 degenerate breakdown (fees 0) keeps this
  latent, not resolved.

## References

- [PROP-20260810-231500](../proposals/PROP-20260810-231500-cart-current-priced.md) — the option
  space, mockups, sequence diagrams, decision record.
- [ADR-20260720-002217 "Server-side pricing, fail-closed"](20260720-002217-server-side-pricing.md)
  — honored and executed (its follow-up said to price the cart read from the same catalog source).
- [ADR-0028 "Pricing & 3-way split"](0028-pricing-3way-split-model.md) — §5 corrected by addendum.
- Spec surfaces changed with this ADR: `specs/ordering/api.yaml` (`Cart` type, `cart`, `current`),
  `specs/database/tables/projection_tables.yaml#/Cart`, `specs/ordering/rules.yaml`
  (`CartPricedFromLiveCatalog`, `ServerPriceAuthority`, `CheckoutPricesCartCreatesPaymentIntent`),
  `specs/observability.yaml#/cart-price`, `specs/stories.yaml` (`ViewCurrentCart`),
  `specs/screens/restaurant_frontoffice.yaml` (`cart.current` resolver).
