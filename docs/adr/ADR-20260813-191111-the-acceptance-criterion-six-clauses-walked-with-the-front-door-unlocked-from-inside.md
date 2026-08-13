# ADR-20260813-191111 — The acceptance criterion for "a working version": six clauses walked on the local stack, with the front door deliberately unlocked from the inside

- **Status**: Accepted (founder directive, 2026-08-13)
- **Date**: 2026-08-13
- **Governed by**: [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)
  (every founder message goes to the whole team before any answer; a record created from a founder
  directive carries a `Consulted:` block)
- **Register row**: [DECISIONS §35 INV-1](../proposals/DECISIONS.md) — the one founder-owed leg of
  the 2026-08-12 answer sheet, now answered
- **Relates**: [ADR-20260812-214021](ADR-20260812-214021-the-founder-answer-sheet-of-2026-08-12.md)
  (INV-1: the inverted critical path, the spend gate that had no exit) ·
  [ADR-20260808-195315](ADR-20260808-195315-customer-brief-answers.md) §1.2 (capture timing D2 —
  the founder's clause order restates his own decision) ·
  [ADR-20260719-014434](20260719-014434-checkout-snapshot-on-paymentintentcreated.md) (the older
  capture-at-checkout recording, superseded in its capture-timing aspect by ADR-20260808-195315) ·
  [ADR-20260813-004634](ADR-20260813-004634-supabase-auth-is-retained-for-v0-and-the-window-closes-at-the-first-real-order.md)
  (Supabase retained; the demo-leg mechanics) ·
  [ADR-20260813-013211](ADR-20260813-013211-a-token-must-prove-the-product-not-only-the-provider.md)
  ([#519 "A token must prove the product, not only the provider"](https://github.com/TheCaptainCompany/captain-food/issues/519) /
  PR [#520 "fix(auth): a token must prove WHO issued it and WHAT it is for — the verifier stops failing open"](https://github.com/TheCaptainCompany/captain-food/pull/520):
  the fail-open verifier shape is deleted and stays deleted) ·
  [ADR-20260813-132540](ADR-20260813-132540-the-weekly-cap-stops-being-a-stop-sign.md) (whose
  exit condition points at this criterion)

## Status

Accepted.

## Context

[DECISIONS §35 INV-1](../proposals/DECISIONS.md) recorded the inverted critical path — *"I'm
waiting for a working version before paying OVH"* — and named its one founder-owed leg: *"a working
version"* carried **no acceptance criterion**, so the spend gate had no exit. The team offered a
two-half proposal (smoke L1→L4 green on local k3s **plus** a recorded browser walk with login) to be
confirmed or replaced.

The founder replaced it. Verbatim (2026-08-13, in session — the founder posts no GitHub comments;
the session transcript is the record):

> **"For the acceptance, i need to have all the dbs, apps deployed locally and working without
> considering the authentication contraints with supabase from the creation of the customer, payment
> authorisation, order creation order accepted delivered payment captured"**

The whole ten-lens mob read the sentence before this record was written (`Consulted:` below).

## Decision

### 1. The criterion — six observable clauses, in his order

Acceptance = **all databases and apps deployed locally and working**, demonstrated by ONE walk whose
six clauses are each asserted through the deployed stack's **own API and read models** (never by
inspecting internals from outside the product):

1. **Customer created** — a genuine `CustomerRegistered` exists for the walk's customer;
2. **Payment authorised** — the Stripe money leg reaches its pre-capture hold (see §4 for what this
   clause forces);
3. **Order created** — the order is queryable through the customer read path;
4. **Order accepted** — the restaurant's acceptance is folded and visible;
5. **Delivered** — the delivery leg completes and the order reaches its delivered state;
6. **Payment captured** — the capture fact is recorded and folded, **after** delivery (§4).

**Plus a browser walk of the two surfaces that ARE the product**: the customer **storefront**
(browse → cart → checkout → tracking) and the **backoffice order queue** (the acceptance action),
both driven in a real browser against the local stack. The **rider leg may be a labeled script** —
a bot rider is acceptable as long as the evidence says so.

**This supersedes the team's two-half proposal recorded in §35 INV-1.** The recorded
browser-walk-**with-login** half **drops from gating to demo artifact**: the login walk is
explicitly OUT of acceptance (*"without considering the authentication contraints with supabase"*),
and the login/auth-walk pair —
[#529](https://github.com/TheCaptainCompany/captain-food/issues/529) and
[#532](https://github.com/TheCaptainCompany/captain-food/issues/532) (titles live on GitHub; the
API was unreachable from this session) — is the **named first lane after acceptance**, with
[#533](https://github.com/TheCaptainCompany/captain-food/issues/533) the first item of the
**first-real-order gate** that follows it.

### 2. Two stated assumptions — correctable by the founder at zero cost

Recorded under his own *"just keep me informed"* (ADR-20260810-221840); either can be reversed by
one sentence before the walk is built.

- **"All the apps" = the monolith `server` bin and its surfaces, until the
  [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) cutover.** CUT-1 = B
  (§35, 2026-08-12) excluded the runtime decomposition into the bin fleet from the cutover; reading
  *"all the apps"* as the 56-bin fleet would be a **decision reversal by inference**, which is
  exactly what a stated assumption exists to avoid.
- **"Creation of the customer" = the real `verifyPhone` path, driven via Supabase's
  test-phone/static-OTP facility.** That creates a **genuine `CustomerRegistered`** through the real
  command path with **zero SMS spend** — exactly what the authentication exclusion licenses: skip
  the *constraint* (a real phone receiving a real SMS), not the *command*. If the facility proves
  unavailable on the project, the fallback is the admin claim-stamped identity, **labeled honestly
  as such in the evidence**.

### 3. The auth-bypass posture — unlocked from the inside, never weakened

*"Without considering the authentication contraints"* means **real tokens through the real,
fail-closed verifier**: admin-minted tokens (the smoke's existing magic-link mint) or, as the
recorded fallback, a **local-JWKS stub issuer** whose keys the verifier is pointed at — an
asymmetric key we hold, verified by the same unmodified code path. It never means a weakened
verifier: the fail-open shape ("no issuer ⇒ skip the check") was **deliberately deleted** by
[#519](https://github.com/TheCaptainCompany/captain-food/issues/519) /
PR [#520](https://github.com/TheCaptainCompany/captain-food/pull/520) and **stays deleted** —
`Verifier { jwks_url, issuer }` cannot spell the bypass, and this acceptance does not reintroduce
it under a demo excuse.

### 4. The D2 record↔code drift — the criterion's biggest finding

The founder's clause order — *"payment authorisation … order accepted delivered payment
captured"* — **restates [ADR-20260808-195315](ADR-20260808-195315-customer-brief-answers.md) §1.2**,
his own decision, Accepted 2026-08-08: *"Authorise on checkout. Capture on delivered / picked up"*
(PROP-165000 D2). That ADR **supersedes the older capture-at-checkout recording in
[ADR-20260719-014434](20260719-014434-checkout-snapshot-on-paymentintentcreated.md)** on the
capture-timing point — stated here because nothing had said so explicitly.

**The implementation does not do this yet.** Verified on `main`:

- `crates/adapters/stripe/src/outbound.rs` posts `/v1/payment_intents` with **no `capture_method`**
  (zero occurrences repo-wide), so Stripe's default — capture at confirmation — applies;
- the order is **materialized on `PaymentCaptured`** (rule
  `specs/common/rules.yaml#/OrderMaterializedOnPaymentCapture`, hooks in
  `crates/application/src/process_managers/place_order.rs`) — i.e. capture happens at checkout,
  before acceptance, the reverse of the decided order;
- **no `AUTHORIZED` state exists anywhere**: `PaymentStatus` is
  `[PENDING, CAPTURED, FAILED, REFUNDED]` (`specs/common/scalars.yaml:307`).

**Implementing D2 therefore joins the acceptance path as its own slice.** It is **GREEN** — it
implements a recorded decision, reverses nothing — and the empty-log window (Q-L3 = no real user,
2026-08-12) makes it cheap **now**: `manual` capture, an authorized state, capture triggered by the
delivered/picked-up fact, release on the acceptance-timeout path (*"no need to refund because no
capture"*, ADR-20260808-195315 §1.3). The walk harness's **capture assertions are written against
D2 semantics** (capture recorded after the delivered clause); any interim walk run on the
implemented capture-at-confirm flow is **labeled as such**, never presented as the criterion met.

### 5. The program of record

Sequenced; each step is its own claim/PR under the ordinary gates. The architect owns re-cutting
this if reality disagrees, but this is the plan this record commits to:

1. **[#536](https://github.com/TheCaptainCompany/captain-food/issues/536)** (in merge at the time
   of writing) — then **split slice 1** (the declaration site).
2. **Smoke L5 — lifecycle legs on the current stack**: accept → ready → dispatch-job-present →
   delivered, each leg **seen red first**, on the DELIVERY service type, with restaurant/rider
   claim stamps. (`tools/smoke/prod-smoke.sh` today ends at L4's capture assertion; nothing green
   yet asserts anything past payment.)
3. **The four non-auth browser walls + local-issuer tooling**: the add-to-cart input mismatch
   (plus the sheets-validator blind spot that let it through), the checkout conditional, the
   backoffice card bindings, the rider button ids — the four defects that stop a browser walk
   before authentication is even reached.
4. **The D2 slice** (§4).
5. **[#514 "per-database migration chains and a `REQUIRED_SCHEMA_VERSION` map"](https://github.com/TheCaptainCompany/captain-food/issues/514)
   + the Database CRs + the local overlay, as ONE slice** — the eleven-database stack the walk
   deploys on. The delivered leg **forces the `View_DeliveryJob` → table conversion inside this
   slice**: a cross-database fold cannot stay a SQL VIEW over a log it can no longer reach.
6. **Acceptance itself**: the walk, in his clause order, storefront + backoffice **in a browser**,
   on the **all-databases stack**, evidenced by a **checkable JSON record** — the causal event
   chain linked by `cause_id`, plus **all eleven migration heads** — with the synthetic identities
   stated (§6) and the **Stripe Connect shape verified in the script** (separate charges &
   transfers, per ADR-20260808-195315 §1.1).

### 6. The honesty sentence

Carried verbatim from the mob's focus lens, because it is the sentence that must survive this
record:

> **"This acceptance proves the order machine end-to-end with the front door deliberately unlocked
> from the inside — no path from a real customer's phone through OTP sign-in to this flow has ever
> been walked, so 'accepted' certifies the machine, never that a customer can use it, and the auth
> walk (#529/#532) is the named remainder between this record and the first real order."**

## Alternatives considered

- **The team's two-half proposal (smoke L1→L4 + a recorded browser walk with login)** — the
  standing offer INV-1 recorded. Replaced by the founder's own criterion; its login half drops to
  demo artifact, its money-path half is subsumed and extended (L4 stops at capture-at-confirm; the
  criterion demands the full lifecycle through delivered-then-captured).
- **Reading "all the apps" as the bin fleet** — rejected as a decision reversal by inference
  (CUT-1 = B excluded the fleet from the cutover); recorded as a stated assumption instead (§2).
- **A weakened/fail-open verifier for the auth bypass** — rejected outright; the shape was
  deliberately deleted by #519/#520 and the bypass is real tokens through the real verifier (§3).

## Consequences

### Positive
- **The spend gate has an exit.** INV-1's criterion is now the founder's own sentence, parsed into
  six clauses anyone can check; "a working version" can no longer be argued both ways on the same
  evidence.
- The criterion **surfaced a real drift** (D2, §4) before the walk was built against the wrong
  semantics — the review paid for itself.
- Login (#529/#532) has a named place: first lane after acceptance, before the first real order,
  with #533 opening the first-real-order gate.

### Negative
- The acceptance deliberately certifies the machine, not the customer's path to it (§6) — a known,
  named gap, not a discovered one.
- The D2 slice adds work to the acceptance path that the team's own proposal would not have
  required. It is the correct cost: walking capture-at-confirm and calling it done would have
  certified a flow the founder already decided against.
- Local remains demo, never evidence (§35 INV-1 unchanged): no recovery claim may cite this walk,
  and [#429 "Production with test data"](https://github.com/TheCaptainCompany/captain-food/issues/429)
  still closes only on the provisioned cluster.

### Follow-up actions
- `docs/STATUS.md`'s budget-override exit condition now cites this ADR (same change).
- The architect sequences §5's program onto the prioritised backlog; steps 2–5 need issues where
  none exist.
- If the Supabase test-phone facility proves unavailable, the §2 fallback label is mandatory in
  the evidence artifact — a silent substitution would misstate what clause 1 proved.

## Consulted (ADR-20260812-143619)

All ten lenses read the founder's sentence before this record was written. Nothing here is legal
advice or clearance, and agreement between lenses does not upgrade a hedged finding to a settled
one.

- **architect** — the clause order is **D2 restated** (ADR-20260808-195315 §1.2), which turns the
  criterion into a drift detector; the team's two-half proposal is superseded, not amended; the
  re-rank follows (the program in §5); and the two §2 assumptions must be *stated*, because both
  are silently decidable the wrong way.
- **beck** — L5 is designed as three-plus seen-red legs (accept → ready → dispatch-job-present →
  delivered), each asserted red before green; the Supabase test-phone/static-OTP facility is the
  right customer-creation option (real command, zero SMS); and **nothing green today can assert
  capture-after-delivery** — the harness must be written against D2 semantics or it certifies the
  wrong machine.
- **dba** — eleven databases are **free locally, the plumbing is not**: the chains
  ([#514](https://github.com/TheCaptainCompany/captain-food/issues/514)), the Database CRs and the
  local overlay belong in ONE slice; and the delivered leg **forces the `View_DeliveryJob` → table
  conversion there**, because a SQL VIEW cannot fold across database boundaries.
- **farley** — confirms the monolith reading of *"all the apps"* (the fleet is OUT of the cutover
  by CUT-1 = B); the browser walk needs the **wasm bundle actually served**, not just the API up;
  and supplied the what-breaks-first list that became §5 step 3's ordering.
- **ux-designer** — five browser walls stand between today and the walk, **four of them non-auth**
  (add-to-cart input mismatch — plus the sheets-validator blind spot that admitted it — the
  checkout conditional, the backoffice card bindings, the rider button ids); the minimum screen
  set is **storefront + backoffice queue**; a labeled bot rider is acceptable, an unlabeled one is
  not.
- **holub** — walk-first is the right shape for the criterion; authored the honesty sentence (§6);
  and named what leaves the lane: anything not on the six-clause path is out of the acceptance
  claim, whatever else it improves.
- **graphql-architect** — the walk is drivable end-to-end via the product's own GraphQL surface,
  with two caveats: customer creation needs the §2 facility (the mutation path, not an insert) and
  the money leg needs Stripe's webhook loop; a **local-JWKS stub issuer is a legitimate bypass**
  because the verifier code path stays unmodified and asymmetric.
- **observability** — the evidence is a **checkable JSON artifact**, not a video: the causal event
  chain by `cause_id`, the eleven migration heads, and every id printed so each claim can be
  **re-derived** against the stack rather than trusted.
- **business-specialist** — capture-at-confirm **forfeits roughly EUR 0.60–0.90 per rejected
  order** (Stripe fees on a charge that must then be refunded, versus releasing an authorization
  for free), so D2 is money as well as correctness; and **do not rework the capture flow
  mid-acceptance** — it is its own slice (§5 step 4), not a drive-by.
- **legal-specialist** — the synthetic-identities sentence must appear in the evidence (no real
  personal data is processed by the walk, keeping Q-L3 true); the **Connect shape check** belongs
  in the script (separate charges & transfers is the recorded posture); and **test mode waives
  KYB** — this acceptance proves **nothing about payout readiness**, which arrives only with the
  SASU's onboarded Connect account. *A grade, not clearance.*
