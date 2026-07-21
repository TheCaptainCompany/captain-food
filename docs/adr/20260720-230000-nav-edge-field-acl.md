# ADR-20260720-230000 — Per-edge ACL on FK-derived navigation fields (`navRoles`)

## Status

Accepted (issue #22 contract; design pre-agreed)

## Context

api.yaml carried op-level `roles` only. FK-derived navigation edges (synthesized from view `fk:`
columns) were bare fields: any role that could reach the parent type could traverse every edge.
Concretely, the PUBLIC-reachable `Restaurant` exposed `carts`, `orders` and `deliveryJobs` —
customer-PII and ops edges — in every role's schema (resolved empty today, but an open contract
that #21 would freeze). Every new FK silently widened the hole; #20's token/connection tables must
never be reachable this way.

## Decision

- New OPTIONAL api.yaml surface: `types.<T>.navRoles: { <edgeField>: [roles] }`. LITERAL semantics
  (ADR-20260720-191500): an edge absent from the map is OPEN (inherits parent reachability); a
  listed edge is traversable only on those role paths.
- Enforcement is the operations' exact machinery: `@auth(requires: [...])` in the SDL and the
  generated `guard`/`visible` pair on the SimpleObject field — unlisted roles get FORBIDDEN on
  execution and never see the field (or types reachable only through it) in introspection.
- Validator: `nav-roles-unknown-field` (key must be a derived edge) + the UserType check.
- Seeded: `Restaurant.carts` → [ADMIN]; `Restaurant.orders` → [RESTAURANT, RESTAURANT_ACCOUNT,
  ADMIN]; `Restaurant.deliveryJobs` / `Order.deliveryJobs` → [RESTAURANT, RESTAURANT_ACCOUNT,
  RIDER, ADMIN]. Customer order tracking is unaffected (`Order.deliveryStatus`/`courier` columns).

## Alternatives considered

- `roles:` on the FK column in the database DSL — rejected: API ACL doesn't belong in the store
  schema layer.
- ComplexObject resolvers per edge — unnecessary: async-graphql guards/visibility work on
  SimpleObject fields.

## Consequences

### Positive
- Schema growth is safe by default-open + explicit-deny; #20's sensitive tables can ship edges
  guarded from day one; #21 freezes correct contracts.
- The seeded edges close real PUBLIC-schema PII exposure before any client exists.

### Negative
- Default-open mirrors the operations trade-off: forgetting `navRoles` leaves an edge public —
  same review rule as ADR-20260720-191500 (an unguarded edge is a positive claim).

### Follow-up actions
- #20: declare `navRoles` on any connection/token-adjacent edges in the same change that adds them.
- Per-row ownership on guarded edges (a restaurant sees ITS orders only) rides the future
  caller↔restaurant binding (ADR-20260720-220000 gap).
