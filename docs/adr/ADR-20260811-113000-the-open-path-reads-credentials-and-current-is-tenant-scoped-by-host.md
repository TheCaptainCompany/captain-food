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
`public_credential_degraded_total{reason}` — **four** reasons: `invalid_token` |
`verifier_unavailable` | `role_not_customer` | `claim_absent` (the last being the pre-claim-stamp
window, see the S3 note under Consequences) — so the degradation is visible rather than silent. Both
branches are asserted, not assumed: `crates/server/tests/public_credential_degraded_metric.rs` drives
the real router and observes that the open path bumps `claim_absent` and leaves
`read_authorization_bridge_unresolved_total` silent, while the SAME token on `/customer` bumps the
bridge counter.

**The verifier is bounded for peak, not just per fetch.** One JWKS fetch is capped at 3 s, and — since
the storefront rather than a handful of staff is now the caller — the refresh itself is
**single-flight with a negative cache**: N concurrent requests at the hourly TTL boundary cost ONE
outbound fetch, a failed fetch silences re-attempts for 10 s, and an unknown `kid` (attacker-supplied
on an open path) can drive a rotation refetch at most once per 5 s. Without these, a Supabase blip at
Friday 19:00 taxes *every* cookie-carrying storefront request the full 3 s and lets one forged token
buy one outbound request. The trade-off taken deliberately: a token signed with a brand-new key can
be refused for up to 5 s after that key appears — bounded and self-healing, unlike the amplifier.

**2. It grants at most the CUSTOMER identity.** A verified ADMIN / RESTAURANT / RESTAURANT_ACCOUNT /
RIDER token presented to `/public` is anonymous, never elevated. "Role = path" is the reason staff
present their token to their own path, and elevating here would convert a dead claim leg into
privilege escalation on the one path anyone can reach — widening every role-omitted read (an omitted
`roles` key is open to every role path). This is enforced **by the type**, not by review — and the
first version of this ADR overstated it, which is the harm the correction records. `Principal` was a
struct with `pub` fields, so `Principal { role: RequestRole::Customer, customer_id: None, .. }` was
spellable inside AND outside the crate, and `public_customer` taking the id by value constrained one
helper rather than the state space. `Principal` now holds ONE private `Identity`:

```rust
enum Identity { Anonymous, External{..}, Admin{..}, Customer{sub, customer_id}, Restaurant{..},
                RestaurantAccount{..}, Rider{..}, Unbound{sub, role} }
```

The role is DERIVED from the identity (`Principal::role()`), so a role can never disagree with the
claim beside it; the CUSTOMER identity has no field a restaurant claim could land in; and the one
legitimate "authenticated but unbound" state — a ROLE-path caller whose token carries no domain
claim, which `read_authorization_bridge_unresolved_total` exists to count — is the NAMED
`Identity::Unbound`, unreachable from `/public`. A role-path principal now also keeps only the claim
matching its own path role, so a `/restaurant` token's `captain_customer_id` is dropped at
construction rather than merely ignored downstream. Compiler-first (ADR-20260803-234035): the
previous shape put the guarantee in a doc comment sitting at exactly the function a future edit would
re-open.

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
  caller's own `sub` — **but only for a claim-stamped customer**: those two consume `Principal.user_id`
  DIRECTLY rather than through `ReadScope`, and a pre-claim caller degrades to anonymous, so for that
  population (every signed-in customer for one token lifetime after rollout) operation ownership rests
  **solely on `X-SESSION-ID`**, exactly as it does on `main`. Fail-closed and unchanged, so not a
  regression — but it is a capability the first radius claimed for a population that does not have it.
  The same split explains the envelope below: the command envelope of the open mutations (`addCartLine`,
  `removeCartLine`, `changeCartLineQuantity`, `requestPhoneVerification`, `verifyPhone`,
  `claimRestaurantListing`, `optOutRestaurantListing`) stamps `user_id`/`user_type = CUSTOMER`
  instead of `PUBLIC` — truthful attribution (ADR-0041), and new data in stored envelopes. Every
  other open read (`catalog`, `categories`, `restaurants`, `restaurant`) consumes neither datum and
  is unchanged; every guarded operation still runs its `RoleGuard` against the PATH role and stays
  forbidden and invisible on `/public`.
