# ADR-20260830-191457 — A role guard takes a witness, not a path segment; and an unbound caller is recorded as PUBLIC

**Status**: Accepted · **Date**: 2026-08-30 ·
**Decider**: the team, executing a slice already ruled on — no founder decision is taken or reopened
here ·
**Realizes**: [#639 "STAFF-AUTH: restaurant staff, account managers and riders cannot sign in at
all"](https://github.com/TheCaptainCompany/captain-food/issues/639) parts **A** and **B** ·
**Implements**:
[ADR-20260818-094500](ADR-20260818-094500-staff-auth-mechanism-and-refund-approval-stays-with-the-restaurant.md)
(ruling A — the rider's identity; ruling B — refund approval stays with the restaurant, so the hole
must be closed by BINDING) ·
[ADR-20260818-101500](ADR-20260818-101500-the-restaurant-signs-in-by-email-link-and-638-freezes-at-chunk-1.md)
(the banked constraints: `unbound ⇒ denied`, `Identity::Unbound` never stamps a role, every
transport, `claimRestaurantListing` resolved explicitly) ·
[ADR-20260818-004646](ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md)
(Correction 3 is the defect; the mapping resolves in our Postgres) ·
[ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) (compiler
first) ·
**Session**: https://claude.ai/code/session_01BXTg9ZhjzYHyRkVq3g9uxJ

## Status

Accepted and landed. **Part C is not claimed and is not started**: it is AMBER in #639's own
estimation table and owes a proposal plus a founder decision. Nothing here decides how staff sign
in.

## What was wrong

`RoleGuard::check` tested membership of a `RequestRole` read out of the GraphQL context, and
`routes.rs` put that value there by parsing the URL path segment. The verified `Principal` was never
consulted. So a token asserting `captain_food.role = "RESTAURANT"` and carrying no `restaurant_id`
became `Identity::Unbound`, whose `role()` returned RESTAURANT, and it satisfied `approveRefund`'s
`ALLOW_RESTAURANT_ADMIN` guard — a mutation that resolves its actor from the payload's `orderId` and
never from the caller. That is approval of **any** pending refund, on the money path.

The same identity also stamped its asserted role into `domain_events.user_type`.

## Decision 1 — the guard's input is a witness type, minted only from an identity

`ActingRole` is a newtype over `RequestRole` with a private field, declared in a **child module** of
`auth` (`mod acting_role`), so its only producer is `ActingRole::of(&Identity, path_role)` — which
owns the `Identity` match itself. The child module is the `FetchIntent` shape and it is there for
the reason that type records: **privacy is MODULE-scoped, not type-scoped**, so a private field
declared beside `Principal` would still leave `ActingRole(RequestRole::Restaurant)` spellable
everywhere in `auth.rs`, including in the function whose mistake this exists to prevent.

`of`'s `Identity::Unbound` arm yields PUBLIC and no other arm can. The privileged value therefore
does not exist for that caller — the refusal is a property of the type rather than a check.

Three consequences taken deliberately:

- **The witness takes the PATH role, not the identity's own role.** The ACL is path authorization
  (ADR-0006), and on `/public` an identified customer (`Principal::public_customer`, #469 — the
  storefront IS the open path) must still evaluate as PUBLIC. Deriving from the identity would make
  every `roles: [CUSTOMER]` operation reachable, and introspectable, from the one path anyone can
  reach.
- **It is the ONLY role value in the GraphQL context now.** The bare `RequestRole` injection is
  gone, and the four ad-hoc `matches!(ctx.data_opt::<RequestRole>(), Some(Admin))` sites in the
  generated resolvers were swept onto `request_role(ctx)` in the same change. Two role values in one
  context, with the same type name meaning "who I am" at four sites and "which URL I used"
  everywhere else, is the shape the next reader gets wrong — and the wrong one was the privileged
  one. Not exploitable today (`role_permitted` is strict equality, so a path role of ADMIN implies a
  verified ADMIN), swept anyway so the emitter stops reproducing the idiom.
- **Absence fails closed to PUBLIC, and forgetting to inject is a COMPILE error.** A context bag is
  a dynamic type-keyed lookup, so "is it present?" can never be a type property. What can be is
  this: `authorize_and_resolve_scope` — the one function both GraphQL transports go through —
  returns the witness as a tuple element they both destructure. Dropping it does not compile. The
  third transport, the SSR page renderer, derives its own from `Principal::anonymous()` rather than
  naming PUBLIC.

**Rejected: computing the acting role inside `role_allows` from the `Principal` and `RequestRole`
already in the context.** It removes the injection step, which is real, but its failure mode when
the `Principal` is absent is to fall back to the PATH role — reopening precisely this hole — whereas
the witness form's failure mode is PUBLIC. A safe default beats a removed step.

**Rejected: a public or test-only constructor for `ActingRole`.** Either deletes the guarantee, and
the executor reaches for one at the first `cargo test` (the ACL suite is a separate crate and cannot
spell a private field). The repair instead is `Principal::role_binding(role, sub, binding)`, a
public constructor that runs the SAME match `authorize` runs: it can produce an unbound principal
(`binding: None` — the case a test must be able to spell) but cannot produce a *lying* one. Every
role assertion in the server test suite now names the identity holding the role rather than a bare
enum somebody typed, which is the property under test.

**That migration is where this change nearly shipped a defect, and the repair is a gate.** The sweep
matched the LITERAL spelling `.data(RequestRole::X)`, converted 45 of them, and missed three
variable-bound `.data(role)` sites inside `for role in …` loops — twice in `mailbox_lanes.rs`, once
in `graphql_subscriptions.rs`. All three suites stayed GREEN, because a role that never arrives
reads as absent and fails closed to PUBLIC, and PUBLIC is refused too: three role-refusal loops
asserting nothing, one of which `mailbox_lanes.rs` itself records as having been caught at 4-of-6
coverage on #536 and was now at 0-of-6. Found by the independent reviewer pass, fixed here, and the
recurrence class is now held by `crates/server/tests/role_injection_gate.rs` — a source scan, which
is the "check as fallback" level and is legitimate precisely because the compiler cannot reach it:
`async_graphql::Data::insert` is `TypeId`-keyed over `Any`, so injecting the wrong type is neither
an error nor a warning. Seen RED against the exact mutant before it was trusted, and it asserts it
scanned a non-empty set, because a gate that scans nothing passes forever.

## Decision 2 — `Principal::role()` becomes `recorded_role()`, and Unbound records as PUBLIC

Two questions were being answered by one method. They are now two, named so the difference is
unmissable:

- **`acting_role(path_role) -> ActingRole`** — *what may they do?* Follows the PATH.
- **`recorded_role() -> RequestRole`** — *whose act was this?* Follows the IDENTITY, and is what the
  mutation envelope stamps into `domain_events.user_type` (ADR-0041).

They must not be merged, and the case that proves it is the storefront: an identified customer on
`/public` records CUSTOMER and acts as PUBLIC. Using the acting role for the envelope would stamp
PUBLIC on every storefront order — a regression; using the identity role for the guard is the hole.

`Unbound` now records PUBLIC. Stamping the asserted role writes a **false author into an immutable
log**, which is worse than the authorization hole beside it because events are never rewritten and
the log is what we would later reason from. The declared role survives where it is a DIAGNOSIS
rather than an attribution: `read_scope` destructures `Identity::Unbound { role, .. }` itself to
label `read_authorization_bridge_unresolved_total{role}`.

**Historical rows are not touched.** Events already stamped `RESTAURANT` for an unbound caller are
correct *as history* — they record what the system believed — and rewriting them would destroy the
evidence that the defect existed. No upcaster, no migration, no new `UserType` value. What DID
change is a pair that had never been inhabited: `(user_type = PUBLIC, user_id = <subject>)` now
exists and means "a credential proved no usable role". That reading is recorded in
`specs/common/scalars.yaml#/UserType`, because a fold inferring "anonymous" from PUBLIC would
silently reclassify.

One consumer of `user_type` is not telemetry and was missing from that enumeration: the mailbox's
`resolve_actor` branches on `message.user_type == "CUSTOMER"` to resolve a domain id through
`by_auth_ref`. A claimless CUSTOMER on `/customer` used to enqueue as CUSTOMER and pick up a domain
id there; it now enqueues as PUBLIC with none. Unreachable rather than fixed, and deliberately so:
every CUSTOMER-only operation denies such a caller at the guard, and the only PUBLIC-inclusive
customer mutations are `requestPhoneVerification` and `verifyPhone`, which MINT identity rather than
consume it. Recorded because "unreachable" is a claim about today's API surface, and the next
PUBLIC-inclusive customer mutation is what would falsify it.

One derived signal moved with it, and it took two attempts. The `auth.read_scope` span's
`bridge_resolved` was computed inline as `scope != Public || role == Public || role == External`,
which went silently "always true" the moment Unbound stopped reporting its declared role. Restating
it as a predicate on the IDENTITY alone fixed that end and broke the other, which the independent
reviewer caught: a CUSTOMER under `CustomerIdentitySource::Postgres` whose lookup returns
`NoMapping` or `LookupFailed` **is** a bound identity and degrades to `Public` anyway (#641), so an
identity-only predicate reports the seam's own outage as resolved — and `LookupFailed` is the
PAGE-classed one. `Principal::bridge_resolved(&scope)` asks both questions, which is the only form
right at both ends: an Unbound caller is `false` because nothing could resolve, and a bound caller is
`false` when nothing did.

The span's `business.role` attribute now carries `recorded_role()`, so an Unbound caller's
`auth.read_scope` span reads PUBLIC. The declared role survives on
`read_authorization_bridge_unresolved_total{role}` and nowhere else — thinner attribution than
before, accepted rather than overlooked: a fourth role accessor existing purely for a span attribute
would cost more clarity at the seam than it buys in telemetry.

## Decision 3 — the rider read model is a TABLE, and its constraints are the interesting part

`RiderRegistered` has always carried `authRef` as required and nothing projected it. The new `Rider`
table (`read_common`, `recovery: replay`) is what makes the RIDER role's `sub → domain id` mapping
resolvable in our own Postgres.

A TABLE rather than a `View_*` for the reason `SlugAlias` already states: it is read on every
authenticated request and must never fold on read. Its own group and its own checkpoint, never a
prefix bolted onto an existing one (the #424 lesson).

Three column decisions are load-bearing and each was a checkpoint stop:

- **`auth_ref` is `UNIQUE`, not indexed** — a security property, not a performance one. The
  repository lookup is `fetch_optional`, which on multiplicity returns an ARBITRARY row,
  plan-dependent and without error, and `ScopeMembership` keys grants on `member_id = rider_id`; a
  duplicate would hand one rider another rider's order scope. The constraint converts a silent
  breach into a **denial** — though "visible" overstates it, and part C's author must not read it
  as a promise that an operator is told: under the production default `DbFaultPolicy::Skip` the fold
  fails inside its savepoint, is logged at ERROR and SKIPPED with the checkpoint advancing, so the
  second rider is simply absent until a reprojection, with a log line and no metric. Strictly better
  than an ambiguous resolve; not an alert. It does **not** create the invariant — nothing on the write side
  prevents two `RiderRegistered` with the same `authRef`, and the reservation that would (the
  `slug_reservations` shape) is designed and unbuilt, owed by the sign-in door. `index: true`
  beside `unique:` is deliberately absent: they are separate emitter passes and would emit two
  btrees on one column.
- **`display_name` and `phone` are NOT NULL** — the projector emitter branches on the COLUMN's
  nullability, so a nullable column gets a blind assignment and a NOT NULL one gets an `if let
  Some(v)` guard. `RiderInfoUpdated` is a PARTIAL update despite the `*Updated` replace convention,
  so a nullable `phone` means a name-only update erases the phone, deterministically and on every
  replay. Planting that variant showed it is caught by `rustc` at the hand-written store, not only
  by the test — the generated row field becomes `Option<PhoneNumber>` and the store stops
  typechecking.
- **`phone` carries no unique and no index** — copying `Customer` would have injected a defect:
  `Customer.phone` is unique because it is that aggregate's identity key, whereas a rider is keyed
  by `authRef` precisely so the phone never becomes a domain key. French mobile numbers are
  recycled; a unique phone here is a scheduled future projector fault on a number's second owner.

The table is `internal: true` — an identity/authorization index, the `ScopeMembership` class — and
the declaration says plainly that it is **written and not yet read**. No `c4-l3` `reads:` is
declared, because the declaration follows the code and not the intention.

## What this does NOT close, stated so a green branch is not misread

- **The money path is not bound.** `unbound ⇒ denied` is true at the edge; `other-restaurant ⇒
  denied` is still false everywhere. `approveRefund`'s ownership comparison does not exist, its
  source is still open between `PaymentState.restaurant_id` and a `refund_process_manager` column
  (ADR-20260818-101500), and the ordering defect that ADR records stands: the Stripe refund fires
  before the `Payment-<intentId>` stream is loaded, so an ownership check on that leg as it stands
  would happen after the money moved.
- **No rider can sign in.** Part A ships a fold with no consumer. A green part A is not evidence
  that rider sign-in works.
- **The refusal an incompletely-provisioned restaurateur sees is still wrong.** They get *"role
  PUBLIC is not authorized"*, in English, and the back-office screen renders an empty order queue
  rather than a "not linked" state. Filed, not fixed here.

## Consulted (ADR-20260812-143619)

Reversibility class **HOLD: human** (identity on A, money path on B). Briefed before any code; each
lens named what it would catch, and every item below was verified against the tree by the lens that
raised it.

- **reviewer** (independent, full-diff, post-implementation — the third look) — returned **FAIL**
  on one blocking finding and it was a real one: the test migration had missed three variable-bound
  `.data(role)` sites, leaving three role-refusal loops green while asserting nothing, and two
  sentences in this record and in `STATUS.md` asserted otherwise. Fixed here, with a gate. Also
  found the `bridge_resolved` narrowing at the #641 end, the `DbFaultPolicy::Skip` overstatement in
  the `auth_ref` rationale, and the unrecorded `resolve_actor` interaction — all three corrected
  above. Verified independently that no non-PUBLIC `ActingRole` is obtainable for an unbound caller
  by any path, that the migration is byte-identical to the generated schema, that the store's column
  list matches the DDL, and that no fenced path was touched.
- **beck** — owns the evidence bar this change was held to: four planted violations, each with its
  message recorded (the Unbound guard arm and the envelope arm go RED; dropping the witness from
  either transport is a COMPILE error). Found that the existing assertion
  `unbound.role() == Customer, "the unbound caller keeps its role"` was the defect stated as a
  PASSING test, and that a lone "unbound is refused" assertion passes when `acting_role` returns
  PUBLIC for everything — hence every case asserted as a pair. Named the child-module fix and the
  ACL-suite crisis before either was hit, and its own §5 supplied the "what this does not close"
  section above.
- **young** — the `display_name`/`phone` nullability defect, from the emitter's branch rather than
  from the event's shape; `status` via `from:` and never a `derive:` map, which would be a second
  copy of the Rider lifecycle table on the read side; the checkpoint trap; that this chunk is what
  makes a rebuild of the rider mapping a business event, and the three foreclosures for it; and the
  `(PUBLIC, user_id)` tolerant-reader correction that became the `UserType` gloss. Confirmed no
  upcasting concern: the alphabet does not change, only which callers reach which token.
- **dba** — `read_common` derived from readers rather than from resemblance, and why not
  `captain_write` (a PITR of a `pitr` database would restore a mapping that disagrees with the log's
  head); `unique` over `index` with the `fetch_optional` mechanism spelled out; the double-index
  emitter trap; `phone` unconstrained and why "mirror Customer" is the wrong instinct; that the
  write-side reservation must be designed even if it ships later; and that the index and row width
  are a non-issue at peak, so nobody should optimise them.
- **graphql-architect** — caught the **third transport** (`web_ssr.rs`), which the card named two of;
  verified in the async-graphql source that `visible` is a discovery filter that no validation path
  consults, so guard and `visible` must keep sharing one predicate; enumerated the seven PUBLIC-listed
  operations to show degrade-to-PUBLIC buys zero extra capability, leaving the envelope as the real
  residue; wrote the `claimRestaurantListing` justification ADR-20260818-101500 demanded; and named
  the four ad-hoc `RequestRole` sites and the ADMIN/EXTERNAL blackout risk that the seven-role table
  test now covers.
- **vernon**, **evans**, **holub**, **farley**, **ux-designer**, **legal-specialist**,
  **business-specialist**, **architect**, **observability-agent** — not briefed on this dispatch. The
  slice implements rulings already taken with the full roster consulted (ADR-20260818-094500's
  eleven-lens block), and it introduces no aggregate boundary, no new vocabulary, no user-visible
  surface and no runtime to analyse. The one item that would have gone to **ux-designer** and
  **legal-specialist** — what an incompletely-provisioned restaurateur is told — is deliberately NOT
  designed here and is filed instead.
