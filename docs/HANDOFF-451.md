# Handoff — #451 keystone (cart priced LIVE on read)

Written 2026-08-10 when the session ran out of usage budget mid-Phase-2. Everything below is
pushed to `claude/epic-429-production-test-order-9atwb8` (PR [#460](https://github.com/TheCaptainCompany/captain-food/pull/460), draft). A fresh session should be able to
resume from this file alone.

## Where the work stands

| Step | State |
|---|---|
| Mob briefing (ten lenses) | done |
| Phase 1 — AMBER spec slice | landed, `740ea29` + `8989668` |
| Fold-purity checkpoint (architect) | PASSED, 4/4 judgment calls sanctioned |
| Product-owner correction (3 facts) | verified + ruled; correction commit `e9704a0` |
| Phase 2 — GREEN code | **IN FLIGHT, UNVERIFIED** — `57b7330` |
| Independent full-diff review | not started |
| ready + auto-merge, supervise to MERGED | not started |

## The three product-owner facts (2026-08-10, in-session, verbatim intent)

1. **Carts are built before identification** — keyed by a session id (cookie on web, stored in the
   native app). On identification a process manager associates the cart to the customer via the
   shared session id. **Verified in the tree**: `CartStarted` requires `sessionId` with nullable
   `customerId`; `CartBindingProcess` reacts to `CustomerIdentified` and sends `BindCartToCustomer`.
2. **The cart is locked at payment intent**, so it cannot be modified during authorization (the
   order does not exist yet). **NOT modelled in the tree** — `CartStatus` is `OPEN | CHECKED_OUT`
   only, no `CartLocked` anywhere, and line edits are explicitly legal until checkout. Filed as
   [#465 "CartLocked lifecycle: lock the cart at payment intent"](https://github.com/TheCaptainCompany/captain-food/issues/465).
3. **The cart is saved at intent** so the order can be created once payment is authorized.
   **Verified**: `PaymentIntentCreated.checkout` (CheckoutSnapshot) freezes the priced cart at
   intent and capture materializes the Order from that snapshot alone.

### What fact 1 caught

Phase 1 broke the **anonymous cart read** twice: it re-gated by-id `cart` to `[CUSTOMER, ADMIN]`
and repointed the storefront SDUI binding to `current` (`[CUSTOMER]`), while the cart and menu
screens are `roles: [PUBLIC, CUSTOMER]`. A guest's `/cart` bound to an unreachable resolver.
`make validate` passed with 0 errors — there is no screen-roles ⊆ resolver-roles rule. Gate hole
filed as [#466 "Validator rule: every screen binding's resolver must be reachable by the screen's roles"](https://github.com/TheCaptainCompany/captain-food/issues/466).
The correction commit `e9704a0` fixed the spec side.

## The ruling Phase 2 implements

- `cart.current` is **two-leg**: verified CUSTOMER claim → most-recently-updated OPEN cart by
  `customer_id`; else a valid `X-SESSION-ID` → most-recently-updated OPEN cart by `session_id`
  **where `customer_id IS NULL OR = claim`** (covers anonymous visitors and the window before the
  binding PM lands; the filter stops anyone reading a cart already bound to someone else).
  `X-SESSION-ID` is an unauthenticated correlator — scoping only, never identity.
- **OPEN only**, and **LIVE reprice for everything `current` can return** (LOCKED does not exist
  yet; when #465 lands, a locked cart renders the intended amount from the snapshot, never a
  fresh reprice).
- The by-id `cart` resolver body must enforce **claim ownership** — the spec already promises it,
  so the PR is dishonest until the body does it. This is the checkpoint's **hard DONE-WHEN**,
  verified before ready-for-review.

## What is in `57b7330` (WIP, no gates run)

New: `crates/server/src/graphql/cart_read.rs` (two-leg seam + `readable_by` + unit tests),
`crates/server/tests/graphql_cart_read.rs`. Modified: memoized catalog read in
`crates/application/src/pricing.rs` (N+1 → 1), money-free lines fold in
`crates/application/src/projectors/cart.rs`, `cart.price` span + `cart_price_ms` +
`cart_price_unresolvable_total{reason}` in `crates/telemetry/*`, Cart mapping literal in
`tools/codegen-rs/src/emit/server_graphql.rs`.

**Nothing on this tree has been compiled, tested or validated.** Re-run gates before trusting it.

## Remaining Phase-2 work

1. Wire the seam into the generated `current` / `cart` resolvers (the `current` resolver is still
   the generated `not implemented` stub; the by-id narrowing DONE-WHEN is not wired).
2. The unresolvable spy test (own binary, `OnceLock` isolation): seeded cart line whose offer is
   gone → the query errors and `cart_price_unresolvable_total{reason="offer_gone"} == 1`, seen red
   then green.
3. Anonymous-leg ownership tests: session A never sees session B's cart; a bound cart is invisible
   to a different customer's session leg.
4. Smoke: `tools/smoke/prod-smoke.sh` L4 cart assertion gains `totalAmount.amountCents > 0` for the
   seeded fixture. Note for the PR body: after this change the smoke would FAIL against a
   pre-#451 server, so deploy ordering matters.
5. `docs/STATUS.md` entry; then independent full-diff review (third look), then ready +
   auto-merge together, supervised to MERGED.

## Gate traps that already cost this session time

- `make rust` in the **foreground** — a background gate run's completion notification was lost once.
- Disk hits 100% linking the gateway test binaries; clear `target/debug/incremental` **first** and
  run targeted suites rather than `cargo test --workspace` (`df` lies about the allowance).
- `check-drift` is `git diff --quiet` over the whole tree — commit in-flight edits before running it
  or it goes spuriously red.
- Validator baseline: re-measure on a pristine `main` worktree. Phase 1 measured **37 warnings**,
  kind-for-kind identical to its base; the rule is 0 errors and no NEW warning.

## Other open state (not #451)

- [#448 "bake the Stripe test publishable key"](https://github.com/TheCaptainCompany/captain-food/issues/448)
  — **DONE**, spec-only on `main` (`836f885`); the product owner supplied the authoritative value.
  One product-owner action remains from it: delete the now-redundant `STRIPE_PUBLISHABLE_KEY` env
  var from the Render dashboard after the next deploy (env shadows the baked value; the sync never
  deletes).
- Leaked infra secrets: the product owner said rotation was "done for the keys" — treated as handled
  unless contradicted.
- [#463](https://github.com/TheCaptainCompany/captain-food/issues/463) impure-fold survivors outside
  cart (OrderTracking `uber_*`, Catalog `uberPrice`), [#465](https://github.com/TheCaptainCompany/captain-food/issues/465),
  [#466](https://github.com/TheCaptainCompany/captain-food/issues/466) — all filed, none claimed.