- **The write-path widening reaches the MAILBOX HANDLER, not only the resolvers** (reviewer S1).
  `infrastructure/src/mailbox/handler.rs::resolve_actor` branches on `message.user_type ==
  "CUSTOMER"`, and the seven open mutations now take that branch **at delivery time**, where they
  previously short-circuited to `domain_id: None`. The operational consequence is **one extra
  `customers.by_auth_ref(sub)` read per delivery on the storefront's hottest write path** — the three
  cart mutations, at Friday peak. The `claim_absent` degrade in decision (1) bounds who pays it: a
  pre-claim customer is `user_type = PUBLIC` and never takes the branch.
  **Corrected in review round 2** (the first version of this bullet had the mechanism wrong twice):
  the branch is taken on `user_type` ALONE, so a claim-stamped customer whose Customer projection is
  lagging or absent takes it too — the `by_auth_ref` resolving is not a precondition of entering it;
  and a lagging projection yields `Ok(None)`, **not** `Err`, so it does not abort the delivery. Only a
  genuine read-model FAILURE aborts (the row stays RECEIVED for redelivery — the right protection
  against a wrong-class terminal verdict). The `Ok(None)` case leaves `domain_id: None`, whose single
  consumer (`application/src/commands.rs`, `NotAParticipant`) sits on a guarded operation unreachable
  from `/public`, so command OUTCOMES are unaffected either way. Worth recording *why* the first
  radius missed this bullet entirely: it was enumerated from the generated resolvers, which
  structurally cannot show a change that lands in the mailbox handler.
