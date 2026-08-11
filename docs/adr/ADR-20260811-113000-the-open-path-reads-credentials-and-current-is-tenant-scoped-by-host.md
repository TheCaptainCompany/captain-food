# ADR-20260811-113000 — The open GraphQL path reads credentials (degrading to anonymous), and `current` is tenant-scoped by `Host`

- **Status**: Accepted
- **Date**: 2026-08-11
- **Issue**: [#469 "`current` leg 1 is dead on the web AND is not tenant-scoped — fix both together or neither"](https://github.com/TheCaptainCompany/captain-food/issues/469)
- **Supersedes nothing.** Extends [ADR-0047](0047-api-auth-supabase-jwt-jwks.md) (role = path)
  and [ADR-20260810-120531](ADR-20260810-120531-cart-current-two-leg-resolution.md) (the two-leg `current`).

## Context

`cart.current` resolves in two legs: a verified CUSTOMER claim, then an anonymous session id. Two
facts made leg 1 both unreachable and unsafe, and they are inseparable.

1. **`/public` was credential-blind.** `AuthContext::authorize` returned `Principal::anonymous()` for
   the PUBLIC path *before reading any credential*. The storefront is pinned to `Role::Public`
   (`web::router`), so a signed-in customer's request arrived as `ReadScope::Public` and leg 1 never
   fired from a browser. Every test passed because every test injected `ReadScope::Customer` by hand.
2. **Leg 1 was bounded by nothing.** It resolved the claim-holder's newest OPEN cart *anywhere*, and
   the GraphQL edge injected role, principal, session, trace and scope — but never the tenant, even
   though the platform is multi-tenant by `Host` and the SSR renderer already resolves it.

Fixing (1) alone ships (2) as a live cross-tenant cart: a customer with an open cart at
`b.captain.food` would see it, be priced for it and pay for it on `a.captain.food`.

## Decision

**1. The open path attempts verification and degrades to anonymous.** `/public` reads the
`captain_auth` cookie / bearer and verifies it, and is the ONE path that never refuses: absent,
invalid, expired, tampered, JWKS-unreachable and non-CUSTOMER credentials all serve `200` anonymous.
A stale cookie is the common case; a JWKS outage must not take anonymous browsing down with it, on a
path that worked with no JWKS at all before. Each degrade is counted as
`public_credential_degraded_total{reason}` (`invalid_token` | `verifier_unavailable` |
`role_not_customer`) so the degradation is visible rather than silent, and the JWKS fetch is bounded
at 3 s — key refresh now sits on the storefront's critical path.

**2. It grants at most the CUSTOMER identity.** A verified ADMIN / RESTAURANT / RESTAURANT_ACCOUNT /
RIDER token presented to `/public` is anonymous, never elevated. "Role = path" is the reason staff
present their token to their own path, and elevating here would convert a dead claim leg into
privilege escalation on the one path anyone can reach — widening every role-omitted read (an omitted
`roles` key is open to every role path). This is enforced by construction, not by review:
`Principal::public_customer(sub, customer_id)` is the only non-anonymous construction the open path
can reach, and it takes the customer claim and nothing else, so the staff claims have no field to
land in.

**3. The tenant is a request datum, resolved from the `Host` at the edge, beside `ReadScope`.**
`TenantScope` (`crate::graphql::tenant`) is `Host` → `{slug}` → `RestaurantId`, resolved once per
request (POST and WebSocket) and injected as its own `.data(...)`. It is NOT folded into `ReadScope`:
different provenance (verified claims vs. host) and different lifetime (legitimately absent on the
marketplace), and folding them would force every existing `match scope` arm to re-handle a product
and make ADMIN/SYSTEM carry a tenant they must ignore. Where the SSR fallback fails OPEN on a lookup
error (a DB hiccup must never show "this address is available" for a real restaurant), scope fails
CLOSED: a request whose tenant cannot be established reads nothing tenant-scoped.

**4. `current` stays zero-argument, and both legs are bounded in SQL.** The tenant is authorization
input, so a client must not be able to assert it; an *optional* tenant argument would mean
"unbounded when omitted", which is the original bug with a nicer name. The bound lives in two new
port methods (`open_by_customer_at`, `open_by_session_at`) whose signatures make the tenant
non-optional, with the predicate in the store — a caller-side filter would prove a Rust `if` while
unfiltered SQL shipped. `by_customer` / `open_by_session` stay unbounded for the consumers that are
right to span restaurants (`carts`, CartBindingProcess).

**5. A host that names no restaurant resolves `null`.** On the marketplace, an unknown slug, or a
failed lookup, `current` is `null` — never "the newest cart anywhere". The legitimate "my carts
elsewhere" surface is the existing `carts` query.

## Consequences

- **Blast radius of (1)+(2)**, enumerated from the generated resolvers rather than recalled: on
  `/public` with a verifiable CUSTOMER credential, `current` gains leg 1 (the fix); `paymentStatus`
  can now match `customer_owned` (a customer follows their own payment without still holding the
  anonymous session id); `operationStatus` / `operationStatusChanged` also match ownership by the
  caller's own `sub`; and the command envelope of the open mutations (`addCartLine`,
  `removeCartLine`, `changeCartLineQuantity`, `requestPhoneVerification`, `verifyPhone`,
  `claimRestaurantListing`, `optOutRestaurantListing`) stamps `user_id`/`user_type = CUSTOMER`
  instead of `PUBLIC` — truthful attribution (ADR-0041), and new data in stored envelopes. Every
  other open read (`catalog`, `categories`, `restaurants`, `restaurant`) consumes neither datum and
  is unchanged; every guarded operation still runs its `RoleGuard` against the PATH role and stays
  forbidden and invisible on `/public`.
