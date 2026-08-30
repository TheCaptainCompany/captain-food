# ADR-20260830-213135 — The STAFF-AUTH answers: Captain binds the first person, the owner invites the rest, and a rider login ships with the fact that ends it

**Status**: Accepted · **Date**: 2026-08-30 ·
**Decider**: the **FOUNDER / Tech CEO**, answering the four-item decision queue put to him after parts
A and B landed. The whole roster was consulted before this record was composed
(`Consulted:` below, ADR-20260812-143619) ·
**Closes**: [DECISIONS §46 **STAFF-AUTH**](../proposals/DECISIONS.md) — 🟠 OPEN since 2026-08-18,
founder-owned ·
**Amends**:
[ADR-20260818-094500](ADR-20260818-094500-staff-auth-mechanism-and-refund-approval-stays-with-the-restaurant.md)
(ruling A recorded the restaurant's SURFACE and named onboarding open; this answers it) ·
[ADR-20260818-101500](ADR-20260818-101500-the-restaurant-signs-in-by-email-link-and-638-freezes-at-chunk-1.md)
(recorded the MECHANISM — email link; this answers the ROSTER) ·
**Relates**:
[ADR-20260818-004646](ADR-20260818-004646-no-business-identifier-lives-in-the-identity-provider.md) ·
[ADR-20260830-191457](ADR-20260830-191457-a-role-guard-takes-a-witness-and-an-unbound-caller-is-recorded-as-public.md)
(parts A and B, which these answers sequence part C behind) ·
**Issues**:
[#639 "STAFF-AUTH"](https://github.com/TheCaptainCompany/captain-food/issues/639) ·
[#792](https://github.com/TheCaptainCompany/captain-food/issues/792) ·
[#800](https://github.com/TheCaptainCompany/captain-food/issues/800) ·
**Session**: https://claude.ai/code/session_01BXTg9ZhjzYHyRkVq3g9uxJ

## Status

Accepted. **Nothing here is built.** This records four answers and what the roster found they commit
us to; the design is a proposal (#639 already owes one for part C), not this record.

## The answers, verbatim

**Q1 — the roster for part C.** Option chosen: *"Captain provisions the roster by hand during
onboarding"*, with these notes:

> *"I remind you that we will crawl all the restaurant and food truck from France based in the Insee
> api*
> *Restaurant will be able to claim their ownership but following the onboarding process*
> *Restaurant owners can declare their restaurants if they are not already registered by captain*
> *Restaurants owners will need to have colleagues invitations and manage them"*

**Q2 — rider revocation.** *"Revocation ships in the same slice as the rider login."*

**Q3 — the unlinked restaurateur.** *"Tell them in French that the account is not yet linked, with a
named support contact."* **The support contact itself was left blank**, and the field asked for it.

**Q4 — this branch.** *"Open the draft PR now, with the mob evidence in the body."* Done: PR
[#799](https://github.com/TheCaptainCompany/captain-food/pull/799), draft, `HOLD: human`.

## Q1 is a composite, and reading it as the bare option would build the wrong thing

The option's own words are "Captain provisions by hand". The notes add three more doors, and two
lenses reconstructed the same reconciliation independently: **Captain hand-binds the FIRST person at
onboarding; the owner invites everyone after.** Captain hand-provisioning each kitchen hire does not
survive one season of front-of-house turnover, let alone a Friday.

So Q1 is not "the manual option instead of the invitation option". It is: **a hand-bound owner, and
an invitation model behind them** — and `business-specialist` found the reason it is more right than
a staging argument would suggest. `ClaimRestaurantListing` requires a Google Business Profile
ownership proof, and an estimated **15–30 % of French independents cannot reach a verified GBP**
(`UNVERIFIED input` — a platform-side prior, no measurement exists) because an agency or a relative
holds it. The hand-bound route is therefore a **permanent fallback, not a temporary shim**.

**One reading of Q1 must be foreclosed before anyone builds it** (`evans`): *"provisions"* has to mean
**appending a domain fact** — an access grant with an actor and a timestamp — and never *creating a
user in the identity provider's console*. The second reading reproduces exactly the hand-stamped
credential that ADR-20260818-004646 exists to remove, and STAFF-AUTH describes as the present-tense
cost.

## What the four answers commit us to

### The model has to gain a word for the person, and it is a kernel change

`principals.RESTAURANT` is a `RestaurantId` — an organisation. ADR-20260818-094500 finding 2 said the
model has no word for the person; the invitation answer makes that unavoidable, and Q2 makes it
money-bearing, because after ruling B *"who approved this refund"* must be answerable **per natural
person**.

`evans` proposes **`RestaurantMember` / `RestaurantMemberId`** — *a natural person authorised to act
within one `RestaurantAccount`'s scope* — and rules out the near-misses by name: not
`RestaurantAccount` (HubRise's *restaurant*: legal entity and billing), not `Restaurant` (its
*location*), not `UserType.RESTAURANT` (a role, which is a URL path), not the auth subject. The
`ScopeMembership` index already speaks this vocabulary — `member_type` / `member_id` — and the new
term is precisely the `member_id` value that is currently forced to be a `RestaurantId`.

**`principals` lives in `specs/common/`, the shared kernel**, and the change reaches `ScopeType` and
`UserType` too. That is a **register row, not an inline edit**, and it is named here so the proposal
starts from that footing.

### Access must stop being a side effect of listing events

Today `ScopeMembership` grants from `RestaurantRegistered` and `RestaurantListingClaimed`: *"someone
gained rights"* has no act of its own, so listing facts do it as a by-product. The four doors Q1
describes — Captain provisions, an owner proves ownership, an owner declares, a member invites a
colleague — are **one act with an evidence discriminator**, not four commands: a single
`GrantRestaurantAccess` carrying a closed `AccessEvidence`. Three sibling commands would be three
write paths into the table every read-authorization predicate resolves against, each with its own
guard to get wrong.

`vernon` found the live defect underneath this, and it is worse than the modelling point:
`ClaimRestaurantListing`'s `accountId` is **caller-supplied, nullable, never resolved by the handler,
and the mutation is `PUBLIC`-callable** — and the projector turns that field into a RESTAURANT-scope
grant. A caller names the beneficiary of an authorization grant for an aggregate nobody loaded.
Changing `RestaurantListingClaimed`'s shape is a **stored-event-shape migration, `HOLD: human`**; the
direction is not in doubt.

### Where the roster lives

`vernon`: **its own aggregate, one per `(scope, person)` membership**, keyed by a derived id over
`(scopeType, scopeId, authSubject)` — so *"one active membership per restaurant per person"* needs no
check at all, because a second invite lands on the same stream and the fold rejects it. Explicitly
**not** on `Restaurant`: it would put "invite a colleague" on the same lane as "change opening hours"
(head-of-line at peak, for no invariant's benefit), take version conflicts from a SIRENE sync, and
bury a natural person's data in a stream that cannot be erased per person.

**Invitation is an aggregate lifecycle, not a process manager** — one aggregate, one linear
lifecycle, nothing to compensate across a boundary. `DeliveryPartnerRegistration` is literally that
shape already and cost almost nothing. The email token and its clock stay the identity provider's
(the `RequestEmailVerification` precedent emits nothing); the business "this invitation went stale"
clock is `reminders:` + `schedules:` on the membership aggregate, inheriting the three prices
`OrderAcceptanceTimedOut` already paid: expiry is a **recorded fact** and never an engine timer,
`reschedule: keep` so a resend cannot silently extend the window, and the schedule co-commits with
the fact.

**And Q1 removes the need for a coordinator in V0**: if Captain provisions by hand, *the human is the
process manager* and the system owes only that each step is an independently issued, idempotent
command against one aggregate. Build the commands; do not build a saga for a two-step human process.

### Q2 turns revocation from a deferral into a build, and the highest-value decision in it is a vocabulary

`RiderStatusChanged` carries `{ riderId, status }` and nothing else, so **a platform sanction and a
rider going offline are the same event** in an immutable log. `legal-specialist` maps eleven required
artifacts; the ones that are blockers are the reason, `decidedAt` ≠ `effectiveAt`, the notice with
proof of delivery, a challenge route, human review by someone who is not the decider, and the
revocation actually unbinding — today `ScopeMembership`'s only revoke rule is delivery-related, so a
suspended rider keeps the order scope, including the customer's name, address and phone.

**The single highest-value thing the slice can do** is make the reason a **closed, declared
vocabulary — a `$ref`'d scalar, never free text.** French case law treats the power to sanction as a
criterion of *lien de subordination*, and a suspension keyed on declining jobs or an acceptance rate
is the strongest requalification evidence obtainable — while `DeliveryDeclinedByRider` already exists
as a stored event with a reason field. A free-text field lets an ops person type *"suspendu pour avoir
refusé trois courses"* into a log we cannot rewrite. A closed enum makes that sentence unspellable,
and the enum's contents become the one page counsel reviews instead of a code audit.

Two design consequences, neither needing counsel: **split the fact** (availability is the rider's,
restriction is the platform's, different payloads and different authoring actors), and **do not reuse
`RiderStatus::SUSPENDED`** — the branch that just landed already forbids leaning on that column in
the auth path, in writing.

**And revocation is theatre until the binding is re-derived per request.** A JWT claim is a cached
fold we cannot invalidate; an outstanding token keeps working after the fact is appended. So this
slice depends on IDENT-1 phase A being in force for the role, and **the token TTL — how long an
issued session survives a revocation — is a founder-visible number**, not an implementation detail.
It is named in the residue below rather than chosen here.

The rule that keeps the read model from becoming an oracle, in one line (`vernon`): **no irreversible
act is authorized by the read index; the write path re-derives from the stream.**

### Q3 needs one value that does not exist, and the screen is not where the cost is

`ux-designer` drafted the French copy and made the structural call: this is **its own screen**, not a
banner on the order queue — because a banner leaves the nav in place and the next tap serves the same
English refusal on eight other screens. Six behavioural differences separate it from the empty state,
and the load-bearing one is that **if the Entrantes / En préparation / Prêtes tabs are on screen, the
system is claiming orders could arrive**.

The blank is handled by making it unspellable: `SUPPORT_CONTACT` as a **required configuration key
with no default**, so the surface cannot boot without a value.

But `legal-specialist` found that printing that string is not a copy decision. It becomes a mentions
légales / pre-contractual identification element, it must actually answer, a phone number may not be
surcharged, and — the one that matters most — **it becomes our GDPR request intake by default**: an
*"enlevez ma fiche"* email arriving there is a valid Art. 21 objection with a one-month clock. It is
also **not** the médiateur de la consommation, which is a separate, paid, pre-launch obligation that
naming a support route makes people believe is handled. Seven confirmations are owed by the founder
before the string is printed; they are listed in the residue.

## Named residue — owed, and NOT decided here

1. **The support contact itself** (Q3's blank). Seven questions, all facts only the founder has: a
   functional address on a domain we control rather than a personal one; who reads it, including
   Friday 19:00–21:30, since that is when the screen is hit; whether it routes GDPR requests with the
   one-month clock actually tracked; phone as well as email, and whether it is his mobile; whether
   the published business address is one he wants public; whether a médiateur de la consommation has
   been engaged separately; and that naming a French route commits to answering in French.
   Recommendation: **name a channel, not a delay.**
2. **The crawl's PUBLISH scope.** Q1 says *"all the restaurant and food truck from France"*; the cron
   is scoped to Touraine today. Crawling wide is nearly free; **publishing** wide is not — no density
   value outside Tours, and an opt-out and support surface sized to roughly 200k rows. Both
   `business-specialist` and `legal-specialist` reach the same recommendation from different sides:
   **crawl wide, publish Tours only.** It is a founder decision either way and owes a register row.
3. **What must exist before the first crawled listing is shown publicly.** `legal-specialist` grades
   four as illegal-to-launch-without: an Art. 14 notice reachable from every seeded listing; an
   objection channel that is not GBP-gated, keyed on SIREN/SIRET so a re-import cannot resurrect a
   removal; INSEE diffusion-status filtering **at import, not at display**; and the suppression list
   honoured by both the ACL and the prospection fold. Plus a written balancing test, a DPIA covering
   prospects, and an Art. 30 record. **No lens output is legal advice or clearance.**
4. **The token TTL** after a revocation (above).
5. **The kernel change** to `principals` / `ScopeType` / `UserType` — a register row.
6. **Whether a revoked colleague must be notified** — `ux-designer` flagged it as outside their lens
   and it is an employment question between the restaurant and its staff, not a platform-work one.
   Counsel.

## Sequencing, and one thing that is broken today

Part C is a **proposal first**, per #639 and CLAUDE.md proportionality: a real option space, screen
mockups per use case, per-flow sequence diagrams, per-option pros and cons, and a tracking issue.
The lenses have supplied most of its raw material.

Two facts the proposal must start from rather than discover:

- **The claim journey the story map promises cannot be completed by anyone alive today**
  (`ux-designer`): the Google ownership verifier is fail-closed and returns "not verified" for every
  proof. Q1 is what makes that survivable — the binding evidence becomes Captain's human check, and
  the claim becomes an onboarding **request** rather than an authorization act.
- **The crawl is switched off**, behind a blocker that closed the same day it was recorded, over a
  month ago ([#800](https://github.com/TheCaptainCompany/captain-food/issues/800)). The claim-your-
  listing journey Q1 chose needs listings to claim. Two guards must land before it restarts: an
  owner-declared restaurant carries no SIRET — the crawl's idempotency key — so un-pausing first
  manufactures a second row for the same restaurant; and a self-declared listing currently enters the
  prospect funnel and is eligible to be cold-emailed.

## Consulted (ADR-20260812-143619)

Reversibility class **HOLD: human** — identity, a regulated act, a legal surface, and Tours-facing.
Five lenses were briefed on the founder's message before this record was composed; each verified its
claims against the tree.

- **evans** — the two collapsed concepts and the evidence discriminator that keeps them one act;
  `RestaurantMember` with its four named non-synonyms; that "onboarding" has a word for every step
  and none for its state, so the concept is currently spelled as a pattern of nulls across two
  entities; that `NON_PARTNER` conflates provenance with funnel position, which is why a
  self-declared restaurant becomes a prospect; that `ExternalReference` is about to mean a third
  thing; and the Q1 trap — *"provisions"* must be a domain fact, not a console user.
- **vernon** — the membership aggregate and its derived id; why it must not sit on `Restaurant`
  (head-of-line, fold size, erasure); invitation as a lifecycle rather than a process manager, with
  the two precedents and the three prices already paid; that Q1's human coordinator removes the need
  for a saga in V0; the `claimRestaurantListing` caller-named-beneficiary defect; the three-part
  revocation mechanism and the one-line rule that keeps the read index from becoming an oracle; and
  that the identity port should return the id newtype, not the row, so the oracle is not one field
  access away.
- **legal-specialist** — the eleven revocation artifacts with their instruments; the closed-ground
  vocabulary as the highest-value decision, and why acceptance-rate grounds are the dangerous ones;
  the six obligations that precede the first public seeded listing; what a self-declaration must
  capture to bind a legal person and to make a compliant receipt possible at all; that the delegation
  must never assert employment (*"personne habilitée"*, not "employee"); and that the support string
  is a statutory commitment which is **not** the médiateur. **No clearance given; a counsel packet
  accompanies it.**
- **ux-designer** — one door, not two, because a restaurateur cannot answer "did Captain crawl you?";
  the nine-row onboarding checklist as the process rather than documentation of it, with a control
  shown only on rows that are theirs; the roster screen with three states and two roles and the
  subtraction that keeps it readable; the French copy and `SUPPORT_CONTACT` as a required key with no
  default; the refusal as its own screen with six behavioural differences from the empty state; and
  that the dead `/onboarding` CTA becomes real under Q1 — but must not ship as a search whose button
  does nothing, which is the same defect one layer down.
- **business-specialist** — that provisioning is ~4 % of onboarding cost, so the owner's credential
  should stay hand-issued and it is **staff** that breaks; that the trigger is latency, not a
  restaurant count, and the real failure is a shared password nobody reports; the GBP finding that
  makes the hand-bound route permanent; that the worst funnel leak is signed-but-never-live and the
  existing metric is structurally blind to it; crawl wide / publish Tours; and what a support route
  costs at 10 and at 50 restaurants.
- **architect**, **young**, **beck**, **dba**, **farley**, **holub**, **graphql-architect**,
  **observability-agent** — not briefed on this message. It takes no engineering decision: it records
  four founder answers and the shape of the work they imply. They are the roster for the part C
  proposal, where the write surface, the fold semantics, the tests and the gates are actually chosen.
