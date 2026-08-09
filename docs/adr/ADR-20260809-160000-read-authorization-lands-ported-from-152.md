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
   log carries only throwaway smoke fixtures** at this change. Post-merge correction (review
   finding on #430): the original wording here said pre-#144 orders stay "Admin-only" — that is
   only the PROJECTION side (the worker log-skips the undeserializable event, so no grants fold).
   The WRITE side has no such tolerance: `event_store.rs::load_inner` hard-errors on a pre-#144
   `OrderPlaced`, so any command against such a stream (accept, refund, cancel, tip) is rejected —
   **pre-#144 `Order-*` streams are FROZEN, unreadable through scoped reads and un-actionable**.
   Acceptable solely for smoke data nobody acts on retroactively; the deploy-day check is
   `SELECT count(*) FROM domain_events WHERE stream_name LIKE 'Order-%' AND occurred_at < '2026-08-09T14:00Z'`
   via `claude_ro` — a non-zero count of NON-TERMINAL such streams would need clearing or a
   deserialization fallback BEFORE this narrowing class is ever repeated. This is a recorded
   exception to additive-only evolution, not a precedent — on a live event store the same change
   would be a landmine, which is exactly why it is named here. The "guest order" class is
   unrepresentable from here on; `OrderDeleted.customerId` stays nullable on the wire for logs
   that predate this.
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

## Addendum (independent review, same day)

The third-look review over the full branch diff returned three findings, resolved before
ready-for-review:

1. **Stale ownership prose swept**: the `orderStatusChanged` api.yaml description still described
   the trusted-RESTAURANT gap this change closes; rewritten (and regenerated) to say what is now
   true — the old-term sweep the operating model demands after a reshape.
2. **`RestaurantListingClaimed` now grants**: the post-registration account-attachment path (a
   Sirene-seeded listing registers with NO accountId and gains one at claim time) is folded —
   without it, the claiming account would never hold RESTAURANT membership and
   `resolve_restaurant_account` would find nothing for every subsequent order, a deny-safe coverage
   hole that would have needed a checkpoint-reset replay to repair once discovered.
   `RestaurantRemoved`/`RestaurantAccountDeleted` deliberately do NOT revoke in this change: the
   product decision is "rider revoked / others permanent", a removed restaurant's principals cannot
   mint tokens anyway, and designing restaurant-lifecycle revocation is recorded on
   [#432 "Read-scope remainder"](https://github.com/TheCaptainCompany/captain-food/issues/432)
   rather than improvised here.
3. **A failed REVOKE is named, not generic**: the drain loop's log-and-skip is a liveness choice
   that on THIS table converts a transient failure into a STANDING STALE GRANT (the silent-breach
   mode). Accepted risk, bounded and now visible: the revoke path emits a dedicated, searchable
   error naming the consequence and the repair — delete the `ScopeMembership`
   `projection_checkpoint` row, which replays the whole (idempotent) fold from position 0. A
   dedicated metric, if wanted, rides #432 with the contract change it needs.

Also hardened from review notes: the prod-smoke negative assertion now fails on an ERRORED
response instead of reading empty `.data` as proof (outage-honesty), and the known limits worth
carrying travelled to #432 (bridged-non-member proof exists only in the DB suite; `bridge_unresolved`
conflates DB outage with missing projection row; WS scope frozen at connection init).

**Post-merge findings** (two review comments landed as auto-merge fired; both verified real,
tracked on #432, neither reopens this PR): (a) the write-side loader intolerance above — the §3
framing was corrected in this file; and (b) the smoke `l4_negative` outage-honesty fix is
INCOMPLETE: `gql()` discards the HTTP status, so a transport failure collapses to `{}` which
passes both jq checks (`has("errors")` → false, `.data.order` → null via jq null-chaining). The
assertion must also require `has("data")` (or read `gql_raw`'s status line) — a `tools/**` change,
so it rides the #432 claim flow rather than a docs push.

## Addendum 2 — product-owner correction, same day (#433): the JWT carries ALL the domain ids

*"It's not the principal id we have to use in scope membership but the customer_id, the
restaurant_id, the rider_id"* — *"This information is provided in the jwt."* The ScopeMembership
table already stored the domain ids; what this corrected was the EDGE. Decision 2 above is
**superseded**: CUSTOMER no longer bridges through `by_auth_ref`, and the rider `sub`-parse
placeholder is dead. Realized by
[#433](https://github.com/TheCaptainCompany/captain-food/issues/433) /
[PR #434](https://github.com/TheCaptainCompany/captain-food/pull/434), mobbed (ten lenses):

- `read_scope` is a PURE function of the verified claims (`captain_customer_id` /
  `captain_rider_id` join the two restaurant claims); a missing or malformed claim fails closed to
  Public, and `sub` is never an identity (pinned with distinct-uuid tests, seen RED under a planted
  sub-fallback). `ScopeResolver` is deleted — resolution has no dependency that could be missing,
  and the Friday-peak auth path no longer shares fate with the database (dba lens).
- The four generated resolvers that still AUTHORIZED via `by_auth_ref` — `paymentStatus`,
  `paymentStatusChanged`, `myReclamations`, `customerCredit` (graphql + architect lenses) — read
  the same claim-derived ReadScope, so a customer's order read and their payment stream can never
  disagree on identity (the post-payment split-brain spinner).
- **Precisely scoped deletion claim**: `by_auth_ref` REMAINS the customer identity mechanism at the
  write-side seams (the mailbox `resolve_actor`, the generated mutation edge bridges — envelope
  territory) and `myDeliveries` keeps its own rider `sub`-parse until #415 mints the rider claim —
  both recorded on #432/#415, not silently overclaimed.
- **BLOCKING precondition on the #429 customer-bearer item** (business + ux + farley lenses,
  independently): the product's `verifyPhone` must stamp `captain_customer_id` BEFORE the client's
  token is issued (or force one refresh in the success path) — otherwise the FIRST paid session is
  the one denied its tracking screen. The identity-port seam is not clean today (no admin
  capability, a new server-held secret, an ordering design decision), so minting is that item's
  work, not this one's.
- **Erasure obligation** (legal + dba lenses): Supabase `app_metadata` is now a storage location
  for domain ids, and a claim OUTLIVES erasure until token expiry — where the old lookup
  fail-closed for free. The #194 erasure sequence must scrub `app_metadata` (or delete the auth
  user) AND revoke refresh tokens; recorded on #194. §6.4 claim staleness now covers
  customers/riders (only transition: null → set at mint).
- prod-smoke upgraded to the claims it can mint itself: the L4 order poll is the customer-POSITIVE
  production proof (token-decode asserts the claim per run — an unconditional stamp BEFORE link
  generation, both keys in the PUT, per beck/farley), and the negative probe is a BRIDGED stranger
  (the membership EXISTS path), outage-honest (`has("data")` — a `{}` transport body fails the
  proof).
- Observability contract comments corrected (span semantics unchanged, names kept): `bridge_
  resolved=false` now reads as "missing/stale claim", never "missing projection row" — the
  on-call rabbit hole the old text would have opened.
