# ADR-20260818-094500 — Staff sign-in has a mechanism; refund approval stays with the restaurant; the executor refuses a stale base

**Status**: Accepted · **Date**: 2026-08-18 ·
**Decider**: the **FOUNDER / Tech CEO**, three rulings in one message, the whole roster consulted
before the answer was composed (`Consulted:` block below, ADR-20260812-143619) ·
**Register**: [DECISIONS §39](../proposals/DECISIONS.md) (IDOR-DEADLINE / the write-side gap) ·
**Relates**:
[ADR-20260818-004646](ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md)
(no business identifier in the IdP — this ADR is what that mapping is *for*) ·
[ADR-0041](0041-acting-user-is-envelope-not-payload.md) (the acting user is envelope metadata) ·
[ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) ·
**Issues**: [#639 "STAFF-AUTH: restaurant staff, account managers and riders cannot sign in at
all"](https://github.com/TheCaptainCompany/captain-food/issues/639) ·
[#178 "Enforcement: the write-side authorization
seam"](https://github.com/TheCaptainCompany/captain-food/issues/178) ·
[#618 "pendingRefunds is an unscoped cross-tenant
read"](https://github.com/TheCaptainCompany/captain-food/issues/618) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## Status

Accepted. Ruling C is **landed** (`.claude/agents/executor.md`, this change). Rulings A and B are
recorded here and are **not yet built**; the work they authorise is sequenced in the
"Consequences" section.

## Ruling A — staff sign-in, verbatim

> *"I agree we did not talk about that. For the rider the mobile app will ask the phone number
> handled by Supabase with OVH sms is required because it's their tool for working.*
> *For the restaurant, they have an app but they will not download it yet they will start with the
> web site and register directly by finding their restaurant, we did not talk about the onboarding
> process."*

**Ruled**: the rider signs in by phone number, Supabase-handled, **OVH SMS** as the sender, and this
is required for V0 because the phone is the rider's working tool. The restaurant starts on the
**web**, not the app, and self-registers by **finding their restaurant**.

**Named as open by the founder himself**: the restaurant onboarding process. **Not mentioned, and
therefore not ruled**: account managers.

## Ruling B — refund approval, verbatim

> *"this is an exception where the admin makes an intervention. The approval of the refund must be
> done by the restaurant by default."*

Narrowing `approveRefund` to `[ADMIN]` is **refused**. `roles: [RESTAURANT, ADMIN]`
(`specs/payments/api.yaml:163-172`) stands, and the live `approve_refund` widget stays on the
restaurant back office (`specs/screens/restaurant_backoffice.yaml:70`).

The consequence is not neutral and is the reason this is an ADR rather than a comment: the cheap fix
for the write-side hole is off the table, so the hole **must** be closed by **binding** — requiring
an identity actually bound to the restaurant that owns the order. That moves the write-side
authorization seam from "beside the critical path" to "on it".

## Ruling C — the executor refuses a base it was not given

Founder-approved in conversation, no lens input sought: a `git rev-parse HEAD` precondition in
`.claude/agents/executor.md`. Six consecutive dispatch cards carried a stale base commit, including
the one whose own text warned about that failure — the warning lived with the party that had the
incentive to skip it. It now lives with the executor, as a refusal.

## What the mob caught that changes the work

Each item is one lens's finding, verified against the tree before it was written down.

1. **The restaurant onboarding the founder calls undesigned is designed, and it is
   unauthenticated.** `ClaimRestaurantListing` exists (`specs/network/commands.yaml:323-339`), the
   story exists (`specs/stories.yaml:141-149`), and the mutation is
   `roles: [PUBLIC, RESTAURANT_ACCOUNT]` (`specs/network/api.yaml:239-242`) — an **anonymous**
   caller may issue it, and `RestaurantListingClaimed` **grants a `ScopeMembership` row**
   (`specs/database/tables/projection_tables.yaml:1038`). Restaurant self-registration is therefore
   the write path into the table every RLS predicate resolves against. It is `HOLD: human` on that
   ground alone. Its `accountId` is **nullable and not required**, so a self-service claim with no
   pre-existing account grants membership to nobody.
2. **The model has no word for the person.** `principals.RESTAURANT` is a `RestaurantId`
   (`specs/common/actors.yaml:114`) — an organisation. "Restaurant staff" appears in screen prose
   and in C4, with no entity, no scalar and no id behind it. Issuing a credential against the
   organisation id is a shared login, and it makes *"who approved this refund"* unanswerable at
   exactly the moment ruling B makes it money-bearing.
3. **`ExternalReference` means two things.** It is declared as HubRise's import `ref`
   (`specs/common/scalars.yaml:97-103`) and is also the type of every `authRef`. The mapping table
   this work creates would type its auth-subject column as "HubRise ref" — a kernel-purity
   regression on the highest-fan-out scalar in the tree, cheapest to fix before three more roles'
   mapping facts are minted against the wrong name.
4. **The rider's OTP shares the customer's global kill switch.** `SMS_MAX_SENDS_PER_DAY_GLOBAL` is
   described in its own declaration as *"THE KILL SWITCH AND THE ONLY CEILING ON THE BILL"*,
   platform-wide (`specs/customer/configuration.yaml:91-103`). Once the rider's working tool draws
   on it, a flood against the by-design-anonymous customer endpoint at 19:30 on a Friday grounds the
   fleet — the guard firing correctly and the delivery side going dark. The rider population is
   bounded and known; the customer one is not, and that asymmetry is the fix.
5. **Revocation is the regulated act, and today it cannot be represented.** A rider deactivation is
   a *rupture* with a statement of reasons and a challenge route (ordonnance 2022-492 lineage;
   Platform Work Directive 2024/2831 Arts. 10–11 add a human-review duty) — so it needs a reason, an
   actor, a timestamp and a notified-to-rider artifact. In the model there is no unbinding fact for
   anyone, and a JWT claim is a cached fold we cannot invalidate. Revocation must be an appended
   fact, and the binding must be re-derived per request rather than trusted from the token.
6. **The ownership fact ruling B needs is already in the handler's hands.** `PaymentIntentCreated`
   carries `restaurantId` as required (`specs/common/events.yaml`), `PaymentState.restaurant_id` is
   already folded (`crates/domain/src/payment.rs:47`), and the approve leg already folds that stream
   (`crates/application/src/process_managers/refund.rs:36-42`). The binding comparison is available
   with **no new column and no projection read**. A second reading argues for persisting
   `restaurant_id` on `refund_process_manager` (it has none;
   `specs/database/tables/process_managers.yaml:33-45`) from `RefundOpened`, which carries it as
   required. **Both routes avoid the projection**; the disagreement is which is the smaller change,
   and it is settled by the diff, not by this ADR.
7. **`Identity::Unbound` is the actual mechanism.** It returns the declared role
   (`crates/server/src/auth.rs:251`), so a RESTAURANT token with no binding reaches the handler
   looking like a restaurant — and, worse than the authz hole, the envelope would record
   *"RESTAURANT approved this refund"* for a credential bound to no restaurant. That is a false
   author written into an immutable log. Unbound must be a distinct, denied, separately logged
   outcome on the money path.
8. **A live control already points at a route that does not exist.**
   `specs/screens/captain_frontoffice.yaml:270` ships a CTA to `https://restos.captain.food/onboarding`;
   no screen file declares that route. It is the first thing a Tours restaurateur touches.
9. **The partial grant is not a narrowing on the read path.** Both read repositories select one
   role-independent column list and the decode requires every column, so a per-role grant kills the
   read rather than scoping it. Per-role column sets land in the repositories first.
10. **B cannot land before A.** There is no verified restaurant binding to compare against until the
    mapping exists, and `ReadScope::Restaurant` is currently unreachable because nothing mints the
    claim — which is also what makes fail-closed cost zero today.

## Consequences

- **A and B are one slice, not two programmes.** The user outcome is: *one real Tours restaurateur,
  on their phone browser, finds their own restaurant, proves it, signs in, and sees and can act on
  only their own orders and only their own refunds.* `approveRefund`, `denyRefund` and
  `pendingRefunds` are **three** operations, not the 83 in the §39 scope.
- **Ruling A trips the §39 IDOR deadline.** The first restaurant credential outside the team is one
  of that row's named triggers, and ruling A is the event that issues it. The read hole and the
  write hole must close in the same slice as the credential that makes them reachable.
- **The rider is sequenced after the restaurant door it reuses**, without re-litigating "V0": the
  rider inherits the OTP machinery at near-zero marginal cost once the restaurant door exists, and
  the delivery-channel sequencing puts a partner channel before Captain's own riders. What starts
  **now**, because its clock is external and it costs no code, is the **OVH SMS sender registration
  and credit provisioning** — all three OVH failure modes (unapproved sender, no credits, consumer
  key missing `POST /sms/*/jobs`) fail at send time and pass every gate we own.
- **Account managers stay unmodelled for V0.** The `RESTAURANT_ACCOUNT`/`RESTAURANT` split is a
  story-map persona with no command, event or projection behind the assignment relationship. One
  person bound to one location is expressible with what exists; a delegation model invented before
  there is anyone to delegate to is the supporting subdomain displacing the core.
- **Binding ships three-valued, not boolean.** `OFF / OBSERVE / ENFORCE`, with the mismatch metric
  declared in `specs/observability.yaml` before the enforcing code lands, so flipping the default is
  a reading rather than a guess — which is what gate-then-stabilize asks for. The flag is read per
  request, so rollback at 20:00 on a Friday is a flip and not a redeploy.

## Alternatives considered and rejected

- **Narrow `approveRefund` to `[ADMIN]`.** Refused by the founder in ruling B. It would have closed
  the hole in hours with no dependencies, and it takes the approve button off the restaurant's
  screen — which is the product, not an implementation detail.
- **Clone `verifyPhone` with a wider `roles:` list for staff.** Rejected on the model: that command
  is *register-or-identify* — a first verified phone **creates** the Customer, with a
  client-supplied id used as the mailbox actor address. Cloned for staff, possession of any phone
  that receives an SMS mints a RIDER claim and an unauthenticated caller addresses a staff actor's
  mailbox by an id it chose. Staff sign-in is identify-only against a pre-provisioned roster.
- **Parameterise `stamp_put_body`'s role.** Rejected: it hardcodes `"role": "CUSTOMER"` on purpose,
  so a wrong-role stamp is unspellable rather than validated (#437). One stamper per role, each
  hardcoded, selected at compile time.
- **Authorize the refund from `View_PendingRefunds.restaurant_id`.** Rejected: it makes projector
  lag an authorization oracle on the money path — fail-closed denies a legitimate restaurant with
  the customer on the phone, fail-open is cross-tenant refund, and the queue is longest at peak.

## Consulted

Every lens was invited; the brief said *"nothing in my lens" is a complete answer*. Reversibility
class **HOLD: human** — identity, revocation and liability on A, money movement on B.

- **ux-designer** — the rider OTP journey on a bike, the "find your restaurant" sequence, and the
  dead-control risk of a refusal on a live approve button. Found the shipped CTA to a non-existent
  `/onboarding` route, and that the fail-closed Google verifier means nobody can claim today.
- **legal-specialist** — rider deactivation as a regulated *rupture*; phone as sole factor and the
  Art. 32 proportionality question; Supabase as processor rather than joint controller under the
  ruling; the alphanumeric SMS sender as a regulated identity; what a self-registration must capture
  to bind a legal person; P2B, DSA Art. 30, per-line VAT, and the documented refund-queue exposure
  as an Art. 83(2) aggravating factor. **No clearance given; a counsel packet accompanies it.**
- **business-specialist** — self-serve as a post-touch converter rather than a cold-start channel;
  claim ≠ activation ≠ orderable; the refund **decision** leg has no SLA and no metric while the
  machine leg does; and `ApproveRefund` carries no liability attribution, so a delivery-caused
  refund is silently a restaurant cost.
- **vernon** — the mapping is a co-committed grant, not a projection; keep staff memberships out of
  the `Restaurant` aggregate; `ClaimRestaurantListing` is already a two-aggregate mutation with no
  process manager; the guard belongs in the PM leg against state it owns.
- **young** — the fact is already folded on the approve path, so no projection and no new table;
  `RiderRegistered.phone` is a permanent payload with no invariant reading it; make `authRef` the
  lookup key so the phone never becomes a domain key; binding without its revocation fact is a
  permanent grant.
- **evans** — the model has no word for the person; `ExternalReference` means two things; "approve"
  and "arbitrate" are one act distinguished by envelope metadata, and the escalation the story map
  promises has no clock behind it; the `requires.acting` grammar already exists and has exactly one
  user.
- **dba** — the mapping table is the anchor for *who this connection is* and must never become the
  anchor for *what it may see*; subject must be the IdP's immutable id and never the phone, because
  French mobile numbers are recycled; revoked rows are load-bearing because
  `domain_events.user_id` must resolve as-of; no PII, no `last_seen`; RLS cannot reach the write
  half at all.
- **graphql-architect** — the PUBLIC surface already exists and generalises mechanically but must
  not generalise semantically; ownership needs a declared per-operation key emitted as a guard, not
  a fourth bespoke resolver `if`; the final-vision read fix is an arg-free `myPendingRefunds`
  alongside the admin operation, sequenced add → migrate → deprecate → remove.
- **beck** — the rider revocation test cannot be written because there is no seam, and that is the
  finding; the binding is dropped at five distinct places, so a test that fixes only the last one is
  green over an open hole; the companion `unbound ⇒ denied` test matters more than the cross-tenant
  one; the smallest real step is scoping `pendingRefunds` server-side, testable today with no new
  machinery.
- **holub** — one slice, three operations, one sentence of user outcome; freeze #638 at chunk 1
  rather than starting chunk 2, because it is a second authorization layer under a first that does
  not exist; generalise the `access:` grammar from three worked bindings rather than from zero.
- **farley** — the OTP path is structurally unwalkable on the local stack and has zero coverage
  against a real provider in any gate; the console hand-stamp is a second, ungated credential-issuing
  path; the binding gate must be three-valued with a real OBSERVE mode and read per request.
- **architect** — not separately briefed on this message; the slice it authorises returns through
  the ordinary dispatch route.
- **observability-agent** — not briefed: no runtime to analyse (production is suspended). The
  observability concerns above were raised by the lenses that hold them.

## Follow-ups

Filed rather than folded into this record, so each has an owner: the OVH SMS sender registration
(external clock, starts now), the `ExternalReference` split, the dead `/onboarding` CTA, the refund
decision SLA and its fold, and the `claude-review` CI job failing at init on every pull request.
