# ADR-20260809-160000 — Read-side per-instance authorization lands, ported from PR #152

- **Status**: Accepted
- **Date**: 2026-08-09
- **Realizes**: [PROP-20260725-185140 "Read-side per-instance authorization"](../proposals/PROP-20260725-185140-read-side-per-instance-authorization.md) (flipped to `Approved` in this change)
- **Issue / PR**: [#144 "Read-side per-instance authorization: ReadScope on the read ports + RESTAURANT/RIDER identity bridges"](https://github.com/TheCaptainCompany/captain-food/issues/144) · [PR #430 "feat(#144): read-side per-instance authorization — ScopeMembership (port of #152)"](https://github.com/TheCaptainCompany/captain-food/pull/430), superseding the parked [PR #152](https://github.com/TheCaptainCompany/captain-food/pull/152)
- **Directive**: [#429 "Production with test data: a test customer places a real order against a test restaurant, paid with Stripe test mode"](https://github.com/TheCaptainCompany/captain-food/issues/429) — *"its fix is ~80% written in #152 … This one is rebase-and-land, not build."*

## Context

The product-owner decisions of 2026-07-25 (enforce immediately, no shadow mode · rider revoked /
others permanent · scope types ORDER + RESTAURANT) were taken in-session but the proposal header was
never flipped, and the implementing branch parked on 27 July — ~380 commits behind `main`, on the
wrong side of the spec reorg (ADR-20260807-183024), the per-scope crate split, the TEXT enum-column
migration (ADR-20260728) and the QW1 delivery-payload reshape. A mechanical rebase was not viable;
this change is a semantic port, mobbed per ADR-20260809-013142 (ten lenses briefed before code).

## Decisions recorded by the port

1. **The proposal is `Approved` and realized.** The 2026-07-25 decisions plus the #429 rebase-and-land
   directive are the approval record; the header now says so.
2. **No `Rider` bridge table** (deviation from #152). ADR-20260809-050000 CARD-11 decided the
   login-to-domain bridge lives in **JWT claims** — the Rider read-model bridge was exactly the
   option the register's identity-bridge row said must not be embedded undecided. `ReadScope::Rider`
   resolves from the existing `sub`-as-RiderId placeholder (the generated `myDeliveries` convention);
   [#415](https://github.com/TheCaptainCompany/captain-food/issues/415) replaces it with a minted
   per-person claim. RESTAURANT/RESTAURANT_ACCOUNT resolve from `app_metadata` claims (the claim IS
   the domain id); CUSTOMER keeps the existing `auth_ref` bridge. No new identity mechanism exists.
3. **`OrderPlaced.customerId` (and the PaymentIntentCreated/CheckoutSnapshot/PlaceOrder chain)
   narrows nullable → REQUIRED.** A stored-contract narrowing, legal **solely because the production
   log is empty** at this change (throwaway smoke fixtures only; pre-#144 smoke orders deserialize
   as log-skipped rows and simply hold no grants — Admin-only, by design). This is a recorded
   exception to additive-only evolution, not a precedent. The "guest order" class is unrepresentable
   from here on; `OrderDeleted.customerId` stays nullable on the wire for logs that predate this.
4. **Enforce-immediately stands, with no toggle** — reaffirmed on today's facts: the CURRENT
   behaviour is the breach (`orders` dumps the whole tracking table to any authenticated customer),
   nothing is hosted for real customers (D1: nothing hosted), backfill is free (new checkpoint
   replays an empty-to-tiny log), and `deploy.yml` being manual dispatch is the natural dark stage.
   A shadow switch whose OFF position is a live vulnerability is strictly worse releasability.
5. **Enum columns store TEXT** (the branch predated ADR-20260728): `scope_type`/`principal_type`
   store variant names verbatim; the UUIDv5 `membership_id` hashes those names, pinned by a literal
   test (`membership_id_is_pinned`) because a variant rename would re-key every row.
6. **The revoke events ride the column lineage** (`scope_type`/`scope_id`/`principal_type` `from`
   lists) rather than being fedBy-only: the DELETE predicate reads exactly those columns, the
   lineage is honest, and the warning histogram stays byte-identical to `main` (37, same kinds).
7. **Resolver postures** (mob findings, ux/graphql/architect lenses):
   `myDeliveries` hydrates its order join as `ReadScope::System` — the row-level decision is
   `for_rider` itself, and caller-scoped hydration would silently drop the PENDING offer pool
   (no membership before acceptance): a self-sealing dispatch outage. `delivery(orderId:)` degrades
   an out-of-scope order hydration to `null` instead of a GraphQL error (no existence oracle). The
   `orderStatusChanged` subscription reads through the caller's ReadScope, which also closes its
   recorded "RESTAURANT paths are trusted" gap (ADR-20260720-220000). `carts` forces a CUSTOMER
   caller onto their own customerId (client-supplied argument ignored; SDL updated to say so).
8. **prod-smoke L4 changes in the same PR** (farley lens): `placeOrder` carries a generated
   `customerId`; the captured-order poll runs as ADMIN (the smoke Supabase user has no domain
   Customer — `verifyPhone` needs a real SMS OTP); and a **negative assertion** now proves in
   production that a non-member authenticated customer reads nothing (by-id null, list empty) —
   the only executable proof of the closed vulnerability, since rules.yaml cannot carry read-guard
   coverage (#212).
9. **Erasure**: `scopemembership` is an Order-fed read model holding a customer-to-order link; it
   OWES an `OrderExpired` tombstone fold when the #194 deletion engine lands — named in the table
   spec's rules and the projector doc so the sweep cannot miss an app-projected table the generated
   dispatch skips (legal lens).

## Consequences

- An unscoped order read is no longer spellable (`OrderReadRepository::{list,by_id}` take
  `&ReadScope`); process managers pass `ReadScope::System` explicitly.
- Restaurant back-office order reads stay EMPTY until minted tokens carry
  `captain_restaurant_id` (#415 direction): nobody holds such a token today and the #429 restaurant
  leg runs on ADMIN — recorded there. The degraded-state screen for a claimless token is a named
  GAP on the follow-up issue.
- Projection lag on the ACL index is a user-visible denial; the `read-authorization` contract ships
  with server-boundary instrumentation (spans, denial counters — by-id/subscription paths only, a
  list "denial" is structurally invisible — and the `scope_membership_lag_positions` gauge emitted
  by the worker).
- Remaining tenant read surfaces (orderConversation first, reclamation by-id, delivery/refund/
  satisfaction ports, cart session flows), LIMIT/pagination on `orders`, and the
  ownership-declared-vs-emitted validator rule are ONE follow-up issue, not five.
