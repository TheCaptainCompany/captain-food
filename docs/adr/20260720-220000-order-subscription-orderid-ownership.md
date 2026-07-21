# ADR-20260720-220000 — `orderStatusChanged` tracks by orderId with per-row ownership

## Status

Accepted (issue #14 contract)

## Context

`subscriptions/orderStatusChanged` was the last operation on the pre-acceptance-first convention:
it took a `correlationId` argument and matched envelopes by correlation, extracting the order id
from the stream name. Acceptance-first (ADR-20260720-015500) moved `operationStatusChanged` to
`messageId` + resolver-side ownership; the order stream needed the same decision before any client
(#17/#21) built against the stale key. The confirmation screen holds an **orderId** (route
`/orders/:orderId/confirmation`) — a correlation id is dispatch plumbing the UI has no reason to
keep.

## Decision

1. **Tracking key = `orderId`** (the issue's recommended option). The resolver subscribes to the
   event bus and matches exactly the `Order-<orderId>` stream; each matching envelope re-resolves
   the CURRENT OrderTracking row (queries never read raw `domain_events`, ADR-0005/0035).
2. **Ownership, resolved once at setup and applied per resolved row** (the row may not be projected
   yet when the checkout page subscribes):
   - **ADMIN**: any order (roles gain ADMIN — support impersonation, consistent with
     `paymentStatus`).
   - **CUSTOMER path**: the caller must BE the order's customer (`auth_ref` → Customer →
     `customer_id == row.customer_id`); strangers and anonymous CUSTOMER-path callers get silence,
     never an existence oracle.
   - **RESTAURANT / RESTAURANT_ACCOUNT paths**: trusted as-is — **recorded gap**: no
     caller↔restaurant identity binding exists anywhere yet (the `orders`/`order` queries have the
     same trust level). Scoping these paths is one coherent follow-up across
     `order`/`orders`/`orderStatusChanged` once a restaurant-identity read model exists.
   - **Guests**: no session scope on Order reads (ADR-20260720-213000 §3) — a guest follows
     `paymentStatusChanged` until phone verification binds their orders.
3. Roles become the literal `[CUSTOMER, RESTAURANT, RESTAURANT_ACCOUNT, ADMIN]`
   (ADR-20260720-191500 semantics).

## Alternatives considered

- **Keep `correlationId`** — rejected: the client would have to persist dispatch metadata per order
  for the lifetime of the tracking screen; the orderId is already its route parameter.
- **Full restaurant-side ownership now** — rejected: requires inventing a caller↔restaurant
  binding that no other operation has; doing it only here would fake a guarantee the query surface
  doesn't hold.
- **Guest session scope on order streams** — rejected in ADR-20260720-213000 §3 (OrderTracking has
  no session column).

## Consequences

### Positive
- The subscription contract matches what the UI holds; #17/#21 build against the final shape.
- Customer streams are ownership-scoped for the first time (they were correlation-keyed but
  unowned: anyone holding a correlation id could follow any order).

### Negative
- Restaurant-path trust is inherited, not fixed (recorded gap above).
- A client that cached correlation ids must switch keys — none exist yet (frontend unbuilt); the
  prod smoke never used this subscription.

### Follow-up actions
- Restaurant-identity binding + scoping sweep over `order`/`orders`/`orderStatusChanged` (new
  issue when a restaurant client approaches).
- #17/#21 consume `orderId` + `paymentStatusChanged` per this contract.
