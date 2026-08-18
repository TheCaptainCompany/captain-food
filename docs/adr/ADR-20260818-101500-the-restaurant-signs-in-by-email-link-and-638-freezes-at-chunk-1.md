# ADR-20260818-101500 — The restaurant signs in by email link, and #638 freezes at chunk 1

**Status**: Accepted · **Date**: 2026-08-18 ·
**Decider**: the **FOUNDER / Tech CEO**, answering the two-item decision queue put to him after the
mob pass recorded in
[ADR-20260818-094500](ADR-20260818-094500-staff-auth-mechanism-and-refund-approval-stays-with-the-restaurant.md) ·
**Amends**: that ADR, which recorded the restaurant's sign-in **surface** (web) as ruled and its
**mechanism** as open ·
**Relates**:
[ADR-20260818-004646](ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md)
(the mapping lives in our Postgres) ·
[ADR-20260810-215503](ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md)
(prioritisation is the team's; decision 2 is the founder exercising the override he reserved) ·
[ADR-20260808-235113](ADR-20260808-235113-final-vision-first-no-intermediate-steps.md) ·
**Issues**:
[#639 "STAFF-AUTH: restaurant staff, account managers and riders cannot sign in at all"](https://github.com/TheCaptainCompany/captain-food/issues/639) ·
[#638 "Database-level security: the RLS authorization matrix"](https://github.com/TheCaptainCompany/captain-food/issues/638) ·
[#178 "Enforcement: the write-side authorization seam"](https://github.com/TheCaptainCompany/captain-food/issues/178) ·
[#618 "pendingRefunds is an unscoped cross-tenant read"](https://github.com/TheCaptainCompany/captain-food/issues/618) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## Status

Accepted. Both answers were a single word — *"Agreed"* to each of the two items as put, with the
recommendation and its reasoning stated in the question. Neither is built.

## Decision 1 — the restaurant's session is an email link, not a phone OTP

ADR-20260818-094500 recorded the founder's ruling on the **surface** (*"they will start with the
website"*) and recorded the **mechanism** as unruled. Two options were put, with a recommendation:

| | Phone OTP | **Email link** (chosen) |
|---|---|---|
| Machinery | Reuses `requestPhoneVerification` / `verifyPhone`, already running | New, but the same inbound-hook shape |
| Fit to the ruled surface | A phone step in a web flow the owner is doing at a laptop or a counter | Web-native; the link opens where the work is |
| SMS budget | Restaurant traffic draws on the same platform-wide ceiling as the rider | Rider budget stays uncontaminated |
| Cost of the wrong choice | A restaurant sign-in surge and a rider sign-in are the same bucket | — |

**The deciding argument is the fourth row.** `SMS_MAX_SENDS_PER_DAY_GLOBAL` is described in its own
declaration as *"THE KILL SWITCH AND THE ONLY CEILING ON THE BILL"*, platform-wide
(`specs/customer/configuration.yaml:91-103`). Ruling A of the previous ADR already puts the rider's
working tool on that bucket; putting the restaurant on it too means a restaurant-side surge and a
rider lockout share one number. The rider population is bounded and known and the restaurant
population is not, so keeping them apart is worth more than reusing a running code path.

**What this does not decide.** It does not license cloning `verifyPhone` with a wider `roles:` list
for the email variant: that command is *register-or-identify* — a first verified phone **creates**
the Customer, with a client-supplied id used as the mailbox actor address. Staff sign-in is
**identify-only against a pre-provisioned roster**, whatever the factor. Nor does it parameterise
`stamp_put_body`, which hardcodes `"role": "CUSTOMER"` on purpose so a wrong-role stamp is
unspellable rather than validated (#437) — one stamper per role, each hardcoded, selected at compile
time.

**Still unruled, and deliberately so**: the rider's mechanism is settled (phone, OVH SMS,
ADR-20260818-094500) and account managers remain unmentioned and therefore unmodelled for V0.

## Decision 2 — #638 freezes at chunk 1; chunk 2 does not start

Chunk 1 is merged (PR #644). **No chunk 2 is dispatched** until the authorization layer beneath it
exists.

The reason is ordering, not correctness. The founder's own rationale for building row security early
— *"this will help us to avoid AI errors and unauthorised access"* — is the strongest structural
argument in the repository and is untouched by this decision: row security holds when a resolver
written by an agent forgets a filter, which is the failure application-layer review is worst at
catching, because the code looks correct. What the freeze says is that a **second** authorization
layer under a **first** that does not exist defends nothing: today every restaurant caller is
`Identity::Unbound`, so `ReadScope::Restaurant` is unreachable and there is no bound identity for a
policy to resolve against. Chunk 1 also guards one table that is not the money path, on a database
that does not exist yet.

And row security structurally **cannot** close the refund hole: `approveRefund` is a participant
check against folded state, not a row predicate on a member column — the UNBINDABLE class in
[DECISIONS §39](../proposals/DECISIONS.md).

**This is the founder exercising the override he reserved in ADR-20260810-215503**, not a team
re-ranking: the recommendation came from the team, the call is his, and it is recorded here so a
concurrent session cannot read the frozen chunk as available work.

## What starts instead — the slice

One sentence, and every clause of it is the deliverable:

> **One real Tours restaurateur finds their own restaurant on their phone browser, proves it, signs
> in, and can see and act on only their own orders and only their own refunds.**

Three operations — `approveRefund`, `denyRefund`, `pendingRefunds` — not the 83 in the §39 scope.
The slice discharges ruling A of ADR-20260818-094500, ruling B of the same, the V0 sequencing of
ADR-20260818-004646, and trigger (i) of the §39 IDOR deadline, which ruling A is itself the event
that trips.

Constraints the mob banked at the briefing and that the dispatch card must carry:

- The binding ships **three-valued** — `OFF / OBSERVE / ENFORCE` — with the mismatch metric declared
  in `specs/observability.yaml` **before** the enforcing code lands, and the flag read per request so
  rollback is a flip and not a redeploy (gate-then-stabilize).
- `Identity::Unbound` **denies** on the money path and never stamps a role into
  `domain_events.user_id` / `user_type`. A false author in the immutable log is worse than the authz
  hole it hides.
- The ownership comparison uses the fact **already folded on the approve path**
  (`PaymentState.restaurant_id`, `crates/domain/src/payment.rs:47`) — never a `View_*` read, which
  would make projector lag an authorization oracle on the money path at exactly the hour the queue is
  longest.
- The companion test matters more than the obvious one: `unbound ⇒ denied`, not only
  `other-restaurant ⇒ denied`. Without it, `domain_id: None` gets coded as "unknown ⇒ allow", the
  cross-tenant test passes, and the hole is untouched.
- `claimRestaurantListing`'s `PUBLIC` role is resolved explicitly — kept with a written justification
  (the ownership proof *is* the authentication) or removed. Never left to be discovered later.
- The card names the restaurateur who signs in at the end of it. A card describing authorization
  mechanism without that sentence is stopped at the checkpoint.

## Consulted

No fresh consultation: both answers resolve a decision queue that the **whole roster** had already
been briefed on the same morning, and each option carried the lens findings that produced it. The
`Consulted:` block of
[ADR-20260818-094500](ADR-20260818-094500-staff-auth-mechanism-and-refund-approval-stays-with-the-restaurant.md)
is the record — eleven lenses, none of which returned "nothing in my lens". The two decisions here
trace to specific lenses in that block:

- **Decision 1** rests on the SMS-ceiling asymmetry raised independently by **farley**, **beck** and
  **ux-designer**, and on **graphql-architect**'s finding that the customer command's
  register-or-identify semantics must not be generalised to staff.
- **Decision 2** is **holub**'s recommendation as put, with **dba**'s point that row security cannot
  reach the write half at all, and **beck**'s that `ReadScope::Restaurant` is currently unreachable.
