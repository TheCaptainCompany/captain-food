# Dispatch — slice chunk 1: bind the refund queue to the caller, on the one operation that can be proved today

- **Issue**: tracking [#618 "Read surfaces missing `ReadScope` — the read half of the write-path authorization gap (#178)"](https://github.com/TheCaptainCompany/captain-food/issues/618) · slice record [ADR-20260818-101500](../adr/ADR-20260818-101500-the-restaurant-signs-in-by-email-link-and-638-freezes-at-chunk-1.md) · rulings [ADR-20260818-094500](../adr/ADR-20260818-094500-staff-auth-mechanism-and-refund-approval-stays-with-the-restaurant.md) · register [DECISIONS §39 IDOR-1](../proposals/DECISIONS.md)
- **Base**: `main` @ **`10866d6`** — verified this run: `git rev-parse HEAD` **and** `git rev-parse origin/main` both `10866d6fa2c324bedbc8e37db03f26350f933b70`, working tree clean, after `git fetch origin main`. The executor now REFUSES a run whose HEAD does not match this line (`.claude/agents/executor.md` precondition 4, founder-approved 2026-08-18): **re-verify it yourself before writing anything** — if `origin/main` has moved, stop and get the card re-based rather than branching from a different tree.
- **Reversibility class**: **`HOLD: human`.** Not because money moves in this diff — none does, and no event shape changes — but on two grounds that the named class covers: (a) it is the **money-path read surface** of §39, the queue whose rows are the input to `approveRefund`; and (b) the failure mode of an over-narrow predicate is an **empty queue**, which is silence, not an error. That is the same silent-skip class [ADR-20260818-004647](../adr/ADR-20260818-004647-database-level-security-lands-at-the-cutover-and-the-settlement-read-returns-to-scope.md) used to refuse a policy on `OrderTracking`: a filter removes rows, it does not raise. A restaurant that cannot see a refund cannot decide it, the money stays captured, and every dashboard is green.
- **Roster**: **whole roster at the BRIEFING**, a lens excuses itself (ADR-20260816-134352: the `HOLD: human` axis sizes the briefing and wins when the two axes disagree). **CHECKPOINT goes only to lenses that declared a concern at briefing**, any lens may opt back in.
- **Merge posture**: `HOLD: human` ⇒ **NOT auto-merge-on-green**. The PR stops at ready-for-review until the TEAM's independent reviewer pass over the full branch diff; after PASS + green gates the coordinator merges ([ADR-20260815-115220](../adr/ADR-20260815-115220-auto-merge-on-green-by-default-hold-human-for-the-named-class.md), amended by [ADR-20260815-134655](../adr/ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md)). **No founder wait.**
- **Branch**: `618-pending-refunds-read-scope`. **Do NOT write `Closes #618`** in the PR body — see "What this does not do".

---

## Why this is chunk 1, and what was rejected

The slice is *"one real Tours restaurateur finds their own restaurant on their phone browser, proves it, signs in, and can see and act on only their own orders and only their own refunds"* — an email-link sign-in, a `sub → domain id` mapping in our Postgres, the claim path, and three operations. Four candidate first chunks were weighed:

| Candidate | Verdict |
|---|---|
| **Scope `pendingRefunds` to the caller's `ReadScope`** (`beck`'s candidate) | ✅ **CHOSEN.** Vertical: it changes what a real persona sees on a real screen (`specs/screens/restaurant_backoffice.yaml:36` binds `refunds.pending` to this query **with no arguments**, so today that widget renders every restaurant's refund queue). It is **binding, not narrowing** — the `restaurantId` arg stays for ADMIN, so the GraphQL schema does not change at all. It needs no new table, no migration, no credential, and no provider. And it is testable **today** in-process with fake repos and no database, by the pattern the immediately-following resolver in the same generated file already uses (`customerCredit`, `crates/server/src/graphql/generated/query.rs`, destructures `ReadScope::Customer` and returns nothing otherwise). |
| **The `sub → domain id` mapping table first** | ❌ Rejected: it is a **layer**, not a vertical step. It leaves the product exactly as good as it was, it is [#641](https://github.com/TheCaptainCompany/captain-food/issues/641)'s CUSTOMER slice plus the staff extension in [#639](https://github.com/TheCaptainCompany/captain-food/issues/639), and it is a MIGRATION (tokens in the wild carry `captain_food.customer_id`) whose phase order must be recorded first — DECISIONS §46 IDENT-1. |
| **The email-link sign-in first** | ❌ Rejected as first: it is the largest piece, it needs a provider mechanism that does not exist, and — decisively — **it is the event that trips the §39 IDOR deadline** (trigger (i): a restaurant credential outside the team, including demos and pilots). Minting the credential before the surfaces it reaches are bound is doing the two halves in the wrong order. |
| **Write-side binding of `approveRefund`/`denyRefund` first** | ❌ Rejected as first, and it is not blocked on taste: those two commands are decided by a **process manager**, not an aggregate, and their payload `{orderId, amount, reason}` has **no field corresponding to the caller** — [#635](https://github.com/TheCaptainCompany/captain-food/issues/635), the money-path member of the "unbindable" class. The binding must read `PaymentState.restaurant_id` inside the PM leg. That is a real design step and it needs a bound identity to compare against, which is why ADR-20260818-094500 finding 10 says **"B cannot land before A"**. |

**The property that decided it**: this chunk is the only one of the four that is **independent of how identity is resolved**. It consumes `ReadScope` out of the GraphQL context; whether that value came from a JWT claim (today, `crates/server/src/auth.rs` `read_scope`) or from a Postgres mapping lookup (after IDENT-1 reverses the read-scope half of CARD-11) does not change one line of it. Everything else in the slice has to be written twice or waited on.

---

## The person at the end of it — and the fact that she has no name

The constraint from ADR-20260818-101500: *"The card names the restaurateur who signs in at the end of it. A card describing authorization mechanism without that sentence is stopped at the checkpoint."*

**Chunk 1's sentence, honestly**: *when the first Tours restaurateur signs in and opens her back office, the refund queue on her screen contains her refunds and nobody else's — and until she exists, every RESTAURANT credential that can be minted sees an **empty** queue instead of the whole platform's captured money.*

**And the finding underneath it, which the executor must not smooth over**: **no restaurateur is named anywhere in this repository.** Repo-wide there is no pilot restaurant, no design partner, no first customer — `grep -rni "pilot restaurant\|design partner\|first restaurant" docs/` returns only ADR prose about the *category* of person. That is not a card defect; it is a **slice defect**, and it matters operationally rather than sentimentally: the three §39 IDOR triggers are all team acts, so *who* she is and *when* she is handed a credential is the event that starts a published deadline. **Owed to the founder, named here and not filed by this dispatch**: name the restaurateur, or record that the slice completes against a team-held demo account and that doing so trips trigger (i) anyway.

---

## The failing test, FIRST — semantic edit and its expected message

**Write this before touching the emitter.** New test binary `crates/server/tests/graphql_pending_refunds_scope.rs`, modelled on `crates/server/tests/graphql_payment_status.rs` (schema built with `build_schema(None, None, None)`, dependencies injected per request with `.data(...)`, no database anywhere).

**The semantic edit**: define a fake `RefundReadRepository` over the card-defined fixture below that honours `RefundFilter` exactly as the SQL repository does; execute `pendingRefunds` twice against the **same schema and the same fixture** — once as `RequestRole::Admin` with `ReadScope::Admin`, once as `RequestRole::Restaurant` with the `ReadScope` that `read_scope` returns for `Identity::Unbound { role: Restaurant }`, which is **`ReadScope::Public`** (`crates/server/src/auth.rs`, the `Unbound` arm: it fires `read_authorization_bridge_unresolved_total` and returns `ReadScope::Public`) — and assert the **pair**, on the set of `orderId`s, never on a count:

```
assert_eq!(
    (admin_order_ids, unbound_order_ids),
    (all_five(), Vec::<String>::new()),
    "the only RESTAURANT credential that can exist today is Identity::Unbound, and it reads the \
     whole platform's refund queue"
);
```

**Expected failure on `10866d6`, before any production edit:**

```
assertion `left == right` failed: the only RESTAURANT credential that can exist today is
Identity::Unbound, and it reads the whole platform's refund queue
  left: (["O1", "O2", "O3", "O4", "O5"], ["O1", "O2", "O3", "O4", "O5"])
 right: (["O1", "O2", "O3", "O4", "O5"], [])
```

**This is the companion test the ADR privileges over the obvious one**: *"the companion test matters more than the obvious one: `unbound ⇒ denied`, not only `other-restaurant ⇒ denied`. Without it, `domain_id: None` gets coded as 'unknown ⇒ allow', the cross-tenant test passes, and the hole is untouched."* It is also the **only arm reachable in production today**, because nothing mints a restaurant claim (see the antecedents below) — the cross-tenant arm is a test of the future, this one is a test of the present.

**No bare zero** (`beck`'s rule, carried from `docs/dispatch/638-rls-authorization-matrix-chunk1.md`): every empty result in this suite is asserted **jointly** with a non-empty one from the same fixture, same schema, same execution. A lone `assert!(rows.is_empty())` passes when the fake repo was never populated, when the query name was misspelled, when the guard rejected the request, and when the resolver returned an error — four ways to be green over nothing.

---

## Scope: one operation, three modes, two arms

The three-valued gate the founder banked (*"the binding ships three-valued — `OFF / OBSERVE / ENFORCE` — … the flag read per request so rollback is a flip and not a redeploy"*) governs the **binding comparison**. It does **not** govern the absence of an identity, and the card is explicit about that because it is the one semantic a reviewer will challenge:

| Caller | `OFF` | `OBSERVE` (default) | `ENFORCE` |
|---|---|---|---|
| **Unbound / any non-Restaurant, non-Admin scope** (`ReadScope::Public`, `Customer`, `Rider`, `RestaurantAccount`, `System`) | today's behaviour: the caller-supplied filter, i.e. **everything** | **empty** | **empty** |
| **`ReadScope::Restaurant(R1)`, no filter** | everything | R1's rows | R1's rows |
| **`ReadScope::Restaurant(R1)`, filter `R2`** | R2's rows | **R2's rows** + mismatch counted | **R1's rows** + mismatch counted |
| **`ReadScope::Admin`** | unchanged | unchanged | unchanged — the admin arbitrates across restaurants (`specs/stories.yaml`, `ArbitrateRefunds`: *"the cross-restaurant refund queue"*) |
| **mode absent from the context** | — | — | **treated as `ENFORCE`** — fail closed, the same posture as `ctx.data_opt::<ReadScope>().unwrap_or(Public)` on every scoped resolver |

**Why the unbound arm is not gated.** It is not a binding; it is the absence of one, and the system already has a recorded answer for that: *"a missing claim fails closed inside `read_scope`"* (`crates/server/src/graphql/routes.rs`, at the `resolve_read_scope` call). `customerCredit` and `myReclamations` both return nothing for a scope that is not theirs. Making `pendingRefunds` behave like every other scoped read is **alignment with an existing decision**, not a new discretionary narrowing — and `OFF` remains the rung that restores today's behaviour in full, so nothing is one-way.

**Why `OBSERVE` as the default costs nothing** — and this is the reading that makes the founder's *"flipping the default is a reading rather than a guess"* work rather than merely be obeyed: **`OBSERVE` and `ENFORCE` are observationally identical until a restaurant claim exists.** They differ only on the bound-caller row of that table, and `ReadScope::Restaurant` is **unreachable in production**. Antecedents for that claim, both verified this run and both greppable: the only claim writer in the tree is `stamp_customer_claim` / `stamp_put_body(customer_id)` in `crates/infrastructure/src/integrations/supabase_auth.rs`, which writes `app_metadata.captain_food = { role, customer_id }` and has no restaurant sibling; and `#437` hardcodes `"role": "CUSTOMER"` there on purpose so a wrong-role stamp is unspellable. So the default is free today, and the flip to `ENFORCE` becomes a real reading exactly when there is something to read.

---

## The fixture (card-defined, not measured)

Five rows across two restaurants, sizes deliberately **unequal** so that a count-only assertion cannot pass by luck, and ids distinguishable so that "returned the other tenant's rows" is visible rather than merely equinumerous:

| Refund row | restaurant |
|---|---|
| O1, O2 | R1 |
| O3, O4, O5 | R2 |

Assert on the **sorted set of `orderId`s**, never on `len()`. With 2 vs 3 a swapped-set bug changes the length too, but the next fixture will not be so lucky, and the assertion should be the one that stays correct.

## Probes — eight, enumerated here, and this table is their antecedent

| # | Probe | Paired assertion |
|---|---|---|
| **P1** | unbound RESTAURANT vs ADMIN | `(all_five, [])` — **the first test, above** |
| **P2** | `Restaurant(R1)`, no filter, `ENFORCE` | `({O1,O2}, all_five)` against the admin probe |
| **P3** | `Restaurant(R1)`, filter `R2`, `ENFORCE` | `({O1,O2}, {O1,O2})` against the same caller with no filter — **bound, not narrowed** |
| **P4** | ADMIN untouched | `(all_five, {O3,O4,O5})` for no-filter vs filter `R2` |
| **P5** | the two modes, asserted to **differ** on one arm and **agree** on the other | `OBSERVE`+P3 → `{O3,O4,O5}` while `ENFORCE`+P3 → `{O1,O2}`; `OBSERVE`+P1 == `ENFORCE`+P1 == `[]` |
| **P6** | `OFF` restores today's behaviour **exactly** | `OFF`+unbound → `all_five` |
| **P7** | the mismatch counter (own test binary — see CI wiring) | `OBSERVE`+P3 emits exactly one `read_authorization_filter_mismatch_total` **and** returns `{O3,O4,O5}`; the P1 unbound probe emits **none** |
| **P8** | mode absent from the context | equals the explicit `ENFORCE` result, paired |

**P6 is the one most likely to be skipped and the one that matters most operationally.** An untested rollback rung is a rollback that fails at 20:00 on a Friday with the operator holding the flag. The RLS card learned the same lesson about its permissive mode: *"everyone remembers to test enforcing; the untested clause is the one the mitigation rests on."*

**P7's both-ways shape is required, not decorative** — `crates/server/tests/public_credential_degraded_metric.rs` states the reason in its own doc comment: an "it stays zero" assertion whose metric name is simply wrong passes vacuously. Assert the counter fires for the mismatch population **and** does not fire for the unbound one.

## Mutations — six, enumerated here; plant, see red, revert, claim the count in the PR body

The mutation goes in the **emitter source** and the generator is re-run; the test reads only the generated resolver.

| # | Semantic edit | Expected red |
|---|---|---|
| **M1** | the non-Restaurant, non-Admin arm returns the unfiltered list | P1 |
| **M2** | `filter.restaurant_id = input.restaurant_id.or(bound_id)` — narrowing instead of binding, i.e. "caller knows best" | P3 |
| **M3** | the unbound arm applies under `ENFORCE` only | P5's agreement arm |
| **M4** | absent mode defaults to `OFF` | P8 |
| **M5** | the mismatch counter also fires for the unbound population | P7's second arm |
| **M6** | swap R1 and R2 in the fixture | P2 **and** P3 both flip — the fixture's own mutation test, and the answer to "is this grading its own homework" |

---

## The constraints the ADRs bank, carried verbatim into this diff

1. **Three-valued, read per request.** `OFF / OBSERVE / ENFORCE`, default `OBSERVE`, injected as request data. **"Read per request" has a limit that must be stated rather than implied**: the value's *source* is the typed `Config`, resolved at startup, and `specs/common/configuration.yaml` documents the precedence *"environment variable > baked profile value > `default`"* with the stated intent that *"the env var wins so an operator keeps a seconds-fast override for an incident."* So flipping is **an env override plus a pod restart — no rebuild, no image, no CI, no migration**. It is not a live toggle, because no runtime settings source exists in this system. Say that in the PR body in those words; do not let the card's own phrase "a flip and not a redeploy" be read as more than it is.
2. **`Identity::Unbound` denies on the money path and never stamps a role.** On this read path the deny half is already true and the test locks it: `read_scope`'s `Unbound` arm returns `ReadScope::Public` and fires `read_authorization_bridge_unresolved_total{role}`. The **stamping** half is a write-path concern (`domain_events.user_id` / `user_type`) and belongs to [#635](https://github.com/TheCaptainCompany/captain-food/issues/635) / [#178](https://github.com/TheCaptainCompany/captain-food/issues/178) — **this chunk must not touch envelope stamping**, and must not "helpfully" make `Principal::role()` stop returning the declared role for an unbound caller: that method's doc comment says the role survives on purpose, *"which is precisely what makes its denial attributable."*
3. **The ownership comparison reads folded state, never a `View_*`.** Not exercised by this chunk and therefore easy to get wrong later: this chunk compares the caller's scope to a **filter**, not to a resource. It reads no ownership fact at all. When the write half lands, its comparison is against `PaymentState.restaurant_id` (`crates/domain/src/payment.rs:47`), already folded on the approve leg (`crates/application/src/process_managers/refund.rs`) — **never** `View_PendingRefunds.restaurant_id`, which would make projector lag an authorization oracle on the money path at the hour the queue is longest.
4. **`unbound ⇒ denied` outranks `other-restaurant ⇒ denied`.** Enacted as P1 being the first test written, and as the only arm reachable today.
5. **`claimRestaurantListing`'s `PUBLIC` role is resolved explicitly, never discovered later.** This chunk **does not touch the claim path**, so it does not resolve it here — it carries it forward with an owner and a moment: it is the **first checkpoint item** of the chunk that builds the claim path, on [#639](https://github.com/TheCaptainCompany/captain-food/issues/639). The facts that make it urgent are already verified in ADR-20260818-094500 finding 1: `specs/network/api.yaml:239-242` is `roles: [PUBLIC, RESTAURANT_ACCOUNT]`, and `RestaurantListingClaimed` **grants a `ScopeMembership` row** (`specs/database/tables/projection_tables.yaml:1038`) whose `accountId` is nullable — an anonymous caller writing the table every RLS predicate resolves against, granting membership to nobody. **Do not let this chunk quietly become the place that "fixes" it.**

---

## `specs/**` changes this dispatch carries, and the approval each rests on

| Change | Approval |
|---|---|
| **`specs/common/configuration.yaml`** — one new key, `type: enum, values: [off, observe, enforce], default: observe` (`APP_PROFILE` is the shape precedent). Placed in `common`, not `payments`, because a bin's key subset is *"its linked scopes + owning scope + common"* (`tools/codegen-rs/src/emit/bins.rs`) and this seam is served from `crates/server` by every gateway; a scope-local key risks a surface that serves the query without the flag that governs it | **Recorded**: ADR-20260818-101500 requires the three-valued flag by name |
| **`specs/observability.yaml`** — one new metric on the existing `read_authorization` contract: `read_authorization_filter_mismatch_total{operation, role, mode}`, all three attributes drawn from bounded populations | **Recorded**: the same ADR requires *"the mismatch metric declared in `specs/observability.yaml` before the enforcing code lands"* |
| **`specs/payments/api.yaml`** — rewrite `pendingRefunds`' description, which currently documents the hole in the present tense (*"NO ownership check exists today … reads every restaurant's refund queue"*). Leaving it is worse than never having written it: it is a doc that will be false the moment this merges | **Team's** under [ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md): it contradicts no recorded decision (it records one already made), and it changes **no shape** — the `restaurantId` arg **stays**, so nothing about the GraphQL contract is non-additive. **One `docs/SPEC-LOG.md` sentence in the SAME commit** |

**Two things this dispatch does NOT have approval for, and must not do:**

- **No `access:` / `authorization:` DSL block.** DECISIONS §46 **AUTHZ-GRAMMAR** records it as **declined** as new grammar. Deriving the read-side declaration from the DSL is [#649 "The read side has no access declaration…"](https://github.com/TheCaptainCompany/captain-food/issues/649), which the founder raised himself. This chunk authors the policy in the emitter, like every other resolver body, and says so.
- **No new `rules.yaml` entry.** ADR-0032 requires every rule to be linked from a behaviour test, and `specs/tests.yaml` tests are **aggregate-level** (`actor` + `given` events + `when` + `then`, e.g. `TestPendingRefundVisibleUntilDecided` against `actors.yaml#/Payment`). A resolver-scoping rule is inexpressible there, so adding the rule would force either a red gate or a fake domain test. **Fence it and report it** — that the DSL has no home for a read-side authorization rule is exactly #649's subject.

---

## Where the code goes, and the two plumbing traps

- **The resolver body is emitted, not written.** Edit the `"pendingRefunds"` arm in `tools/codegen-rs/src/emit/server_graphql.rs` and run `make generate`; `crates/server/src/graphql/generated/query.rs` carries the GENERATED header and is inside `check-drift`. The shape to copy is the `"customerCredit"` arm a few entries below it in the same function — same `ctx.data_opt::<ReadScope>().cloned().unwrap_or(ReadScope::Public)` opening, same fail-closed default.
- **Trap 1 — the mode must be injected on BOTH transports.** `crates/server/src/graphql/routes.rs` injects request data in two places: the POST handler (`.data(role) … .data(scope) .data(tenant)`) and the WS `on_connection_init` closure (`data.insert(...)`), whose own comment says *"a subscription must not widen what a query would refuse (#144/#433)."* If the mode is injected only on POST, the socket falls to the absent-default `ENFORCE` — **safe, but the `OFF` rung then does not cover the socket**, i.e. the escape hatch fails on one transport. An escape hatch that does not cover every transport is not an escape hatch. Extend `GraphqlState` and `graphql_routes(schema, tenants)` (three production call sites: `crates/server/src/lib.rs`, `crates/server/src/bin_support.rs`, and the in-crate router test) and insert at both sites.
- **Trap 2 — a process global would make P5 unwritable.** Do not reach for a `OnceLock` mode read from the composition root: the two-modes-differ probe needs two modes in one test binary. Per-request injection is what makes the test possible, which is the same reason `telemetry::meters`' once-per-process meter forces the metric probe into its own binary.
- **`crates/telemetry`**: one `pub const` in `contract.rs` and one instrument built from it in `meters.rs`, inside the existing `read_authorization` module. **This is not optional and not deferrable to a follow-up**: `tools/codegen-rs/src/validate/metric_emitters.rs` (§20) warns `obs-metric-no-emitter` for a declared metric with no constant or no instrument, and that warning is on the per-rule ratchet where *"ONE MORE is a hard gate failure."* Declaring the metric in the spec without its emit site in the **same commit** turns `make validate` red. Adding a metric **with** its emitter adds no warning, so no baseline churn is expected — but if the gate says the surface moved, run `make warning-baseline` and commit the artifact in the same commit with the reason.
- **Do not emit the mismatch on `read_authorization_denied_total`.** That contract's own comment says list denials are *"structurally unemittable"* and *"do not 'fix' the missing list denials."* A **filter mismatch** is a discrete per-request fact, not a per-row decision, so it is legitimately emittable on a list path — but it needs its own name, or the next reader will read it as the thing the comment forbids.
- **Tests**: `crates/server/tests/graphql_pending_refunds_scope.rs` (P1–P6, P8) and `crates/server/tests/pending_refunds_mismatch_metric.rs` (P7, own binary). Both are picked up by the existing `-p server --tests` invocation with **no workflow edit** — do not create a new crate ([#335](https://github.com/TheCaptainCompany/captain-food/issues/335): a suite in a new crate never runs and nothing reports it).

---

## What this chunk does NOT do, and who carries the remainder

- **It does not close [#618](https://github.com/TheCaptainCompany/captain-food/issues/618).** That issue is a **class**: *"7 unscoped read surfaces"*, two of which return other tenants' rows when called with no arguments (antecedent: DECISIONS §39 scope correction of 2026-08-17 — **quoted, not re-measured by this card**). This chunk fixes **one** of them. The PR references the issue and ticks one box; it does not write `Closes`.
- **It does not bind any write.** `approveRefund` / `denyRefund` consult no identity anywhere — [#635](https://github.com/TheCaptainCompany/captain-food/issues/635) (the PM-decided, "unbindable" money-path pair) and [#178](https://github.com/TheCaptainCompany/captain-food/issues/178) (the write seam). The live `approve_refund` widget on `specs/screens/restaurant_backoffice.yaml` stays exactly as authorized as it is today, which is: not at all. **This is the compounding chain to say out loud in the PR body** — after this chunk a restaurant sees only its own refunds and can still approve anybody's.
- **It mints no credential and builds no sign-in** — [#639](https://github.com/TheCaptainCompany/captain-food/issues/639); the email-link mechanism is decided (ADR-20260818-101500 decision 1) and unbuilt.
- **It creates no `sub → domain id` mapping** — [#641](https://github.com/TheCaptainCompany/captain-food/issues/641) / DECISIONS §46 IDENT-1, a recorded MIGRATION.
- **It does not resolve `claimRestaurantListing`'s `PUBLIC` role** — carried to #639 as its first checkpoint item, per constraint 5 above.
- **It does not touch [#638](https://github.com/TheCaptainCompany/captain-food/issues/638)**, frozen at chunk 1 by founder decision. Nor #649's DSL derivation.
- **It does not flip anything to `ENFORCE`.** The flip is a separate recorded decision, and its natural moment is the chunk that mints the first restaurant claim: **that chunk may not merge with this flag below `ENFORCE`**, because its first authenticated request is §39 trigger (i).
- **Visible age, so the ungated half cannot be forgotten**: add one line to `docs/STATUS.md` at merge — *`READ_SCOPE_BINDING_MODE` defaults to OBSERVE; the bound-caller arm is unreachable and unflipped, since 2026-08-18*. If that line is still there when a restaurant credential exists, the chunk failed and it will be legible.

---

## What BANKS at the checkpoint

Per ADR-20260816-134352 and ADR-20260817-105845, the executor states each of these explicitly at the checkpoint rather than leaving them to the reviewer:

1. **Did the narrowed checkpoint set miss anything**, with an **attribution**: card defect · invited-lens depth miss · roster width. Only a roster-width miss returns to the founder.
2. **`OBSERVE` denies the unbound arm.** Is "observe" the honest name for a mode that changes behaviour for the population that exists? The card's answer is that the gate is over the *binding*, and the unbound arm is `read_scope`'s already-recorded fail-closed contract — challenge it here or not at all.
3. **Default `OBSERVE`, on the claim that the two modes are observationally identical today.** Antecedent: no claim writer for `restaurant_id` exists (`stamp_put_body` writes `{role, customer_id}` only). If a hand-stamped console token exists somewhere, this claim is false and the default deserves re-argument.
4. **Absent mode ⇒ `ENFORCE`** (fail closed), and its asymmetric consequence on the WS transport (trap 1).
5. **The new counter's home** in the `read_authorization` contract, against that contract's "list denials are structurally invisible" note.
6. **`common` vs `payments` for the config key**, and the size of the generated bin-config diff it produces — **`UNVERIFIED input`: this card did not count the bin crates that regenerate.**

## Definition of done

- P1–P8 green; the six mutations each planted, seen red, reverted, and **claimed in the PR body with this card's table as their antecedent**.
- `make rust` green · `make validate` **0 errors** · `check-drift` clean · one `docs/SPEC-LOG.md` sentence · one `docs/STATUS.md` line.
- PR body states, in the words above: what "read per request" actually costs to flip; that the write half is still open; and that #618 is not closed.
- `HOLD: human` — ready-for-review, independent reviewer pass over the full diff, then the coordinator merges. **Never auto-merge.**

## RISK — the one most likely to go wrong

**The gate lands and the behaviour does not.** Every artifact of this chunk — a config key, a metric, a mode enum — is the kind of thing that can be present, well-tested and inert: the mode injected on one transport, the counter built but never called on the path that matters, and a green suite over a resolver whose default arm no real caller reaches. The specific defence is P7's both-ways assertion and P6's `OFF` probe; the general defence is that **P1 is written first and fails first**, on the only arm production can actually reach today. Secondary: PR [#587](https://github.com/TheCaptainCompany/captain-food/pull/587) is open against the codegen tree (`564-reader-derivation-pr2`) — a textual conflict in `tools/codegen-rs/src/emit/` is possible; rebase rather than merge, and re-run `make generate` afterwards.

## Findings

_(Lenses and the executor append here.)_
