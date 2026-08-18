# Dispatch — slice chunk 1: bind the refund queue to the caller, on the one operation that can be proved today

> **REVISION 2 (2026-08-18)** — the whole roster was briefed on revision 1 and **eleven lenses
> replied; none returned "nothing in my lens" on the card as a whole**. The chunk CHOICE survived
> unanimously (`holub` and `young` defended it explicitly). Thirteen specific defects in the card did
> not. **Every attribution was CARD DEFECT — none was a roster-width miss**, so nothing here went back
> to the founder as a class question. What each lens caught, and the two findings this revision
> **rejected with a reason**, are in `## Findings` at the end. Revision 1 is in git; this file is the
> card.

- **Issue**: tracking [#618 "Read surfaces missing `ReadScope` — the read half of the write-path authorization gap (#178)"](https://github.com/TheCaptainCompany/captain-food/issues/618) · slice record [ADR-20260818-101500](../adr/ADR-20260818-101500-the-restaurant-signs-in-by-email-link-and-638-freezes-at-chunk-1.md) · rulings [ADR-20260818-094500](../adr/ADR-20260818-094500-staff-auth-mechanism-and-refund-approval-stays-with-the-restaurant.md) · register [DECISIONS §39 IDOR-1](../proposals/DECISIONS.md)
- **Base**: `main`, at **the commit that last touched this card** — resolve it mechanically, do not copy a SHA from prose:

  ```
  git fetch origin main
  CARD=docs/dispatch/618-pending-refunds-read-scope-chunk1.md
  test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" || exit 1
  test "$(git rev-parse HEAD)" = "$(git log -1 --format=%H -- "$CARD")" || exit 1
  test -z "$(git status --porcelain)" || exit 1
  ```

  That is the base check precondition 4 asks for (`.claude/agents/executor.md`, founder-approved
  2026-08-18), in the only form that is not self-referentially impossible: a card cannot name the SHA
  of the commit that contains it, so it names **the commit that last modified it**. The check is
  unchanged by this revision — the third line still resolves to the revision commit, because that
  commit is both HEAD and the last commit touching `$CARD`. All three lines must pass. If the second
  fails, `main` has moved since this card landed — **stop and get the card re-based**; do not rebase,
  reset, or work from HEAD, because a card written against a different tree may describe code that no
  longer exists.

  **This revision's content was re-verified against `3332871b8063bda83509d1861a1d517c08ed3838`**
  (`git rev-parse HEAD` and `git rev-parse origin/main` both, tree clean). Every `file:line` below was
  re-read in that tree during the revision; several moved from revision 1 and were corrected in place.
  The only commit expected between it and your base is the one that lands this revision.

- **Reversibility class**: **`HOLD: human`.** Not because money moves in this diff — none does, and no
  event shape changes — but on two grounds that the named class covers: (a) it is the **money-path
  read surface** of §39, the queue whose rows are the input to `approveRefund`; and (b) the failure
  mode of an over-narrow predicate is an **empty queue**, which is silence, not an error. That is the
  same silent-skip class [ADR-20260818-004647](../adr/ADR-20260818-004647-database-level-security-lands-at-the-cutover-and-the-settlement-read-returns-to-scope.md)
  used to refuse a policy on `OrderTracking`: a filter removes rows, it does not raise. A restaurant
  that cannot see a refund cannot decide it, the money stays captured, and every dashboard is green.
- **Roster**: **whole roster at the BRIEFING** — done, revision 1, eleven replies (ADR-20260816-134352:
  the `HOLD: human` axis sizes the briefing and wins when the two axes disagree). **CHECKPOINT goes
  only to the lenses that declared a concern at briefing** — that is now a named list, in `## Findings`;
  any lens may opt back in.
- **Merge posture**: `HOLD: human` ⇒ **NOT auto-merge-on-green**. The PR stops at ready-for-review until
  the TEAM's independent reviewer pass over the full branch diff; after PASS + green gates the
  coordinator merges ([ADR-20260815-115220](../adr/ADR-20260815-115220-auto-merge-on-green-by-default-hold-human-for-the-named-class.md),
  amended by [ADR-20260815-134655](../adr/ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md)). **No founder wait.**
- **Branch**: `618-pending-refunds-read-scope`. **Do NOT write `Closes #618`** in the PR body — see "What this does not do".

---

## Why this is chunk 1, and what was rejected

The slice is *"one real Tours restaurateur finds their own restaurant on their phone browser, proves
it, signs in, and can see and act on only their own orders and only their own refunds"* — an
email-link sign-in, a `sub → domain id` mapping in our Postgres, the claim path, and three
operations. Four candidate first chunks were weighed:

| Candidate | Verdict |
|---|---|
| **Scope `pendingRefunds` to the caller's `ReadScope`** (`beck`'s candidate) | ✅ **CHOSEN**, and re-confirmed by the whole roster at briefing. Vertical: it changes what a real persona sees on a real screen (`specs/screens/restaurant_backoffice.yaml:36` binds `refunds.pending` to this query **with no arguments**, so today that widget renders every restaurant's refund queue). It is **binding, not narrowing** — the `restaurantId` arg stays for ADMIN, so the GraphQL schema does not change at all. It needs no new table, no migration, no credential, and no provider. And it is testable **today** in-process with fake repos and no database, by the pattern the `customerCredit` arm already uses (`tools/codegen-rs/src/emit/server_graphql.rs:742`, emitted at `crates/server/src/graphql/generated/query.rs:584`: destructures `ReadScope::Customer` and returns nothing otherwise). |
| **The `sub → domain id` mapping table first** | ❌ Rejected: it is a **layer**, not a vertical step. It leaves the product exactly as good as it was, it is [#641](https://github.com/TheCaptainCompany/captain-food/issues/641)'s CUSTOMER slice plus the staff extension in [#639](https://github.com/TheCaptainCompany/captain-food/issues/639), and it is a MIGRATION (tokens in the wild carry `captain_food.customer_id`) whose phase order must be recorded first — DECISIONS §46 IDENT-1. |
| **The email-link sign-in first** | ❌ Rejected as first: it is the largest piece, it needs a provider mechanism that does not exist, and — decisively — **it is the event that trips the §39 IDOR deadline** (trigger (i): a restaurant credential outside the team, including demos and pilots). Minting the credential before the surfaces it reaches are bound is doing the two halves in the wrong order. |
| **Write-side binding of `approveRefund`/`denyRefund` first** | ❌ Rejected as first, and it is not blocked on taste: those two commands are decided by a **process manager**, not an aggregate, and their payload `{orderId, amount, reason}` has **no field corresponding to the caller** — [#635](https://github.com/TheCaptainCompany/captain-food/issues/635), the money-path member of the "unbindable" class. That is a real design step and it needs a bound identity to compare against, which is why ADR-20260818-094500 finding 10 (`:118`) says **"B cannot land before A"**. |

**The property that decided it**: this chunk is the only one of the four that is **independent of how
identity is resolved**. It consumes `ReadScope` out of the GraphQL context; whether that value came
from a JWT claim (today, `crates/server/src/auth.rs:1786` `read_scope`) or from a Postgres mapping
lookup (after IDENT-1 reverses the read-scope half of CARD-11) does not change one line of it.
Everything else in the slice has to be written twice or waited on.

### `holub`'s counting question, answered here rather than at the checkpoint

*"Name the chunk sequence from this merge to the first real sign-in, and its length. Three chunks or
eleven? Which one is chunk 2?"* — because if the executor cannot state the number, this is chunk 1 of
an unbounded program and the "she signs in" sentence is decoration.

**Chunk 2 is nameable. The total is not, and I will say so in those words: I cannot bound it above.**

The **minimum** sequence, and every step is forced by a fact verified in this tree:

| # | Chunk | Why it is forced, and what it is blocked on |
|---|---|---|
| 1 | *(this one)* bind `pendingRefunds` to the caller's `ReadScope`, three-valued | — |
| 2 | **The RESTAURANT `authRef` fact + the `sub → restaurant` mapping resolution.** No credential minted. | **This is chunk 2.** `auth_ref` is **one column in the whole projection set**, on `Customer` (`specs/database/tables/projection_tables.yaml:395`); RESTAURANT and RESTAURANT_ACCOUNT declare no `authRef` **anywhere** (DECISIONS §46 STAFF-AUTH, re-verified). There is nothing for IDENT-1 to extend for this role — the fact has to be authored first. ⚠️ **AMBER**: its shape depends on how a person becomes bound to a restaurant (invite / ADMIN-provisions / claim), which is the open half of **STAFF-AUTH**, 🟠 FOUNDER-OWNED in DECISIONS §46. ADR-20260818-101500 answered the **factor** (email link); it did not answer the **roster**. |
| 3 | **Write-side binding of `approveRefund`/`denyRefund`** ([#635](https://github.com/TheCaptainCompany/captain-food/issues/635)) | Needs a bound identity to compare against (finding 10). Testable before any credential exists, exactly the way this chunk is. |
| 4 | **Mint the first credential**: email-link sign-in + the claim path + `claimRestaurantListing`'s `PUBLIC` resolution + the flip to `ENFORCE` ([#639](https://github.com/TheCaptainCompany/captain-food/issues/639)) | Must be LAST of the four: it is §39 trigger (i), and minting a real restaurant credential while chunk 3 is unlanded means a real restaurateur can approve any restaurant's refund. |

**Two clauses of the slice sentence are already discharged and do not need a chunk**: *"finds their own
restaurant"* is the existing storefront/claim surface, and *"sees only their own orders"* is already
bound — the `orders` query threads the caller's scope into the repository
(`crates/server/src/graphql/generated/query.rs:450,458`), unlike `pendingRefunds`. That is why the
refund queue was the one worth doing.

**So: three chunks after this merge, minimum, and chunk 2 is named.** The reason that is a floor and
not a bound is a single open decision: **if STAFF-AUTH's answer separates provisioning from mapping,
chunk 2 splits in two, and if it introduces an operator back office it splits further.** The number
becomes stateable the day that row is answered, and not before. It has been open **since 2026-08-18**
(raised while landing IDENT-1) — one day, so it is not yet a stale decision, but it is the single
thing standing between this program and a stated length.

**On `holub`'s wider point, which stands — with one figure corrected.** `holub` said the last five
dispatch cards are *all* authorization/attribution internals. Ordered by last commit
(`git log -1 --format=%ad --date=short -- docs/dispatch/*.md`), the five before this one are
`638-rls-authorization-matrix-chunk1` · `623-placeorder-unattributable` · `614-erasure-fails-open` ·
`556-local-walk-harness` · `178-write-binding-slice-1`. **Three** are authorization/attribution; the
other two are internals of a different kind. **The point survives the correction intact and is the one
that matters**: none of the five is user-facing, and the last commit touching `specs/screens` or
`crates/web` is `2a035ff`, **2026-08-16** — *"Lane width comes from the declaration, not a seeded
registry (#596)"* — itself not user-facing either. Four more internals chunks is defensible under
external force (the §39 deadline). It is not a posture. **The card bounds it at four and says the
bound's condition out loud.**

---

## The person at the end of it — and the fact that she has no name

The constraint from ADR-20260818-101500: *"The card names the restaurateur who signs in at the end of
it. A card describing authorization mechanism without that sentence is stopped at the checkpoint."*

**Chunk 1's sentence, honestly**: *when the first Tours restaurateur signs in and opens her back
office, the refund queue on her screen contains her refunds and nobody else's — and until she exists,
every RESTAURANT credential that can be minted sees an **empty** queue instead of the whole platform's
captured money.*

**And the finding underneath it, which the executor must not smooth over**: **no restaurateur is named
anywhere in this repository.** Repo-wide there is no pilot restaurant, no design partner, no first
customer. Re-run this run (the card must exclude itself, or it pollutes its own grep):
`grep -rni "pilot restaurant\|design partner\|first restaurant" docs/ | grep -v 618-pending-refunds`
returns **three** hits — `ADR-20260818-094500:128`, `docs/adr/0024-uber-eats-price-estimation.md:40`
and `DECISIONS.md:2040` — every one of them prose about the *category* of person, none a name. That is not a card defect; it is a **slice defect**, and it matters
operationally rather than sentimentally: the three §39 IDOR triggers are all team acts, so *who* she is
and *when* she is handed a credential is the event that starts a published deadline.

**Disposition, corrected after `holub`'s challenge to revision 1.** Revision 1 parked it (*"owed to the
founder, named here and not filed by this dispatch"*), and `holub` was right that an obligation with no
row and no owner is inventory. Two things now carry it, neither of which is this chunk's work:

1. It is **in the founder's decision queue this turn**, in the card's own words: *name the restaurateur,
   or record that the slice completes against a team-held demo account and that this trips §39 trigger
   (i) anyway.*
2. **If it is not answered this turn, a `DECISIONS.md` row is owed by this run's records** — the register
   is the durable owner, the decision queue is not. The `architect` owns filing it. Naming it here
   without a register row is exactly the inventory `holub` objected to.

---

## The failing test, FIRST — semantic edit and its expected message

**Write this before touching the emitter.** New test binary
`crates/server/tests/graphql_pending_refunds_scope.rs`, modelled on
`crates/server/tests/graphql_payment_status.rs` (schema built with `build_schema(None, None, None)`,
`crates/server/src/graphql/schema.rs:107`; dependencies injected per request with `.data(...)`; no
database anywhere).

**The semantic edit**: define a fake `RefundReadRepository` over the card-defined fixture below;
execute `pendingRefunds` twice against the **same schema and the same fixture** — once as
`RequestRole::Admin` with `ReadScope::Admin`, once as `RequestRole::Restaurant` with `ReadScope::Public`
(which is what an unbound RESTAURANT credential resolves to; **antecedent below, and it is an
assertion, not prose**) — and assert the **pair**, on the set of `orderId`s, never on a count:

```
assert_eq!(
    (admin_order_ids, unbound_order_ids),
    (all_five(), Vec::<String>::new()),
    "the only RESTAURANT credential that can exist today is Identity::Unbound, and it reads the \
     whole platform's refund queue"
);
```

**Expected failure on the base commit, before any production edit:**

```
assertion `left == right` failed: the only RESTAURANT credential that can exist today is
Identity::Unbound, and it reads the whole platform's refund queue
  left: (["O1", "O2", "O3", "O4", "O5"], ["O1", "O2", "O3", "O4", "O5"])
 right: (["O1", "O2", "O3", "O4", "O5"], [])
```

### P1's antecedent is an existing assertion, and the test must cite it — not restate it

`beck` objected that revision 1 **narrated** `Unbound ⇒ ReadScope::Public` and asked for
`auth::read_scope(...)` to be called instead of the constant written. **That specific repair is not
possible and this card rejects it** — see `## Findings` — but the concern is real and has a better
answer, because the antecedent is already asserted:

```
crates/server/src/auth.rs:1910
    assert_eq!(read_scope(&principal(RequestRole::Restaurant, "s", None)), ReadScope::Public);
```

So the day a claim path changes that arm, **`auth.rs:1910` goes red first** and P1 never gets the
chance to stay green over a reopened hole. Two things are required of the diff, both cheap:

- the new test binary's module doc comment cites `crates/server/src/auth.rs:1910` **by file and line**
  as the antecedent for the hardcoded `ReadScope::Public`;
- that assertion, which today carries **no message**, gains one naming its dependent — e.g.
  `"pendingRefunds' P1 probe hardcodes Public on the strength of this line (#618)"`. One line, in the
  test that would break, at the site that would break it.

**This is the companion test the ADR privileges over the obvious one**: *"the companion test matters
more than the obvious one: `unbound ⇒ denied`, not only `other-restaurant ⇒ denied`. Without it,
`domain_id: None` gets coded as 'unknown ⇒ allow', the cross-tenant test passes, and the hole is
untouched."* It is also the **only arm reachable in production today**, because nothing mints a
restaurant claim (antecedents below) — the cross-tenant arm is a test of the future, this one is a test
of the present.

**No bare zero** (`beck`'s rule, carried from `docs/dispatch/638-rls-authorization-matrix-chunk1.md`):
every empty result in this suite is asserted **jointly** with a non-empty one from the same fixture,
same schema, same execution. A lone `assert!(rows.is_empty())` passes when the fake repo was never
populated, when the query name was misspelled, when the guard rejected the request, and when the
resolver returned an error — four ways to be green over nothing. **Revision 1 applied this rule in P1–P4,
P6 and P7 and broke it in P5 and P8**; both are re-anchored as triples below.

### The fake repository — the disqualifying constraint, stated because the in-repo precedent teaches the opposite

`crates/server/tests/graphql_subscriptions.rs:50` defines
`fn scoped(row: Option<OrderTrackingRow>, scope: &application::queries::ReadScope) -> Option<OrderTrackingRow>`
— a stand-in that **enforces the policy itself**. Copy that shape here and **every probe passes against
an unmodified resolver**, with most of the mutations still green. So:

- **The fake must not take, see, or be constructed with a `ReadScope`.** Its only input is
  `RefundFilter` (`crates/application/src/queries.rs:468`), and the real SQL is a two-clause `WHERE`
  (`crates/infrastructure/src/persistence/refund_queue.rs:63-71`) that is trivially faithful to
  reproduce.
- **The fake captures every call it receives**, as a `Vec<RefundFilter>`, and each probe asserts the
  captured calls alongside the row set. The filter is the seam, so structure-sensitivity **is** the
  behaviour here — and it makes M2 red even if the fake's own filtering is wrong.
- On the intersection-empty arm (P3) the resolver returns **before** touching the repository, so the
  correct assertion there is `(rows, captured) == ([], vec![])` — the fake was not called at all. That
  is a stronger statement than an empty row set and it is free.

---

## Scope: one operation, three modes, two arms

The three-valued gate the founder banked (*"the binding ships three-valued — `OFF / OBSERVE / ENFORCE`
— … the flag read per request so rollback is a flip and not a redeploy"*, as corrected in the ADR
itself) governs the **binding comparison**. It does **not** govern the absence of an identity.

**Who can actually reach this resolver.** `pendingRefunds` carries
`guard = "RoleGuard::new(ALLOW_RESTAURANT_ADMIN)"` (`crates/server/src/graphql/generated/query.rs:570`,
from `roles: [RESTAURANT, ADMIN]` at `specs/payments/api.yaml:114`). So the reachable caller set is
exactly three: a **bound** RESTAURANT (`ReadScope::Restaurant`), an **unbound** RESTAURANT
(`ReadScope::Public` — `crates/server/src/auth.rs:1802-1805`), and ADMIN. `Customer`, `Rider`,
`RestaurantAccount` and `System` never arrive: the first three are rejected by the guard, and
`read_scope` **never returns `System`** at all (`crates/server/src/auth.rs:1790-1806` — no arm produces
it). Revision 1's row 1 listed five scopes as if they were live; that was wrong and is corrected.

| Caller (all that can reach the resolver) | `OFF` | `OBSERVE` (default) | `ENFORCE` |
|---|---|---|---|
| **Unbound RESTAURANT** (`ReadScope::Public`) | today's behaviour: the caller-supplied filter, i.e. **everything** | **empty** | **empty** |
| **`ReadScope::Restaurant(R1)`, no filter** | everything | R1's rows | R1's rows |
| **`ReadScope::Restaurant(R1)`, filter `R1`** | R1's rows | R1's rows | R1's rows |
| **`ReadScope::Restaurant(R1)`, filter `R2`** | R2's rows | **R2's rows** + mismatch counted | **EMPTY** + mismatch counted |
| **`ReadScope::Admin`** | unchanged | unchanged | unchanged — the admin arbitrates across restaurants (`specs/stories.yaml`, `ArbitrateRefunds`: *"the cross-restaurant refund queue"*) |
| **mode absent from the context** | — | — | **treated as `ENFORCE`** — fail closed, the same posture as `ctx.data_opt::<ReadScope>().unwrap_or(Public)` on every scoped resolver |

### The rule is intersection. Substitution is a defect, not a design choice.

**`ENFORCE` + a conflicting filter returns EMPTY, never the bound rows.** Revision 1 said it returned
R1's rows; `graphql-architect` is right that that is **substitution**, and substitution makes the
schema lie — the client asked for R2, got HTTP 200 with R1's rows, no error, and no way to tell. The
rule, in one line the executor must implement literally:

> **bound scope ∩ requested filter.** No filter → the bound scope. Filter == bound → the bound scope.
> Filter ≠ bound → **empty**, plus the mismatch counter. The argument can narrow within the caller's
> scope; it can never move it.

This is still **binding, not narrowing**: M2 (`input.restaurant_id.or(bound_id)` — "caller knows best")
still goes red, because under M2 the conflicting filter would return R2's rows instead of nothing. And
the empty-queue silence risk does **not** apply on this arm, because the only real caller
(`specs/screens/restaurant_backoffice.yaml:36`) passes **no arguments** and never takes it.

`graphql-architect` withdrew its own morning position — a separate `myPendingRefunds` twin — **on this
condition**: role = path already gives RESTAURANT and ADMIN separate schema documents, and #649's
endgame is one operation with a declared scoping, so the twin would be built and then deleted, which is
the intermediate step ADR-20260808-235113 forbids. *Without intersection, this card is the shim.*

### `OBSERVE` compares filter VALUES, in ONE execution. Never two result sets.

Nothing in revision 1 forbade the shadow-execution reading: run the caller's filter, run the bound
filter, diff the rows. That doubles the most expensive read on the money path at the hour the queue is
longest, and **no mutation in this card would catch it**, because none counts executions. Binding
sentence: **one query execution; the comparison is `Option<RestaurantId>` against `RestaurantId`, in
memory, before the repository is called.**

### Why the unbound arm is not gated *between `OBSERVE` and `ENFORCE`* — and why `OFF` is different

`evans` caught a self-contradiction in revision 1: the heading said the unbound arm "is not gated", and
the table said `OFF` returns **everything** for it, and P6 asserts exactly that. Both cannot stand. The
corrected statement:

> The unbound arm is **not gated between `OBSERVE` and `ENFORCE`** — it is not a binding, it is the
> absence of one, and the system has a recorded answer for that: *"a missing claim fails closed inside
> `read_scope`"* (`crates/server/src/graphql/routes.rs:162-168`). `customerCredit` and `myReclamations`
> both return nothing for a scope that is not theirs, so making `pendingRefunds` behave the same is
> **alignment with an existing decision**, not a new discretionary narrowing.
>
> **`OFF` does restore it, and that is what `OFF` means.** It is an incident rung, not a configuration
> choice — the one value of the three that re-opens the only hole production can reach today.

**Why `OBSERVE` as the default costs nothing** — and this is the reading that makes the founder's
*"flipping the default is a reading rather than a guess"* work rather than merely be obeyed: **`OBSERVE`
and `ENFORCE` are observationally identical until a restaurant claim exists.** They differ only on the
bound-caller rows, and `ReadScope::Restaurant` is **unreachable in production**. Antecedents, both
re-verified this run and both greppable: the only claim writer in the tree is `stamp_customer_claim` /
`stamp_put_body` (`crates/infrastructure/src/integrations/supabase_auth.rs:80,424`), which writes
`app_metadata.captain_food = { role, customer_id }` and has no restaurant sibling; and `#437` hardcodes
`"role": "CUSTOMER"` there on purpose (`:428`) so a wrong-role stamp is unspellable. So the default is
free today, and the flip to `ENFORCE` becomes a real reading exactly when there is something to read.

**Not this chunk's arm, and do not "helpfully" add it**: `RESTAURANT_ACCOUNT` — the role that manages
several locations — is rejected by the guard today (`roles: [RESTAURANT, ADMIN]`,
`specs/payments/api.yaml:114`). An account manager's cross-location refund queue is a real future
product question and it is not decided anywhere. Leave it rejected.

---

## The fixture (card-defined, not measured)

Five rows across two restaurants, sizes deliberately **unequal** so that a count-only assertion cannot
pass by luck, and ids distinguishable so that "returned the other tenant's rows" is visible rather than
merely equinumerous:

| Refund row | restaurant |
|---|---|
| O1, O2 | R1 |
| O3, O4, O5 | R2 |

Assert on the **sorted set of `orderId`s**, never on `len()`. With 2 vs 3 a swapped-set bug changes the
length too, but the next fixture will not be so lucky, and the assertion should be the one that stays
correct.

## Probes — NINE, enumerated here, and this table is their antecedent

| # | Probe | Paired assertion |
|---|---|---|
| **P1** | unbound RESTAURANT vs ADMIN | `(all_five, [])` — **the first test, above** |
| **P2** | `Restaurant(R1)`, no filter, `ENFORCE` | `({O1,O2}, all_five)` against the admin probe |
| **P3** | `Restaurant(R1)`, filter `R2`, `ENFORCE` — **intersection** | `(rows, captured_calls, no_filter_rows) == ([], vec![], {O1,O2})` — empty, the repository untouched, and the same caller with no filter still sees its own two rows |
| **P4** | ADMIN untouched | `(all_five, {O3,O4,O5})` for no-filter vs filter `R2` |
| **P5** | the two modes, asserted to **differ** on one arm and **agree** on the other | as a **triple**: `(OBSERVE+filter-R2, ENFORCE+filter-R2, ADMIN_no_filter) == ({O3,O4,O5}, [], all_five)`; and `(admin_observe, unbound_observe, unbound_enforce) == (all_five, [], [])` |
| **P6** | `OFF` restores today's behaviour **exactly** | as a **pair**: `(OFF+unbound, ENFORCE+unbound) == (all_five, [])` |
| **P7** | the mismatch counter (own test binary — see CI wiring) | `OBSERVE`+P3's caller emits exactly one `read_authorization_scope_mismatch_total` **and** returns `{O3,O4,O5}`; the P1 unbound probe emits **none** |
| **P8** | mode absent from the context | as a **triple**, on the unbound caller: `(absent, ENFORCE, OFF) == ([], [], all_five)` |
| **P9** | the resolved mode is legible at boot | `Config::boot_report()` contains a `READ_SCOPE_BINDING_MODE = <value>` line — see "the mode nobody can observe" |

**No probe may anchor on `[] == []`.** Revision 1's P5 agreement arm and P8 both did, which made them
green over an unpopulated fixture, a misspelled query name or a rejected request — and made M3
undetectable. Every arm above now carries a non-empty member from the same execution.

**P6 is the one most likely to be skipped and the one that matters most operationally.** An untested
rollback rung is a rollback that fails at 20:00 on a Friday with the operator holding the flag. The RLS
card learned the same lesson about its permissive mode: *"everyone remembers to test enforcing; the
untested clause is the one the mitigation rests on."*

**P7's both-ways shape is required, not decorative** — `crates/server/tests/public_credential_degraded_metric.rs`
states the reason in its own doc comment: an "it stays zero" assertion whose metric name is simply wrong
passes vacuously. Assert the counter fires for the mismatch population **and** does not fire for the
unbound one.

**P7's binary contains EXACTLY ONE `#[test]`.** Its two arms are two assertions inside one test fn.
Precedent and reason at `crates/server/tests/otp_guard_liveness_metric.rs:53-57`: the global meter
provider is *"process-global, so separate `#[test]`s in one binary would race each other."* Two tests
there is a coin-flip red on a `HOLD: human` PR a human is watching.

**P9 is not decoration.** The only signal carrying `mode` fires on mismatch, and mismatch traffic is
zero today, so nothing else in the system distinguishes a pod running `OFF` from one running `ENFORCE`
— CLAUDE.md's named defect class. The boot report is the one artifact that already answers it, for
free. Reason and file:line under "the mode nobody can observe" below.

## Mutations — NINE, enumerated here; plant, see red, revert, claim the count in the PR body

The mutation goes in the **emitter source** and the generator is re-run; the test reads only the
generated resolver. **The one exception is M6, and it is called out because it contradicts that rule.**

| # | Semantic edit | Expected red |
|---|---|---|
| **M1** | the non-Restaurant, non-Admin arm returns the unfiltered list | P1 |
| **M2** | `filter.restaurant_id = input.restaurant_id.or(bound_id)` — narrowing instead of binding, i.e. "caller knows best" | P3 |
| **M3** | the unbound arm applies under `ENFORCE` only | P5's agreement arm |
| **M4** | absent mode defaults to `OFF` | P8 |
| **M5** | the mismatch counter also fires for the unbound population | P7's second arm |
| **M6** | swap R1 and R2 **in the fixture** | P2 **and** P3 both flip — the fixture's own mutation test |
| **M7** | the `Restaurant` arm leaves `filter.restaurant_id = None` | P2 — the likeliest real typo, and revision 1 left P2 with no mutation at all |
| **M8** | the `Admin` arm binds like the others | P4 |
| **M9** | `OFF` behaves as `OBSERVE` | P6 — the rollback rung the card itself calls the one that matters most |

**M6 only works if every expectation in the suite is a hardcoded literal.** If expectations are derived
from the fixture by a helper, swapping R1/R2 moves both sides and the suite stays green — the exact
tautology M6 exists to disprove. Write the expected sets as literals, or strike M6 and stop claiming the
homework is graded. **The PR body's count is nine, not six.**

---

## The constraints the ADRs bank, carried into this diff

1. **Three-valued, read per request.** `OFF / OBSERVE / ENFORCE`, default `OBSERVE`, injected as request
   data. **"Read per request" has a limit that must be stated rather than implied**: the value's
   *source* is the typed `Config`, resolved at startup, and `specs/common/configuration.yaml:84-86`
   documents the precedence *"environment variable > baked profile value > `default`"* with the stated
   intent that *"the env var wins so an operator keeps a seconds-fast override for an incident."* So
   flipping is **an env override plus a pod restart — no rebuild, no image, no CI, no migration**. It is
   not a live toggle, because no runtime settings source exists in this system. Say that in the PR body
   in those words; do not let the card's own phrase "a flip and not a redeploy" be read as more than it
   is. **And see "the `OFF` rung has a dated expiry" below — that sentence is true on Render and false on
   MKS.**
2. **`Identity::Unbound` denies on the money path and never stamps a role.** On this read path the deny
   half is already true and the test locks it: `read_scope`'s `Unbound` arm returns `ReadScope::Public`
   and fires `read_authorization_bridge_unresolved_total{role}`
   (`crates/server/src/auth.rs:1802-1805`). The **stamping** half is a write-path concern
   (`domain_events.user_id` / `user_type`) and belongs to
   [#635](https://github.com/TheCaptainCompany/captain-food/issues/635) /
   [#178](https://github.com/TheCaptainCompany/captain-food/issues/178) — **this chunk must not touch
   envelope stamping**, and must not "helpfully" make `Principal::role()` stop returning the declared
   role for an unbound caller (`crates/server/src/auth.rs:250`): that method's doc comment says the role
   survives on purpose, *"which is precisely what makes its denial attributable."*
3. **The ownership comparison reads folded state, never a read model — and the reachable wrong answer is
   not the one revision 1 named.**

   > ⚠️ **Revision 1's justification was invented and is withdrawn.** It said `View_PendingRefunds` must
   > not be the oracle because *"projector lag would make it an authorization oracle."* **There is no
   > projector.** `View_PendingRefunds` is a plain SQL `VIEW` folded **on read** over `domain_events`
   > (`migrations/20260730043600_enum_text_recreate_views.sql:139`, declared at
   > `specs/database/projection_views.yaml:370`); `crates/infrastructure/src/projections/` **does not
   > exist**. Lag is zero by construction. The conclusion was right and the antecedent was fabricated,
   > which is precisely the defect [ADR-20260817-105845](../adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)
   > was written for. **The same wrong reason appears in ADR-20260818-101500's own constraint list** —
   > the card inherited it. A dated one-line correction note is owed on that ADR, in the same shape as
   > the ⚠️ CORRECTED block it already carries for the rollback clause. **Do not rewrite the clause; add
   > the note.**

   **The true reason, which needs no antecedent**: a read model is not an authorization oracle. Full
   stop.

   **And `young` is right that the decoy was wrong.** Nobody was going to reach for
   `View_PendingRefunds.restaurant_id`. **The reachable wrong answer is that `RefundProcess` already
   reads a read model on the approve leg** — `self.orders.by_id(order_id, &crate::queries::ReadScope::System)`
   (`crates/application/src/process_managers/refund.rs:52`), whose declared `read:` column set is
   `{payment_status, payment_intent_id, total_amount_cents}`
   (`crates/application/src/generated/process_managers.rs:1076-1084`). **Adding `restaurant_id` to that
   list is a one-column spec edit that looks like plumbing and is a first-order violation on the money
   path.** That is the sentence #635 inherits.

   **The SOURCE of the write-side comparison is OPEN, not decided here.** Two routes, both avoiding the
   read model, and the diff settles it (ADR-20260818-094500 finding 6, `:98-105`):
   - **(a)** fold `PaymentState.restaurant_id` (`crates/domain/src/payment.rs:47`) on the approve leg —
     no new column, but a cross-aggregate load inside the PM leg, because `RefundProcessRow` has **no**
     `restaurant_id` (`crates/application/src/generated/pm_state.rs:54-68`, all eight fields listed).
   - **(b)** persist `restaurant_id` on the run row at open, from the opening fact, which carries it as
     required (`RefundOpened.restaurant_id`, asserted at
     `crates/application/src/process_managers/refund.rs:474`). The IDDD-shaped answer: the process
     manager keeps its own process state.

   **The ordering fact #635 must carry, and it is the sharp one**: in
   `crates/application/src/generated/process_managers.rs` the Stripe refund fires at **:1443**
   (`payment.refund(input, …)`) and the `Payment-<intentId>` stream is loaded at **:1457**. An ownership
   check placed on folded Payment state, as the leg stands today, would happen **after the money has
   already moved** — the same prepare-before-the-fence shape as `actor_runtime/src/completion.rs:69`.
   Route (b) does not have that problem; route (a) must move the load.
4. **`unbound ⇒ denied` outranks `other-restaurant ⇒ denied`.** Enacted as P1 being the first test written,
   and as the only arm reachable today.
5. **`claimRestaurantListing`'s `PUBLIC` role is resolved explicitly, never discovered later.** This chunk
   **does not touch the claim path**, so it does not resolve it here — it carries it forward with an owner
   and a moment: it is the **first checkpoint item** of the chunk that builds the claim path, on
   [#639](https://github.com/TheCaptainCompany/captain-food/issues/639). The facts that make it urgent are
   already verified in ADR-20260818-094500 finding 1: `specs/network/api.yaml:239-242` is
   `roles: [PUBLIC, RESTAURANT_ACCOUNT]`, and `RestaurantListingClaimed` **grants a `ScopeMembership` row**
   (`specs/database/tables/projection_tables.yaml:1038`) whose `accountId` is nullable — an anonymous
   caller writing the table every RLS predicate resolves against, granting membership to nobody. **Do not
   let this chunk quietly become the place that "fixes" it.**

---

## `specs/**` changes this dispatch carries, and the approval each rests on

| Change | Approval |
|---|---|
| **`specs/common/configuration.yaml`** — one new key, **named here so the executor does not coin it**: **`READ_SCOPE_BINDING_MODE`**, `type: enum, values: [off, observe, enforce], default: observe` (`APP_PROFILE`, `:94-100`, is the shape precedent). Placed in `common`, not `payments`, because a bin's key subset is *"its linked scopes + owning scope + common"* (`tools/codegen-rs/src/emit/bins.rs`) and this seam is served from `crates/server` by every gateway; a scope-local key risks a surface that serves the query without the flag that governs it. **`BINDING` is the right stem**: it matches the card's own bind-vs-narrow distinction and leaves #635's write-side sibling a symmetric name | **Recorded**: ADR-20260818-101500 requires the three-valued flag by name |
| **`specs/observability.yaml`** — one new metric on the existing `read_authorization` contract (`:1436-1442`): **`read_authorization_scope_mismatch_total{operation, role, mode}`**, all three attributes drawn from bounded populations. **Renamed from revision 1's `…_filter_mismatch_total`** on `evans`' reading: every sibling on that contract names an authorization fact (`_denied_total`, `_checks_total`, `_bridge_unresolved_total`), `filter` is the implementation word (the `RefundFilter` field), and the ADR banks *"the mismatch metric"* — not "filter". Free now; a migration once emitted | **Recorded**: the same ADR requires *"the mismatch metric declared in `specs/observability.yaml` before the enforcing code lands"* |
| **`specs/payments/api.yaml`** — rewrite `pendingRefunds`' description (`:102-109`) **and fix the false inline comments on `approveRefund` (`:165`) and `denyRefund` (`:170`)**. Replacement text is in the card, below — it is not left to the executor | **Team's** under [ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md): it contradicts no recorded decision (it records one already made), and it changes **no shape** — the `restaurantId` arg **stays**, so nothing about the GraphQL contract is non-additive. **One `docs/SPEC-LOG.md` sentence in the SAME commit** |
| **`specs/screens/restaurant_backoffice.yaml:30-31`** — the resolver-block comment says the restaurant scope *"is caller-supplied at runtime from the staff session's restaurant binding."* False for `refunds.pending` after this merges. Same sentence, same commit | **Team's**, same clause; comment only, no shape |
| **`specs/screens/restaurant_backoffice.yaml:187-207`** — one `gaps:` entry on `refunds_queue`, which has none today | **Team's**, same clause; see below |

### The `gaps:` entry, and why no copy rewrite can substitute for it

`refunds_queue` (`:187-207`) has no `gaps:` block. After this merge the **not-bound** case and the
**genuinely empty** case are indistinguishable on screen: the empty state asserts
`back.refunds.empty.title` / `.body` = *"No pending refunds"* / *"Aucun remboursement en attente"*
(`specs/screens/restaurant_backoffice.translations.yaml:47-48`) — an assertion about the world, and false
for the only RESTAURANT population that exists. `staff_topbar` renders a static title and never the
restaurant's name, so there is no second signifier either.

**This is not a request for a copy rewrite.** No wording can discriminate the two cases client-side today,
so changing the French would be theatre. The `gaps:` entry names the missing discriminating state and its
owner ([#639](https://github.com/TheCaptainCompany/captain-food/issues/639)), which is what a `gaps:` entry
is for.

### The replacement description — the literal text, reviewed here rather than in the diff

`evans` was right that on a card that reviews every assertion in a test suite, the one sentence a future
reader uses to decide who may call this operation must not be the only unreviewed artifact. And `legal-specialist`
is right that revision 1's justification was **half true**: *"a credential that OMITS the filter reads every
restaurant's queue"* becomes false at merge, but *"one that supplies another restaurant's id reads that queue"*
**stays true at the shipped `OBSERVE` default**. A spec that describes a control as active when it is
observing is an inaccurate record of technical measures, in the artifact we would hand a DPO.

```yaml
  pendingRefunds:
    description: >
      The refund queue (RefundProcess): refunds opened for decision, with their lifecycle status
      (status = REQUESTED is the pending, awaiting-decision queue). An ADMIN arbitrates across
      restaurants and reads the whole queue.

      `restaurantId` is a SELECTOR WITHIN THE CALLER'S SCOPE, NEVER A GRANT. For a RESTAURANT caller
      the server binds the queue to the restaurant the caller's verified claim resolves to, and the
      argument may only narrow within it: no argument returns the caller's own queue, the caller's own
      id returns the same queue, and any OTHER restaurant's id returns an EMPTY list (bound scope
      INTERSECT requested filter) and increments read_authorization_scope_mismatch_total. A RESTAURANT
      caller whose token carries no restaurant binding reads nothing.

      MODE-CONDITIONAL, and this is the shipped default: the binding above applies in full only under
      READ_SCOPE_BINDING_MODE=ENFORCE. Under the default OBSERVE the unbound caller is already denied,
      but a BOUND caller supplying another restaurant's id is still served that restaurant's rows and
      the mismatch is only counted. Under OFF -- the incident rollback rung -- every RESTAURANT
      credential reads the whole platform's queue again.

      NOT YET CLOSED, present tense: the matching WRITE half is unbound (approveRefund / denyRefund,
      #635 / #178), and this is ONE of the seven unscoped read surfaces of #618 -- the others are
      unchanged. The dated record of what this operation exposed, from when, and how far it is
      remediated is DECISIONS.md section 39 (IDOR-1).
```

**`legal-specialist`'s three requirements, each satisfiable by reading that text**: (i) mode-conditionality
is stated in the description itself, third paragraph; (ii) the live remainder is present tense, fourth
paragraph, with the #618 / §39 pointers kept rather than dropped as historical; (iii) the historical fact
is **moved, dated, not deleted** — into DECISIONS §39 and the `docs/SPEC-LOG.md` sentence, with the
description pointing at it.

> ⚠️ **One clause of requirement (ii) is deliberately narrower than `legal-specialist` asked, and the
> reason is a fact neither of us had at briefing.** Revision 1's description states the hole
> operationally — *"a RESTAURANT credential that OMITS it reads every restaurant's refund queue."* That
> text is **not** in the committed SDL (`specs/generated/schema.generated.graphql:1830` carries no
> descriptions), but it **is** a field description in the **runtime** schema: the generated doc comment
> at `crates/server/src/graphql/generated/query.rs:569` becomes an async-graphql field description, and
> `visible_restaurant_admin` makes it introspectable by **any RESTAURANT credential**. Publishing *"the
> write half is unbound, so you can approve anybody's refund"* to the population that would exploit it is
> not a records improvement. The text above therefore names the gap, its issues and its register row — a
> pointer a DPO can follow — and stops short of the operational recipe. **The detailed record lives in
> the register and the SPEC-LOG, which is where a contemporaneous-records defence actually lives.**
> Legal should confirm or overrule this at the checkpoint; the surrounding requirements are unaffected
> either way.

### `approveRefund` / `denyRefund` — the surviving false sentence

`approveRefund` has **no description at all**; its only prose is the inline comment
`# restaurant decides its own orders; admin arbitrates` (`specs/payments/api.yaml:165`, repeated for
`denyRefund` at `:170`). That is **false today and still false after this chunk** — those two mutations
consult no identity anywhere. Right now `pendingRefunds`' description is the only place in that file
where the write hole is written down; fix the read description without fixing these and the surviving
sentence is the false one. Both comments become, in the same commit:

```
    roles: [RESTAURANT, ADMIN]   # NOT bound to the caller: neither command consults any identity
                                 # (#635 / #178, DECISIONS §39). The role list is a membership test
                                 # only -- any RESTAURANT credential can decide any restaurant's refund.
```

Prose, no shape moves.

**Two things this dispatch does NOT have approval for, and must not do:**

- **No `access:` / `authorization:` DSL block.** DECISIONS §46 **AUTHZ-GRAMMAR** records it as **declined**
  as new grammar. Deriving the read-side declaration from the DSL is
  [#649 "The read side has no access declaration…"](https://github.com/TheCaptainCompany/captain-food/issues/649),
  which the founder raised himself. This chunk authors the policy in the emitter, like every other resolver
  body, and says so.
- **No new `rules.yaml` entry.** ADR-0032 requires every rule to be linked from a behaviour test, and
  `specs/tests.yaml` tests are **aggregate-level** (`actor` + `given` events + `when` + `then`, e.g.
  `TestPendingRefundVisibleUntilDecided` against `actors.yaml#/Payment`). A resolver-scoping rule is
  inexpressible there, so adding the rule would force either a red gate or a fake domain test. **Fence it
  and report it** — that the DSL has no home for a read-side authorization rule is exactly #649's subject.
- **Do not try to `$ref` the mode's value set.** It will exist three times with no `$ref` joining them (the
  config key's `values:`, the Rust enum, the metric's `mode` attribute note). The usual repair — one enum
  scalar `$ref`'d from the key — is **not expressible today**: `scalar:` binding is pattern-based and an
  enum scalar declares no `pattern`, so it raises `config-scalar-no-pattern`
  (`tools/codegen-rs/src/config.rs:186-199`). State the triplication as a known invisible edge in the PR
  body; leave it to #649's neighbourhood.

---

## Where the code goes, and the plumbing traps

### The prelude comes from a TABLE, not a fourth hand-copied arm — this is the one addition in scope

`tools/codegen-rs/src/emit/server_graphql.rs` is a `match op_name` over Rust source literals.
`myReclamations` (`:729`) and `customerCredit` (`:742`) **each carry a hand-copied**
`ctx.data_opt::<ReadScope>().cloned().unwrap_or(ReadScope::Public)` prelude — the same 200-character
comment and all — while `restaurantReclamations` (`:732`) has **none**, with a comment conceding that the
narrowing is a follow-up gap. Same defect, three arms apart. Second occurrence ⇒ a rule, not a comment
(ADR-20260803-234035, compiler-first). `graphql-architect` will stop the work at review if this lands as a
bespoke `if` inside the `"pendingRefunds"` arm.

**Wanted in this diff:**

1. **One table.** `read_scope_binding(op_name) -> Option<Binding>`, consulted by the shared emit path,
   which emits the prelude and the scope destructure. The **bodies** stay per-arm and the table does not
   pretend otherwise — `myReclamations`/`customerCredit` bind by *replacing the read*
   (`by_customer(...)`), `pendingRefunds` binds by *intersecting a filter*. What the table removes is the
   copy-pasted prelude, which is exactly what was copied wrong.
2. **One validator rule.** Every `api.yaml` **query** whose `roles` include a tenant-scoped role
   (`CUSTOMER` · `RESTAURANT` · `RESTAURANT_ACCOUNT` · `RIDER`) is **either** in that table **or** on an
   explicit exempt list. **The exempt reason is not prose**: it is a `file:line` pointing at where the
   scoping actually happens, or the literal token `UNSCOPED — #618`. That makes the list mechanical rather
   than inventive, and it makes #618's headline count a **derived artifact** instead of a measured claim.
3. **No drift is possible between the two**, because the validator lives in the same binary and calls the
   same function. Do not duplicate the table in `validate/`.

**Population, with its antecedent** — computed this run by parsing `specs/*/api.yaml` and testing each
`queries.*.roles` against that four-role set: **20 of 32 declared queries** are tenant-scoped. Three of
them are bound in the emitter after this chunk (`pendingRefunds`, `myReclamations`, `customerCredit`); the
rest thread `ReadScope` into their repository call or are genuinely unscoped. **So the exempt list starts
at ~17 rows**, and writing it honestly is the largest single piece of this diff. **If any row's reason is
not a `file:line` you can point at, do not invent one — mark it `UNSCOPED — #618` and say so at the
checkpoint.** #649 later swaps the table's SOURCE (literal → DSL-derived) without touching the emit path or
a single test.

### The rest

- **The resolver body is emitted, not written.** Edit the `"pendingRefunds"` arm at
  `tools/codegen-rs/src/emit/server_graphql.rs:712` and run `make generate`;
  `crates/server/src/graphql/generated/query.rs:571-581` carries the GENERATED header and is inside
  `check-drift`.
- **Trap 1 — the mode must reach BOTH transports, and the compiler should be what guarantees it.**
  `crates/server/src/graphql/routes.rs` injects request data in two independent places: the POST handler
  (`:175-182`) and the WS `on_connection_init` closure (`:273-295`), whose own comment says *"a subscription
  must not widen what a query would refuse (#144/#433)."* If the mode is injected only on POST, the socket
  falls to the absent-default `ENFORCE` — **safe, but the `OFF` rung then does not cover the socket**, i.e.
  the escape hatch fails on one transport. **Compiler-first repair, preferred**: build the per-request datum
  set in **one** function both sites call, so a one-transport injection is unspellable. **If that is not
  reachable** — the two sites use different async-graphql APIs (`Request::data(..)` chaining vs
  `Data::insert`) and the executor may find the shapes do not unify cleanly — then add a probe in
  `routes.rs`'s existing `mod tests` (`:348`) asserting both sites carry it, and **say at the checkpoint
  which of the two you did and why**. Either way `graphql_routes` gains the mode as a **required, non-`Option`
  parameter**: an `Option<Mode>` defaulting to `ENFORCE` inside would make "forgot to wire it" indistinguishable
  from "chose ENFORCE", which is trap 1 in a new costume. **Six call sites**, verified by grep this run:
  the definition (`routes.rs:49`), two production (`crates/server/src/lib.rs:1332`,
  `crates/server/src/bin_support.rs:73`) and three test (`routes.rs:413`,
  `crates/server/tests/public_credential_degraded_metric.rs:84`, `crates/server/tests/graphql_cart_read.rs:931`).
  Revision 1 said "three production call sites" and omitted the tests; the number is six.
- **Trap 2 — a process global would make P5 unwritable.** Do not reach for a `OnceLock` mode read from the
  composition root: the two-modes-differ probe needs two modes in one test binary. Per-request injection is
  what makes the test possible, which is the same reason `telemetry::meters`' once-per-process meter forces
  the metric probe into its own binary.
- **`crates/telemetry`**: one `pub const` in `contract.rs` (beside
  `READ_AUTHORIZATION_BRIDGE_UNRESOLVED_TOTAL`, `:200`) and one instrument built from it in `meters.rs`
  (inside `pub mod read_authorization`, `:428`). **This is not optional and not deferrable**:
  `tools/codegen-rs/src/validate/metric_emitters.rs:131,141` warns `obs-metric-no-emitter` for a declared
  metric with no constant **or** no instrument, and that warning is on the per-rule ratchet where *"ONE MORE
  is a hard gate failure."* Declaring the metric in the spec without its emit site in the **same commit**
  turns `make validate` red. Adding a metric **with** its emitter adds no warning, so no baseline churn is
  expected — but if the gate says the surface moved, run `make warning-baseline` and commit the artifact in
  the same commit with the reason.
- **Do not emit the mismatch on `read_authorization_denied_total`.** That contract's own note says list
  denials are *"structurally invisible"* (`specs/observability.yaml:1435`;
  `crates/telemetry/src/meters.rs:469`) and *"do not 'fix' the missing list denials."* A **scope mismatch** is
  a discrete per-request fact, not a per-row decision, so it is legitimately emittable on a list path — but it
  needs its own name, or the next reader will read it as the thing the comment forbids.
- **Add no index.** `domain_events` has three (`migrations/20260717120000_domain_schema.sql:125-127`:
  `(stream_name, version)`, `(event_type)`, `(occurred_at)`). The view's `restaurant_id` is
  `(c.payload->>'restaurantId')::uuid` — unindexable as written — but the qual inlines to the base relation
  and is evaluated **before** the six correlated target-list subqueries run for a row. So binding makes the
  restaurant caller **cheaper** than today. *(Revision note: `dba` said five subqueries; there are six —
  `status`, `approved_amount_cents`, `reason`, `refund_id`, `decided_at`, `updated_at`,
  `migrations/20260730043600_enum_text_recreate_views.sql:139-165`. It does not change the conclusion.)*
- **The ADMIN arm is unchanged — and unbounded.** `status = 'REQUESTED'`
  (`crates/infrastructure/src/persistence/refund_queue.rs:69-71`) references a target-list subquery, so it
  **cannot be pushed down**: the full fold runs over every `RefundOpened` ever recorded, then the pending
  subset is taken, then a sort with **no `LIMIT`** (`:72`). Small today, monotonic, never sheds. Write it as
  *"unchanged — and unbounded"* in the PR body and **carry it to a follow-up issue rather than to nobody**;
  a follow-up issue is needed and this dispatch does not file it. **Pagination stays out of this chunk**:
  `first/after` on `[Refund!]` is a non-additive money-path change and would destroy the *"the schema does not
  change at all"* property every probe here rests on.
- **Tests**: `crates/server/tests/graphql_pending_refunds_scope.rs` (P1–P6, P8, P9) and
  `crates/server/tests/pending_refunds_mismatch_metric.rs` (P7, own binary, exactly one `#[test]`). **Both are
  auto-discovered and picked up by TWO CI invocations, not one** — `crates/server/Cargo.toml` declares no
  `[[test]]` and no `autotests = false`, so they run under `ci.yml:207` (`cargo test --workspace`, the
  build-test job, `DB_TESTS_REQUIRED: '0'`, no `DATABASE_URL`) **and** `ci.yml:285` (`-p server --tests`, the
  db-test job). The no-DB job is the one that proves they need no database. **No workflow edit** — and do not
  create a new crate ([#335](https://github.com/TheCaptainCompany/captain-food/issues/335): a suite in a new
  crate never runs and nothing reports it).

### The mode nobody can observe, and the rung with a dated expiry

Two facts `farley` verified that revision 1 did not carry. Both are one sentence each in the PR body and the
STATUS line; neither is code beyond P9.

**(1) `OFF` is Render-only and expires at the #358 cutover.** The claim *"an env override plus a pod restart,
no rebuild, no image, no CI, no migration"* is **true today** — `crates/server/src/generated/config.rs` reads
every declared key from the process env at startup, and `deploy.yml` is the only workflow touching Render. It
is **false on MKS**: `tools/codegen-rs/src/emit/deploy.rs:210-226` (`env_yaml`) renders **`APP_PROFILE` + `PORT`
+ secret-sourced keys only** — verified in the emitted output at
`deploy/generated/manifests/bins/gateway-restaurant.yaml:40-52`, which carries exactly those. A **non-secret**
enum key gets no env line in any of the 57 bin manifests, there is no ConfigMap, no overlay, and no Argo
`Application` anywhere in `deploy/`. After cutover the only override is hand-editing a GENERATED manifest
(drift, erased by the next `make generate`) or an out-of-band `kubectl set env`. **And this card ties the
`ENFORCE` flip to a future chunk that may land on the far side of that cutover.** The sentence:
*the `OFF` rung is Render-env-only; the manifest emitter does not render this key, so #358's cutover chunk or
the `ENFORCE`-flip chunk owns rendering it.*

**(2) Nothing proves which mode a pod is running.** `render-config-sync.yml:18-19` explicitly does not push
non-secret values (they are baked into the image), so an operator-set `READ_SCOPE_BINDING_MODE` is untracked
mutable config — the exact `RUN_SIRENE_WORKER` failure that workflow's own header narrates. And the only signal
carrying `mode` fires **only on mismatch**, which is zero traffic today, so a pod silently running `OFF` is
indistinguishable from one running `ENFORCE`. That is CLAUDE.md's named defect class: *a monitoring path that
can only fire when a signal arrives.* **The fix is free**: `Config::boot_report()`
(`crates/server/src/generated/config.rs:695`, called at `crates/server/src/main.rs:56`) already prints every
declared **non-secret** key's resolved value verbatim — `LOG_LEVEL` at `:714` is the shape. So the new key gets
a boot-report line for nothing, and **P9 asserts it appears there.** Say so in the PR body.

**(3) `OFF` has an unnamed expiry, and it is a disclosure switch.** `business-specialist`: `OFF` restores *"every
restaurant credential reads the whole platform's refund queue."* Fine while the population is zero; the moment a
real restaurateur holds a credential, `OFF` is not a rollback, it is a **cross-member disclosure switch an
operator can throw at 20:00 on a Friday with no second signature**. The `docs/STATUS.md` line therefore carries
the **sunset** as well as the default.

---

## What this chunk does NOT do, and who carries the remainder

- **It does not close [#618](https://github.com/TheCaptainCompany/captain-food/issues/618).** That issue is a
  **class**: *"7 unscoped read surfaces"*, two of which return other tenants' rows when called with no arguments
  (antecedent: DECISIONS §39 scope correction of 2026-08-17 — **quoted, not re-measured by this card**). This
  chunk fixes **one** of them. The PR references the issue and ticks one box; it does not write `Closes`.
- **It does not bind any write.** `approveRefund` / `denyRefund` consult no identity anywhere —
  [#635](https://github.com/TheCaptainCompany/captain-food/issues/635) (the PM-decided, "unbindable" money-path
  pair) and [#178](https://github.com/TheCaptainCompany/captain-food/issues/178) (the write seam). The live
  `approve_refund` widget on `specs/screens/restaurant_backoffice.yaml:205` stays exactly as authorized as it is
  today, which is: not at all. **This is the compounding chain to say out loud in the PR body, and it is
  `business-specialist`'s one artifact keeping the remaining half priced — keep it verbatim**: after this chunk a
  restaurant sees only its own refunds and can still approve anybody's. Closing the read surface also removes the
  only place a co-op member could **notice** an out-of-tenant decision: net exposure moves from *embarrassing and
  self-detecting* to *smaller and undetectable*. That is acceptable **only** because the write half has a named
  owner and this card forbids `Closes`.
- **It adds no `operation` attribute to `read_authorization_bridge_unresolved_total`.** `ux-designer` is right
  that the unbound denial fires with `{role}` only (`crates/server/src/auth.rs:1803`), so when the queue goes dark
  for the population that exists, nothing answers *"which surface, how often"*. **Fenced, not omitted**: that
  counter is **emitted today**, so adding a label changes the identity of every existing series — a shipped-shape
  change under CLAUDE.md question 2, needing its own versioning sentence. It belongs with #618's next surface, not
  here. **A follow-up issue is needed.**
- **It mints no credential and builds no sign-in** — [#639](https://github.com/TheCaptainCompany/captain-food/issues/639);
  the email-link mechanism is decided (ADR-20260818-101500 decision 1) and unbuilt.
- **It creates no `sub → domain id` mapping** — [#641](https://github.com/TheCaptainCompany/captain-food/issues/641) /
  DECISIONS §46 IDENT-1, a recorded MIGRATION.
- **It does not resolve `claimRestaurantListing`'s `PUBLIC` role** — carried to #639 as its first checkpoint item,
  per constraint 5.
- **It does not paginate the ADMIN queue** — see above; non-additive on the money path.
- **It does not touch [#638](https://github.com/TheCaptainCompany/captain-food/issues/638)**, frozen at chunk 1 by
  founder decision. Nor #649's DSL derivation.
- **It does not flip anything to `ENFORCE`.** The flip is a separate recorded decision, and its natural moment is
  the chunk that mints the first restaurant claim: **that chunk may not merge with this flag below `ENFORCE`**,
  because its first authenticated request is §39 trigger (i).
- **It does not start, toll or discharge any Art. 33 clock.** The 72 hours run from awareness of an actual breach,
  not of a vulnerability; remediating one instance does none of the three. Two consequences the PR body must
  respect: the new mismatch counter is **prospective and bound-caller-only**, so it must never be cited as *"no
  mismatches, therefore no breach"*; and the legally material event remains §39 trigger (i). *(Not this chunk's
  work, flagged for #618's parent: whether request logs retain enough to determine that no cross-tenant read
  occurred — without that, an Art. 33(1) "unlikely to result in a risk" determination can only be asserted, not
  made.)*
- **Visible age, so the ungated half cannot be forgotten**: add one line to `docs/STATUS.md` at merge —
  *`READ_SCOPE_BINDING_MODE` defaults to `OBSERVE`; the bound-caller arm is unreachable and unflipped, since
  2026-08-18. The `OFF` rung is Render-env-only (the manifest emitter renders no non-secret key) and **dies at the
  credential-minting chunk**, where it stops being a rollback and becomes a cross-member disclosure switch.* If
  that line is still there when a restaurant credential exists, the chunk failed and it will be legible.

---

## What BANKS at the checkpoint

Per ADR-20260816-134352 and ADR-20260817-105845, the executor states each of these explicitly rather than leaving
them to the reviewer:

1. **Did the narrowed checkpoint set miss anything**, with an **attribution**: card defect · invited-lens depth
   miss · roster width. Only a roster-width miss returns to the founder. **The checkpoint set is the eleven lenses
   listed in `## Findings`** — every one declared a concern at briefing, so the narrowed set is the full roster this
   time, and item 1 is answered accordingly.
2. **The literal rewritten description text**, checked against `legal-specialist`'s (i)/(ii)/(iii) — **and the
   narrowing of (ii)** for the introspection reason given above. Legal confirms or overrules.
3. **Intersection, not substitution**, on the `ENFORCE` + conflicting-filter arm — and that P3 asserts the
   repository was **not called**.
4. **`OBSERVE` denies the unbound arm**, and the corrected framing that the arm is ungated *between `OBSERVE` and
   `ENFORCE`* while `OFF` restores it as an incident rung. `evans` banked that `OBSERVE` is the wrong industry word
   for a mode that changes behaviour for the population that exists; **the three tokens are the founder's, recorded
   verbatim in ADR-20260818-101500, so this is not a rename** — the repair is the four-row table at the declaration
   site (below).
5. **Where the four-row mode table went**, and why it is a YAML comment block above the key rather than inside
   `gates:` — see the note below; the executor confirms the generated doc comment reads correctly.
6. **Default `OBSERVE`**, on the claim that the two modes are observationally identical today. Antecedent: no claim
   writer for `restaurant_id` exists (`stamp_put_body` writes `{role, customer_id}` only,
   `crates/infrastructure/src/integrations/supabase_auth.rs:424-433`). If a hand-stamped console token exists
   somewhere, this claim is false and the default deserves re-argument.
7. **Absent mode ⇒ `ENFORCE`** (fail closed), the required non-`Option` router parameter, and which of the two trap-1
   repairs was taken.
8. **The binding table + validator rule**, and the honesty of the ~17-row exempt list: how many rows carry a real
   `file:line` and how many carry `UNSCOPED — #618`.
9. **The new counter's home** in the `read_authorization` contract, against that contract's *"list denials are
   structurally invisible"* note — and its **new name**.
10. **The dated correction note owed on ADR-20260818-101500** for the "projector lag" clause.

### The four-row table goes at the declaration site — as a comment, and here is why not in `gates:`

`evans` is right that a mode enum whose definition lives only in a dispatch card is a convention, not a published
term, and that the repair belongs at the declaration site. **The literal instruction — put the four-row table in the
key's `gates:` prose — does not work**, and the reason is mechanical and verified:
`tools/codegen-rs/src/config.rs:496,536,605` all do `k.gates.replace('\n', " ")`. A table in `gates:` is flattened to
one line in the generated Rust doc comment **and in the operator-facing boot report** — the worst place for an
unreadable line is the artifact someone reads at 20:00.

**So**: the four-row table goes in a **YAML comment block immediately above the key** in
`specs/common/configuration.yaml` — which is where every other substantive note in that file already lives (see the
Identity block at `:131-140`) — and `gates:` carries one sentence naming the two axes the three tokens encode:
*(a) does the caller's scope apply when they asked for nothing; (b) who wins when request and scope conflict.* The
declaration site then defines the term, and nothing gets mangled.

## Definition of done

- P1–P9 green; the **nine** mutations each planted, seen red, reverted, and **claimed in the PR body with this
  card's table as their antecedent**.
- `make rust` green · `make validate` **0 errors** · `check-drift` clean · one `docs/SPEC-LOG.md` sentence · one
  `docs/STATUS.md` line.
- PR body states, in the words above: what "read per request" actually costs to flip **and that `OFF` is
  Render-only with a named owner for the MKS gap**; that the write half is still open, with the compounding-chain
  sentence verbatim; that #618 is not closed; **why the mismatch counter owes no `fold:`** (see below); the ADMIN
  arm as *"unchanged — and unbounded"*; and the mode triplication as a known invisible edge.
- `HOLD: human` — ready-for-review, independent reviewer pass over the full diff, then the coordinator merges.
  **Never auto-merge.**

**Why the mismatch counter owes no `fold:` — one paragraph the PR body must contain** (`young`). Under
[ADR-20260811-014129](../adr/ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md)
every feature carries a business metric, and a business metric is a **declared `fold:` over `domain_events`**. This
counter is not one and cannot be: **a read that returned the wrong rows writes no event**, so there is nothing in the
log to fold — not as a matter of effort, as a matter of what the log contains. It is a **defect counter** on exactly
the same footing as `read_authorization_bridge_unresolved_total`, which lives on OTLP for the same reason, not a
persona-activity outcome. Without that paragraph a reviewer demands a `bam` fold and the executor either argues it
badly or builds a projection over a fact that does not exist.

**Standing instruction for this PR** (`farley`): a red `build-test` is triaged against the
`crash-evidence-build-test` artifact (`.github/workflows/ci.yml:215-230`) **before any re-run** — the #388 SIGSEGV
scaffolding exists because the flake is live. *"Re-ran it and it went green"* is not a verdict, and on a `HOLD: human`
PR it trains the reviewer to stop reading the gate.

## RISK — the one most likely to go wrong

**The gate lands and the behaviour does not.** Every artifact of this chunk — a config key, a metric, a mode enum, a
binding table — is the kind of thing that can be present, well-tested and inert: the mode injected on one transport,
the counter built but never called on the path that matters, the table added while `"pendingRefunds"` keeps a bespoke
`if`, and a green suite over a resolver whose default arm no real caller reaches. The specific defences are P7's
both-ways assertion, P6's `OFF` probe, P9's boot-report assertion, and the `ReadScope`-blind fake; the general defence
is that **P1 is written first and fails first**, on the only arm production can actually reach today.

**Secondary**: PR [#587](https://github.com/TheCaptainCompany/captain-food/pull/587) is believed open against the
codegen tree (`564-reader-derivation-pr2`) — **`UNVERIFIED input`: no GitHub read was available in the revision
session, so confirm before starting.** If it is open, a textual conflict in `tools/codegen-rs/src/emit/` is likely,
and the binding table makes it more likely, not less; rebase rather than merge, and re-run `make generate` afterwards.

## Findings

Lens concerns banked at the briefing on revision 1, each carried into the revision above. **Attribution for every
one: CARD DEFECT.** No roster-width miss; nothing escalated to the founder.

| Lens | Concern banked | Where it landed |
|---|---|---|
| **graphql-architect** | `ENFORCE` + conflicting filter returned the bound rows — **substitution**, which makes the schema lie. Withdraws its `myPendingRefunds` twin **only if** the retained argument has an honest semantic | §Scope, intersection rule; P3; P5; the description's *"selector within the caller's scope, never a grant"* |
| **graphql-architect** | The prelude is hand-copied across three arms; a fourth copy will be stopped at review. Wants a table + a validator rule over `roles` | "The prelude comes from a TABLE" — population verified at 20 of 32 queries |
| **graphql-architect** | `restaurant_backoffice.yaml:31` claims runtime caller-supplied scoping — false after merge | `specs/**` table, row 4 |
| **legal-specialist** | Only **half** the description becomes false; a control described as active while it observes is an inaccurate record of technical measures. Three diff-checkable requirements | The literal replacement text, (i)/(ii)/(iii) — with (ii) deliberately narrowed for introspection, flagged for legal to overrule |
| **legal-specialist** | Art. 33 clock unchanged; the counter is prospective and bound-caller-only and must never be cited as "no mismatches, no breach" | "What this chunk does NOT do", Art. 33 bullet |
| **dba** | Constraint 3's reason is **invented** — no projector exists; the view folds on read | Constraint 3, withdrawn-and-replaced block, plus the correction note owed on the ADR |
| **dba** | `OBSERVE` must compare filter **values** in one execution, not two result sets | §Scope, its own subsection |
| **dba** | The ADMIN arm is *"unchanged — **and unbounded**"*; add no index | Two bullets under "The rest"; subquery count corrected 5 → 6 |
| **young** | The card named the wrong decoy: the reachable wrong answer is widening `RefundProcess`'s OrderTracking `read:` columns by one | Constraint 3 |
| **young** | The PR body must say why the counter owes no `fold:` | Definition of done, its own paragraph |
| **vernon** | Constraint 3 over-commits the write design; mark the SOURCE **open**, with the two routes | Constraint 3, routes (a)/(b) |
| **vernon + young** | The refund fires at `:1443` before the stream loads at `:1457` — an ownership check on folded state would land after the money moved | Constraint 3, ordering fact |
| **beck** | The fake must be `ReadScope`-blind and filter-capturing; the in-repo precedent (`graphql_subscriptions.rs:50`) teaches the opposite | "The fake repository" |
| **beck** | P5 and P8 anchored on `[] == []`; M6 tautological unless expectations are literals; P2/P4/P6 had no mutation | Probe table (triples); M6 note; M7/M8/M9 — count is nine |
| **beck** | Trap 1 should be unspellable, not remembered | Trap 1, with a named fallback and a checkpoint report |
| **beck** | P1's antecedent is narrated | **Partially rejected** — see below; replaced by citing `auth.rs:1910` and giving that assertion a message |
| **evans** | The card contradicts itself on the unbound arm: "not gated" vs `OFF` → everything, which P6 asserts | "Why the unbound arm is not gated *between…*" |
| **evans** | `OBSERVE` is the wrong industry word for a two-axis policy; the repair is at the declaration site, not a rename | The four-row table subsection |
| **evans** | The config key is never named where it is declared | `READ_SCOPE_BINDING_MODE`, named in the `specs/**` table |
| **evans** | `filter` is the implementation word; the ADR banks "the mismatch metric" | Renamed `read_authorization_scope_mismatch_total` |
| **evans** | `approveRefund`'s comment at `:165` is false and survives the rewrite | Its own subsection, fixed in the same commit |
| **evans** | Do not attempt to `$ref` the mode's value set — `config-scalar-no-pattern` | "Two things this dispatch does NOT have approval for", third bullet |
| **ux-designer** | `refunds_queue` has no `gaps:`; not-bound is indistinguishable from empty and no copy can fix it | The `gaps:` subsection |
| **ux-designer** | The unbound denial carries no `operation` attribute | **Fenced** — the counter is emitted today, so a label change is a shipped-shape change; follow-up issue needed |
| **farley** | `OFF` is Render-only and expires at the #358 cutover; the manifest emitter renders no non-secret key | "The mode nobody can observe", (1) — verified in the emitted manifest |
| **farley** | Nothing proves which mode a pod runs; the only signal fires on mismatch | (2), plus **P9** |
| **farley** | P7's binary must hold exactly one `#[test]` | Probe table, P7 |
| **farley** | Both binaries run in **two** CI invocations, not one; a `common` key regenerates **58** config readers and **0** manifests | "Tests" bullet; both figures verified this run and stated plainly — item 6 of revision 1's `UNVERIFIED input` list is resolved |
| **farley** | Triage a red `build-test` against the crash-evidence artifact before any re-run | Standing instruction |
| **business-specialist** | `OFF` has an unnamed expiry and becomes a disclosure switch; keep the compounding-chain sentence verbatim | "The mode nobody can observe", (3); STATUS line sunset; the `Closes`-forbidden bullet |
| **business-specialist** | Right to leave out: the refund-decision SLA fold and liability attribution on the command | Left out, deliberately — no population and no writer respectively; a nullable column never populated is worse than none |
| **holub** | The restaurateur item is parked with no row and no owner — that is inventory | The disposition, rewritten: founder's queue this turn **plus** a `DECISIONS.md` row owed if unanswered |
| **holub** | Nothing in the card bounds the number of internals chunks. Name chunk 2 and the length | "`holub`'s counting question, answered here" — three-chunk floor, chunk 2 named, upper bound explicitly unstateable and why |

### What this revision REJECTED, and why

Two items were not absorbed. A lens can be wrong, and saying so is cheaper than a defect shipped under a lens's name.

1. **`beck`, item 2 — "call `auth::read_scope(...)` instead of hardcoding `ReadScope::Public`" — is not possible from
   this test binary, and the concern has a better answer.** `mod auth;` is **private**
   (`crates/server/src/lib.rs:74`); only `AuthContext` (`:100`) and `Principal` (`:103`) are re-exported, and
   `Identity` plus every `Principal` constructor are `pub(crate)`. An integration test in `crates/server/tests/` can
   neither call `read_scope` nor build an `Identity::Unbound`. Doing it would mean widening the server crate's public
   API to satisfy a test. **The antecedent is already asserted anyway**, at `crates/server/src/auth.rs:1910` — so the
   hole cannot silently reopen; that line goes red first. The repair taken instead: cite `auth.rs:1910` by file and
   line in the test's doc comment, and give that assertion the message it lacks, naming its dependent. The concern is
   met at the site that would break, for one line and no API change.
2. **`evans`' repair site is right and its literal form is wrong.** The four-row mode table **cannot** go inside the
   config key's `gates:` prose: `tools/codegen-rs/src/config.rs:496,536,605` flatten `gates` with
   `.replace('\n', " ")`, so a table becomes one unreadable line in the generated Rust doc comment **and** in the
   operator boot report. The intent — the term is defined at its declaration site, not only in a dispatch card — is
   met by a YAML comment block above the key, which is the pattern that file already uses for every substantive note.

**Two corrections to lens arithmetic, neither a rejection.** `dba` counted five correlated target-list subqueries in
`View_PendingRefunds`; there are six. The conclusion (unindexable `restaurant_id`, qual evaluated first, binding makes
the restaurant caller cheaper) is unaffected. And `holub` said the last five dispatch cards are *all*
authorization/attribution; three of the five are. The conclusion (none is user-facing; nothing has touched
`specs/screens` or `crates/web` since 2026-08-16) is unaffected, and it is the half that carries the argument.

_(Lenses and the executor append below.)_
