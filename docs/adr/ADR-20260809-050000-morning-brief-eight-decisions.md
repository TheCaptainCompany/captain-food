# ADR-20260809-050000 — The 2026-08-09 morning brief: eight decisions

- **Status**: Accepted
- **Date**: 2026-08-09
- **Decided by**: product owner, answer sheet in-session
- **Supersedes/updates**: closes §23 (D1–D7) and §24 (D1, D3, D4) of
  [DECISIONS.md](../proposals/DECISIONS.md); defers
  [PROP-20260809-021351](../proposals/PROP-20260809-021351-public-demo-one-continuous-walk.md);
  approves [PROP-20260809-003000](../proposals/PROP-20260809-003000-process-manager-step-dsl-conditional-branching.md)

## Context

Eight decisions were put to the product owner as a morning brief after an overnight run in which
ten agent lenses reviewed the open queue. Six of those lenses had been invited late onto an
already-committed proposal and did not refine its design — they contested its place in the queue.
The brief carried each option with the arguing lens named, so the answers below arbitrate a real
option space and belong in the record rather than in a commit message.

## Decisions

### 1. DEMO-NEXT — the demo is deferred; the production-critical work is re-filed on its own

Chosen: **defer the demo epic, re-file its production-critical remainder separately.**

> *"The production is the work we will have on production test customers making test orders on test
> restaurants with test payment on stripe."*

This note does more than pick an option: it **defines the next target**. The goal is not a demo and
not a staging rehearsal — it is the real production deployment, exercised end to end by test
customers ordering from test restaurants and paying through Stripe test mode. Testing production on
production, with test data.

Two consequences follow immediately. Roughly 80% of the deferred epic was never demo work — it was
production correctness wearing a marketing label, and it now stands on its own where it can be
prioritised honestly — re-filed as [#429 "Production with test data: a test customer places a real order against a test restaurant, paid with Stripe test mode"](https://github.com/TheCaptainCompany/captain-food/issues/429). And the target's own precondition is that a real order can be placed at all,
which today it cannot.

### 2. UBER-COMP — the comparison stays named, and the substantiation is funded

Chosen: **keep the named-competitor comparison, and fund the work that makes it verifiable.**

> *"And we also show the restaurant numbers full transparency."*

The legal lens called the nominative comparison the largest exposure in the epic: French and EU
comparative-advertising law requires that compared features be **verifiable**, and today the figure
is a coefficient we chose (1.30–1.45) multiplied by our own price. Funding the substantiation is
what converts that from an exposure into a claim we can defend.

The note widens the scope: the restaurant's own numbers are published alongside. That is the
transparency posture already recorded (radical transparency,
[ADR-20260808-195315](ADR-20260808-195315-customer-brief-answers.md)) applied to the comparison
itself — we are not asserting a competitor is expensive, we are showing both sides of our own
margin. **The comparison must still be computed before it can be published**: the cart projector
carries `uber_comparison` forward from a row no event ever populates, so it is always `None` and the
cart total is `0`. Substantiation and computation are one work item, not two.

### 3. DEMO-D1 — nothing hosted yet; the environment is production with test data in it

Chosen: **(c) nothing hosted yet** — no demo namespace, no resumed Render deployment.

> *"Same production environment with test data in it for testing production on production with test
> data."*

This closes the D1⊕D2 contradiction the architect lens raised: two namespaces over one database is
not a supported configuration of this codebase (the projector takes no lock and overwrites its
checkpoint unconditionally, and at least one projector is a true accumulator — a re-fold doubles a
customer's credit balance silently and permanently). One environment removes the contradiction
rather than resolving it.

It also spends no customer console time, which the D1 alternatives priced at 75–100 minutes across
at least two sittings.

### 4. DEMO-D3 — a pre-identified demo session, no SMS

Chosen: **(a) pre-identified demo session.** No real SMS OTP: it costs money per send, exposes an
unauthenticated SMS-send surface on a public page, and dead-ends at 503 when the hook is
unconfigured.

**This remains blocked by a live defect, not by a decision.** `orders` / `order` / `carts` apply no
ownership filter for any role — `orders` with no arguments returns the entire tracking table,
un-paginated, while the SDL describes it as *"ownership/scope enforced server-side"*. A mintable
session would read every real order. Recorded with evidence on
[#144 "Read-side per-instance authorization"](https://github.com/TheCaptainCompany/captain-food/issues/144);
its fix is ~80% written in a draft PR parked since 26 July.

### 5. DEMO-D4 — one deployment, Stripe keys chosen per order mode

Chosen: **(b) one deployment, key selected by mode.**

> *"The system will use the right keys based on the context test customer test restaurant test
> order."*

Combined with decision 1, this is safe **now and for a specific reason**: in the target being built,
every customer, restaurant and order is a test one, so the deployment holds test keys only. There is
no live key for a misclassification to reach.

**The hazard arrives the day a live key is added**, and it should be met before then rather than
discovered: a runtime branch that picks between live and test Stripe credentials by inspecting data
is a classification bug away from charging a real card in test mode, or failing a real payment in
live mode. Per the compiler-first directive
([ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)), the mode
should be carried by a type that makes the wrong pairing unspellable — a credential handle obtained
only from a mode witness — not by an `if` on a row. Recording this as the named condition on
decision 5 rather than as an objection to it: **the decision stands; the type-level form is due
before the first live key exists.**

### 6. COPY — the neutral checkout-failure wording is approved

Chosen: **(a) approve the neutral wording.**

> *"We must be the more precise has possible."*

Read as a standing principle for customer-facing copy, not a change to this string: prefer the
precise statement over the reassuring one. The approved wording tells the customer what happened to
their money and what is intact, without euphemism.

### 7. CARD-11 — the login-to-domain bridge lives in JWT claims

Chosen: **JWT claims.**

> *"Every rider must onboard with their own account. Same for the restaurant staff. Everyone has
> their own account."*

The note is the larger half of this decision. It rules out shared restaurant logins and shared rider
devices as a supported shape: identity is per-person, always, and the domain binding travels in the
token's claims rather than being looked up per request. This unblocks
[#415](https://github.com/TheCaptainCompany/captain-food/issues/415), which was gated on this answer
plus the legal review.

Per-person accounts for restaurant staff and riders is a real scope statement — onboarding,
credential recovery and offboarding now exist for every individual, not per venue. It is also the
posture the platform-work regulatory exposure wants: a rider with their own account is a rider whose
own record exists.

### 8. DSL-SET — all seven step-DSL branching decisions confirmed as recommended

Chosen: **confirm D1–D7 as recommended.**
[PROP-20260809-003000](../proposals/PROP-20260809-003000-process-manager-step-dsl-conditional-branching.md)
moves to `Approved`; §23 of the register closes.

## Consequences

- The register drops from 21 open decisions to the 8 in §22.
- The demo proposal is **Deferred**, not rejected — the design stands and returns when the target is
  reached; its production-critical concerns are re-filed out from under it.
- The next work is production readiness for a real order placed by a test customer against a test
  restaurant with a Stripe test payment. What blocks that is known and recorded: no publishable key
  anywhere, no route params carrying a cart or order id, no customer bearer token in the web client,
  no ownership filter on order reads, a cart total that never computes, and no notification when a
  paid order arrives.
- Two decisions carry named follow-on conditions rather than being closed outright: the Stripe mode
  selection is due a type-level form before a live key exists (decision 5), and the comparison must
  be computed before it can be published (decision 2).
