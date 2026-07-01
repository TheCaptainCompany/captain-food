# ADR-0028 — Pricing & 3-way split domain model

## Status

Accepted

## Context

ADR-0016 (0% commission on food + proportional Captain service fee), ADR-0017 (3-way Stripe Connect
split), and ADR-0018 (transparent checkout fee display) were `Proposed`. The specs modelled only
`Order.totalAmount` (one `Money`) — no fee, no split, no breakdown. This records the concrete model that
realizes those three in the DSL.

## Decision

1. **One `PaymentBreakdown` value object** (entities.yaml, all `Money`) carries BOTH the buyer-facing
   checkout lines — `articles`, `delivery`, `serviceFee`, `total` (ADR-0018) — and the 3-way split —
   `restaurantContribution`, `restaurantPayout`, `riderPayout`, `captainNet` (ADR-0017). One source, no
   divergence. Invariants: `total = articles + delivery + serviceFee`; `restaurantPayout = articles −
   restaurantContribution`; `riderPayout = delivery`; `captainNet = serviceFee + restaurantContribution`.
2. **Computed server-side** by `PlaceOrderProcess` and carried on **`OrderPlaced.breakdown`**
   (`totalAmount` kept as the top-line buyer total == `breakdown.total`). No client input; `PlaceOrder`
   is unchanged. `View_OrderTracking` exposes the breakdown as discrete `*_cents` columns (BAM /
   Open-Collective totals); `Order.breakdown` surfaces it.
3. **Fee policy = calibratable reference data** (`View_PricingPolicy`, `source: reference`): `fee_rate`,
   `buyer_share`, `margin_low`, `margin_high` — seeded with the ADR-0017 indicative values (5% / 60% /
   55–70%). Tunable without code; queryable by admin (`pricingPolicy`).
4. **Restaurant margin** is a nullable `marginRate` (`MarginPercent`) on the Restaurant, driving the
   restaurant's variable contribution (`clamp((margin−low)/(high−low),0,1)`); when null the contribution
   is 0 (buyer-fee-only), so the full formula is modelled without requiring margin data yet. It is
   **back-office only** — not exposed on the public `Restaurant` type.
5. **Checkout estimate** lives on `View_Cart.estimated_breakdown` (jsonb, derived by the projection from
   the food total + policy + margin) and is surfaced as `Cart.breakdown` for the transparent display
   (ADR-0018), recomputed authoritatively on the order.
6. **Stripe Connect** = Separate Charges & Transfers (`transfer_group`), Captain = merchant of record;
   transfers to the restaurant/rider run **after capture**.

## Alternatives considered

- **Flat buyer fee (€3 cap)** — loss-making on large baskets; penalizes group orders (ADR-0016). Rejected.
- **Commission on food** — contradicts the 0%-commission positioning. Rejected.
- **Fee params hard-coded in the projection** — chose a reference table instead (calibratable + transparent).
- **Separate buyer-breakdown vs split VOs** — chose one `PaymentBreakdown` to avoid divergence.

## Consequences

### Positive
- Positive Captain margin on every basket; DGCCRF-friendly transparency; Open-Collective-compatible
  per-order split; fee % tunable without a deploy; 3-way structure ready for delivery.

### Negative
- The Restaurant aggregate/read model now carries a (sensitive) margin input.
- `OrderPlaced` grew a rich breakdown; `totalAmount` semantics shifted from food-only to buyer-total.

### Follow-up actions (runtime, deferred)
- Execute the post-capture transfers + document the **refund transfer-reversal** flow when the runtime lands.
- Delivery/rider amounts are 0 until the delivery domain (Avelo37) is modelled (post-V0).
- Calibrate the policy by simulation before launch.

## References
ADR-0016/0017/0018; `specs/entities.yaml#/PaymentBreakdown`, `specs/events.yaml#/OrderPlaced`,
`specs/views.yaml` (`View_OrderTracking`, `View_Cart`, `View_PricingPolicy`), `specs/api.yaml`
(`Order`/`Cart`/`PricingPolicy`), the `place-order` observability contract.