- **The write-path widening reaches the MAILBOX HANDLER, not only the resolvers** (reviewer S1).
  `infrastructure/src/mailbox/handler.rs::resolve_actor` branches on `message.user_type ==
  "CUSTOMER"`, and the seven open mutations now take that branch **at delivery time**, where they
  previously short-circuited to `domain_id: None`. Two operational consequences: **(a)** one extra
  `customers.by_auth_ref(sub)` read per delivery on the storefront's hottest write path — the three
  cart mutations, at Friday peak; **(b)** `resolve_actor` returns `Err` on a read-model failure and
  that **aborts the delivery** (the row stays RECEIVED for redelivery — the right protection against
  a wrong-class terminal verdict), so a Customer-projection outage can now stall cart writes that
  were already accepted PENDING, a dependency an open-path cart command could not previously reach.
  Command OUTCOMES are unaffected: every `domain_id` consumer sits on a guarded operation,
  unreachable from `/public`. The `claim_absent` degrade in decision (1) bounds the exposure — a
  pre-claim customer is `user_type = PUBLIC` and never takes the branch, so only claim-stamped
  customers, precisely the population whose `by_auth_ref` resolves, pay the extra read. Worth
  recording *why* the first radius missed this: it was enumerated from the generated resolvers,
  which structurally cannot show a change that lands in the mailbox handler.
- **The stored-identity widening lands on streams with NO erasure path** (reviewer S2). Only
  `Order` declares a deletion policy and the deletion engine is stream-keyed, so it cannot reach
  `Cart-*`, `Customer-*` or `Restaurant-*`
  ([#194 "GDPR Article 17 has no technical answer: PII lives in an immutable event log with no erasure path, and no DPIA/privacy policy/terms exist"](https://github.com/TheCaptainCompany/captain-food/issues/194)).
  The change is **structural rather than volumetric**: the marginal PII is small, but cart and
  listing streams were previously an erasure-free ZONE and after this essentially every stream is
  subject-attributable. That widens what #194 must answer for, which is why this is a bullet rather
  than a note about "more PII".
- **SSR stays anonymous, deliberately.** `web_ssr::SchemaTransport` still injects
  `Principal::anonymous()` although the SSR request does carry the cookie. Making it
  identity-aware would emit personalised HTML from a path that sets no `Cache-Control` — a caching
  decision to take deliberately, not to discover (the module's own recorded warning). Consequence:
  the storefront's first paint shows the anonymous cart and the hydrate pass fills in the customer's.
- **`/public` GraphQL responses now vary by cookie.** POST responses are not CDN-cacheable by
  default and nothing in the tree sets `Cache-Control` on them, so this is safe today — but any
  future host+path-keyed cache in front of `/public/graphql` would serve one customer's cart to
  another. Recorded here because the constraint is now load-bearing.
- **One `by_slug` read per GraphQL request on a tenant host** (marketplace, `api.` and dev hosts
  short-circuit before touching the database) — the same indexed lookup the SSR fallback already
  performs per page render.
- **`graphql_routes` takes the tenant lookup as a parameter**, not an `Extension`: mounting the
  GraphQL surface without a way to resolve the tenant does not compile.