- **An EXTERNAL IdP identifier now enters the immutable write envelope of streams with no erasure
  path** (reviewer S2, **re-characterised in review round 2** — the first version said these streams
  "became subject-attributable", which was wrong). They already were: `CartStarted` requires
  `sessionId`, whose scalar is documented as tracking a user across devices, and `CustomerRegistered`
  requires `phone`. What #469 genuinely creates is narrower and different in kind: seven open-path
  commands now stamp `domain_events.user_id` with the **Supabase `sub`** — an identifier owned by an
  external identity provider — across three stream categories (`Cart-*`, `Customer-*`,
  `Restaurant-*`), where it **survives deletion of the Supabase identity**. Deleting the IdP account
  therefore no longer removes the link between the person and those events. Only `Order` declares a
  deletion policy and the deletion engine is stream-keyed, so nothing can reach the other three
  ([#194 "GDPR Article 17 has no technical answer: PII lives in an immutable event log with no erasure path, and no DPIA/privacy policy/terms exist"](https://github.com/TheCaptainCompany/captain-food/issues/194)).
  The production event log is empty **by decision** (nothing has launched), so this is an unmet
  launch precondition already filed as #194, not a pre-existing breach — but it enlarges what #194
  must answer for, which is why it is a bullet rather than a note about "more PII".
- **SSR stays anonymous, deliberately.** `web_ssr::SchemaTransport` still injects
  `Principal::anonymous()` although the SSR request does carry the cookie. Making it
  identity-aware would emit personalised HTML from a path that sets no `Cache-Control` — a caching
  decision to take deliberately, not to discover (the module's own recorded warning). Consequence:
  the storefront's first paint shows the anonymous cart and the hydrate pass fills in the customer's.
- **`/public` GraphQL responses now vary by cookie — so the surface declares itself uncacheable.**
  A host+path-keyed cache in front of `/public/graphql` would serve one customer's cart to another:
  a GDPR Art. 32(1)(b) confidentiality failure and an Art. 33 notifiable breach. Safe *today* only
  because POSTs are not cached by default and nothing fronts them — both organisational assumptions
  about deployments not yet made. Every response of the GraphQL surface now carries
  `Cache-Control: private, no-store`, applied as one response layer in `graphql_routes` so a new
  route or a new early return cannot forget it (legal lens; the first version of this ADR recorded
  the risk and left it as an assumption).
- **One `by_slug` read per GraphQL request on a tenant host** (marketplace, `api.` and dev hosts
  short-circuit before touching the database) — the same indexed lookup the SSR fallback already
  performs per page render.
- **`graphql_routes` takes the tenant lookup as a parameter**, not an `Extension`: mounting the
  GraphQL surface without a way to resolve the tenant does not compile.
- **The identity this ADR adds is per-STOREFRONT, not per-customer** (reviewer R8, not introduced
  here — recorded because the decisions above now depend on it). `auth_routes` sets `captain_auth`
  with no `Domain=`, so it is a host-only cookie, and the web client posts the session to the window
  origin. On `{slug}.captain.food` that means the same signed-in customer is **anonymous on another
  restaurant's storefront**, with no degrade counted — there is no cookie on that request at all, so
  it does not even reach the verifier. Every test hands the cookie to the host under test, so nothing
  in the suite covers reach. Whether identity should span storefronts is a separate **authn-scope
  decision** (a parent-domain cookie, or a token exchange at the marketplace) and is deliberately NOT
  taken here.
- **The tenant chain is a trap for the split topology, live for the #358 cutover** (reviewer R6).
  `graphql::tenant` justifies the `X-Forwarded-Host` → `Host` chain by "a page rendered for one
  tenant whose GraphQL reads resolved another tenant" — true for the MONOLITH, where SSR resolves
  in-process. In the surface-bin topology SSR reads through the gateway over HTTP
  (`surface_runtime` builds an `HttpTransport`) and `web::graphql` sends only `X-SESSION-ID`,
  dropping the `Host` — so every tenant-scoped read on that path would resolve `TenantScope::None`.
  Harmless today (that transport is anonymous and sessionless, and the monolith is the deployed
  runtime), and a silent tenant loss the day the cutover points traffic at the surface bins. The
  cutover must forward the tenant host on that transport.
- **The `no-store` measure above is absent at the split topology's browser edge — a second cutover
  precondition of the same kind** (reviewer, medium and non-blocking at merge). The gateway rebuilds
  each subgraph response from status + `content-type` + body ALONE
  (`crates/gateway_runtime/src/lib.rs:268-285`), discarding every other header — including the
  `Cache-Control: private, no-store` the subgraph's own `graphql_routes` layer has just set — and its
  own error paths emit none either (`:244-255` routing rejection, `:292-301` `subgraph_unreachable`).
  In the monolith this is invisible: the `server` bin answers `/public/graphql` itself, so the
  response layer applies. After the #358 cutover the GATEWAY *is* the browser-facing
  `/public/graphql`, so the technical measure that replaces "nothing fronts POSTs with a cache" would
  be missing at precisely the hop where a shared cache would sit, on responses that now vary by the
  `captain_auth` cookie — the same Art. 32(1)(b) / Art. 33 exposure the layer was introduced to close.
  Exposure today is ZERO (the monolith is the deployed runtime and nothing fronts it with a cache),
  which is why this is a precondition rather than a defect: `gateway_runtime` must propagate
  `Cache-Control` from the subgraph response and set `private, no-store` on its own error paths
  before the gateway serves browser traffic.
- **`X-Forwarded-Host` is now an AUTHORIZATION input from a client-forgeable header.** The read site
  prefers it over `Host` (mirroring the pre-existing SSR chain), and the impact is bounded — a caller
  can at most surface their OWN cart, or an unbound cart whose session id they already hold, at
  another tenant. It nevertheless creates an **infrastructure precondition** for the OVH MKS ingress:
  it must **overwrite** client-supplied `X-Forwarded-Host`, never append to it. Recorded at the read
  site in code as well, so the dependency is visible where the value is consumed.
