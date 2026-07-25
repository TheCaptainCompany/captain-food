# PROP-20260725-185140 — Read-side per-instance authorization (`ReadScope` on the read ports + the identity bridges)

- **Status**: Proposed — plan-mode proposal. **No `specs/**` or code changed yet.** On approval it becomes an ADR that lands with the first implementation slice.
- **Date**: 2026-07-25
- **Tracking issue**: [#144 "Read-side per-instance authorization: ReadScope on the read ports + RESTAURANT/RIDER identity bridges"](https://github.com/TheCaptainCompany/captain-food/issues/144) (every proposal MUST have one, ADR-20260724-143000; always the full clickable link, never a bare number)
- **Blocks**: [#134 "Order conversations: generic file-attachment framework"](https://github.com/TheCaptainCompany/captain-food/issues/134)
- **Realized by**: _(filled at completion — ADR + PR)_
- **Related**: [PROP-20260725-120055 §4.5](PROP-20260725-120055-generic-file-attachment-framework.md) (where this was discovered) · ADR-0047 (JWT via JWKS at the role-path boundary) · ADR-0006 (role-as-path ACL) · ADR-0035 (Clean-Architecture layering) · ADR-0041 (acting user is envelope, not payload) · ADR-20260719-031136 (write-side Repository) · [#112 "Client auth-token wiring"](https://github.com/TheCaptainCompany/captain-food/issues/112) (the transport this builds on)

---

## TL;DR

**A customer who knows an order id can read another customer's order today.** Same for restaurants and
riders. The read ports take no principal, so an unscoped read is expressible by signature — safety
depends on every caller remembering, and nothing makes them.

This proposes: thread a **`ReadScope`** (constructible only from a verified `Principal`) through the
read repository traits in `crates/application/src/queries.rs`, push it into the SQL predicate in each
`Pg*Repository`, and build the two **identity bridges** (RESTAURANT, RIDER) that make the scope
resolvable at all.

**This is not a defect in ADR-0047.** The role-path gate works exactly as designed. The issue is that
`@auth`/`@public` answers *may this **role** call this operation?* and **nothing** yet answers *may this
**principal** see this row?* The second gate was never built because, until now, nothing needed it.

## 1. Context — how this surfaced, and why it was invisible

Discovered while designing the `/files` guard for
[#134](https://github.com/TheCaptainCompany/captain-food/issues/134) (PROP-20260725-120055 §3.3): the
attachment ACL resolves "is this principal a member of this order?", and answering that turned out to be
impossible for two of four roles — and, worse, revealed that the *existing* read path never asks.

It stayed latent for a good reason. ADR-0047 gates the **path** (`/{role}/graphql`) and explicitly defers
per-field `@auth`; the resolvers are thin read models over projections. So nothing has yet had to enforce
*which* restaurant a staff user belongs to. `/files` is simply the first surface that must, because it is
the first one serving personal data (doorstep photos, reclamation photos) **outside** the GraphQL ACL
entirely.

The current signature, verbatim:

```rust
// crates/application/src/queries.rs:252 — no principal, no scope
async fn by_id(&self, id: OrderId) -> Result<Option<OrderTrackingRow>, DomainError>;
```

Same shape on `CartReadRepository::by_id`, `CustomerReadRepository::by_id`, and the rest.

**Two distinct gates, only one of which exists:**

| gate | question | granularity | where | status |
|---|---|---|---|---|
| `@auth` / `@public` (ADR-0006) | may this **role** call this operation? | coarse, static, per-schema | api.yaml → role path | ✅ built |
| **`ReadScope`** | may this **principal** see this **row**? | fine, dynamic, per-instance | application read ports | ❌ **missing** |

The first structurally cannot express the second: the schema does not know *which* order is being asked
for. They are not redundant, and neither substitutes for the other.

## 2. Which layer owns the check

On the **write** side the aggregate is the choke point — every mutation funnels through it, and business
security checks belong there (product-owner position, 2026-07-25). On the **read** side there is no
aggregate: queries go straight from resolver to read model (projection-on-read, ADR-0035). So the choke
point must move, and the only thing every read funnels through is the **read repository trait**.

| layer | verdict |
|---|---|
| `domain` | **No.** No aggregate on the read side, and domain must not know auth subjects — ADR-0041 deliberately keeps the acting user in the envelope. Row ownership is not a business invariant. |
| `server` (resolver, `/files` route) | **Supplies** the verified `Principal`; must not **own** the rule. Otherwise every transport — GraphQL resolvers, `/files`, the SSR renderer, later the UniFFI mobile shells — reimplements it, and they will drift. |
| **`application` (the read ports)** | **Yes.** "A customer reads their own order" is a use-case rule. This is where use cases live. |
| `infrastructure` (`Pg*Repository`) | Where the predicate **executes** (the SQL `WHERE`), not where the decision **lives**. |
| Postgres RLS | **No** — forks the ACL into a second engine that must be kept in agreement with ours. The exact failure mode ADR-0006 exists to avoid. |

**The symmetry worth recording in the ADR:** on the write side the *aggregate* is the choke point; on the
read side the *port signature* is. Same principle — make the safe thing the only expressible thing —
different mechanism, because there is no aggregate to hold the rule.

> **Vocabulary note** (hexagonal jargon, since it caused confusion): the thing being changed is the
> **read repository trait** — `trait OrderReadRepository` in `application`. "Port" is that same trait seen
> from the architecture's point of view (an interface the core owns, which `infrastructure` must fit);
> "repository" is it seen from the DDD pattern. Not two things. There is **no service layer** on the read
> side — the chain is `resolver → Arc<dyn OrderReadRepository> → PgOrderRepository → SQL` — which is
> precisely *why* the scope must go in the trait signature: there is no intermediate service to hold it.

## 3. The design

### 3.1 `ReadScope`

```rust
// crates/application — constructible ONLY from a verified Principal
enum ReadScope { Public, Customer(CustomerId), Restaurant(RestaurantId), Rider(RiderId), Admin }

async fn by_id(&self, id: OrderId, scope: &ReadScope) -> Result<Option<OrderTrackingRow>, DomainError>;
```

Each `Pg*Repository` turns the scope into a predicate: `AND customer_id = $2` · `AND restaurant_id = $2` ·
a join to `View_DeliveryJob` for rider · nothing for admin · `Public` restricted to genuinely public
projections.

Two properties carry the whole proposal:

- **Structural, not procedural.** An unscoped read stops compiling. A review checklist catches this most
  of the time; a type signature catches it every time. Given that the failure mode is silent
  cross-tenant data disclosure, "most of the time" is not a safety property.
- **Filter, don't check.** Push the scope into the `WHERE` rather than load-then-compare. List queries
  become correct by construction (nobody "forgets to filter the order history"), collections leak no
  existence, and it is one round trip instead of two.

### 3.2 The identity bridges

| role | resolution | today |
|---|---|---|
| ADMIN | role alone, no lookup | ✅ |
| CUSTOMER | `sub` → `Customer.auth_ref` → `customer_id` = `OrderTracking.customer_id` | ✅ both columns exist |
| RIDER | `sub` → `riderId` = `View_DeliveryJob.rider_id WHERE order_id = $scope` | ⚠️ `RiderRegistered.authRef` exists in `events.yaml` but is **projected nowhere** |
| RESTAURANT | `sub` → `restaurant_id` = `OrderTracking.restaurant_id` | ❌ **no auth bridge exists at all** |

Only `Customer` has an `auth_ref`. Needed:

- **A `Rider` read model** projecting `RiderRegistered.authRef`. The event already carries it, so this is
  a projection, not a model change — small.
- **A restaurant staff ↔ restaurant membership model.** The genuinely open piece: it drags in
  multi-staff-per-restaurant, and possibly roles *within* a restaurant (owner vs counter staff). Worth
  scoping deliberately rather than bolting a single `authRef` column onto the restaurant aggregate and
  discovering the multiplicity later. **This is the part that makes this a proposal rather than a
  mechanical change** (decision D2, §6).

> ⚠️ **Do not resolve the restaurant from the `Host` header.** The tenant middleware maps
> `{slug}.captain.food` → restaurant, and reusing it here looks free — but it identifies the storefront
> being *viewed*, not the restaurant the user *works for*. Staff visiting a competitor's subdomain would
> authorize as that competitor's staff. Host is tenant **routing**, never authorization. Recording this
> explicitly because the shortcut is available, tempting, and silently catastrophic.

### 3.3 The `ScopeMembership` port

A port in `crates/application/ports` answering `is_member(principal, scope_type, scope_id) -> bool`, so
`/files` — which is not a GraphQL resolver — shares one implementation with the resolvers instead of
growing its own copy.

**Caching:** memoize per `(sub, scope_id)` in request extensions — a conversation thread rendering 5
images then costs 1 membership check, not 5. Beyond the request: cache **negatives** freely, but
**positives** briefly or not at all. Instant revocation on rider reassignment is the entire point of the
`audience` × `scope` design (PROP-20260725-120055 §3.3), and a 60-second positive cache hands the
previous rider a 60-second window after they are off the job.

## 4. Blast radius

Every read port gains a parameter, and every resolver gains a `ReadScope` construction from its
`Principal`. That is mechanical but **wide** — `queries.rs` declares read repositories for restaurants,
catalog, carts, customers, orders, deliveries, prospection, refunds and the referential policies.

Two mitigations:

- **`Public` and `Admin` are explicit variants**, so genuinely public reads (restaurant discovery,
  catalog) stay one-word changes rather than exceptions carved out of the type.
- **The referential/policy repositories** (pricing, Uber estimation/split) are config, not tenant data —
  they take `Public` and are unaffected in substance.

Honest residual: this touches nearly every read path, so it wants to land as **one focused change**, not
spread across feature work where a missed call site hides in a diff about something else.

## 5. Alternatives considered

- **(a) Enforce in the GraphQL resolvers.** Rejected: the rule would live in the outermost layer, so
  `/files`, SSR and the future mobile transports each reimplement it. Also leaves the read ports still
  unsafe by signature for any future caller.
- **(b) Extend `@auth` in api.yaml to per-instance rules.** Rejected: the ACL is declarative over
  *operations and roles*; it has no access to the argument values or the row. Making it instance-aware
  would mean inventing a policy language inside the schema — a second authorization engine.
- **(c) Postgres RLS with the JWT claims.** Rejected: forks the ACL into two engines that must agree, and
  our authorization model lives in the role path (ADR-0047), not in the database session. Also invisible
  to the behaviour tests, which run against the application layer.
- **(d) Load-then-check inside each resolver** (`if row.customer_id != me { Forbidden }`). Rejected: it is
  the procedural version of the same rule — forgettable, leaks existence via 403-vs-404 on collections,
  and doubles the round trips on lists.
- **(e) Denormalize an `allowed_subjects` array onto each read model.** Rejected for the same reason
  PROP-20260725-120055 §3.3 rejected it for files: it freezes *people* rather than *rules*, so rider
  reassignment, staff churn and admin escalation all produce stale grants in both directions.
- **(f) Ship `/files` with CUSTOMER + ADMIN only, defer the rest.** Rejected as a way to avoid this work:
  it would 403 the restaurant on the order photo it had just uploaded, which guts the feature it was
  meant to unblock.

## 6. Decisions this proposal asks the product owner to make

| # | decision | recommendation |
|---|---|---|
| **D1** | Priority relative to [#134](https://github.com/TheCaptainCompany/captain-food/issues/134) — this is filed as a *blocker* for attachments, but on its own terms it is a live cross-tenant read exposure | treat as **higher** priority than the epic it blocks; re-prioritising is a product-owner call made in the project, so this is a recommendation only |
| **D2** | **Restaurant staff membership model**: single `authRef` per restaurant (simplest) vs a staff↔restaurant membership table (multi-staff, future roles-within-a-restaurant) | membership table — the single-`authRef` shortcut is a retrofit the first time a restaurant has two employees |
| **D3** | Scope for `Public` reads — confirm which projections are legitimately unrestricted (restaurant discovery, catalog, referential policies) | as listed; anything else defaults to scoped |
| **D4** | Land as one focused change vs incrementally per repository | one change (§4) — a missed call site is invisible inside unrelated feature diffs |

## 7. Completeness obligations (ADR-0032)

- **Rule** in `rules.yaml`: *a read never returns a row outside the caller's scope*.
- **Behaviour tests** per role, and the **negatives are the point**: customer → another customer's order;
  restaurant → another restaurant's order; rider → an unassigned job. A test that only proves the happy
  path proves nothing here.
- **Observability**: authorization-denial rate as an operator signal — a spike is enumeration, not user
  error.

## 8. Verification plan

- `make rust` green; `make validate` at 0 errors, no new warnings.
- The negative behaviour tests of §7 fail on `main` today (they are a reproduction of the hole) and pass
  after the change — the honest before/after.
- A compile-level check that an unscoped read is inexpressible: removing the scope argument from a call
  site must fail the build, not a test.
