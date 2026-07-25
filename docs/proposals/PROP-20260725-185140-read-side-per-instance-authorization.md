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

### 3.3 One generic check: a **binding** (where the scope comes from) × a **resolution** (how membership is tested)

The check is a single generic function used by every surface. It takes two inputs that must not be
confused, because they come from different places and answer different questions:

| input | question | declared in |
|---|---|---|
| **binding** | *which scope instance is this request about?* → `(scope_type, scope_id)` | **api.yaml** per operation (GraphQL) · the **`files` row** (`/files`) |
| **resolution** | *what does membership in a scope type mean for a role?* → table/column | **`scope_membership.yaml`**, shared by both |

```rust
fn authorize(binding: &ScopeBinding, principal: &ReadScope) -> Allow | Deny
```

Separating them is what keeps it generic. If the table/column triple lived on each operation it would
be repeated on every query touching an order; if the binding lived in the membership DSL it could not
vary per operation. **api.yaml says *which* order; `scope_membership.yaml` says what "member of an
order" means.**

#### 3.3.1 The binding — api.yaml for GraphQL, the row for `/files`

api.yaml already owns the role ACL, so the instance binding belongs beside it — one place answers both
*which roles* and *on which instance*:

```yaml
# specs/api.yaml
orderConversation:
  roles: [CUSTOMER, RESTAURANT, RIDER, ADMIN]      # existing: the @auth ACL
  scope: { type: ORDER, from: arg.orderId }        # NEW: the instance binding
```

`from: arg.orderId` names the argument carrying the scope id, so the guard stays generic — it never
knows what an order is.

**`/files` has no api.yaml entry, and that is exactly the case you flagged.** It is one route serving
any object, so its binding cannot be declared per operation — it comes from the **`FilesRow`**:
`scope_type`, `scope_id` and `audience` are columns, written at upload time. **The row *is* the
declaration.** Same checker, same resolution table; the binding is data instead of spec. That is the
whole reason those three columns exist on the row rather than being inferred.

#### 3.3.2 Where it runs — middleware for `/files`, executor extension for GraphQL

The check is cross-cutting and must never be re-implemented per resolver. But the interception point
differs, and this is a **real constraint, not a detail**:

- **`/files` → a genuine Axum middleware.** One route, one object, binding from the row. Exactly as
  intended: `layer(from_fn(scope_guard))` in front of the handler.
- **GraphQL → an async-graphql *extension/guard*, not an HTTP middleware.** An Axum middleware runs
  **before the GraphQL body is parsed**: at that point the request is an opaque JSON blob, so the layer
  cannot know which query was selected or what its arguments are — and one POST may carry several
  operations with different scopes. The equivalent hook inside the executor (`Guard`/`Extension`) runs
  per operation/field, *after* parsing, where the selected field and its arguments are known.

Both call the same `authorize`. "Middleware" in the sense that matters — one cross-cutting
implementation, zero authorization code in resolvers — holds; only the mounting point differs, because
HTTP layering physically cannot see into a GraphQL body.

#### 3.3.3 What the guard cannot do: list queries

A guard authorizes *a named instance*. **List queries have no scope-id argument** — "my orders" is not a
check, it is a filter. `orders`/history must be enforced by the repository predicate (§3.1), not by the
guard, and no amount of middleware changes that.

So api.yaml declares which of the two applies:

```yaml
order:   { scope: { type: ORDER, from: arg.orderId } }   # by-id  -> guard checks
orders:  { scope: { type: ORDER, filter: customer_id } } # list   -> repository filters
```

Stating it explicitly because the failure is silent: mount the guard, see it pass on every by-id query,
and assume lists are covered — while `orders` happily returns every customer's history.

#### 3.3.4 Resolution

**Resolution must be generic over `scope_type`.** `scope_type` is a configured column on the `files`
row, so hand-writing one SQL statement per (scope type × role) is a combinatorial trap: it is N×M
statements, and adding a scope type means editing code in a security-critical path. That defeats the
point of making the scope configurable at all.

Instead, membership resolution is **declared as DSL** — consistent with ADR-0001 (the YAML DSL is the
source of truth) and ADR-0037 (schema-driven codegen). Every membership rule is the same 3-tuple:
*which table, which column holds the scope key, which column holds the principal.*

