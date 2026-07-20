# ADR-20260720-191500 — Literal `roles:` lists: explicit PUBLIC = the anonymous path only; omitted roles = open to everyone

## Status

Accepted (product-owner directive, 2026-07-20)

## Context

Under ADR-0006 (role-as-path), every api.yaml operation declares `roles:`, and `PUBLIC` anywhere in
the list meant "open to every role path — no guard, `@public` in the SDL". That made a literal list
like `[PUBLIC, CUSTOMER, ADMIN]` inexpressible: the moment PUBLIC appeared, the other entries were
dead text. #13 hit this exactly — its recommendation ("extend paymentStatus to
`[PUBLIC, CUSTOMER, ADMIN]`") had to be flattened to `[PUBLIC]` in PR #30 because the extra roles
meant nothing. Meanwhile the spec already carried literal-intent lists whose restriction was
silently ignored: `[PUBLIC, CUSTOMER]` on `requestPhoneVerification`/`verifyPhone` ("anonymous
visitor starts phone sign-in") and `[PUBLIC, RESTAURANT_ACCOUNT]` on the listing-claim mutations.

## Decision

The `roles:` list is **literal**:

1. **`roles:` omitted** → the operation is open to **every** role path. SDL `@public`, no
   guard/visible pair — the previous "PUBLIC in list" behaviour.
2. **`roles:` present** → exactly the listed role paths may call the operation. `PUBLIC` in the
   list is simply the anonymous path (`/public/graphql`) — one more member, no special power. SDL
   `@auth(requires: [...])` (PUBLIC may appear inside `requires`); the generated guard/visible pair
   carries the literal set.

Validator: `op-no-authz` (an operation must declare ≥1 role) is retired — a missing `roles:` is now
a legal, deliberate "open to everyone" claim. Story authorization becomes
`roles omitted || persona.role ∈ roles`. Runtime `role_allows` is plain set membership.

Migration (same change): the 11 standalone `[PUBLIC]` operations (browse, cart, operationStatus &
its subscription, phoneCountries) drop their `roles:` line — behaviour-preserving; the
literal-intent lists keep their text and finally gain their restriction;
`paymentStatus`/`paymentStatusChanged` adopt #13's original `[PUBLIC, CUSTOMER, ADMIN]`.

## Alternatives considered

- **Keep PUBLIC-blows-open** — rejected: the DSL cannot state "these paths incl. anonymous, no
  others", and lists lie to the reader.
- **Explicit `roles: [ALL]` sentinel instead of omission** — more fail-closed, but adds a
  pseudo-role to the UserType vocabulary and noise on the ~11 genuinely-open operations; rejected
  by the product owner in favour of the terser "nothing indicated = available for everyone".
- **PUBLIC-only via a separate `anonymousOnly: true` flag** — two ACL mechanisms instead of one
  list; rejected.

## Consequences

### Positive
- `roles:` reads as it acts — per-path ACL is expressible with the anonymous path as a first-class
  member; #13's intent is now stated literally.
- The pre-existing literal lists (`verifyPhone`, listing claims) get their intended restriction;
  unrelated role paths (RIDER, RESTAURANT, EXTERNAL) lose access they never should have had.
- #22 (per-field `roles:` on nav edges) inherits coherent semantics.

### Negative
- **Fail-open by omission**: forgetting `roles:` on a new sensitive operation opens it to every
  path. Accepted trade-off; review rule — a missing `roles:` line is a positive claim ("open to
  everyone") and must be challenged in review like any other ACL statement.
- `op-no-authz` no longer forces the author to think about ACL per operation.

### Follow-up actions
- Fold the same semantics into #22's nav-edge `roles:` design.
- The SDUI screens' `roles:` (screen visibility) are untouched — they were already literal.
