# ADR-20260818-004646 — No business identifier lives in the identity provider: the token carries the auth subject, the mapping lives in our Postgres

**Status**: Accepted · **Date**: 2026-08-18 ·
**Decider**: the **FOUNDER / Tech CEO**, ruling on the night of 2026-08-17/18 after the whole roster
was consulted (`Consulted:` block below, ADR-20260812-143619) ·
**Register**: [DECISIONS §46](../proposals/DECISIONS.md) **IDENT-1** ·
**Amends**: [ADR-0015](0015-wrap-supabase-auth-behind-graphql.md) (Supabase Auth wrapped behind our
GraphQL) in the direction its own posture already claimed, and **reverses the read-scope half of
#433 / CARD-11** — the team decision that replaced the per-request `by_auth_ref` bridge with a
claim ·
**Relates**: [ADR-20260813-004634](ADR-20260813-004634-supabase-auth-is-retained-for-v0-and-the-window-closes-at-the-first-real-order.md)
(the provider is retained for V0) · [ADR-0041](0041-acting-user-is-envelope-not-payload.md)
(`domain_events.user_id` is the auth subject, envelope metadata) ·
[ADR-20260807-002705](ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md) (self-hosted Postgres) ·
[ADR-20260818-004647](ADR-20260818-004647-database-level-security-lands-at-the-cutover-and-the-settlement-read-returns-to-scope.md)
(the same night's database-security ruling, which this one sequences before) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## Status

Accepted. **V0** — asked explicitly whether this was V0 or post-first-order, the founder answered
*"v0"*, so it sequences **before** the write-side enforcement seam
([#178](https://github.com/TheCaptainCompany/captain-food/issues/178) slice 1), not after it.

## The ruling

> **"No business info stored inside Supabase; the mapping with business identifiers will be done in
> the OVH Postgres."**

Read literally and adopted literally: a token proves **who authenticated** — the auth subject
(`sub`) and the standard registered claims the signature covers. It carries **no Captain Food
identifier**. The `sub → domain id` mapping is resolved from our own Postgres, by us, per request.

## The present state — measured, not assumed

> ⚠️ **CORRECTED 2026-08-18** (records-only run, measured on `main` at `987416c`). As first written
> this section said that **four** business identifiers are stored in the identity provider and that
> the work is to extend the `by_auth_ref` bridge to three more roles. **Exactly one is stored**, and
> for three of the four roles there is nothing to extend, because those roles **cannot authenticate
> at all**. **The founder's ruling above is unchanged and still right** — only the measured premise
> beneath it was wrong, and the ruling's **cost is correspondingly smaller than recorded: one role's
> lookup, not four.** A third correction, below, is a security fact the original text missed
> entirely. Records that are wrong are worse than records that are missing, because the next session
> cites them.

**This is still a change, not a confirmation of the existing posture.** CLAUDE.md describes Supabase
Auth as *"wrapped, identity-only — no business data"*, and that is false today — for **one**
identifier, for **one** role.

### Correction 1 — one business identifier is stored in the provider, not four

`crates/infrastructure/src/integrations/supabase_auth.rs:424-433` (`stamp_put_body`) writes
`app_metadata.captain_food = { role, customer_id }` and nothing else; `role` is hardcoded
`"CUSTOMER"` by construction. Grepping that same file for `restaurant_id`, `rider_id` or
`restaurant_account_id` returns **nothing**. The CUSTOMER's `CustomerId` is the whole of the
business data the processor holds, so **Phase C erases one key on one role's users**, not four.

### Correction 2 — the other three claim fields are read by the verifier and written by nobody

`struct ProductClaims` (`crates/server/src/auth.rs:311-333`) declares `restaurant_id`,
`restaurant_account_id` and `rider_id` beside `customer_id`, and `Principal::role_path` binds them
at `auth.rs:207`, `:211` and `:216`. **No code path in the repository populates them.** Their only
other occurrences are that verifier's own unit tests (`auth.rs:1864`, `:1867`). They are a
**declared-but-unfed** surface: they describe an intention (#144, #433) that was never fed, and the
original text read the declaration as if it were a stored fact.

### Correction 3 — `Identity::Unbound` does not deny on the WRITE path (a security fact)

The original text called `Identity::Unbound` *"fail-closed"*. That is true of the **read** path only:
`read_scope` maps it to `ReadScope::Public` and counts it (`crates/server/src/auth.rs:1802-1805`,
`bridge_unresolved`). On the **write** path it is not:

- `Principal::role()` returns the declared role for it — `crates/server/src/auth.rs:251` is
  `Identity::Unbound { role, .. } => *role`.
- The mutation guard consults the role and nothing else: `approveRefund` carries
  `guard = "RoleGuard::new(ALLOW_RESTAURANT_ADMIN)"`
  (`crates/server/src/graphql/generated/mutation.rs:6166`), `ALLOW_RESTAURANT_ADMIN` is
  `[Restaurant, Admin]` (`crates/server/src/graphql/generated/acl.rs:112`), and `role_allows` is a
  membership test on the request's role (`crates/server/src/graphql/acl.rs:76-78`).

So a token asserting the RESTAURANT role and carrying **no business identifier** reaches
`approveRefund` and can approve **any** pending refund — the mutation resolves its actor from the
payload's `orderId` (`mutation.rs:6186`), never from the caller. This is the §39 **IDOR-1** surface
seen from the identity side, and it is one more reason the ruling is right: a token that proves only
*who authenticated* forces the binding to be resolved by us, where it can be checked.

**Exposure today is nil, and dated.** `app_metadata` is writable only through the provider's
privileged server-side path, and the only writer in this repository stamps `role: "CUSTOMER"`
(Correction 1) — so no such token can exist unless one is hand-stamped in the provider's console,
and none is. The exposure opens at the **first pilot sign-in for a non-CUSTOMER role**, which is
also the IDOR-DEADLINE trigger (§45 **IDOR-DEADLINE**).

## The mechanism already exists — for exactly one role

The bridge this decision promotes is not new:

- **`crates/application/src/queries.rs:341`** — `CustomerReadRepository::by_auth_ref(ExternalReference)`,
  implemented at `crates/infrastructure/src/persistence/customer.rs:53-55` as an indexed lookup on
  `Customer.auth_ref`.
- **`crates/infrastructure/src/mailbox/handler.rs:244-258`** — `MailboxCommandHandler::resolve_actor`
  calls it, **only when `message.user_type == "CUSTOMER"`**; every other role gets `domain_id: None`
  ("other roles stay None until their bridges land (#144)", its own comment).

The decision **promotes that bridge to the request seam**. It does **not** extend it to three more
roles, because — see below — three of the four roles have no way to sign in, so there is no subject
to key a mapping on.

### There is nothing to extend: three of four roles cannot authenticate at all

⚠️ **CORRECTED** — this subsection replaces a table that priced *"extend the bridge to the other
three roles"* as this ruling's largest cost. That work does not exist to be done.

The only authentication operations in the whole DSL are in `specs/customer/api.yaml`:
`requestPhoneVerification` (:38, roles `[PUBLIC, CUSTOMER]`) and `verifyPhone` (:43, same), plus the
V1 email pair (`requestEmailVerification` :50, `confirmEmailVerification`, both `[CUSTOMER]`).
Nothing in `specs/*/api.yaml` offers a sign-in to RESTAURANT, RESTAURANT_ACCOUNT or RIDER.

| Role | Can it authenticate? | The `sub -> domain id` fact today |
|---|---|---|
| CUSTOMER | **Yes** — phone OTP (`specs/customer/api.yaml:38`, `:43`) | **Exists end to end** — `CustomerRegistered.authRef` (`specs/customer/events.yaml:24`), projected to `Customer.auth_ref`, indexed and nullable (`specs/database/tables/projection_tables.yaml:395-398`) |
| RIDER | **No sign-in operation exists** | Event only — `RiderRegistered.authRef` (`specs/delivery/events.yaml:343-351`) and `RegisterRider.authRef` (`specs/delivery/commands.yaml:196`), projected into no column: `auth_ref` appears exactly once in the whole projection set, on `Customer` |
| RESTAURANT | **No sign-in operation exists** | Does not exist — no `authRef` in `specs/network/**` |
| RESTAURANT_ACCOUNT | **No sign-in operation exists** | Does not exist — same |

**What follows, and it reshapes the chunk**: for those three roles there is no login flow, no claim
writer and no binding fact, so **nothing to migrate and no subject to key a mapping on**. Giving
them a `sub -> domain id` mapping is not identity plumbing that this ruling can order — it is
**staff onboarding**, a product question about how a restaurant operator, an account manager and a
rider come to exist as sign-in-capable people at all. It is tracked as
[#639](https://github.com/TheCaptainCompany/captain-food/issues/639), and it is a founder-owned open
question in the register (**STAFF-AUTH**, [DECISIONS §46](../proposals/DECISIONS.md)) — until it is
answered, standing up any non-CUSTOMER role for a pilot means hand-stamping a claim in a third-party
console, which is exactly the shape Correction 3 shows nothing checks.

**So this ruling's implementable scope is one role**: move CUSTOMER's `sub -> domain id` resolution
from the token to our Postgres, stop stamping, erase what is stored. That is the whole of it.

## The cost, stated honestly — `read_scope` stops being pure

This is the price, and it is not softened here:

⚠️ **CORRECTED scale**: the price below is real, and it is paid **for one role**. CUSTOMER is the
only role that can authenticate (see above), so this is one indexed lookup per authenticated
customer request — not four roles' worth of new bridges.

- `crates/server/src/auth.rs:1833` — `resolve_read_scope(&Principal, RequestCorrelationId)` is
  **synchronous** and delegates to a pure `read_scope(principal)`. After this decision it needs a
  repository and an `.await`.
- `crates/server/src/graphql/routes.rs:166` — it is called **once per GraphQL request**, and again
  per WebSocket connection at `routes.rs:285`.
- The comment justifying that placement (`routes.rs:162-165`) reads: *"a PURE function of the
  token's claims (CARD-11), **no lookup, no dependency that could be missing**"*. **That sentence
  becomes false** and must be rewritten rather than left standing.
- Consequence in one line: **the enforcement slice's "zero I/O at peak" claim dies with it.**
  Resolving the actor's domain id becomes a database lookup per authenticated request, on the read
  path too — and peak is Friday/Saturday 19:00–21:30.

Mitigations are conceivable (a request-scoped resolution reused by every resolver, the existing
index on `auth_ref`, a bounded cache with an explicit invalidation story). **None is decided here,
and an undesigned mitigation is not a mitigation.** Whatever is chosen carries its own
`specs/observability.yaml` contract: this lookup is on the authenticated read path, so its latency
and its failure rate are operator-visible facts, not implementation detail.

Two failure classes also move:

- **Gone**: `StampFailure::ClaimConflict` (`supabase_auth.rs:331-360`) — the "auth user already
  carries a different `captain_food.customer_id`" defect cannot exist once nothing is stamped.
- **New**: the mapping lookup can be *unavailable*. It **fails closed** — no row, or no answer, is
  `Public`, never an elevation — and the difference between *"no mapping"* and *"could not ask"*
  must be distinguishable in telemetry, because they have opposite operator responses.

## Why now is the cheapest moment this will ever be

- Production is **suspended, as a recorded state** (DECISIONS §45 **PROD-1**), and there is no real
  phone-verified end user (**Q-L3 = no**, ADR-20260812-214021). No live session depends on a claim,
  so **dropping the claims strands no issued credential**.
- After a pilot it is a different act entirely: every issued credential carries a claim the server
  has stopped reading, and any transition that needs the claim gone forces a **re-auth of every
  credential** — the one migration cost that is paid by users rather than by us.

## The three-question test (CLAUDE.md), explicitly

**(1) Does it contradict or create a recorded decision?** It **creates** one, and it **reverses**
the read-scope half of #433 / CARD-11 — the team's own decision to replace the per-request
`by_auth_ref` bridge with a claim, which is why `auth.rs:1782-1784` records the bridge as
*"replaced"*. A founder ruling plus this ADR plus the register row **is** the reversal record.
Nothing else about #433 is reopened: the role-path door, the single-private-value `Identity`, and
the fail-closed posture all stand.

**(2) Is the shape already emitted, stored or promised?** **Yes — so this is a MIGRATION**, and the
versioning story is recorded here, before it lands:

1. **Phase A — resolve, do not trust.** Build the mapping port at the request seam and resolve
   `sub → domain id` from Postgres. Tokens in the wild still carry `captain_food.customer_id`; the
   server simply **stops reading it**. No token is invalidated, no re-auth, no user-visible event —
   this ordering is the property that makes the whole migration safe.
2. **Phase B — stop writing.** Retire the admin claim-stamp path so no new business identifier
   reaches the provider.
3. **Phase C — erase what is stored.** Remove `captain_food.customer_id` from the stored
   `app_metadata` of existing auth users. This is also GDPR hygiene: it is a business identifier
   held by a processor for no remaining purpose.
4. **Stored events are untouched.** `domain_events.user_id` is the auth subject (ADR-0041), not a
   domain id; nothing in the log changes, and there is **no upcasting** on the event side. ⚠️
   **CORRECTED**: this clause used to end *"the only new stored shapes are the missing mapping facts
   for RIDER/RESTAURANT/RESTAURANT_ACCOUNT"*. Those facts are **not in this ruling's scope** — those
   roles have no sign-in at all, so the mapping question does not arise until **STAFF-AUTH** is
   answered ([#639](https://github.com/TheCaptainCompany/captain-food/issues/639)), and whatever
   answers it re-enters this three-question test on its own. **This migration introduces no new
   stored shape.**

**(3) Otherwise it is the team's** — not reached: (1) and (2) both fire.

## What this does NOT change

Asymmetric JWKS verification and the refusal of symmetric algorithms at the `/{role}/graphql` door;
ADR-0015's ACL wrapping of the provider; `domain_events.user_id` as the acting subject; the
fail-closed default of the READ path. The provider keeps doing the one job it is retained for —
proving that a human authenticated.

It also does **not** fix Correction 3: the write-side guard consults the role and nothing else
before and after this ruling. Resolving the binding from our Postgres is what makes a per-instance
write check *possible*; it is not that check. That is [#178](https://github.com/TheCaptainCompany/captain-food/issues/178)
slice 1 and the `requires:` emitter ([#636](https://github.com/TheCaptainCompany/captain-food/issues/636)).
The narrowing of `approveRefund`'s own role set is a **founder decision, not a consequence of this
one** — it removes the restaurant's ability to approve a refund on its own orders.

## Named residue — not decided here

- **Does `role` stay in the token?** This ADR records the founder's framing as given: the subject is
  *the only thing a token carries*, so `role` is resolved from the mapping too. A Captain Food role
  is arguably business information (it states that this person is a restaurant operator in our
  marketplace), which is the reading taken. **If the founder meant `role` to remain a claim, that is
  a one-line correction to this record** — and it changes the cost materially, because the role-path
  door (`AuthContext::authorize`) currently decides on the claim before any lookup exists.
- **The `specs/**` changes this implies are OWED and NOT APPROVED.** Named so the next session does
  not rediscover them: `specs/services.yaml:189` (`identity.stamp_customer_claim`),
  `specs/observability.yaml:718` (the claim-stamp contract) and `specs/common/configuration.yaml:221-222`
  (the declared configuration of the admin stamp path). ⚠️ **CORRECTED**: the RIDER `auth_ref`
  projection column and the RESTAURANT / RESTAURANT_ACCOUNT mapping facts used to be listed here as
  owed by this ruling. They are not — they belong to **STAFF-AUTH**
  ([#639](https://github.com/TheCaptainCompany/captain-food/issues/639)), which must be answered
  first, because a mapping needs a subject and those roles have none. None of this is touched by a
  records-only change.
- **The write-side guard is untouched by this ruling** (Correction 3), and narrowing
  `approveRefund`'s role set is a **founder decision** in its own right, deliberately not taken
  here.

## Consulted (ADR-20260812-143619)

Thirteen lenses were asked before any answer was composed. One clause each; a lens with nothing to
say on this ruling is recorded as such, and **no lens output is legal advice or clearance**.

- **evans** — the `requires.acting` grammar **already exists and is validated**
  (`specs/comms/actors.yaml:65-72`; `tools/codegen-rs/src/refs.rs:453-455` binds
  `*.receives[*].requires.acting.*` to a `StateField`, with rules `requires-acting-untyped` and
  `req-state-unknown`), so the actor-side vocabulary for "who may act" needs no new invention —
  which is what makes moving the *resolution* of the actor a self-contained change.
- **vernon** — the record **layers on rather than supersedes** the seam design; and
  `Restaurant.accountId` is **declared but unfolded**: only 3 of the 15 `type: aggregate` actors
  declare a `state:` block at all — antecedents: `grep -rn '^  state:' specs/*/actors.yaml | wc -l`
  = **3** (comms, ordering, payments) against `grep -rn '^  type: aggregate' specs/*/actors.yaml | wc -l`
  = **15**, measured on `b77c487` — and `Restaurant` is not one of them — so a `requires.acting` binding for the restaurant roles has nothing to bind to today.
- **architect** — found the **three process-manager-received commands** (`PlaceOrder`,
  `ApproveRefund`, `DenyRefund`, `specs/payments/actors.yaml`) that an aggregate-keyed rule cannot
  see, and filed
  [#635](https://github.com/TheCaptainCompany/captain-food/issues/635) and
  [#636](https://github.com/TheCaptainCompany/captain-food/issues/636).
- **holub** — warn-only enforcement has **already been rejected on the record** for the
  configuration gate (ADR-20260729-010500 §"the warn-only rollout (PROP D5) was dropped,
  deliberately"; `docs/STATUS.md:4686`); he reported a **second** prior rejection, which this run
  could not locate in the tree — `UNVERIFIED input`, and his session finding stands as the record
  for it. He also named the **competing per-actor role model** that a claims-free token pushes
  against.
- **legal-specialist** — named the **settlement/RLS money hazard** and the **rider-own-data**
  problem; both are carried by
  [ADR-20260818-004647](ADR-20260818-004647-database-level-security-lands-at-the-cutover-and-the-settlement-read-returns-to-scope.md).
  On this ruling: removing a business identifier from a processor's store is a data-minimisation
  improvement, not a compliance claim. *A grade, not clearance.*
- **dba** — consulted on the RLS and rollout surface (see the companion ADR); on this one, the
  lookup rides an existing index (`Customer.auth_ref`, `index: true`) and the interesting question
  is the per-request repetition, not the query.
- **farley** — consulted on the rollout surface: Phase A is invisible to clients by construction,
  which is what makes it deployable without a window; the phase that is NOT invisible is Phase C.
- **beck** — on the test shape: the seam is where the test goes, and the negative case (a `sub` with
  no mapping row resolves to `Public`) is the one that must exist before the positive one.
- **young** — nothing in this ruling changes fold or projection semantics; the mapping is a lookup
  over an existing read model, not a new authority on state.
- **graphql-architect** — the resolution point is the transport boundary either way, so the schema
  and the `/{role}/graphql` role-path shape are unaffected; the change is the *arity* of the work
  done at that boundary.
- **observability-agent** — a new per-request dependency on the read path needs its own contract
  before it ships; *"no mapping"* and *"could not ask"* must not collapse into one signal.
- **ux-designer** — nothing in this lens: no customer-visible surface changes, and Phase A causes no
  sign-out.
- **business-specialist** — nothing in this lens beyond the timing argument already in the decision:
  doing it before any pilot costs nobody anything; doing it after costs every user a re-auth.

**Appended for the 2026-08-18 correction** (the run that rewrote the present-state section; the
ruling itself was not reopened, so the roster was not re-consulted):

- **architect** — measured the premise this record had stated and found it wrong in three places:
  the provider holds **one** business identifier, not four
  (`crates/infrastructure/src/integrations/supabase_auth.rs:424-433`); the three other claim fields
  are read by the verifier and **fed by nothing** (`crates/server/src/auth.rs:311-333`, bound at
  `:207`/`:211`/`:216`, populated only in that file's tests); and `Identity::Unbound` **does not
  deny on the write path** (`auth.rs:251` returns the declared role;
  `crates/server/src/graphql/generated/mutation.rs:6166` guards `approveRefund` on role alone). The
  finding that reshapes the chunk is the one underneath all three: **three of the four roles have no
  authentication operation anywhere in the DSL**, so their "missing mapping fact" was never identity
  plumbing — it is staff onboarding, and it belongs to the founder
  ([#639](https://github.com/TheCaptainCompany/captain-food/issues/639), register row
  **STAFF-AUTH**).