```yaml
# specs/database/scope_membership.yaml
ORDER:
  description: "Membership in one order."
  resolvers:
    ADMIN:      { always: true }
    CUSTOMER:   { table: OrderTracking,    scope_column: order_id, principal_column: customer_id }
    RESTAURANT: { table: OrderTracking,    scope_column: order_id, principal_column: restaurant_id }
    RIDER:      { table: View_DeliveryJob, scope_column: order_id, principal_column: rider_id }

RESTAURANT:
  description: "Membership in one restaurant (KYC documents, cover photos)."
  resolvers:
    ADMIN:      { always: true }
    RESTAURANT: { identity: true }   # the principal id IS the scope id — no query at all
```

Three forms cover every case: `always` (admin), `identity` (the principal *is* the scope — a pure
comparison, zero queries), and the table/column triple (one indexed `EXISTS`). Adding a scope type
becomes a **data change plus a regenerate**, not new code in the guard.

**Generic in the spec, static in the binary.** The codegen emits a `match` with one `sqlx::query_scalar!`
per arm and the identifiers baked in as **literals** — it does **not** build SQL strings at runtime:

```rust
// GENERATED — no runtime SQL construction, no dynamic identifiers
match (scope_type, scope) {
    (_, ReadScope::Admin) => true,
    (ScopeType::Order, ReadScope::Customer(id)) => sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM "OrderTracking" WHERE order_id=$1 AND customer_id=$2)"#,
        scope_id, id.0).fetch_one(pool).await?.unwrap_or(false),
    // … one arm per declared resolver …
    (ScopeType::Restaurant, ReadScope::Restaurant(id)) => id.0 == scope_id,   // identity
    _ => false,                                                               // deny by default
}
```

This is the load-bearing detail. Interpolating a table name from a runtime string would (a) reintroduce
SQL injection into the one code path that must not have it, and (b) forfeit SQLx's **compile-time
checking** (CLAUDE.md), so a renamed column would fail in production instead of at build. Generating the
arms keeps both properties while the *authoring* stays declarative.

**Deny by default, but never silently.** An unmatched `(scope_type, role)` pair returns `false` at
runtime, and the validator **rejects the spec at build time** if any role reachable in a `FileKind`'s
`audience` lacks a resolver for that scope type. Silent denial is a confusing outage; silent permission
is a breach. Neither is acceptable, so the gap is caught by `make validate` (ADR-0032 completeness,
the bidirectional style already used for rules↔tests).

**Caching:** memoize per `(scope_type, scope_id)` in request extensions — a conversation thread
rendering 5 images then costs 1 membership check, not 5. Beyond the request the two halves have
*opposite* volatility and must be cached differently:

- **`sub` → domain id** (§3.1, one bridge lookup per request): **immutable** — an auth subject's
  `customerId` never changes — so cache it hard and long.
- **`is_member`**: **volatile** — cache negatives freely, positives briefly or not at all. Instant
  revocation on rider reassignment is the entire point of the `audience` × `scope` design
  (PROP-20260725-120055 §3.3), and a 60-second positive cache hands the previous rider a 60-second
  window after they are off the job.

Resolving the principal to a domain id **once per request** is what keeps step 2 join-free: the
membership check compares two ids against an index, rather than re-joining `Customer.auth_ref` on every
file in the thread.

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
| **D5** | Membership resolution as **DSL + codegen** (§3.3.4) vs hand-written per scope type | DSL — hand-writing is N×M statements in a security-critical path, and a new scope type should be a data change, not a code change |
| **D6** | Scope **binding** declared per operation in **api.yaml** (`scope: { type, from }`), beside the existing `roles` ACL (§3.3.1) | yes — api.yaml already owns *which roles*; *which instance* belongs in the same place, and `/files` supplies the same binding from its row |
| **D7** | List queries are **filtered by the repository**, not guarded (§3.3.3) — the guard cannot authorize a query with no scope-id argument | confirm; api.yaml marks each op `from:` (guard) or `filter:` (repository) so the gap cannot be assumed away |
| **D4** | Land as one focused change vs incrementally per repository | one change (§4) — a missed call site is invisible inside unrelated feature diffs |

## 7. Completeness obligations (ADR-0032)

- **Rule** in `rules.yaml`: *a read never returns a row outside the caller's scope*.
- **Validator gates** — a missing rule fails `make validate`, never degrades to a silent deny in prod:
  - every role reachable in an `audience` has a declared resolver for that `scope_type` (§3.3.4);
  - **every api.yaml operation returning tenant data declares a `scope:`** — either `from:` (guarded)
    or `filter:` (repository-enforced). An operation with neither is a hole, and the validator is the
    only thing that can see it, since both failure modes are silent at runtime (§3.3.3).
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
