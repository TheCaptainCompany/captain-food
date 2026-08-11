# Open decisions — the product-owner register

**Every decision the proposals are waiting on, in one place.** Proposals hold the reasoning; this
holds the queue. If a decision is not here, it is not blocking anything.

> **The gate:** implementation does not start from a proposal whose **Status is not `Approved`**.
> The `architect` agent enforces this — an issue whose proposal has unanswered questions is classified
> 🔴 RED and never dispatched. So this page is the throttle on the whole pipeline.

Last reconciled: **2026-08-11 (architect run — isolation end-to-end, and the pre-diagnostic health payload)** — **5 product-owner-owed open**; two more directives arrived and both **closed on arrival**, recorded in [ADR-20260811-120828](../adr/ADR-20260811-120828-behaviour-tracking-isolated-end-to-end-and-a-faulted-worker-pre-diagnoses-itself.md). **TRK-ISO**: behaviour tracking gets its own database **AND its own projector worker** — further than PROP-20260811-000946 D5 asked, and it matters more under the halt decision, because a shared worker would let a malformed behaviour event wedge a group sitting beside the order read models. **HEALTH-2**: a faulted worker reports unhealthy and is **not restarted** — *"K8s does not need to restart the worker"* is **independently the same conclusion** the team reached from the failure analysis, so the convergence is recorded; and *"it's a pre diagnostic"* makes the **payload the deliverable and the status code merely the transport** — which is [no-polling-only-pushing](../adr/ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md) applied one layer up: the failure pushes its own diagnosis into a watched surface instead of a human polling logs. ⚠️ **Two edges reported rather than discovered later.** **HEALTH-2a**: taken literally, *"say it in `/health`"* **would take the storefront down** — the monolith runs the API and the projector in one process (`RUN_PROJECTOR` default on), has a `Service`, and its `/health` is the ADR-0043 deploy interlock, so a halted read model would make the **API** unready and block the deploy that fixes it. The rule is restated as *"the endpoint a pod's **readiness probe** points at returns non-2xx when a component **that pod owns** is faulted"*. **HEALTH-2b**: *"any worker"* does **not** apply to the actor-mailbox workers — they **already quarantine** (poison cap, lane keeps draining, operator requeue), so halting them would turn a parked message into a **stopped order lane**. The principle: **halt is right where there is no quarantine; quarantine is better wherever it exists** — projections halt precisely because they have none. Actor workers still owe the pre-diagnostic half: poison data is admin-GraphQL-only today (**no `/mailbox` exists**), and the surface it needs must be **report-only, never gating readiness**. Previously: **the second answer sheet: four settled, one with legal** — **5 product-owner-owed open**, down from 8. Closed this sitting: **MET-G** (the `DbFaultPolicy` default **flips to `Halt`** — the team recommended quarantine first and was **overruled**; [ADR-20260811-105024](../adr/ADR-20260811-105024-projection-halt-default-and-health-visibility.md)), **MET-Q7** (no hosted SDK, **and behavioural data goes in a database separate from the business data**), **COOP** (all three cooperative properties designed into the first slice), **MET-W** (a named retention-window catalog, sequenced with the erasure work). **TRK-scope stays open and is with LEGAL, not with the product owner** — whether a pseudonymous journey identifier fits the audience-measurement exemption; the proposals are deliberately **not** amended until legal reports. ⚠️ **The headline is a precondition the flip does not have**: verified on `5fdc519`, under `Halt` the worker does not stop, `running` stays `true`, `/projector` returns **200 OK**, and **neither Kubernetes probe looks at projection status at all** (readiness `/health`, liveness `/ping`) — so **flipping today produces a projector that wedges permanently and reports itself completely healthy**. The ADR settles the design: **per-group halt with the process alive**, **readiness not liveness** (projector bins have **no `Service`**, so readiness is a pure signal with no side effect, while liveness would CrashLoopBackOff and take every sibling group down), a **per-group** payload naming the halted group and event, and the **missing observability contract** — `specs/observability.yaml` declares no projection contract at all. Also recorded as a **known accepted consequence** (MET-G2): `ScopeMembership` is the single read-side authorization index and is a projection, so a halted group freezes **revocations** — which touches the *"explicit and immediate"* guarantee of the §6.4 closure. Previously: **8 product-owner-owed open** (MET-R closed; MET-T, MET-T2, MET-S closed or dissolved on arrival; MET-W added as a dependency). Product owner, verbatim: ***"Confirm the reversal, go with the projections"*** and ***"But we need to heavily strongly typed the spec no string in it"*** — recorded as [ADR-20260811-014129](../adr/ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md), which **supersedes ADR-20260810-234225 in part** (clauses 1–3 carried forward; clause 4 and the enforcement table reversed) and is **never a rewrite** of it. The second sentence is a separate decision with a wider blast radius, and **it landed on a real defect in the team's own grammar**: `increment: orders`, `groupBy: [day]` and `value: { sum: orders }` were bare names pointing at declarations *in the same file*, so a typo was not a broken reference the loader could catch — it was a silently wrong metric, which is the exact failure class the proposal exists to remove. [#413](https://github.com/TheCaptainCompany/captain-food/issues/413) is the repo's own receipt for why this is structural and not stylistic. Two more things dissolved rather than needing decisions: **MET-S** — the `serviceType` problem was a **grain error, not a missing field** (every one of the 11 `Order*` events carries `orderId`, so an entity-grained fold is total and **the versioning story is withdrawn**), and **MET-T2** — the existing bare-name surface (40 + 112 sites) is all covered by bespoke rules today, so it is one issue to sequence later, not a sweep to mix in. Previously the headline was **§27bis MET-R, a DECISION REVERSAL filed rather than executed**: the product owner's independently-held design for the metrics half is **metrics as PROJECTIONS** — a declared fold over `domain_events` into the `bam` schema, read through GraphQL — and **the team evaluated it and changed its recommendation to match**. What decided it was not deference: the instrument design the team had recommended one day earlier **forfeits replay by construction** (`orders_placed_metric.rs:129` asserts the point does not fire on a rebuild, so a metric added later has zero history), **cannot express a ratio or a distinct-identity denominator at all**, and **had quietly diverged from the C4**, which already declares `bam` as a projector with a schema in read-models (`c4-l2.yaml:343,370,484`) that has **zero tables** (`grep bam specs/database/` = 0). Two clauses of [ADR-20260810-234225](../adr/ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md) — *"never entity ids"* and *"generated instruments"* — are contradicted; the ADR is `Accepted` and one day old, so **nothing implements either design until MET-R closes** and the ADR is superseded rather than rewritten. Its principle (unit = persona activity, declared + emitted + asserted, states its question) is untouched. **MET-Q7 is now recommended for CLOSURE as "no"** rather than deferral, because the projection design plus §28's behaviour store together remove a hosted analytics SDK's motivation on both the order and the browse side. On the tracking half the two designs **converged independently** (§28 D10) — interaction name, declared properties, principal from the JWT, a mutation as the write path — with one measured blocker: `op-missing-command` is an ERROR and all 86 mutations bind a command, so a mutation today **cannot** be a non-command and would land behaviour events in `domain_events` by default, gate green. Previously: **9 product-owner-owed open** (two added), **plus a 7-row TEAM-OWNED block in §28** on top of §27's, neither counted here because nobody outside the team owes an answer to them. New this run: **§28, behaviour event tracking declared inside `specs/screens/**`** ([PROP-20260811-000946](PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md), [#485](https://github.com/TheCaptainCompany/captain-food/issues/485)) — the **second** clause of the 2026-08-11 directive; the first clause (*"integrate the metrics in the spec"*) is an **endorsement of §27**, not a new ask, and §27 D1–D7 stand unchanged. The fact that earns §28 is not the absence of tracking, which is expected — it is that **`SetCustomerPreferences.dietaryTags` is `array<Tag>` where `Tag` is a free-form `string` with `maxLength: 80` and no enum, persisted to `View_Customer.preferences` jsonb**: `halal` and `kosher` are spellable values **today**, no screen binds it, and no review caught it because no artifact existed that would make anyone look. **Two rows are product-owner-owed** — **Q1** client storage (and therefore whether a consent banner exists at all; note the device identifier `X-SESSION-ID` **already exists**, so the question is whether a new *purpose* attaches to it) and **Q2** whether the restaurant sees its own storefront's behaviour data (the differentiator, and a controller-posture decision). **[#194](https://github.com/TheCaptainCompany/captain-food/issues/194) is deliberately NOT given a new row** — no DPIA, privacy notice or terms exist, it is open and unchanged, and §28 is *sequenced behind it* with validator rule R10 turning that sequencing into a build failure rather than a promise. Previously: **2026-08-11 (architect run — the Patton business-metrics directive)** — **7 product-owner-owed open** (one added, one closed by subsumption), **plus a 7-row TEAM-OWNED block in §27** that is deliberately *not* counted here because nobody outside the team owes an answer to it. That run added: **§27, business metrics for every feature and every persona** ([ADR-20260810-234225](../adr/ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md), [PROP-20260810-234225](PROP-20260810-234225-business-metrics-for-every-persona.md), [#484](https://github.com/TheCaptainCompany/captain-food/issues/484)). It carries the fact that earns it: **`specs/observability.yaml` declares 29 `business_metrics` and 26 of them have zero occurrences anywhere in `crates/`, `tools/` or `deploy/`** — the slot the directive asks us to fill is already 90% fiction, and the gate that should have noticed covers 3 of 14 contracts and only checks that a string constant exists. D1–D7 are team-owned under the same delegation that lifted the freeze; **exactly one row (Q7, a hosted product-analytics SDK) is product-owner-owed**, and it is a vendor/data-residency question, not a technical one. §22's *"Business-signal observability contracts"* row is **closed by subsumption** into §27 — it named the gap, this is the mechanism. Previously: **2026-08-10 (night, architect run — the `specs/**` freeze is LIFTED)** — **7 open, and none of them blocks the [#429](https://github.com/TheCaptainCompany/captain-food/issues/429) path.** The headline of that run was not a row, it was a **constraint removal**: the product owner delegated `specs/**` to the team (*"I'm surprise that I read that the spec was untouchable now that we have the team working together we don't need to have this constraint anymore… I'm pretty sure the team will ensure the right naming and scope. Just keep me informed"*), recorded as [ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md) and logged in §5. **This page's throttle just got much narrower**: 🟠 AMBER no longer means "touches `specs/**`" — it means *a recorded decision is missing or contradicted*, or *the shape is already emitted/stored/promised*. Eight AMBER-flagged issues and four plan-mode sub-tasks are re-triaged; the "spec window" language throughout this file (§25 especially) is now **historical**. Two rows are added in **§26**: the shape of the reporting gate that replaces the freeze (**SPEC-1**, team-recommended), and a ten-second confirm-or-reverse on the `from:` rename now that measured evidence exists the product owner did not have (**SPEC-2**). Also flagged, and deliberately **not** given a row because it is decided-and-unbuilt rather than open: **`event_version` has zero occurrences across `specs/`, `crates/`, `migrations/` and `tools/`** while PROP-170000 D2 decided *"add `event_version` now (cheaper before the log grows)"* on 2026-08-08 — the freeze was silently standing in for it, and removing the freeze makes it load-bearing. Previously: **still 5 open, and none of them blocks the [#429](https://github.com/TheCaptainCompany/captain-food/issues/429) path.** Read that carefully before treating this page as a throttle: of the five, **one** is genuinely waiting on the product owner and it waits on an *external* trademark process (Solida rebrand, gating only [#411](https://github.com/TheCaptainCompany/captain-food/issues/411)); **two are team-owned work items**, not questions anyone owes an answer to (business-signal observability contracts → [#400](https://github.com/TheCaptainCompany/captain-food/issues/400); geocoding, where the team owes a *proposal*); **one is deferred by design** pending real order data (avelo37 threshold); and **one is already decided-and-deferred** by the product owner (consumer-mediator registration, to first real consumer order). All five date from the 2026-08-08 sweep — ~2 days, ~3 runs — and none is aging dangerously. **No item in the cart/checkout/order path is RED.** This run also recorded the **backlog-prioritisation delegation** (§5, [ADR-20260810-215503](../adr/ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md)) — with the board no longer under product-owner eyes, this page's §1 ordering is now the primary surface they work from. Previously: the product owner's **six-decision answer sheet** ([ADR-20260810-194548](../adr/ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md)) · **5 open decisions**, down from ten (five closed). Closed in that sitting: **451-B** `currency_mismatch` (approved as recommended — the spec window is OPEN, one line of `specs/observability.yaml`, land it without re-asking) · **451-C** #451 retitled (executed) · **§6.4 claim staleness** CLOSED on the legal+business convergence · **451-A** closed by the [#460](https://github.com/TheCaptainCompany/captain-food/pull/460) merge. The **`from:` collision** is DECIDED by the second answer sheet — **(a), rename the screens key**, the product owner's own pick, not delegated; **geocoding** stays open but is now **team-owned** (recommendation (c) approved: the team analyses and returns with a proposal). What remains open: consumer-mediator registration, business-signal observability contracts, the Solida rebrand, the avelo37 threshold, geocoding. Earlier the same day: the [#451](https://github.com/TheCaptainCompany/captain-food/issues/451) Phase-2 adjudication added the §25 rows. The §22 set (consumer-mediator registration, now **deferred to first real order** per the PO 2026-08-10; the §6.4 claim-staleness policy; the `from:` naming collision; the business-signal observability contracts; the Solida rebrand waiting on trademark; the avelo37 threshold; and the geocoding row). **§1 row G is now DECIDED 2026-08-10** — PROP-20260810-231500 D1 = **Option B / LIVE** (cart priced fresh on read via `price_cart`, projection stays a money-free fold; the #429 keystone unblocked to build). **§23 and §24 closed 2026-08-09** by the customer's eight-decision answer sheet ([ADR-20260809-050000](../adr/ADR-20260809-050000-morning-brief-eight-decisions.md)): the step-DSL branching set D1–D7 confirmed as recommended, and the public demo **deferred** with its production-critical remainder re-filed on its own. The register went 21 → 8 in one sitting by ANSWERING rows — the intended way for it to shrink; whoever adds a row updates this number in the same commit. **The ten-decision brief is fully answered** ([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md) + [ADR-20260808-203443](../adr/ADR-20260808-203443-tips-voluntary-contributions-funding-model.md)): the last three closed 2026-08-08 — tips via the customer's voluntary-contribution funding model (HelloAsso « pari », cascade-pricing fallback, public cagnotte), erasure two-path confirmed, admin explicit act-as confirmed (supersedes ADR-0037). **The customer answered the ten-decision brief 2026-08-08** ([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)): seven decided — payout posture and external orders and promo funding as recommended; capture timing per service type (delivered/picked-up/in-advance-at-table); acceptance timeout resolved by release-not-refund; entity path association→SASU→SCIC-federation; radical transparency. The 2026-08-08 five-lens sweep decided **32 rows by ensemble consent** ([ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md); customer veto window open on every one) and retired **7 stale blocks** (three §2 rows + §6, §9, §10, §16).

> **Customer decisions: see [BRIEF-20260808-customer-decisions.md](BRIEF-20260808-customer-decisions.md) (ten decisions).**
> Everything only the product owner can decide — the five-decision money posture, account-level
> erasure scope, admin act-as, the operating entity, transparency levels, and who funds promotions —
> is argued there, lens by lens. Answers land back in this register.

> **2026-07-30 — the actor-runtime set is `Approved`** (product owner, in-session:
> *"we are at the same page, we can build it now"*; ADR-20260730-231500):
> [PROP-20260728-135632 "Aggregate state as spec"](PROP-20260728-135632-aggregate-state-as-spec.md) (D1–D5),
> [PROP-20260728-152752 "The write path becomes an actor mailbox"](PROP-20260728-152752-actor-mailbox-write-path.md) (D1–D7),
> [PROP-20260730-230803 "Projection runtime"](PROP-20260730-230803-projection-runtime-batched-partitioned.md) (D1–D3)
> — all recommended options stand. The one approved-by-default flag (`messages.yaml` as the third
> payload catalog) was **VETOED 2026-07-31** (product owner, in-session): reminder messages are
> typed INSIDE the actor with a validator-proven `receives` handler, and deferred until the first
> real use case —
> [ADR-20260731-120825](../adr/ADR-20260731-120825-actor-messages-typed-inside-the-actor.md).
> The set is now fully decided. Build starts at
> [#242](https://github.com/TheCaptainCompany/captain-food/issues/242)'s foundation slice.

---

## How to decide

Four ways, in increasing formality. All are fine; pick per decision.

1. **Answer in this file** — put the choice in the `Decision` column with the date. Cheapest, good for
   the batch-approvable set below.
2. **Comment on the proposal's tracking issue** — better when the answer needs reasoning that future
   readers will want.
3. **Write an ADR** — required for anything cross-cutting (`docs/adr/ADR-YYYYMMDD-HHMMSS-*.md`).
4. **The interactive decision form** (product-owner directive, 2026-08-08: *"I liked what you did
   with the html file to let me answer in my tempo… keep this approach for other sessions"*) —
   when a BATCH of decisions goes to the customer, publish the brief as an interactive artifact:
   one card per decision (question, per-lens arguments, recommendation) with tap-choices
   **Approve as recommended / Different choice / Let's discuss** plus a free-text box, progress
   saved locally, and a "Copy my answers" button producing a markdown answer sheet the customer
   pastes back into the session. The 2026-08-08 ten-decision brief closed same-day this way.
   Mechanics for rebuilding it: [docs/claude/sessions.md](../claude/sessions.md).

Then flip the proposal's `Status` to `Approved`, naming what recorded the decision. **Do not rewrite
the proposal to match the answer** — it is a historical record of what was on the table; the decision
lives in the header, the register, and the ADR.

A proposal can be **partially approved**: mark the decided rows here and note in the header which
decisions remain. That is often the right move — several proposals have one hard question and four
easy ones.

---

## 1. Decide these first — highest leverage

Six decisions gate roughly two thirds of the backlog. Everything else can wait.

**2026-08-08 (evening)**: all six rows are now decided or in discussion — A, B, E answered by the
customer ([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)); the C
account-scope remainder is in customer discussion; D and F were ensemble-decided.

**2026-08-10**: row **G** added and **DECIDED same day** — the #429 keystone cart-pricing decision
(LIVE re-price on read vs LOCKED at add-to-cart) — **Option B / LIVE**, as recommended. Section §1
is now fully decided.

| # | Decision | Why it is first | Recommendation |
|---|---|---|---|
| **A** | [PROP-165000 D1](PROP-20260726-165000-marketplace-economics-and-money-movement.md) — **payout posture**: Stripe Connect vs merchant-of-record | Determines who the seller is, who invoices whom, how VAT is declared, and Captain's legal standing while holding customer funds. Gates [#173](https://github.com/TheCaptainCompany/captain-food/issues/173), [#172](https://github.com/TheCaptainCompany/captain-food/issues/172), [#174](https://github.com/TheCaptainCompany/captain-food/issues/174). **Gets more expensive with every real order.** | Connect, separate charges & transfers — ✅ **decided by the customer 2026-08-08, as recommended** ([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)); the Connect platform account belongs to the SASU (see PROP-032306 D7) |
| **B** | [PROP-165000 D2](PROP-20260726-165000-marketplace-economics-and-money-movement.md) — **capture timing**: authorize-then-capture vs capture-at-checkout | Changes what a rejection costs the customer, what the acceptance timeout releases ([#167](https://github.com/TheCaptainCompany/captain-food/issues/167)), and how far ahead orders can be scheduled ([#197](https://github.com/TheCaptainCompany/captain-food/issues/197)) | Authorize at checkout, capture on acceptance — ✅ **decided by the customer 2026-08-08, DIFFERENT choice** ([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)): *"Authorise on checkout. Capture on delivered / picked up / paid in advance for at-table service"* — capture is per service type (DELIVERY→delivered, PICKUP→picked up, at-table→in advance); team notes (auth ~7-day life, post-fulfilment capture-failure path, tips-vs-auth ceiling) carried in the ADR |
| **C** | [PROP-170000 D3](PROP-20260726-170000-event-log-integrity-evolution-and-erasure.md) — **GDPR erasure strategy** | **DECIDED for ORDERS 2026-07-31** ([ADR-20260731-160000](../adr/ADR-20260731-160000-order-erasure-tombstone-then-stream-deletion.md), product owner, diverging from the crypto-shredding recommendation): `OrderExpired` = deletion from the system — projections tombstone the order's rows, a technical worker later deletes the streams, an `OrderErasureProcess` PM owns the journey. REMAINING open: customer-account-level erasure (identity, files, Supabase) + the per-phase retention windows. Gates [#194](https://github.com/TheCaptainCompany/captain-food/issues/194) | Orders: tombstone + stream deletion (decided) · account scope: ✅ **decided by the customer 2026-08-08** ([ADR-20260808-203443](../adr/ADR-20260808-203443-tips-voluntary-contributions-funding-model.md)): the **two-path model confirmed** — deactivate (recover anytime, data kept, dormant sunset) + delete (Art. 17, ≤30-day grace, then real erasure; carve-outs per the retention table in [docs/legal/BRIEF-20260808-account-erasure-two-path.md](../legal/BRIEF-20260808-account-erasure-two-path.md)); counsel questions E1–E8 pending |
| **D** | [PROP-165500 D1](PROP-20260726-165500-catalog-compliance-and-merchandising.md) — **allergen representation** | EU FIC 1169/2011 is a launch blocker, and the model must exist before imports can carry it. Gates [#184](https://github.com/TheCaptainCompany/captain-food/issues/184) | Controlled 14-category enum + explicit "not declared" state — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): Annex II enum + explicit "not declared", and NOT_DECLARED **gates orderability** in the distance-selling UI (or, strict minimum, a specific functional contact means on the product sheet) — "required before publishing" is binding (legal evidence) |
| **E** | [PROP-164500 D1+D2](PROP-20260726-164500-order-operational-safety.md) — **acceptance timeout policy and TTL** | Decides whether a customer can be left charged for an ignored order. Gates [#167](https://github.com/TheCaptainCompany/captain-food/issues/167); pairs with B | Auto-cancel + auto-approved refund; 5 min with per-restaurant override — ✅ **resolved 2026-08-08 as a consequence of B** ([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)): *"No need to refund because no capture"* — timeout **releases the authorization**, no refund machinery on this path; TTL tuning (5 min + override) is reversible gated config, team-owned |
| **F** | [PROP-172000 D1](PROP-20260726-172000-spec-to-ui-contract-integrity.md) — **how a screen declares a runtime input source** | The one DSL addition needed before the write-side validator gate can fail closed. Gates the required-field half of [#169](https://github.com/TheCaptainCompany/captain-food/issues/169) and the fix for [#168](https://github.com/TheCaptainCompany/captain-food/issues/168) | Name the input source explicitly (`from:`) — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open), with the `from:` naming-collision row in §22 |
| **G** | [PROP-20260810-231500 D1](PROP-20260810-231500-cart-current-priced.md) — **cart price: LIVE (re-priced every read, option B) vs LOCKED at add-to-cart (money in events, option A)** | Unblocks the #429 keystone (`cart.current` priced) and settles whether the cart is a live estimate or a price-lock; A would reopen ADR-20260720-002217 and need event versioning. Gates [#429](https://github.com/TheCaptainCompany/captain-food/issues/429). | ✅ **DECIDED by the product owner 2026-08-10 — Option B / LIVE, as recommended**: `cart.current` priced fresh on read via the shared `price_cart`, the `Cart` projection stays a money-free fold, the authoritative freeze stays at checkout. Sub-defaults stand: claim-resolved zero-arg `cart.current` (reuses #434 `ReadScope::Customer`); "current" = most-recently-updated OPEN cart. Recorded on [PROP-20260810-231500](PROP-20260810-231500-cart-current-priced.md) (`Status: Approved`). Rationale: honors ADR-20260720-002217 (cart events carry no money), consolidates onto one pricer, no event-versioning cost. The read-side pricing observability Concern is a **build task inside #451**, not a gate. |

---

## 2. Batch-approvable — recommendation is the standard answer

These have a conventional right answer and little genuine trade-off. Reading the recommendation and
saying "yes to all" is a reasonable use of five minutes.

**2026-08-08**: every row here is now decided or retired (PROP-172500 D4 closed by the ADR addendum) —
see the per-row notes.

| Decision | Question | Recommendation |
|---|---|---|
| PROP-170000 D1 | Preventing skipped events ([#189](https://github.com/TheCaptainCompany/captain-food/issues/189)) | Snapshot / `xmin` guard — the only option that is correct rather than probabilistic — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): +xid8 safe-head, 3 adaptations (bounded scans + oldest-write-age alert · idle gate arms on the SAFE head, never `MAX(position)` · one shared safe-head helper across all FOUR readers) |
| PROP-170000 D2 | Event evolution policy | Additive-only + validator gate; add `event_version` now (cheaper before the log grows) — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): condition — the gate must also prove generated-serde tolerance (`#[serde(default)]`-safe), not only the YAML diff |
| PROP-170000 D4 | `$maxAge` / `expired_at` | Implement or delete — a specified-but-inert control is worse than none — ✅ IMPLEMENTED: the retention sweep is live (design: [ADR-20260731-153000](../adr/ADR-20260731-153000-gdpr-expiry-as-scheduled-actor-message.md)); stale row retired 2026-08-08 (ADR-20260808-171056) |
| PROP-170000 D5 | Spec-vs-code divergences (`version` 0- vs 1-based, `id` as idempotency key) | Correct the spec to match the code; the code is what has been running — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): `version` + `id` legs as recommended (the mailbox owns append idempotency); the `ce_events` leg REVERSED to code-to-spec — make the function sargable (`stream_name LIKE category \|\| '-%'`) rather than enshrine a full-log seq scan |
| PROP-170500 D3 | Where the workers run | Advisory lock now (in-process), dedicated service later — ✅ decided by the bins ADRs ([ADR-20260808-062933](../adr/ADR-20260808-062933-one-bin-per-worker.md) one bin per worker · [ADR-20260808-062432](../adr/ADR-20260808-062432-one-bin-per-adapter.md) one bin per adapter); stale row retired 2026-08-08 (ADR-20260808-171056) |
| PROP-170500 D4 | GraphiQL / Voyager in production | Keep, gated to ADMIN — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): **and self-hosted** — a no-CSP CDN bundle on the authenticated admin origin is a script-injection surface in the worst place; self-host or drop Voyager |
| PROP-170500 D5 | Subscription fan-out at >1 instance | Postgres `LISTEN`/`NOTIFY` — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): +reconcile-on-reconnect (NOTIFY has no delivery guarantee); the WS transport lands first — the gateway answers upgrades 501 today |
| PROP-171500 D1 | Where the write-side scope check runs | Dispatch layer, before journaling — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open), REWORDED before consent: the dispatch check is the fast-fail **pre-filter** (no mailbox row for an obviously-forbidden attempt, denial counter); the **authority is the actor's aggregate-state check** per PROP-20260728-135632's #235 correction |
| PROP-171500 D2 | Validate the supplied id, or derive it | Derive where the role implies one scope; validate otherwise — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): timing note — the input-field deletions are breaking-but-free only while no client ships; land before first deploy. The actor-side `requires.acting` stays the final check |
| PROP-171500 D3 | Sequencing against [#144](https://github.com/TheCaptainCompany/captain-food/issues/144) | Immediately after it lands — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open); note: [#144](https://github.com/TheCaptainCompany/captain-food/issues/144) has NOT landed yet (the §4 register note was stale) |
| PROP-172000 D3 | The drifted product spec | Rewrite §4–§5 to match ADR-0034 — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open) |
| PROP-172000 D4 | Fix the four dead actions with the rule | Same PR — a rule landing red breaks "keep main green" — ✅ OVERTAKEN: the rules landed as the two screen-action warning rules and the work is tracked in [#342](https://github.com/TheCaptainCompany/captain-food/issues/342) (the 17 screen-action↔command-input findings); stale row retired 2026-08-08 (ADR-20260808-171056) |
| PROP-172500 D4 | Job-pool filtering | Filter by city, zone and `RiderStatus` — composes with [PROP-20260808-141817](PROP-20260808-141817-rider-delivery-write-surface.md) slice 4's per-rider decline exclusion on the same `myDeliveries` query (the rider write surface itself moved to that proposal, §20) — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056 addendum; veto open; business + ux consented, legal consented with the SUSPENDED-is-deactivation-machinery note carried in the ADR) |
| PROP-172500 D5 | Rider↔customer contact | Route through the order conversation, not phone numbers — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): +masked/bridged call fallback and rider-side one-tap canned chips ("customer unreachable at the door" needs synchronous escalation; a keyboard on a bike violates the one-hand rule). **Customer-endorsed 2026-08-08** |
| PROP-165500 D6 | Menu scheduling | Defer, but record it — needed when combos land — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): deferred until combos |

---

## 3. Genuine trade-offs — worth your time

| Decision | Question | Recommendation | The tension |
|---|---|---|---|
| PROP-165000 D3 | Rounding for fee splits | Buyer total first, residual cent to `captainNet` — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): buyer-total-first, residual cent to a stated leg, pinned by an odd-total test | Undefined today; any answer works, but it must be stated and tested or splits stop reconciling |
| PROP-165000 D4 | Delivery-fee dimension | Per-zone — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): business CONFIRM, fee LEVELS priced from rider economics upward | Pairs with PROP-172500 D1; distance-banded is fairer but needs geocoding you do not have |
| PROP-165000 D5 | Do tips move money? | ✅ **decided by the customer 2026-08-08** ([ADR-20260808-203443](../adr/ADR-20260808-203443-tips-voluntary-contributions-funding-model.md)): rider tips as the team recommended ([BRIEF-20260808-tips-discussion.md](BRIEF-20260808-tips-discussion.md)); restaurant tip **per-restaurant opt-in**; platform **voluntary contribution, HelloAsso model** — public « pari » that contributions cover platform costs, declared cascade-pricing fallback (`fixed cost ÷ restaurant count`, 0 € when covered), public cagnotte with per-contribution name consent | Tips are recorded and displayed today but reach nobody; the transfer-leg gate stands |
| PROP-165500 D2 | Does Captain own stock consumption? | Re-validate at checkout; decrement only Captain-managed offers — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): business emphatic (POS double-decrement manufactures phantom oversell); rejection copy names the item, one-tap remove-and-continue | HubRise restaurants have a POS as stock authority — double-counting is worse than not counting |
| PROP-165500 D3 | Per-service-type pricing | Optional price override on `Offer` — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): resolve the `catalog(restaurantId)` ambiguity in the same change; watch the delta drift as a coaching signal | French practice prices delivery above counter; the model allows per-mode VAT but not per-mode price |
| PROP-165500 D4 | Catalog images on the [#134](https://github.com/TheCaptainCompany/captain-food/issues/134) framework | Confirm a **public** audience now, while it is on paper — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open) | #134 is designed around private per-order attachments; retrofitting public access later is the expensive version |
| PROP-165500 D5 | Merchandising order | Promo codes first — ✅ **decided by the customer 2026-08-08, as recommended** ([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)): restaurant-funded first; platform codes deferred; loyalty next on [#158](https://github.com/TheCaptainCompany/captain-food/issues/158)'s balance | Highest acquisition value for a single-city launch; loyalty must reuse [#158](https://github.com/TheCaptainCompany/captain-food/issues/158)'s balance, not a second one |
| PROP-164500 D3 | V0 notification channel | In-app + sound, then SMS — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): **+SMS in the same V0 slice** (never "then"), escalation SMS at ~60–90 s unacknowledged, visible sound-armed/blocked state on `orders_queue`; two lenses converged independently; **customer-endorsed 2026-08-08**. The supervised pilot on [#166](https://github.com/TheCaptainCompany/captain-food/issues/166) alone stays acceptable only while a human watches every order | Waiting for [#127](https://github.com/TheCaptainCompany/captain-food/issues/127)'s full cascade blocks the entire operational loop behind a post-V0 epic |
| PROP-164500 D4/D5 | Timed pause; opening-hours exception days | Yes to both — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open) | Weekly recurrence alone is wrong on all eleven French public holidays |
| PROP-164500 D6/D7 | Scheduling window; order modification scope | Same-day slots; address correction before `PREPARING` — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): D6 sequenced behind §1 B (authorization life bounds the window) | Bounded by B — card authorizations expire in ~7 days |
| PROP-171500 D4 | ADMIN acting on behalf of a tenant | Explicit, logged bypass — ✅ **decided by the customer 2026-08-08, as recommended** ([ADR-20260808-203443](../adr/ADR-20260808-203443-tips-voluntary-contributions-funding-model.md)): explicit act-as (admin acts as admin on a named restaurant scope, distinct logged authorization path); **supersedes ADR-0037's impersonation-only stance**, reversed by its own author | — |
| PROP-172000 D2 | Rejection reasons: enum or free text | Controlled enum + optional note — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open), cures folded: `OTHER` ships day one; the free-text note is declared in the erasure scope (restaurant-authored PII) | Rejection reasons are the analytics that tell you which restaurants to coach |
| PROP-172500 D1 | Delivery-area model | Postal-code sets now, geocoding next — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): business note — Tours river crossings make zones unusually truthful | Geocoding unlocks distance fees and honest ETAs — sequence it deliberately |
| PROP-172500 D2 | Proof of delivery | Handover photo over [#134](https://github.com/TheCaptainCompany/captain-food/issues/134) — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open), legal evidence: legitimate-interest basis with a recorded LIA; **dispute hold** (open reclamation/chargeback suspends expiry — card windows outrun 90 days); rider UI guidance: package/door, never a person | `NOT_DELIVERED` claims are unadjudicable today, and [#151](https://github.com/TheCaptainCompany/captain-food/issues/151) is already routing them |
| PROP-172500 D3 | Reclaiming an abandoned run | Rider release + stall sweep | A stalled `PICKED_UP` job means the food is with the rider — re-offering is wrong, it needs re-cooking. **Dependency**: the sweep's release must emit the SAME event as the manual release, so this now depends on [PROP-20260808-141817 D3](PROP-20260808-141817-rider-delivery-write-surface.md)'s naming decision (§20) — never a twin event. ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): the stall sweep emits `DeliveryAssignmentReleased` — the dependency is resolved by §20's D3 rename |

---

## 4. Inherited — ✅ swept 2026-08-08 (what remains open moved to §22)

| Proposal | Decisions | Note |
|---|---|---|
| [PROP-20260725-185140](PROP-20260725-185140-read-side-per-instance-authorization.md) read-side authz | **D1–D11** | ✅ decided WITH the graphql restatements 2026-08-08 (ADR-20260808-171056; veto open): intent stands end to end — scope predicates EMITTED into every generated subgraph resolver's SQL (unscoped resolver unspellable), `ScopeMembership` its own consumer-schema projector with one checkpoint and a declared cross-scope GRANT exception, the account-wide snapshot folds network events from the single log (event-carried), the guard mounts in each `graphql-{scope}` service and NEVER in the no-auth gateway, §3.3.4/§3.3.5 struck as moot. **§6.4 claim staleness STAYS OPEN** and the identity-bridge home is genuinely open — two new rows in §22. (Earlier register note was stale: [#144](https://github.com/TheCaptainCompany/captain-food/issues/144) has NOT landed) |
| [PROP-20260725-120055](PROP-20260725-120055-generic-file-attachment-framework.md) file framework | **D1–D5** (D2b decided) | ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open): **D1 RE-PREMISED** — object store = OVH Object Storage (EU), presigned S3 URLs (Supabase Storage references are historical); the `files` registry is IRREPLACEABLE state riding `captain-core`'s backup/PITR, never replay-restorable views; the weekly restore drill gains a bucket↔registry orphan reconciliation. **D2** per-kind retention windows with the dispute hold, tombstone `uploaded_by` anonymized after a stated horizon, and ZERO KYC documents under Connect. **D3/D4/D5** as proposed — dedupe must never share `storage_key` across rows |
| [PROP-20260724-133700](PROP-20260724-133700-runtime-screen-and-translation-delivery.md) · [PROP-20260724-144500](PROP-20260724-144500-admin-flag-translation-keys.md) | — | ✅ 2026-08-08 (ADR-20260808-171056; veto open): PROP-133700 marked **Deferred (post-V0)**, [#96 "Live spec editing + per-tenant customizations (specs/customizations/) with fail-closed branch publishing"](https://github.com/TheCaptainCompany/captain-food/issues/96) stays; PROP-144500 **Deferred** until live-translation work starts |

---

## 5. Decided

| Date | Decision | Answer | Recorded in |
|---|---|---|---|
| 2026-08-10 | **Is `specs/**` touchable by the team?** — never a queued row; a directive that arrived in-session, and the largest constraint removal in the operating model to date | **The freeze is LIFTED**, verbatim: *"I'm surprise that I read that the spec was untouchable now that we have the team working together we don't need to have this constraint anymore"* / *"We can perhaps have a discussion if the team is willing to change the structure of the specs. But I'm pretty sure the team will ensure the right naming and scope. Just keep me informed."* Execution loops may add and amend DSL **content and structure**. The boundary is **not** content-vs-structure (rejected: a scope-folder move rewrites no refs and is free, while a one-word type change on an emitted event is irreversible — the split is anti-correlated with risk in both directions). It is **three questions in order**: (1) does it contradict/create a **recorded decision**? → stop, file a row; (2) is the shape already **emitted, stored or promised**? → it is a **migration**, record the versioning story first; (3) otherwise it is the team's, **`specs/common/` included** — the kernel is high-fan-out, not off-limits, and freezing it would freeze the one place "one name = one dedicated scalar" is enforced. **Structure needs no separate gate**: proportionality already routes any real option space to a proposal + register row, which *is* the "perhaps a discussion" that was offered. **Reporting replaces the freeze**: one sentence per landed spec change in [docs/SPEC-LOG.md](../SPEC-LOG.md), same commit, gated — no cadence, no digest to send. **NOT delegated**: this register, external/legal/admin-gated matters, and the binding value-first method | [ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md) |
| 2026-08-10 | **Who prioritises the backlog** — never a queued row; a directive that arrived in-session | **Delegated to the team**, verbatim: *"Don't care about the project field anymore the team decides without me."* The `Priority` bucket and **row order** pass to the team (`Type`/`Value Size`/`Impact`/`Effort` were already team-set at triage, so only two things actually change hands). **NOT delegated**: this register, external/legal/admin-gated matters, `specs/**` approval, and the **value method itself** — which is promoted from description to binding constraint, standing in for the judgment that left the loop. New prohibition: an agent may never re-bucket or reorder an item **to make it dispatchable or to legitimise its own recommendation**; a blocked top item is reported blocked, never re-ranked. The product owner keeps a silent, immediate, per-item override. Consequence for THIS file: with the board no longer under product-owner eyes, **the register's ordering by leverage is now the main surface they work from** | [ADR-20260810-215503](../adr/ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md) |
| 2026-08-10 | **The six-decision answer sheet** (interactive artifact) | **Four approved as recommended** — 451-B `currency_mismatch` (spec window now OPEN), 451-C #451 retitled (executed), the `from:` collision and geocoding rows both to recommendation (c) = **team picks and records**. **§6.4 claim staleness CLOSED** on a legal+business convergence the PO sent the card back for: keep the ~1h Supabase default, explicit immediate revocation for rider deactivation and staff removal, [#194](https://github.com/TheCaptainCompany/captain-food/issues/194) erasure scrubs `app_metadata` AND revokes refresh tokens. **#474 answered with measurements** (990 tests / 182 binaries / 34s warm; `-p application` = 324 tests in 0.04s linking 9 crates) — the crate split already gives per-part testability, the hole is local-only, and the fix is `make test-crates` invoked from the Stop hook. **Process lesson recorded**: consult the standing lenses before escalating a card — a question a lens can answer is not a decision | [ADR-20260810-194548](../adr/ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md) |
| 2026-08-08 | **The register sweep — 30 rows by ensemble consent** | The five-lens sweep decided 30 open rows with cures folded (per-row notes throughout §§1–4, 9, 11, 19), escalated one to the customer (PROP-165500 D5 → brief ch. 6), retired 7 stale blocks, and added the §22 rows. Customer veto window open on every consent decision | [ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md) + [BRIEF-20260808-customer-decisions.md](BRIEF-20260808-customer-decisions.md) |
| 2026-08-02 | **PROP-20260802-130500 D1–D6** — isolation by construction | **All six answered** (D1 via PROP-20260728-152752 D9). D2 **(a) handler crates per actor** — aggregates AND process managers, domain value types stay one crate · D3 **cargo-deny capability allowlist in phase 1** (who may hold `sqlx`/`reqwest`) · D4 **one generic `ActorClient` with `get_operation_status(message_id)`** — operation status is generic to all operations, so neither a per-actor client method nor a separate `OperationStatusClient` type; per-actor typed clients stay write-side · D5 **`test-fixtures` feature + CI check** · D6 **later, separately — against the recommendation** (own change after phase 1). Scope directive: "per actor" includes the two process managers at every phase. | Product owner, this register (§14) + [PROP-20260802-130500](PROP-20260802-130500-isolation-by-construction.md), realized by [#290 "Actor-client crate isolation (PROP-20260728-152752 D9): compiler-enforced door, then per-actor crates"](https://github.com/TheCaptainCompany/captain-food/issues/290) |
| 2026-07-29 | **PROP-170500 D1 + D2** — telemetry backend and sampling | **D1 answered: Honeycomb**, over OTLP/HTTP, pinned to the **EU (`eu1`)** region — a GDPR constraint, not a default, since spans carry `customerId`/`orderId` and ADR-0042 pinned data to Frankfurt. `HONEYCOMB_API_KEY` supplied as a repo Actions secret and pushed to Render by CI. Telemetry **degrades, never gates**: no telemetry key is `required:`, so a missing ingest key drops the exporter and keeps structured logs rather than refusing to serve orders. **D2 answered but NARROWED — against the recommendation**: parent-based HEAD sampling at `1.0` (keep everything), not tail-based. Tail sampling needs Refinery, i.e. a service to run and pay for, which contradicts ADR-0042's minimal-ops-pre-PMF posture — and D2's own justification says the volume is not there yet. Revisit when ingest cost is measurable. | Product owner + [ADR-20260729-183000](../adr/ADR-20260729-183000-telemetry-is-honeycomb-eu-and-degrades-never-gates.md), realizing [#191](https://github.com/TheCaptainCompany/captain-food/issues/191) |
| 2026-07-28 | **PROP-004616 D1–D6** — slug lifecycle + SIRENE inbound events | **All six answered.** D1 `RestaurantSlugConfigured` + `RestaurantSlugReconfigured` (in session) · D2 slug chosen **between claim and activation**, gated by "no activation without a configured slug" · D3 **write-side reservation table** with a real `UNIQUE` (also holds released slugs) · D4 the ACL stages **`RestaurantRegistered` only** — *against the recommendation*, and stricter: no registry-fact event, no ACL branching, the **aggregate** decides record/ignore/update · D5 **null the slug on `NON_PARTNER` rows** · D6 **both** `IGNORED` and `DUPLICATE`. Partially supersedes ADR-0045. | Product owner, this register + [ADR-20260728-011344](../adr/ADR-20260728-011344-slug-lifecycle-and-sirene-inbound-events.md) |
| 2026-07-26 | **PROP-193000 D1–D4** — continuous development loop | **Deferred.** The daily architecture-review routine is sufficient for now; the dev loop stays off until the proposals are under control. `dev-loop.yml` remains `workflow_dispatch`-only with `dry_run` defaulting true. | Product owner, this register |

## 6. The daily decision cycle — ⚠️ SUPERSEDED 2026-08-08

> **Superseded by [ADR-20260808-144738](../adr/ADR-20260808-144738-product-ownership-lives-in-the-team-no-pm-agent.md)**
> (product ownership lives in the team; consent-based ensemble decisions replace the daily ask
> ritual) — recorded in the 2026-08-08 register sweep (ADR-20260808-171056). D1–D5 below stay as
> the option-space record; none is owed an answer. The proposal's Status is `Superseded`.

[PROP-20260726-201500](PROP-20260726-201500-daily-decision-cycle.md) ([#211](https://github.com/TheCaptainCompany/captain-food/issues/211))
proposes closing the loop: audit → ask → record → implement, daily. Its **D5 is the one that decides
whether the cycle works at all** — whether DSL diffs join the daily approval ritual. Verified finding
behind it: all six §1 decisions above unblock work that needs `specs/**` changes, so answering them
moves items from 🔴 RED to 🟠 AMBER, **not** to 🟢 GREEN. Without D5 the cycle would ask the important
questions and still be unable to act on the answers.

| # | Decision | Recommendation |
|---|---|---|
| D1 | Where the ask lands | One standing pinned GitHub issue, rewritten daily |
| D2 | What counts as an answer | Free prose, re-stated by the next run before use (24h interpretation window) |
| D3 | How many questions per day | Up to 3, plus one batch block |
| D4 | When nothing is answered for days | Keep implementing GREEN; escalate the ask with its age |
| **D5** | **Do DSL diffs join the ritual?** | **Yes — it is what makes the cycle reach the work, and it keeps a human approving every DSL change** |
| — | Start with a week of ask-only? | Recommended |

---

## 7. Slug lifecycle + SIRENE inbound events — ✅ DECIDED 2026-07-28

[PROP-20260728-004616](PROP-20260728-004616-slug-lifecycle-and-sirene-inbound-events.md)
([#220](https://github.com/TheCaptainCompany/captain-food/issues/220)) came out of a Supabase disk-IO
alert that turned out to be a symptom. Three defects underneath it: a **failed INSERT is the
idempotency mechanism** for six creation handlers, **INSEE updates are silently dropped** (no
`UpdateRestaurant` exists in the SIRENE worker at all), and the write path resolves identity through an
**unindexed JSONB scan of the read model** once per staged SIRET. All three trace to deriving the slug
at seeding time.

**✅ CLOSED 2026-07-28 — all six answered, proposal `Approved`, implementation under way.** Kept here for
the audit trail; the answers are in §5 and in
[ADR-20260728-011344](../adr/ADR-20260728-011344-slug-lifecycle-and-sirene-inbound-events.md).

| # | Decision | Recommendation | **Answer** |
|---|---|---|---|
| D1 | Naming of the rename event | `Configured` + `Reconfigured` | ✅ as recommended (in session) |
| D2 | When is the slug chosen? | Between claim and activation, gated by "no activation without a configured slug" | ✅ as recommended |
| D3 | How slug uniqueness is enforced on the write side | Write-side reservation table with a real `UNIQUE` | ✅ as recommended |
| D4 | What the ACL stages as the inbound event | A registry fact (`RestaurantObservedInRegistry`) plus a policy | ⚠️ **against the recommendation** — `RestaurantRegistered` **only**, unconditionally, with the **aggregate** deciding record/ignore/update. Stricter than either option offered: no new event, no ACL branching, no domain decision in the adapter |
| D5 | Migration of the ~200k existing derived slugs | Null them on `NON_PARTNER` rows | ✅ as recommended |
| D6 | Is the `IGNORED` / `DUPLICATE` split worth two statuses? | Both | ✅ as recommended |

**Sequencing (binding):** the slug change lands *before* the SIRENE update path, or an INSEE rename
becomes a live-storefront rename. SIRENE stays paused at both halves until the whole chain is complete.

Reverses part of **ADR-0045** (SIRENE → `RegisterRestaurant` via the command path), so the realizing
change needs an ADR. The `ImportCatalog`-stays-a-command contrast in CLAUDE.md survives intact: the test
is whether the originator can be told no.

---

## 8. SIRENE mirror storage — ✅ DECIDED 2026-07-28

[PROP-20260728-120931](PROP-20260728-120931-sirene-mirror-payload-is-transient.md)
([#231](https://github.com/TheCaptainCompany/captain-food/issues/231)). Measured on production
2026-07-28: `external_sirene_restaurants` is **655 MB — 77% of the database** — at department 37 of 101,
on a **2 GB disk with ~580 MB free** (Free plan, already flagged *exceeding usage limits*). Full France
is ~2 GB for that one table. [#218](https://github.com/TheCaptainCompany/captain-food/issues/218) paced
the sweep correctly, but pacing does not create disk, so this is what actually gates national coverage.

The proposal: the payload is an input to translation with a **lifetime**, the hash is the
change-detection key that persists. Keep the payload only while a row is pending, drop it once
translated. ~1.8 kB/row → ~200 B/row; ~655 MB → ~90 MB today, ~250 MB at full France.

**D5 is the one that needs real thought** — everything else has a clear answer. It asks whether losing
replay/backfill from the mirror is acceptable, given INSEE is the system of record and (since #218) a
full re-fetch is a normal paced operation rather than a special one.

**✅ CLOSED 2026-07-28 — all five answered, proposal `Approved`, implemented in PR #234.** Kept here for
the audit trail; the reasoning is in
[ADR-20260728-143000](../adr/ADR-20260728-143000-sirene-mirror-payload-is-transient.md).

| # | Decision | Recommendation | **Answer** |
|---|---|---|---|
| D1 | What the mirror retains | Payload transient (NULL after successful processing), hash permanent | ✅ as recommended |
| D2 | Hash algorithm + encoding | Keep SHA-256, store as `bytea` not hex text. Keep the column named `payload_hash`, never `payload_md5` — naming a column after an algorithm pins the schema to it | ✅ as recommended, **sequenced after compaction**: `ALTER … TYPE` rewrites the whole table and needs ~655 MB free against ~580 MB. Cheap once live data is ~90 MB |
| D3 | Unmappable / failed rows | KEEP their payload — it is the only evidence of why the record was unusable | ✅ as recommended, with a limit: the CI compaction has no ACL, so **historical** ACL-unmappable payloads are dropped. Holds going forward via the worker |
| D4 | Migration on ~580 MB free | Batched `UPDATE … SET payload = NULL` with `VACUUM` interleaved; a single whole-table UPDATE would likely hit `No space left on device` again | ✅ as recommended |
| **D5** | **Replay/backfill posture** | **Accept re-fetch from INSEE when a new field is needed** — the mirror is a cache, INSEE is the system of record | ✅ accepted, and **cheaper than the proposal argued**: the hash covers the TYPED projection, so adding a field to the wire types invalidates every digest and the next ordinary paced sweep re-translates the whole mirror by itself. The backfill is not an operation to build |
| — | Where the compaction runs | Server-side (it has the ACL, so D3 holds for historical rows too) | ⚠️ **against the recommendation** — the **CI `sirene_ingest` job**. Cost recorded under D3 |

---

## 9. Configuration is declared and validated at startup — PROP-20260729-004500 — ✅ DECIDED (D1–D3/D5 2026-07-29 · D4 2026-08-08)

> **This block had gone stale**: D1/D2/D3/D5 were approved 2026-07-29 and realized
> ([ADR-20260729-010500](../adr/ADR-20260729-010500-configuration-is-declared-and-fails-fast.md));
> only D4 was still genuinely open, and it was ✅ decided by ensemble consent 2026-08-08
> (ADR-20260808-171056; veto open): presence-only `/config` readiness endpoint, as a post-cutover
> follow-up. Rows kept for the audit trail.

Tracking issue: [#246 "Declare the app's configuration in specs/, validate it at startup, and refuse to boot when a required key is missing"](https://github.com/TheCaptainCompany/captain-food/issues/246).
Product-owner directive of 2026-07-29 (the *what* is decided; these are the *how* questions it raises).

Context in one line: configuration is the only part of the system with no source of truth — which is
why `RUN_SIRENE_WORKER` had no home, `API_SECRET` is configured and read by nothing, and a missing
`STRIPE_WEBHOOK_SECRET` would silently produce the worst failure mode in the product.

| Decision | Question | Recommendation |
|---|---|---|
| PROP-004500 D1 | Required-ness model | **Per-profile** (`required: [production, staging]`) — production cannot boot misconfigured, dev/CI still start on a partial secret set |
| PROP-004500 D2 | Which currently-degrading keys become HARD requirements in production | `STRIPE_WEBHOOK_SECRET`, `AUTH_SESSION_KEY`, `DATABASE_URL`, `SUPABASE_URL`/`SUPABASE_PUBLISHABLE_KEY`, `STRIPE_SECRET_KEY`. **Behavioural change** — see D5 for sequencing |
| PROP-004500 D3 | Where the profile comes from | An explicit `APP_PROFILE` key defaulting to `development` — not inferred from the host or from key prefixes |
| PROP-004500 D4 | A `/config` readiness endpoint | Yes, **presence-only** (never values) — the same one-curl answer `/sirene` just proved worth having |
| PROP-004500 D5 | Rollout sequencing | **Warn-only for one deploy, then enforce** — the first deploy reports what production is missing without taking anything down |

---

## 10. CI owns the Render service configuration — PROP-20260729-014500 — ⚠️ SUPERSEDED 2026-08-08

> **Superseded**: Render is no longer the target —
> [ADR-20260805-070138](../adr/ADR-20260805-070138-render-status-reflects-service-suspension.md)
> suspended the service, and
> [ADR-20260807-002705](../adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md) moved
> configuration ownership to GitOps + generated manifests on OVH MKS. Recorded in the 2026-08-08
> register sweep (ADR-20260808-171056). Rows kept as the option-space record; the proposal's
> Status is `Superseded`.

Tracking issue: [#248 "CI owns the Render service configuration: sync specs/configuration.yaml + repo secrets to the service, never the dashboard"](https://github.com/TheCaptainCompany/captain-food/issues/248).
Product-owner directive of 2026-07-29 (*"the settings must be done by the CI itself not by my manual
configuration on render"*) — the *what* is decided; these are the *how*.

[#246](https://github.com/TheCaptainCompany/captain-food/issues/246) gave configuration a declaration;
this gives it an owner. Until it lands, `RUN_SIRENE_WORKER=true` remains a dashboard field nobody can
see from the repo — and the production boot log confirms it was never set, which is why 6,649
department-37 rows are still `PENDING`.

| Decision | Question | Recommendation |
|---|---|---|
| PROP-014500 D1 | Write mode | **Upsert only** to start (never deletes); revisit replace-all once the drift report has been empty for several deploys |
| PROP-014500 D2 | Dry-run first? | **Yes** — the workflow cannot be tested outside CI (no local `RENDER_API_KEY`), so its first real run would otherwise rewrite production config untested |
| PROP-014500 D3 | Secret bootstrap ordering | **Non-secrets first** — unblocks `RUN_SIRENE_WORKER` immediately with zero secret-handling risk |
| PROP-014500 D4 | `RENDER_API_KEY` as a write credential | **Reuse the existing account key** — the exposure already exists; Render issues no narrower token |
| PROP-014500 D5 | Where non-secret values live: **baked into the image** or service env? | **Hybrid — bake non-secrets, sync secrets.** Render has no per-deploy env override (the deploy API takes only `clearCache`/`commitId`/`imageUrl`/`deployMode`), so baking is the only way to attach config to the artifact. It makes the digest determine behaviour, so a rollback restores the config that shipped with that build. Secrets can never be baked — the GHCR image is public |

---

## 11. Uber Eats Marketplace + per-surface Uber Direct credentials — PROP-20260730-032306

Tracking issue [#260 "Epic: Uber Eats Marketplace integration (order centralization + menu sync) and per-surface Uber Direct credentials"](https://github.com/TheCaptainCompany/captain-food/issues/260).
**Partially approved**: D2 and D6 were decided in the 2026-07-30 session and are recorded by
[ADR-20260730-032306](../adr/ADR-20260730-032306-uber-integration-topology-two-orgs-and-asymmetric-app-auth.md).
Three remain, and **D7 is not an engineering decision** — it needs whoever advises on company/tax law.

**2026-08-08**: D1 ratified and D3 decided by ensemble consent (ADR-20260808-171056; veto open);
D4 and D7 were answered by the customer 2026-08-08
([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)); **D5 remains open**.

| Decision | Question | Recommendation |
|---|---|---|
| PROP-032306 D1 | Build the Eats integration directly, or layer on HubRise (which already syncs menus to Uber Eats and Deliveroo)? | **Direct** — effectively chosen by registering the app. Reaches restaurants with no POS at all, which is the segment Captain targets; and allergen relay is contractually ours whether or not we own the pipe — ✅ RATIFIED by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open) |
| PROP-032306 D2 | Which Uber org is billed for a Direct dispatch? | ✅ **DECIDED 2026-07-30: two orgs, split by acquisition surface, storefront first.** (C — one org plus internal attribution — was recommended; A was chosen) |
| PROP-032306 D3 | Where does the acquisition surface live? | **A field on `OrderPlaced`.** Not derivable at dispatch: acceptance-first (ADR-20260720-015500) means the saga runs long after the `Host` header is gone — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056; veto open), cure folded: the acquisition scalar is declared ONCE in `specs/common` so a future `ExternalOrderReceived` shares it |
| **PROP-032306 D4** | How is a marketplace-originated order represented, given it carries **no Captain PaymentIntent**? | **A distinct `ExternalOrderReceived` event.** Making the payment fields nullable on `OrderPlaced` would weaken a money invariant for every order to accommodate a minority. **Pairs with §1 A/B (payout posture, capture timing) — decide together** — ✅ **decided by the customer 2026-08-08, as recommended** ([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)): distinct `ExternalOrderReceived`, provenance visible |
| **PROP-032306 D5** | Menu ownership across Captain / HubRise / Uber, and per-channel price parity | **HubRise authoritative when connected, else Captain**, one-way push. Parity is the sharp edge: restaurants mark Uber prices up to absorb Uber's commission, and ADR-0024's comparison coefficients are calibrated on that — pushing Captain prices unchanged undercuts the restaurant *and* invalidates `basis: REAL` — ✅ decided by ensemble consent 2026-08-08 (ADR-20260808-171056 addendum; veto open; business CONFIRM: uplift preserved as a RATIO, push never defaults to overwrite, pinned by spec test) |
| **PROP-032306 D7** | Is the Provider entity on the signed Uber agreement (**Caring Hope Foundation**, RNA W372020229 — a loi-1901 association) the entity that will operate the platform? | **Needs legal input, not a recommendation.** An Uber API licence follows the entity; if the association holds it while another entity operates and earns commission, access sits outside the licence. Also interacts with the payout posture in §1 A — ✅ **decided by the customer 2026-08-08** ([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)): *"association (now) → SASU (operations, brand pending) → SCIC per area + federation, like CoopCycle"*; Connect onboarding waits for the SASU; the Uber agreement's entity questions become transfer-to-SASU questions in the counsel packet |


---

## 12. The batched send's signature — ✅ DECIDED 2026-08-02 (deferred)

[PROP-20260728-152752](PROP-20260728-152752-actor-mailbox-write-path.md) is `Approved`, but the
product owner raised ONE new decision while reaffirming §2.1 (typed actor clients — realization
tracked by [#284 "Typed actor clients (PROP-20260728-152752 §2.1): one generated client per actor"](https://github.com/TheCaptainCompany/captain-food/issues/284)):
what the TYPED form of the batched send looks like. The interim untyped
`enqueue_inbound_facts` ([#283 "batch the SIRENE drain"](https://github.com/TheCaptainCompany/captain-food/pull/283))
fixed a 6x producer bottleneck and must be absorbed, not kept. Blocks only `send_many` — the
singular typed client can land first.

| # | Decision | Recommendation | **Answer** |
|---|---|---|---|
| **D8** | `send_many` signature + compile-time checks | (a) Homogeneous generic batch | ⚠️ **Deferred — no `send_many` for now** (product owner, 2026-08-02). Build the client as §2.1 always specified it first — per-actor, `send` + `schedule`, compile-time checked — then discuss parallelisation separately. The #283 batching stays infrastructure-internal (`enqueue_inbound_facts`), outside the client's public surface, until that discussion |


---

## 13. Client isolation by crate — ✅ DECIDED 2026-08-02

[PROP-20260728-152752 D9](PROP-20260728-152752-actor-mailbox-write-path.md): make the typed-client
door COMPILER-enforced. Product owner: **Option B** — a dedicated `actor-client` crate between
`application` and `infrastructure` (private-field `MailboxEntry` + constructors + generated clients
in one crate, so bypassing the client does not compile), with **per-actor crates as the target
topology** ("improve the isolation with crates everywhere" — the C# assembly-per-client /
assembly-per-actor practice, crate as the boundary). Phased: one client crate first (the payoff),
per-actor client crates second, per-actor implementation crates gated separately. Tracked by
[#290 "Actor-client crate isolation (PROP-20260728-152752 D9): compiler-enforced door, then per-actor crates"](https://github.com/TheCaptainCompany/captain-food/issues/290).


---

## 14. Isolation by construction — PROP-20260802-130500 — ✅ DECIDED 2026-08-02

[PROP-20260802-130500](PROP-20260802-130500-isolation-by-construction.md)
([#290](https://github.com/TheCaptainCompany/captain-food/issues/290)). The product owner's threat
model, first-class: most code here is written by AI sessions, and a rule an agent can violate
silently is a review burden forever — so buy compile-time enforcement wherever it is for sale.
Measured finding behind it: the typed-client door is level-4 enforced while **nine crates hold
`sqlx`** and can bypass every door with one query. Scope (product-owner directive, 2026-08-02):
**"per actor" includes the process managers** — one crate per PM and one per PM client at every
phase, symmetric with aggregates (16 actors = 14 aggregates + `PlaceOrderProcess` +
`RefundProcess`). All six decisions answered:

| # | Decision | Answer |
|---|---|---|
| D1 | The client door becomes a crate | Dedicated `actor-client` crate (decided as PROP-20260728-152752 D9) |
| D2 | Per-actor IMPLEMENTATION crates (phase-3 endpoint) | (a) handler crates per actor — aggregates AND process managers; domain types stay one crate — ✅ as recommended |
| D3 | `Cargo.toml` as capability allowlist (cargo-deny: who may hold `sqlx`/`reqwest`) | Adopt in phase 1 — ✅ as recommended |
| D4 | The read door | One generic `ActorClient` with `get_operation_status(message_id)` — status is generic to all operations, so neither a per-actor client method nor a separate `OperationStatusClient` type; in the client crate, phase 1 |
| D5 | Cross-crate test fixtures | `test-fixtures` cargo feature + CI check that no release artifact enables it — ✅ as recommended |
| D6 | Lint floor | **Later, separately — against the recommendation**: lands as its own change after phase 1, tracked on #290's checklist |

---

## 15. Push-driven mailbox — PROP-20260802-223522 — ✅ DECIDED 2026-08-02

[PROP-20260802-223522](PROP-20260802-223522-push-driven-mailbox.md)
([#313 "Push-driven mailbox: pg_notify on inbound_messages, idle lane gate, poison policy (PROP-20260802-223522)"](https://github.com/TheCaptainCompany/captain-food/issues/313)).
Extends [#301](https://github.com/TheCaptainCompany/captain-food/pull/301)'s NOTIFY approach to the
last polling surface: the actor mailbox (post-audit: it out-polled what #301 removed, ~8×; interim
width-5 mitigation merged as ADR-20260802-220402). Also closes the up-to-10 s adapter→worker wake
gap on the money path, and bounds the silent infinite-retry poison mode found 2026-08-02.
**Approved as recommended, D1–D5** (product owner, in-session, 2026-08-02; ADR-20260802-224532);
unresolved questions live on the tracking issue's checklist.

| # | Decision | Recommended | Answer |
|---|---|---|---|
| D1 | Wake transport | `pg_notify` in the enqueue transaction (one door: `PgMailbox`) | ✅ as recommended (2026-08-02) |
| D2 | Channel topology | One channel, payload = `actor_type` (per-type coalescing) | ✅ as recommended (2026-08-02) |
| D3 | Idle gate | One lanes-with-work query per pass (partial index exists) | ✅ as recommended (2026-08-02) |
| D4 | Poison policy | `attempts` cap (default 5) → terminal `FAILED` + error on the row | ✅ as recommended (2026-08-02) |
| D5 | Gating | Own toggle (worker-toggle pattern) + `MAILBOX_MAX_DELIVERY_ATTEMPTS` (0 = today) | ✅ as recommended (2026-08-02) |

Unresolved questions — **all four decided 2026-08-03**
([ADR-20260803-002712](../adr/20260803-002712-mailbox-poison-follow-ups-decided.md)): admin
requeue mutation ([#315](https://github.com/TheCaptainCompany/captain-food/issues/315)) ·
exponential backoff ([#316](https://github.com/TheCaptainCompany/captain-food/issues/316)) ·
page on EVERY poison ([#317](https://github.com/TheCaptainCompany/captain-food/issues/317)) ·
fleets default-off until DB-persisted posture ([#318](https://github.com/TheCaptainCompany/captain-food/issues/318)).

---

## 16. Who owns the OVH host — PROP-20260805-181926 — ⚠️ SUPERSEDED 2026-08-08

> **The destination changed to Clever Cloud** ([ADR-20260806-151122](../adr/ADR-20260806-151122-hosting-destination-is-clever-cloud-not-ovh.md),
> product owner: *"Instead of OVH"*). A PaaS means **no host OS of ours**, so **D1–D6 below have no
> subject** — they stay as the costed record of the option space, not as decisions anyone owes an
> answer to. **Only D7 is still open**, in reduced form. **D3 (SaltStack) is settled by construction**:
> there is no machine for it to configure. The one live question moved to the ADR's follow-up —
> **whether Clever Cloud meters egress the way Render did**, which gates any spend, because egress
> exhaustion is one of the incidents that started this migration.
>
> **2026-08-08 — fully superseded**: D7, the one surviving question, is answered by
> [PROP-20260806-223656 D5](PROP-20260806-223656-kubernetes-as-the-deployment-substrate.md) /
> [ADR-20260807-002705](../adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md) (manifests
> generated from the specs, on OVH MKS — the destination moved again, from Clever Cloud to MKS,
> per §17). Nothing here remains open; recorded in the 2026-08-08 register sweep
> (ADR-20260808-171056). The proposal's Status is `Superseded`.

[PROP-20260805-181926](PROP-20260805-181926-host-provisioning-and-configuration-ownership.md)
([#349 "Who owns the OVH host: provisioning IaC + host configuration (SaltStack evaluated)"](https://github.com/TheCaptainCompany/captain-food/issues/349)).
Raised by the product owner as *"SaltStack seems to be an interesting solution"*. It is live because
the OVH cutover ([#271](https://github.com/TheCaptainCompany/captain-food/issues/271),
ADR-20260731-061609) gives us a **host OS of our own for the first time** — on Render nothing about
the machine was ours. Today no file says which OVH resources exist or what is installed on the box,
which is the Render-dashboard failure mode (`RUN_SIRENE_WORKER` set in no file, `API_SECRET` read by
no code) one layer deeper. The proposal splits the question into **provisioning** (what resources
exist — Salt does not address this at all) and **host configuration** (what runs on the box), and
notes that **application** configuration is already owned by `specs/configuration.yaml` and must stay
that way.

| # | Decision | Recommended | Answer |
|---|---|---|---|
| D1 | Layer A — provisioning | OpenTofu + the official `ovh/ovh` provider — the instance, network, firewall, managed PG plan and DNS become reviewed files | _(open)_ |
| D2 | Layer B — host configuration | cloud-init `user_data` from the repo (~80 lines, no agent, no daemon); **Ansible named as the escape hatch** at 3+ hosts. NixOS deferred on **bootstrap risk** (OVH has no first-class NixOS image) and D7 — no longer on authoring cost | _(open)_ |
| D3 | **SaltStack: adopt or reject** | **Reject** — its advantage needs ~1,000 nodes and we have one, it adds a listening root-equivalent control plane to the box terminating payment traffic, its pillars become a second config store, its convergence model contradicts the immutable-artifact doctrine, and its stewardship is consolidating into Broadcom's VMware suite. Revisit only for restaurant-side hardware fleets | _(open)_ |
| D4 | Host posture | Disposable — rebuild, never converge (affordable only because the managed PG is a separate resource, PROP-20260731-061609 D2) | _(open)_ |
| D5 | OpenTofu state | OVH Object Storage S3 backend + committed `.terraform.lock.hcl`; **never** the repo (public — it would leak the PG credential) | _(open)_ |
| D6 | Sequencing | cloud-init now, cut over, **then** `tofu import` the live resources — IaC must not block restoring production | _(open)_ |
| D7 | **Is host config generated from the DSL?** (product owner, 2026-08-05: *"based on the spec in YAML you can generate it… encapsulated in the codegen"*) | **Derive from the specs that ALREADY exist** — compose file, firewall ports and collector config from `configuration.yaml` / `observability.yaml` / `services.yaml` / C4 — **not** a new `specs/host.yaml`, which would be a single-target passthrough with none of the fan-out that earns the repo's other emitters. Target-independent, so it works for cloud-init now and NixOS later | _(open)_ |

Concern registered and unchecked, so this cannot be approved as-is: **cutover-not-blocked** — prod is
down today and nothing here may delay [#271](https://github.com/TheCaptainCompany/captain-food/issues/271).
D6 is the mechanism; checking the concern means confirming that ordering.

---

## 17. Kubernetes as the deployment substrate — PROP-20260806-223656 — ✅ DECIDED 2026-08-07

> **Fully approved** (product owner, D1–D7 across 2026-08-06/07, closed with *"D3 and D5 yes, start
> clean, move the NS to OVH"*), recorded by
> [ADR-20260807-002705](../adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md): **OVH MKS +
> in-cluster CNPG + GitOps-only operations + generated manifests + `Recreate` until #242 + straight
> to the cluster, starting from an EMPTY schema (no dump restore) + NS hosting → OVH DNS (Dynadot
> stays registrar)**. All four concerns checked. The rows below record the option space as decided.

[PROP-20260806-223656](PROP-20260806-223656-kubernetes-as-the-deployment-substrate.md)
([#271](https://github.com/TheCaptainCompany/captain-food/issues/271)). Reopens
[ADR-20260806-151122](../adr/ADR-20260806-151122-hosting-destination-is-clever-cloud-not-ovh.md)
(Clever Cloud), which is **no longer in force**, at the product owner's direction.

**Why**: that ADR's decisive argument was *"a team of one product owner plus agents should not be
operating a PostgreSQL server"* — a premise about the OPERATOR that was **wrong**. The product owner
has run Kubernetes professionally, so the heaviest weight in the decision was mis-specified. Three
further arguments were raised and none appeared in the ADR: **ingress as a light API gateway**
(wildcard TLS is needed on every destination anyway), **lock-in** (previously dismissed as "a
Dockerfile and env vars", which under-weighted Tasks/Cellar/add-ons compounding), and **manifests as a
codegen target** — a cluster can consume generated deployment descriptors, a PaaS cannot, which makes
this the best available home for PROP-20260805-181926's surviving D7.

| # | Decision | Recommended | Answer |
|---|---|---|---|
| D1 | Kubernetes, or the PaaS decided yesterday? | **OVH MKS** if k8s (free control plane, **free egress**, GA — vs CKE still in public beta); Clever Cloud PaaS retained as the costed fallback | ✅ **OVH MKS** (product owner, 2026-08-07: *"MKS of course"*) |
| D2 | **Where does PostgreSQL live?** — the hard one | Managed alongside the cluster was recommended; the option table also carries a vRack-instance shape and in-cluster CNPG | ✅ **In-cluster CNPG** (product owner, 2026-08-06: *"Postgres on Kubernetes"*) — with the operability conditions as part of the answer: ≥3 nodes, required anti-affinity, WAL archiving to object storage, scheduled executed restore drills |
| D3 | Deploy strategy while [#193](https://github.com/TheCaptainCompany/captain-food/issues/193) caps us at one instance | **`Recreate`** — a RollingUpdate runs two write paths at once, exactly what [#242](https://github.com/TheCaptainCompany/captain-food/issues/242)'s leases and fencing exist to prevent | ✅ As recommended (2026-08-07) |
| D4 | Ingress + wildcard TLS | ingress-nginx + cert-manager, DNS-01 for `*.captain.food` | ✅ **As recommended** (product owner, 2026-08-07: *"Ingress yes!"*) — with a zone-host correction: **DNS is at DYNADOT, which has NO cert-manager solver**. Sub-decision open: move zone hosting to OVH DNS (NS change only, Dynadot stays registrar — recommended), CNAME-delegate just the ACME challenge, or write a custom webhook |
| D5 | Manifests generated from the specs? | **Yes** — the strongest argument for a cluster, and PROP-20260805-181926 D7 with a target that fits | ✅ As recommended (2026-08-07) |
| D6 | Sequencing, with prod DOWN | Restore service on the simplest path first, build the cluster deliberately after — the digest-pinned image runs unchanged on either, so it is a redeploy, not a second migration | ✅ **Build the cluster now, cut over once** — AGAINST the recommendation (product owner, 2026-08-07: *"I don't care about prod on Render and Supabase, it was a crash test"*). Opens the data question: restore the dump into CNPG, or start clean? |
| D7 | How does the agent operate the cluster? | GitOps as the only change path + read-mostly RBAC + per-incident break-glass; PVC/StatefulSet/namespace deletes outside every standing role | ✅ **GitOps** (product owner, 2026-08-06: *"Of course gitops"* — diagnostics via cluster + Postgres read access, fixes as repo changes). Practices in the proposal's §2b |

All four concerns ✅ checked. D6's data question is answered — **start clean, no dump restore** — and
D4's zone-host sub-decision is answered — **NS hosting moves to OVH DNS**. Realization proceeds under
[#271](https://github.com/TheCaptainCompany/captain-food/issues/271) per the ADR's consequences.

---

## 18. One decomposition axis: spec folders, schemas, projectors — PROP-20260807-174246 — ✅ DECIDED 2026-08-07

> **Approved as recommended** (product owner, 2026-08-07 — D1–D8, with D2 and D8 in their revised
> forms; the critical-path-growth concern explicitly accepted), recorded by
> [ADR-20260807-183024](../adr/ADR-20260807-183024-one-decomposition-axis.md). The rows below stand
> as the record of the option space; every `_(open)_` cell reads **✅ as recommended (2026-08-07)**.
> Realization order is the ADR's consequences list; unresolved questions live on
> [#374](https://github.com/TheCaptainCompany/captain-food/issues/374)'s checklist.

[PROP-20260807-174246](PROP-20260807-174246-one-decomposition-axis-specs-schemas-projectors.md)
([#374](https://github.com/TheCaptainCompany/captain-food/issues/374)). Product-owner directive
(screaming architecture): spec folders per business domain + common, per-domain storage, per-domain
`configuration.yaml`, per-domain projectors, admin cross-scope queries preserved. Completes the
one-axis chain begun in §17: `specs/{scope}/` → `domain-{scope}` crate → `actor-{scope}` image →
`{scope}` schema → `projector-{scope}`.

| # | Decision | Recommended | Answer |
|---|---|---|---|
| D1 | Spec folders per scope + `common/` | Yes — with placement, cross-scope-DAG and kernel-purity validator rules | _(open)_ |
| D2 | **Storage level** | **REVISED after product-owner pushback** (*"I don't like too many responsibilities on one database"* — the integration-database antipattern): split by RESPONSIBILITY — **`captain-core`** (event log + mailbox only; all backup/PITR budget) and **`captain-views`** (per-scope schemas of projections; rebuildable by replay, EXCLUDED from backups) in the one CNPG cluster. No native cross-DB join is ever needed; cross-scope exposure via projections/GraphQL; per-scope lifts later are connection-string changes | _(open)_ |
| D3 | The event log | Stays SINGLE in a `core` schema — global ordering, PM causality, one PITR timeline, the GDPR erasure path | _(open)_ |
| D4 | Projectors | Per scope over the single log, independent checkpoints; admin/BAM are consumer schemas — scope views never join across schemas | _(open)_ |
| D5 | Configuration | Splits per scope + common; each bin's generated `Config` reads only its own keys | _(open)_ |
| D6 | Admin cross-scope queries | Via **projections + GraphQL composition** (the admin surface reads its own consumer schema); `admin_ro` cross-schema SQL demoted to INCIDENT tooling, never an application path | _(open)_ |
| D7 | Sequencing | Everything pre-cutover — **start-clean makes the storage split FREE** (schemas created, nothing migrated); this window does not recur | _(open)_ |
| D8 | GraphQL per domain (product owner: *"merge them in one graphql"* — closest name: **schema stitching**) | **REVISED after product-owner pushback** (an over-responsible graph = the integration-DB antipattern at the API layer): **`graphql-{scope}` services** (one domain, one graph, one GRANT) + a **thin generated gateway per role** — no DB access, no logic, top-level-field routing from a codegen-emitted composition table (static stitching, no query planner). Cheap here because CQRS denormalization puts composition in the PROJECTOR, so entity resolution/N+1 never arise; a validator rule keeps nested types intra-scope | _(open)_ |

Concern registered and unchecked: **critical-path-growth** — prod is down and this grows the
pre-cutover program again; approving accepts that explicitly.

---

## 19. Build in public — PROP-20260807-190936

[PROP-20260807-190936](PROP-20260807-190936-build-in-public-transparency.md)
([#377](https://github.com/TheCaptainCompany/captain-food/issues/377)). Product-owner directive:
platform transparency — *"Kubernetes completely open"* — for recruitment, press, marketing, branding.
**The line: transparency exposes INFORMATION, never CONTROL** — "Kubernetes open" is a generated,
sanitized public view OF the cluster (from the already-public GitOps state), never network reach INTO
it. Most of L1 already exists as a side effect of the operating model (public repo, ADRs, the git
deploy ledger, public CI). Decisions D1–D4 open (levels · initial aggregate-only metric set · L4 as a
static generated page · L2–L4 after cutover); concerns **pii-and-gdpr** and **attack-surface**
registered. Related, no decision needed: [#378](https://github.com/TheCaptainCompany/captain-food/issues/378)
emits JSON Schemas FROM the validator model (generated, never hand-written) for authoring-time
feedback — the validator stays the semantic authority (`REF_CONTRACT` already gates
$ref-kind-appropriateness).

**2026-08-08** (ADR-20260808-171056; veto open): **D2 decided** — platform-wide aggregates ONLY, no
per-restaurant/per-postcode/per-rider dimension ever without consent (sole-trader metrics are
personal data; a partner's published volume is an adoption killer), k ≥ 10 per cell when slicing
ever starts. **D3 decided** — static generated status page, cure folded: the page renders its own
generation timestamp and goes visibly stale (a frozen "all green" during the outage that killed its
publisher is worse than no page). **D4 decided** — L2–L4 after cutover. **D1 ✅ decided by the customer 2026-08-08, DIFFERENT choice**
([ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md)): **radical
transparency** — public accounting on Open Collective, public Kubernetes/technical usage, public
incidents + postmortems on GitHub, public status page. The D2 aggregates-only and
information-never-control guardrails stand and compose.

---

## 20. The rider/delivery write surface — PROP-20260808-141817 — ✅ FULLY DECIDED 2026-08-08

[PROP-20260808-141817](PROP-20260808-141817-rider-delivery-write-surface.md)
([#348 "Epic: the rider/delivery write surface does not exist"](https://github.com/TheCaptainCompany/captain-food/issues/348)).
**`Approved` 2026-08-08 — fully decided, all six.** **D1, D2, D4, D6 decided by ensemble consent** —
[ADR-20260808-155656](../adr/ADR-20260808-155656-first-consent-based-ensemble-decisions.md),
customer veto window open. **D5 decided by the customer, 2026-08-08: as recommended**
(`PlaceOrder` payload flag + PM step). **D3 DECIDED by the architect lens, 2026-08-08** (customer
delegation — "if we start with specialisation we finish with specialisation"): rename now, while
zero events are emitted — event `DeliveryAssignmentReleased`, command `ReleaseDeliveryAssignment`,
mutation `releaseDeliveryAssignment` (the verdict OVERRIDES the proposal's `unassignDelivery`
mutation name: an `unassign`-named mutation over a release-named event reintroduces the second
vocabulary the rename kills). Actor-neutral by design — manual board action, future rider
self-release and the PROP-172500 stall sweep share ONE fact, releaser on the envelope (ADR-0041);
the ASSIGNED→PENDING-only scope is part of the name's meaning (PICKED_UP is a different journey).
Proposal-text reconciliation to `releaseDeliveryAssignment` rides the next docs batch. Derives the four delivery persona journeys and answers the
epic's vocabulary question (the wired offer/accept vocabulary is canonical); decomposes into 8 V0
slices (+3 V1). Absorbs the rider-write-surface half of PROP-20260726-172500 (whose D1/D2/D3/D4/D5
rows above remain that proposal's). **Both Concerns are checked**: the D3 rename (resolved by the
architect verdict above) and the slice-2 validator-credit semantics (SATISFIED by the D6 decision —
a declared `sends:` is checkable both ways: the ref resolves AND the target inbox accepts; never
an annotation alone). Realization via the 8 V0 slices remains plan-mode/backlog work.

| # | Decision | Recommendation |
|---|---|---|
| D1 | `AssignDeliveryToPartner` family: retire vs keep for manual dispatch | **Retire** — no journey pushes a job at a partner; an assignment no courier agreed to carry is the oversell failure mode as an event type |
| D2 | `UpdateDeliveryPartnerStatus`: retire vs keep as a command-wrapped fact | **Retire** — a command wrapping an external fact (ADR-0004); the ACL already records it as inbound `DeliveryStatusUpdated` |
| D3 | `Unassign…` naming: keep as-is vs generalize to `DeliveryAssignmentReleased` | **Generalize/rename** — one release step for both courier kinds; cheapest now, before production events exist (held as an unchecked Concern; decide before slice 6) |
| D4 | Issue model | **One open issue per job** (V0) — the honest model for `issueId`-less commands; history stays in the log |
| D5 | `ConsumeCustomerCredit` shape | **`PlaceOrder` payload flag + PM step** — consume atomic with payment (ADR-20260726-163737 §checkout-consume) |
| D6 | How does `PlaceReplacementOrder` get spec-checkable dispatch coverage? (no PM step sends it — wrapper-seam dispatch) | **A declared `sends:` on the wrapper-seam receive** — parallel to the existing declared `emits:` precedent (`ordering/processmanager.yaml:194-199`), checkable both ways; alternatives: extend the step DSL (bigger), or leave it in the warning baseline (erodes the diff discipline) |

---

## 21. Disappearance is a designed state — PROP-20260808-142532 — ✅ FULLY DECIDED 2026-08-08

[PROP-20260808-142532](PROP-20260808-142532-disappearance-terminal-states.md)
([#398 "Decide the API contract for tombstoned rows before the #194 projection sweep"](https://github.com/TheCaptainCompany/captain-food/issues/398)
+ [#347 "Decide the last annotated read-model hole: Restaurant fed by RestaurantListingOptedOut"](https://github.com/TheCaptainCompany/captain-food/issues/347)).
**`Approved` 2026-08-08 — fully decided, all five.** **D1 and D5 decided by ensemble consent** —
[ADR-20260808-155656](../adr/ADR-20260808-155656-first-consent-based-ensemble-decisions.md),
customer veto window open. **D2 DECIDED by the customer, 2026-08-08: yes — and WIDENED into a
principle**: the order copies ALL context needed to autonomously build the customer invoice
(customer directive: "the order must copy all information about the context of the order to be
autonomous to build invoice to be sent/displayed to the customer"). Restaurant name/phone are the
floor, not the scope — the frozen checkout snapshot must carry the full invoicing context
(restaurant legal identity incl. invoicing fields, per-line VAT context per the split French
rates, fees, totals); the exact field inventory is enumerated in plan mode at realization, and the
compliant-receipt legal precondition (CLAUDE.md) now binds the snapshot design. **D3 DECIDED by
the customer, 2026-08-08: FOLD to a hidden listing status** — grounded by the legal-specialist's
obligation brief ([docs/legal/BRIEF-20260808-listing-opt-out-objections.md](../legal/BRIEF-20260808-listing-opt-out-objections.md),
exposures in [#401](https://github.com/TheCaptainCompany/captain-food/issues/401)): the fold's
suppression-list shape is legally REQUIRED. **D4 DECIDED by the customer, 2026-08-08: the
ORTHOGONAL `delisted` BOOLEAN** (only opt-out sets it; only the proven re-claim path clears it) —
the brief's audit-defensibility asymmetry tips it (a bypassed guard clears the objection
irreversibly; a forgotten filter is recoverable with the refusal intact), and the founder's
Google-parity directive
([#402](https://github.com/TheCaptainCompany/captain-food/issues/402)) independently requires the
same orthogonality. One
principle, two faces: disappearance is always a designed
state; physical row removal is reserved for legal erasure. **All three Concerns are checked** (see
the proposal header): D2's THREE artifacts (`OrderPlaced` + `CheckoutSnapshot`/`PaymentIntentCreated`
+ the replacement-order emitter) got their event sign-off via the customer's widened D2 decision;
the resolver-policy change stands as a standing realization constraint — emitter-landed, the
`Option<_>` type flip + one shared hydration helper, never a source-text scanner; and under D4 the
`OPTED_OUT` enum value never exists, so the second-door guard is structurally unnecessary — the
remaining `OptOutRestaurantListing` ACTIVE_PARTNER guard error keeps its ADR-0032 completeness
duty (behaviour test + `rules:` link) at realization.

| # | Decision | Recommendation |
|---|---|---|
| D1 | API contract for dangling/tombstoned references | **The scoped mix** — projector-/event-carried composition for money-history surfaces + a thin pinned dangling policy (silent drop and join hard-errors banned) |
| D2 | `OrderTracking` restaurant name/phone | **Event-carried on `OrderPlaced`** — survives projection rebuild after restaurant stream deletion; three artifacts, per the header Concern |
| D3 | [#347](https://github.com/TheCaptainCompany/captain-food/issues/347): tombstone vs `listing_status` fold vs vestigial removal | **Fold to a new `OPTED_OUT` value** — a tombstone is self-defeating under SIRENE re-import; also closes the live cold-email exposure (`ProspectionPipeline` does not fold the opt-out today) |
| D4 | `OPTED_OUT` shape | **Enum value + BOTH write-side guards** (`OptOutRestaurantListing` rejected for ACTIVE_PARTNER, AND `ChangeRestaurantListingStatus` rejecting `OPTED_OUT` as source and target — the guard closes two doors, not one); the orthogonal `delisted` boolean is materially strengthened by the two-door finding and stands ready if the PO prefers unspellable over guarded |
| D5 | Erased-restaurant storefront host | **Parked "closed" page** — never the claim-landing fall-through (invites resurrection of a dead business's address), better than a bare 404 |

---

## 22. New rows from the 2026-08-08 sweep

Surfaced by the five-lens register sweep
([ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md)) — added
here instead of being improvised at realization.

| Decision | Question | Status / owner |
|---|---|---|
| **Consumer-mediator registration** | France mandates médiation de la consommation registration before trading with consumers — a **launch precondition** that sat on no register row until now | ⏸️ **DEFERRED to first real order** (product owner, 2026-08-10) — the PO chose to register at the first real consumer order rather than now, **against the team's "start now" recommendation**. Recorded as the PO's decision. Still a tracked launch precondition (must complete before the first real consumer order clears); pairs with the entity/counsel packet |
| **Identity-bridge home** | Where the role↔domain-id bridge lives: JWT claims for all roles vs common-schema bridge tables — must NOT invent a third mechanism beside `Actor.domain_id` | ✅ **DECIDED by the customer 2026-08-09** ([ADR-20260809-050000](../adr/ADR-20260809-050000-morning-brief-eight-decisions.md) CARD-11): **JWT claims**, per-person accounts for every rider and every restaurant staff member; unblocks [#415 "Rider identity: View_Rider, register/update/profile surface, onboarding screens (#348 slice 3)"](https://github.com/TheCaptainCompany/captain-food/issues/415). The #144 port honoured it: no Rider bridge table landed ([ADR-20260809-160000](../adr/ADR-20260809-160000-read-authorization-lands-ported-from-152.md)) |
| **PROP-185140 §6.4 claim staleness** | How long a scope claim may be trusted before re-derivation — the one real policy question the authorization set leaves open | ✅ **CLOSED 2026-08-10** ([ADR-20260810-194548](../adr/ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md)). The PO sent it back — *"The legal should have an answer and the business expert should know what competitors is doing so I'm surprised there no recommendation"* — and both lenses converged. **Keep the ~1h Supabase default** (the window exists whether or not we decide: Supabase mints 1h access tokens with claims stamped at mint), **make revocation explicit and immediate** for rider deactivation and staff removal, and make [#194](https://github.com/TheCaptainCompany/captain-food/issues/194) erasure scrub `app_metadata` **and** revoke refresh tokens in the same act. Legal's frame: TTL is not the legal object — Art. 32(1)(d) testing + Art. 5(2) accountability, Art. 12(3)'s one-month bound; **riders are a separate regime** (Platform Work Directive (EU) 2024/2831 Arts. 7–11, transposition ~Dec 2026 — **VERIFY-FIRST**): explicit revocation with a reason code, a log and human review, never TTL drift. Access logs 6–12 months (CNIL délib. 2021-122) and owed an Art. 30 entry. Business: the split is **device vs person**, not role vs role, and churn asymmetry decides it — a forced re-auth on the acceptance terminal at 19:45 Friday blocks the only surface that accepts orders. **Do not reopen this on the storage note**: the product owner's *"We will not use Supabase for the business data / Supabase will be used for identify / Postgres will be in Kubernetes on OVH"* confirms the exact split the closure rests on — business data is CNPG-in-cluster (ADR-20260807-002705), and the 1h-token fact is about IDENTITY, which stays Supabase. The reasoning is untouched |
| **`from:` naming collision** | `from:` is about to mean two things — the screens input-source key (§1 F) and api.yaml scope-binding; rename one before both DSLs ship the key | ✅ **DECIDED 2026-08-10 (second answer sheet) — "Different choice", note: "A"**: the product owner picked **(a), rename the SCREENS input-source key**, and did NOT delegate the pick ([ADR-20260810-194548](../adr/ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md) §Revision). (An earlier sheet had answered "Approve as recommended" = (c) team picks; the second sheet supersedes it.) api.yaml keeps `from:` for scope-binding. **Still owed: the rename itself**, which must land BEFORE both DSLs ship the key — after that it stops being a rename and becomes a migration. Tracked by [#476 "Rename the screens input-source key"](https://github.com/TheCaptainCompany/captain-food/issues/476) |
| **Business-signal observability contracts** | Every "revisit with production data" clause (funnel conversion, cohort repeat rates, rider decline/utilization, baskets, notification-acknowledgement latency) has NO observability contract | ✅ **CLOSED 2026-08-10 by SUBSUMPTION into §27** — this row named the gap and pointed at [#400](https://github.com/TheCaptainCompany/captain-food/issues/400); [PROP-20260810-234225](PROP-20260810-234225-business-metrics-for-every-persona.md) is the mechanism it was waiting for (a `specs/business_metrics.yaml` catalog keyed persona × activity, bidirectional coverage rules, generated instruments). Not answered — replaced by a design with a tracking issue, [#484](https://github.com/TheCaptainCompany/captain-food/issues/484) |
| **Rebrand Captain → Solida** | Class-42 trademark opposition on "Solida" — external, only the customer/opposer resolves it; rename sweep pre-scoped in [#411 "Rebrand Captain → Solida (solida.food): rename sweep, BLOCKED on class-42 trademark confirmation"](https://github.com/TheCaptainCompany/captain-food/issues/411) | Waiting on external — **customer** ([ADR-20260808-212741](../adr/ADR-20260808-212741-solida-studio-strategic-frame.md) §4). **2026-08-10 — still PENDING**: the PO confirms the class-42 trademark is unresolved and **no company/entity name is chosen yet**, so [#411](https://github.com/TheCaptainCompany/captain-food/issues/411) stays blocked. "No entity name yet" **also gates the entity-path/rebrand work** (SASU naming per [ADR-20260808-195315](../adr/ADR-20260808-195315-customer-brief-answers.md) ch. 4 — brand and entity land together) |
| **avelo37 partnership threshold** | At what orders-per-week does the avelo37 partnership conversation start — a number to set from real order data, not a guess | Open — needs the #400 order-volume contract; decision deferred by design ([ADR-20260808-212741](../adr/ADR-20260808-212741-solida-studio-strategic-frame.md) §1) |
| **D6 endpoint** (final-vision audit A3) | Whether the declared `sends:` is the final mechanism or staging toward an expressible step DSL | ✅ **DECIDED by the customer 2026-08-09** ([ADR-20260809-002500](../adr/ADR-20260809-002500-quick-wins-approved-d6-dsl-extension-chosen.md)): option (iii) — **build the step-DSL conditional-branching extension now**; the wrapper seam retires and `sends:` is NOT implemented. Design first: the architect prepares the DSL-extension proposal as the discussion surface |
| **Geocoding vs postal-code zones** (final-vision audit A6) | PROP-172500 D1 recorded "postal-code sets now, geocoding next — sequence it deliberately"; zones may BE the Tours final (river-crossing note) or geocoding needs an owner ("geocoding unlocks distance fees and honest ETAs — and the ETA is the product") | **Open — now TEAM-OWNED** (PO 2026-08-10, "Approve as recommended" on recommendation (c): team first, bring a proposal). No longer an unowned row waiting on a product-owner answer it never needed — the team owns the analysis and returns with a proposal |

---

## 23. Process-manager step-DSL conditional branching — PROP-20260809-003000 — ✅ FULLY DECIDED 2026-08-09

> **DECIDED 2026-08-09 (product owner, answer sheet):** *"Confirm all seven as recommended."*
> D1–D7 stand as proposed; the proposal moves to `Approved`.
> Record: [ADR-20260809-050000](../adr/ADR-20260809-050000-morning-brief-eight-decisions.md).


Seven decisions from [PROP-20260809-003000 "Conditional branching in the process-manager step DSL:
the saga branch becomes spec, not wrapper"](PROP-20260809-003000-process-manager-step-dsl-conditional-branching.md)
(tracking [#426 "Conditional branching in the process-manager step DSL: the saga branch becomes spec, not wrapper"](https://github.com/TheCaptainCompany/captain-food/issues/426)),
the design the customer ordered in place of the declared `sends:` (card 10,
[ADR-20260809-002500](../adr/ADR-20260809-002500-quick-wins-approved-d6-dsl-extension-chosen.md)).
All seven are OPEN and gate slice 1.

| # | Decision | Recommendation |
|---|---|---|
| **D1** | The branching construct's shape | `match:` on an enum discriminant (over `when:` arms or per-step `when:`) — the only shape where "is every case handled?" is machine-answered, and answered TWICE (validator, then `rustc` on the arm-complete emitted match) |
| **D2** | Is a `default:`/catch-all allowed? | **No** — every enum member gets an arm; an intentionally empty arm carries a `note:`. A catch-all is how a new member silently does nothing |
| **D3** | Where the REFUND arms live | Move them to `RefundProcess`, which receives `ReclamationResolved` directly — retires the cross-saga call on a **synthesized, never-recorded** `RefundRequested` |
| **D4** | How a computed discriminant is declared | A typed `from_resolver` returning a DECLARED enum — never a raw value an effect consumes |
| **D5** | Nullable discriminants | `present:`/`absent:` conditions (also deletes a generated panic path) |
| **D6** | Sharing steps between arms | Accept duplication in v1 — no aliasing mechanism until a second real case asks for one |
| **D7** | Deterministic derived ids | A `derived_id:` value form — **slice 1 cannot retire the wrapper without it** |

Related finding, deliberately NOT folded in: `call:` has no `with:`, so the Stripe refund **amount**
is entirely hook-built and stays invisible to the validator even after all six slices — its own
proposal, named in §2.1/§9 of PROP-20260809-003000.

---

## 24. The public demo — PROP-20260809-021351 — ⏸️ DEFERRED 2026-08-09

> **DEFERRED 2026-08-09 (product owner, answer sheet).** The demo is not next; its
> production-critical remainder is **re-filed on its own** rather than shipped under a marketing
> epic — the outcome two lenses independently recommended. The three customer-owned rows were
> answered on the way out, so the design is complete when it returns:
>
> - **D1 → (c) nothing hosted yet.** *"Same production environment with test data in it for testing
>   production on production with test data."* One environment, so the D1⊕D2 contradiction (two
>   namespaces over one database, with a checkpoint-overwriting projector and a true accumulator)
>   never arises.
> - **D3 → (a) pre-identified demo session**, no SMS OTP. Still blocked by the unscoped order reads
>   on [#144](https://github.com/TheCaptainCompany/captain-food/issues/144), which is a live defect,
>   not a decision.
> - **D4 → (b) one deployment, Stripe keys chosen per order mode.** Safe while everything is test
>   mode; **due a type-level form before any live key exists** (compiler-first,
>   [ADR-20260803-234035](../adr/ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)).
>
> **D2, D5 and D6 lapse with the deferral** — except D2's substance (`mode` carried onto the
> projection tables + a validator rule), which is production correctness and travels with the
> re-filed work.
>
> The target that replaces this epic: **test customers placing test orders against test restaurants
> with Stripe test payments, on the production deployment** — [#429 "Production with test data: a test customer places a real order against a test restaurant, paid with Stripe test mode"](https://github.com/TheCaptainCompany/captain-food/issues/429).
> Record: [ADR-20260809-050000](../adr/ADR-20260809-050000-morning-brief-eight-decisions.md).


Six decisions from [PROP-20260809-021351 "The public demo: one continuous walk, on production's own
pipeline"](PROP-20260809-021351-public-demo-one-continuous-walk.md) (tracking
[#410 "Epic: public try-before-committing demo — seeded test restaurant/customer/order/rider on the marketing site"](https://github.com/TheCaptainCompany/captain-food/issues/410)),
from the four-lens mob briefing of 2026-08-09 (farley lead · ux-designer · beck · dba).
**D1, D3 and D4 are the customer's** — D1 would reverse a recorded decision or spend console time,
D3 and D4 sit on the money path and the abuse surface. D2, D5 and D6 are team-decidable and are
recorded as recommendations pending the veto window.

| # | Decision | Recommendation | Owner |
|---|---|---|---|
| **D1** | Where the demo runs | **MKS demo namespace, same digest and manifests, `staging` profile** — no spec diff, and the demo namespace becomes the canary every production digest passes through. Cost: ~75–100 customer console-minutes across ≥2 sittings, no URL this week. Resuming Render buys a same-day URL by reversing ADR-20260731-061609 and applying 15 pending migrations to a database nobody intends to keep | **customer** |
| **D2** | Demo data isolation | TEST-mode data in the production database + `mode` carried onto the `Restaurant`/`OrderTracking` projection tables + a validator rule that any projection table fed by a mode-carrying event must carry the column. Today `mode` is enforced in ONE runtime location and NO read model carries it | team |
| **D3** | How a stranger is identified | `startDemo` mints a pre-identified demo session — real SMS OTP costs money, exposes an unauthenticated SMS-send surface on a public page, and dead-ends at 503 if the hook is unconfigured | **customer** |
| **D4** | Stripe mode | Demo namespace carries `sk_test_`, production carries the live key, same image. **One deployment is one Stripe mode today**: a live key means the demo charges strangers' cards; a test key means production cannot take money | **customer** |
| **D5** | Demo world lifetime | Fresh streams per visitor, never a reset — the Order projector is ONE checkpoint over `Order-`/`Payment-`/`DeliveryJob-`, so "replay the demo" resets every real customer's tracking screen. Reclamation is retention, and the `$maxAge` sweeper **does not exist**: demo data is unbounded (~375 MB/month at 500 runs/day) | team |
| **D6** | Who drives the counterparties | The visitor wears all three hats in one walk, with labelled auto-accept as fallback | team |

**Six more lenses were invited on the committed proposal the same night** (legal, business,
graphql-architect, holub, observability, architect — the first briefing had four, chosen by
coordinator taste, which the mob ADR bans; recorded as its first measurement). They did not refine
the design, they **contested its place in the queue** — see §10 of the proposal. Consequences for
this register:

- **D1 and D2 are not independent** and must be answered together: two namespaces sharing one
  database is not a supported configuration of this codebase (the projector takes no lock and
  overwrites its checkpoint unconditionally; one projector is a true accumulator, so a re-fold
  doubles a customer's credit balance).
- **D3 is blocked** by a live read-authorization hole, not by anything in this proposal — recorded
  with evidence on [#144 "Read-side per-instance authorization"](https://github.com/TheCaptainCompany/captain-food/issues/144).
- **Two lenses independently say the demo should not be next**, and that the ~80% of its work which
  is production-critical should be re-filed out from under a marketing epic. That re-filing is the
  customer's, not the team's.

**Not decisions — live defects the briefing surfaced**, none blocked on any row above: the customer
path is inert on `main`; nobody is told about a paid order (no notification port — though the OVH
SMS adapter already exists with only the auth hook calling it); the cart's total and the competitor
comparison **never compute**; `orders`/`order`/`carts` apply no ownership filter; and
`orders_placed_total` — the metric that says a stranger paid us — has zero emission sites, so the
alert that would have caught the inert checkout could never have fired.

---

## 25. New rows from the 2026-08-10 #451 keystone adjudication

Surfaced by the architect's adjudication of the ten-lens mob output on
[#451 "cart.current returns the authenticated customer's priced cart"](https://github.com/TheCaptainCompany/captain-food/issues/451)
/ [PR #460](https://github.com/TheCaptainCompany/captain-food/pull/460). Everything else that
adjudication produced was either dispatched to the executor or filed as an issue; **these three are
the only genuine product-owner decisions in it** — two because they need a `specs/**` edit (frozen
for execution loops, CLAUDE.md), one because only the product owner owns issue titles and scope.
⚠️ **The `specs/**`-freeze half of that reasoning is HISTORICAL** as of 2026-08-10 night
([ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)):
451-A and 451-B were rows because the DSL was untouchable, and it no longer is. Both were already
answered, so nothing changes for them — but **do not use this preamble as precedent** for filing a
future row on "it needs a spec edit". That is no longer a reason. (Row ids are `451-A/B/C`; the
adjudication that produced them numbered the same three E1/E3/E2 respectively — the ids here are
the stable ones.)

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **451-A** | **The cart screen's summary bindings** — `specs/screens/restaurant_frontoffice.yaml:367-371` binds `cart.subtotal`, `cart.deliveryFee`, `cart.serviceFee`, `cart.discount`, `cart.total`, `cart.minimumOrderMet`; the `Cart` API type has `totalAmount` + `breakdown.{articles,delivery,serviceFee,total}` (`specs/ordering/api.yaml:25-26`) and **not one of those six names exists**. So #451 computes the price correctly and the customer cannot see it. Tracked by [#468 "The cart screen cannot render a price"](https://github.com/TheCaptainCompany/captain-food/issues/468) | **(a) Merge #451 as scoped, file the frontend slice.** The seam, the money-free fold, the migration and the `cart-price` contract are real, tested, load-bearing artifacts the frontend slice depends on; the cost is that the delivered value stays invisible until the next slice. **(b) Grow #451 to include rendering.** Not autonomously dispatchable — the binding fix is a `specs/**` edit needing plan mode + approval — and it doubles a diff that already carries a schema migration. Neither option is cheap-and-dirty; (b) is *slower*, not more final-vision, because the final shape of the binding fix is a spec change either way | ✅ **CLOSED 2026-08-10 by the [#460](https://github.com/TheCaptainCompany/captain-food/pull/460) merge.** Standing position **(a)** held, as recommended by the architect and adopted by the coordinator, **with a hard condition**: PR #460's body must state plainly that the price is computed correctly and **cannot yet be displayed**. The reversibility window has now closed: **(b) is off the table** and [#468](https://github.com/TheCaptainCompany/captain-food/issues/468) is simply the next slice |
| **451-B** | **The `currency_mismatch` reason** in the `cart-price` contract's canonical reason set (`specs/observability.yaml:271-273`) — a currency clash is folded into `PriceUnresolvable` at `crates/application/src/pricing.rs:44` and then labelled `offer_gone` at `crates/server/src/graphql/cart_read.rs:136`, so an on-call responder is sent to the catalog for a **monetary** defect | **(a) Add `currency_mismatch` to the canonical set** — one line of `specs/observability.yaml`, needs a spec window; the reason set is what an alert routes on, so a wrong label is a wrong page at 20:00 on a Friday. **(b) Leave it folded** — cheaper, but the contract keeps a documented lie and every future currency defect mis-routes. There is no third option: the set is closed by design (that is what makes it alertable) | ✅ **APPROVED 2026-08-10 — (a), verbatim "Approve as recommended"** ([ADR-20260810-194548](../adr/ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md)). **The spec window is OPEN**: the next session lands it under this recorded approval without re-asking — one line in `specs/observability.yaml:271-273` adding `currency_mismatch`, plus the label selection at `cart_read.rs:136`. Was recommended as: Not urgent — EUR-only in V0 makes the clash currently unreachable in practice — but it is a *contract honesty* item, and those compound. Interim, already dispatched to the executor: make the misleading comment at `cart_read.rs:133-136` state the mis-label instead of claiming coverage. Owner: product owner (spec window) |
| **451-C** | **Does [#451](https://github.com/TheCaptainCompany/captain-food/issues/451) keep its title?** It reads *"cart.current returns the **authenticated customer's** priced cart"*, but the claim leg it names **cannot fire on the surface that calls it**: `crates/server/src/auth.rs:195-197` returns `Principal::anonymous()` on `/public` without reading the `captain_auth` cookie, and the storefront is pinned to `Role::Public` (`crates/web/src/router.rs:57`). What ships is the session leg plus the seam. Tracked by [#469 "`current` leg 1 is dead on the web AND is not tenant-scoped"](https://github.com/TheCaptainCompany/captain-food/issues/469) | **(a) Retitle #451** to what it delivers (the priced cart read seam, session leg live), leaving the authenticated leg to #469. Honest record; costs a title edit and a body note. **(b) Keep the title and grow the scope** until leg 1 works — but leg 1 without Host-scoping ships a live cross-tenant cart, so this pulls #469's *whole* pair into #451. **(c) Keep the title as aspirational** — rejected on principle: an issue title that describes something the merge does not do is the same class of defect as a doc comment claiming enforcement that is not there | ✅ **DONE 2026-08-10 — (a), verbatim "Approve as recommended"**, and already executed: the issue now reads *"cart.current returns the session's priced cart: the read seam + money-free Cart fold (#429 keystone; authenticated leg deferred to #469)"*. The title now describes what merged |

**Deliberately NOT given a row**: the #469 fix itself (public path reads `captain_auth`, leg 1 is
Host-scoped). It is code-only, GREEN, and has one recommended shape with no genuine arbitration —
per this file's own rule, *if a decision is not here, it is not blocking anything*, and padding the
register lowers the odds the real rows get read. It is an issue, not a decision.

---

## 26. New rows from the lifted `specs/**` freeze — 2026-08-10

The delegation itself is **decided** and recorded in §5 / [ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md).
These two are what it leaves open. Neither blocks any work.

| # | Decision | Options & the trade-off | Recommendation / status |
|---|---|---|---|
| **SPEC-1** | **The shape of the reporting gate** that discharges *"Just keep me informed"*. The obligation is real and has no mechanism; an obligation with no mechanism decays. [docs/SPEC-LOG.md](../SPEC-LOG.md) exists as of today, but nothing yet **keeps** it current | **(a) Generated `specs/CHANGELOG.md` from the spec diff.** Mechanical, unforgettable — but a diff summary is still a diff (*"field `x` added to `Y`"*), which is exactly the thing that cannot be read. Answers "what changed", never "what do we now promise". **(b) A validator-enforced "spec delta" section in every PR body.** Zero new cadence — but GitHub is never the record (CLAUDE.md), it fragments across dozens of PRs, and the product owner asked for one place. **(c) A hand-maintained `docs/SPEC-LOG.md`, no gate.** Cheapest to start; decays within a fortnight, like every prose obligation this repo has recorded. **(d) HYBRID — the page, plus a gate**: if a commit range touches `specs/**` and `docs/SPEC-LOG.md` is unchanged, fail. The executor writes ONE sentence in product language; the mechanical half (touched kinds, `specs/common/` fan-out count, `make validate` delta) is computed. Costs one small check in `make rust`/the Stop hook | ✅ **Recommended: (d)** — it is the only option that is both *readable by a non-engineer* and *cannot be forgotten*, and CLAUDE.md's own rule (*prefer executable over prose*, `makefile_recipe_lines_are_ascii` as the model) points at it directly. **Deliberately no cadence**: no weekly digest, no report to send. A pull surface kept current by a gate beats a push ritual nobody runs — that is the design judgement, and it is why this should not become the fifth abandoned process. **The tier column is the boundary's tripwire**: an executor about to write "this reverses a recorded decision" has, by writing it, discovered the change is not theirs. Answering this is ~30 seconds; until then the page is prose and exactly as reliable as CLAUDE.md says prose is |
| **SPEC-2** | **Confirm or reverse the `from:` rename** ([#476](https://github.com/TheCaptainCompany/captain-food/issues/476)). The product owner picked **(a) rename the SCREENS key**, api.yaml keeps `from:` (second answer sheet, 2026-08-10), and did not delegate the pick. **New measured evidence he did not have**: `from:` already appears **178 times** in `specs/`, and in every one of them it means *"the source that populates this value"* — projection-column lineage (`specs/database/tables/projection_tables.yaml`), actor-state lineage (`actors.yaml`), and process-manager step property extraction (`tools/codegen-rs/src/emit/pm_orchestrators.rs:10`). The screens input-source key means **exactly that same thing**; api.yaml's scope-binding means something different (which principal field scopes the query) | **This is an inform-and-confirm, not a re-litigation** — a recorded decision is not the team's to reverse (the boundary's own test 1), so #476 will execute **(a) as decided** unless this row is answered. But the evidence inverts the argument: option (a) renames the key that is *semantically consistent* with all 178 existing uses and lets the *divergent* one keep the name — an Evans ubiquitous-language finding, cited as such. **The reversal is free right now**: verified 2026-08-10, `from:` has **0 occurrences in `specs/screens/**` and 0 in `specs/*/api.yaml`** — neither key has shipped, so either direction is a pre-realization rename. Once either ships it becomes a migration. ✅ **Recommended: reverse to (b) — rename the api.yaml scope-binding key instead.** If you prefer to stand by (a), say so and it lands unchanged; both are cheap, but only today |

---

## 27. Business metrics for every feature and every persona — PROP-20260810-234225

Product-owner directive, 2026-08-10: *"Follow Jeff Patton about the business metrics during the
analysis must be developed with the test and the code, we must have business metrics for all
features for each persona. It's the only way that will allow us to know the usage of the product."*
— *"I let you define and implement them all the business metrics for every features for every
persona."* The principle is recorded and **not open**:
[ADR-20260810-234225](../adr/ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md)
— **superseded in part 2026-08-11** by [ADR-20260811-014129](../adr/ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md):
its clauses 1–3 stand, its clause 4 and enforcement table are reversed (§27bis MET-R). Read the D4/D6
rows below alongside that reversal — they record what was recommended on 2026-08-10, not what is now
decided.

**This block SUBSUMES the §22 row "Business-signal observability contracts"** — that row named the
gap and pointed at [#400](https://github.com/TheCaptainCompany/captain-food/issues/400); this is the
mechanism it was waiting for. §22's row is closed by subsumption, not by an answer.

**D1–D7 are TEAM-OWNED under the 2026-08-10 delegation** (*"I let you define and implement them
all"*, plus [ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)
and [ADR-20260810-215503](../adr/ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md)).
They are listed here for visibility and for the ensemble-consent + veto-window pattern of
[ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md) — **they are
not counted in the product-owner-owed total**, because nobody outside the team owes an answer to
them. **Exactly one row below is genuinely product-owner-owed: Q7.**

The fact that earns the whole block, verified on `168fd77`: **`specs/observability.yaml` declares 29
`business_metrics`; 26 of them have zero occurrences anywhere in `crates/`, `tools/` or `deploy/`.**
Three are emitted. The gate that should have noticed covers 3 of 14 contracts and only checks that a
string constant exists.

| # | Decision | Recommendation |
|---|---|---|
| **D1** | Where a business metric is declared: a new root catalog vs extending `observability.yaml` vs inline in `stories.yaml` | **New root `specs/business_metrics.yaml`**; the 29 entries move; `observability.yaml` keeps only technical `metrics`. Extending `observability.yaml` would force a full contract (spans, `run_identity`, `status_rules`, budgets) onto activities with no critical workflow — `FavoriteRestaurant`, `ConfigureProfile`, admin screens — or leave them permanently unmeasurable |
| **D2** | The unit of obligation: persona **ACTIVITY** (25) vs story STEP (144) vs persona (8) | **Activity.** Patton's backbone; two steps `$ref` the same query (`stories.yaml:57-58`) and one is a ~30x-per-checkout poll loop ([#482](https://github.com/TheCaptainCompany/captain-food/issues/482)) — a per-step rule would mint a metric for a retry mechanism |
| **D3** | The validator rules and their severity | Four **ERROR** rules — `activity-unmeasured`, `metric-story-unknown`, `metric-question-empty`, `metric-name-collision` — plus an enumerated, monotone-shrinking `unmeasured:` waiver list. **Not warnings**: this repo's warning baseline drifts (43 on 2026-08-08) and CLAUDE.md says to re-measure rather than trust it, so a warning changes no behaviour |
| **D4** | What binds declaration to emission | **Generate the instruments** (compiler catches renames, attribute names, types, arity at every call site) **+ a behaviour test** with the `InMemoryMetricExporter` spy. **Not** an extended source-text scanner — it would pass on all 26 dead metrics after a 20-line constants change, and it is the class ADR-20260803-234035 rules out. Link-time registration was evaluated and **does not work**: a `pub` fn in a library links whether or not anything calls it (reasoning kept in the proposal so it is not re-proposed) |
| **D5** | Backfill posture: gate-forward with a shrinking waiver list vs one sweep vs defer | **Gate forward now, backfill one activity per slice** in value-stream order (`customer/OrderFood` → restaurant-manager order ops → `public_user/BrowseForFood` → rider → admin → `restaurant_sync`). The one-sweep option was **already run at this scale, and the 26 dead declarations are its receipt**; with no production and no users, most new declarations would be unfalsifiable for weeks |
| **D6** | What a metric attribute may be | **Bounded sets only** — a `scalars.yaml` enum `$ref` or an enumerated list. **Never an entity id**: ids belong on spans (`business.order_id`, `business.correlation_id`), which is where high-cardinality correlation already lives. Keeps the Honeycomb bill and the GDPR minimisation argument off the table |
| **D7** | One piece of work with [#483](https://github.com/TheCaptainCompany/captain-food/issues/483) (`alerts` is not expressible), or two? | **Two, with one shape constraint**: #483 builds `alerts:` as a **top-level block whose entries `$ref` a metric by name**, not as a per-contract key. That costs #483 nothing, keeps it unblocked (it is Urgent tier-1), and is the only shape that can still alert on a business metric after D1 moves them out. Merging them would park an Urgent observability fix behind a 25-activity backfill plan |
| **Q7** | ⬅️ **PRODUCT-OWNER-OWED.** Do we ever want a **hosted product-analytics SDK** on the front end *in addition* to this? It would answer browsing-funnel questions faster | **Recommended: not now** — revisit after the first real orders. It is asked rather than assumed because it is a **vendor and data-residency** decision, not a technical one: it puts customer behaviour in a third-party (US-default) tenancy, reopens the posture settled by [ADR-20260729-183000](../adr/ADR-20260729-183000-telemetry-is-honeycomb-eu-and-degrades-never-gates.md) and ADR-0042 (Frankfurt), and splits "what happened to this order" across two systems that do not share `correlation_id` |

### 27bis. ✅ MET-R — CLOSED 2026-08-11: the reversal is confirmed, and a second decision arrived with it

**Added 2026-08-11.** The product owner had a design in mind and deliberately withheld it until the
proposal existed, so the two would be independent. Verbatim:

> *"For the metrics I have in mind the approach of the projection but I don't know how we can define
> the properties and increment/decrement to do for each event and how we can define the grouping by
> perhaps if we indicate the properties to group with. We will have to create a query in the graphql
> to allow access to these metrics."*

**The team evaluated it and CHANGED ITS RECOMMENDATION.** `PROP-20260810-234225` D4, D6, D8 and D9 now
recommend the projection approach; the generated-instrument option is recorded there as rejected, with
its reasons. What decided it, in order:

1. **Replay.** A metric is current state, and current state is a left fold of the event stream. The
   instrument design forfeited that *by design* — `crates/infrastructure/tests/orders_placed_metric.rs:129`
   asserts the point does **not** fire on a replay — so a metric added later would have **zero
   history**. Under a fold, adding a metric and replaying gives full history from the first event.
   With no production yet, that is the difference between metrics we can add later and metrics we must
   guess right now. **The team's own audit standard rejects the design the team recommended**: *"a
   `View_*` whose restore path is not replay is a finding regardless of whether it works today."*
2. **The awkward case becomes the normal case.** Ratios, distinct-identity denominators and cohorts are
   ordinary queries over a read model and are *structurally inexpressible* as monotonic pre-aggregated
   counters. A design whose escape hatch covers the most interesting questions has its default backwards.
3. **It is already the declared architecture.** `specs/architecture/c4-l2.yaml:343,370,484` and
   `c4-l3.yaml:102-105` declare `bam` as a **projector** consuming the event stream with a **schema in
   read-models** — and `bam` has **zero hits across `specs/database/`**. The projection design builds
   what the C4 already claims; the instrument design quietly diverged from it.
4. **Erasure.** Metrics needing identity (`CustomerOrderCounts`) are personal data either way. In our
   Postgres they are inside the deletion engine's path; in a vendor telemetry store they are in a
   system with no per-subject deletion API — the erasure problem does not disappear when the metric
   leaves the building, it stops being solvable.
5. **It makes the co-op differentiator free.** A queryable read model is one GraphQL query from a
   restaurant seeing aggregates about its own storefront. Over a telemetry backend that is not merely
   hard, it is the wrong kind of system.

**What is contradicted, precisely** — this is why it is filed and not executed:

| [ADR-20260810-234225](../adr/ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md) | Status under the amendment |
|---|---|
| Decision 1 — the unit is the persona ACTIVITY | **Unchanged** |
| Decision 2 — declaration enforced like ADR-0032, emission is not | **Strengthened, not reversed** — under a fold the gap largely closes: the declaration *is* what runs |
| Decision 3 — a metric declares the QUESTION it answers | **Unchanged** |
| **Decision 4 — "attributes are bounded sets, never entity ids"** | ⚠️ **CONTRADICTED.** D6 relaxes it to *bounded, declared population*, which **permits `restaurantId`** — a Postgres row is not a time series. Without this, `groupBy: [restaurantId]` is unspellable and the restaurant-facing panel cannot exist. The strict rule survives for the `alertable:` OTLP subset |
| **§"How the three properties are enforced" — "instruments generated into `crates/telemetry/src/generated/`"** | ⚠️ **CONTRADICTED.** Replaced by a generated projector + a generated tenant-scoped query. The *compiler-first* principle is unchanged — it now applies to the projector and query types. The source-text scanner is still deleted, for a stronger reason: a fold has no call site to scan |

| # | Decision | Recommendation |
|---|---|---|
| **MET-R** | Confirm the reversal. The ADR is `Accepted` and one day old; two of its clauses no longer hold | ✅ **CLOSED 2026-08-11 — CONFIRMED.** Product owner, verbatim: *"Confirm the reversal, go with the projections"*. Recorded as [ADR-20260811-014129](../adr/ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md); ADR-20260810-234225 is **superseded in part, never rewritten** — clauses 1–3 carried forward, clause 4 and the enforcement table reversed. The CLAUDE.md bullet drops its "under reversal" flag and states the projection design |
| **MET-T** | ⚠️ **NEW, and it arrived in the same breath**: *"But we need to heavily strongly typed the spec no string in it"* | ✅ **DECIDED 2026-08-11** — same ADR, Decision 2. Made precise as **three categories**, because "no strings" read literally would also forbid `description:`, which would be theatre: a **DECLARATION** may introduce a bare name; a **REFERENCE** to something declared elsewhere must be a `$ref` the loader resolves — including same-file, which the repo already does (`specs/ordering/actors.yaml:102`); a **VALUE from a closed set** stays a bare token *provided the set is closed in the loader schema*, **except** where a domain scalar already declares it, where the `$ref` is mandatory. **It landed on a real defect in the team's own grammar**: `increment: orders`, `groupBy: [day]` and `value: { sum: orders }` were bare names pointing at declarations in the same file, so a typo was a *silently wrong metric* — the exact failure class the proposal exists to remove, sitting in the proposal. The product owner spotted it before the team did. **Binding on NEW surface only** — see MET-T2 |
| **MET-T2** | How big is the existing bare-name surface, and does "no string in it" warrant a sweep? | **Measured; recommended: one issue, no sweep.** `data_requirements:`/`actions_used:` = **40 sites** across 5 screen files; `roles:` = **112 sites** across 8 api fragments. **All are checked today** — by *bespoke* rules (`screen-unknown-resolver`, `screen-unknown-role`, `core.rs:1482,1495`) rather than structurally by the loader. `fedBy:`, `emits:`, `throws:`, `command:` are already `$ref`s. So the risk is not "unchecked", it is that **every bare-name key needs someone to remember to write its rule** — and [#413](https://github.com/TheCaptainCompany/captain-food/issues/413) is the recorded case where nobody did: a plain-string `tombstone:` is *"silently invisible everywhere"*, including to the rule written for that key. Sequence the conversion after the new surfaces land; do not mix it with them |
| **MET-S** | The `serviceType` question the product owner asked about | ✅ **DISSOLVED 2026-08-11 — no decision needed and no event changes.** It was a **grain error**, not a missing field. Measured: **every one of the 11 `Order*` events carries `orderId`** (`OrderExpired` carries it and nothing else), so a projection at `grain: ENTITY` keyed by `orderId` is **total over the whole lifecycle** — a cancellation becomes `set: status → CANCELLED` on the order's own row, and the grouping moves to read time where every field is available. **The versioning story is withdrawn.** The review-proposed order-id→key index projection also works, and is provably total because `OrderPlaced` precedes the cancellation *in the same `Order-{id}` stream* — it is kept as the documented answer for when volume demands an aggregate rollup, but it costs two projections and a materially harder totality proof for a compaction nobody needs at V0. **The rule earned its place twice**: `fold-key-not-on-every-event` was written to catch a missing field and what it actually catches is a wrong grain. See [PROP-20260810-234225](PROP-20260810-234225-business-metrics-for-every-persona.md) §3 D8a |
| **MET-S2** | ⬅️ **Product-owner follow-up, answered**: *"For the case if service type this kind of counter must be computed once the order is completed so a process manager can handle it"* | **Half right, and the right half is already built. NO DESIGN CHANGE.** ① *"Computed once the order is completed"* — **correct as a principle and sharper than either earlier answer**: neither the proposal nor its review had questioned *when* the count happens, and "do not count a thing that has not finished" removes the decrement instead of solving it. It is **already what D8a does**: the fold `set`s status, the metric asks `countRows where status equals DELIVERED`, so the count comes from the terminal event and nothing else. ② But **taken literally as a fold shape it does not work** — measured: **no terminal event carries `serviceType`** (`OrderDelivered` = `[orderId, restaurantId]`; the cancels add only `reason`; `OrderExpired` is `orderId` alone), so completion-only hits the same wall. The **entity grain** is what solves it and the completion principle rides on top. ③ **Terminal-only would also be strictly weaker**: no row exists until the order finishes, so *"which orders are placed and still unaccepted right now"* — the platform's worst failure mode — becomes unanswerable. The design is **one projection read two ways**: outcome counts filter on terminal status, in-flight counts on non-terminal. ④ ⚠️ **"so a process manager can handle it" is the WRONG TOOL** — recorded so nobody builds it. PMs here are *"state-table orchestrators"* in the actor mailbox with leases, fencing and head-of-line (`specs/ordering/processmanager.yaml:9-16`): a counter there could **stall an order lane** (a *metric* causing the paid-order-nobody-was-told-about failure), and a PM **is not replayable** — it carries a live state row and issues commands, so "rebuild the metric" would re-drive Stripe. Replayability is the single property the whole reversal chose projections for. The `bam` projector is the read-side tool and it already replays. ⑤ **The valuable reading — "should completion emit a business fact?" — answered NO for the metric's sake**: `OrderDelivered` already IS the completion fact for both service types (`stories.yaml:187`), so a new `OrderCompleted` would be two events for one fact; and adding `serviceType` to it would denormalise the log so a projection need not do its job, when the projection already holds it from `OrderPlaced`. **But the instinct brushes a real gap**: `OrderCompleted`, `Receipt` and `Invoice` are **zero hits across every `specs/*/events.yaml`**, and a compliant receipt is a French legal precondition. That is [#200](https://github.com/TheCaptainCompany/captain-food/issues/200) + legal work with its own decision — deliberately **not** folded in here, because adding an event because a read model finds it handy is how an event log rots |
| **MET-F** | ⬅️ **Product-owner design fork: projection "state" as a JSON blob, and Rust-fold vs generated SQL stored procedure.** *"We need to put in place a concept of state in the projection… a simple json save in database and loaded once and saved with the checkpoint transactionally… The risk is we going to have a big state with all the order incomplete in memory."* — *"What I'm considering is to do this computation directly in SQL with a stored procedure… The problem is that it's not testable… We can still declare the metrics projection in the spec and let the generator do the stored procedure."* | ✅ **A DECISION DELIBERATELY NOT TAKEN, with reasons — recorded so it is not reopened.** ① **The state already exists and is already transactional with the checkpoint.** Measured in `crates/infrastructure/src/projection/worker.rs`: the projector holds **no fold state at all** — every event is load → project → upsert, so the state **is** the projection row and the process is stateless between events; `drain_group` opens **one transaction**, folds up to 500 events with read-your-writes, and writes `projection_checkpoint` **in that same transaction**. So *"loaded once and saved with the checkpoint transactionally"* is **not something to build — it is what the projector does today**. No JSON blob, no state table. **The "big state in memory" risk does not arise**: an incomplete order is a row, and **100k of them is 12 MB**. The precedent for the JSON idea exists and was deliberately **not** JSON — process-manager runs are typed columns in real tables. ② **The SQL option is not new here — it is built and it is the V0 default**: [ADR-0039](../adr/0039-projection-views-generated-from-lineage.md) generates a `CREATE OR REPLACE VIEW` **state-fold over `domain_events`** from column lineage (`specs/generated/views.generated.sql`, `emit/sql.rs:547,561`), and the criterion for the other target is already recorded — expressible from lineage → VIEW; COMPUTED → materialized table + projector. `OrderFacts` is the same shape as the shipped `View_DeliveryJob`. ③ **The grammar is runtime-agnostic**, and the one construct that binds a runtime is **`alertable:`** (a view cannot emit an OTLP counter) — and even that binds at the *tap*, not the fold. ④ **Measured** (200k events / 100k orders): set-based SQL **2.15 s** · row-at-a-time plpgsql **4.92 s** · Rust projector **≈65–70 s**. SQL is ~30× on rebuild, but **only 2.3× is set-versus-row** — the other ~13× is round trips, which [#267](https://github.com/TheCaptainCompany/captain-food/issues/267)'s identity map attacks without leaving Rust. 70 s to rebuild every metric from 100k orders is **~500 days of Tours trading**; read-time grouping is **27 ms**, confirming the entity grain needs no daily rollup. ⑤ **The argument that survives any volume assumption**: testing a generated procedure means golden comparison against a Rust reference fold, so **SQL does not remove the Rust fold — it adds a second one**, and that cost recurs on every fold change. **RECORDED RECOMMENDATION: hybrid, deferred** — keep the vocabulary a **total `(state, event) -> state`** over `set`/`inc`/`max` with **no host-language escape hatch** (which is what makes it both runtime-agnostic *and* replay-deterministic), **emit Rust today**, add an optional per-projection `emit: sql` only if a rebuild ever hurts. Cost of deferring ≈ zero, same DSL; cost of building SQL now = a second fold, expand/contract migrations instead of a deploy, and the loss of `event.consume.projection` spans inside a procedure. ⑥ **Testability objection substantially weakened the same day**: [#478](https://github.com/TheCaptainCompany/captain-food/pull/478) made `make test-crates` run DB tests **required by default**. Real remaining gap: **no test loads `views.generated.sql` into a database and asserts fold behaviour at all** — which is the exact bug class ADR-0039 exists for |
| **MET-G** | ⚠️ **Not about the fork, and must not be lost inside it**: the projector's per-event failure handling is **log-and-skip**, which is correct for a read model and **wrong for a money-adjacent metric** — a skipped event leaves the count permanently wrong with only an ERROR log | ✅ **CLOSED 2026-08-11 — the default FLIPS to `Halt`.** Product owner, verbatim: *"A. The projector has to stop and indicates it in the health. So k8s will detect it and we will be informed."* The team recommended quarantine first and was **overruled**; recorded as a choice, not a concession — `Skip` leaves a read model permanently and silently wrong, which for a money- or authorization-bearing projection is worse than stuck. Recorded in [ADR-20260811-105024](../adr/ADR-20260811-105024-projection-halt-default-and-health-visibility.md) (the gate-then-stabilize default flip; the gated form shipped inert in [#478](https://github.com/TheCaptainCompany/captain-food/pull/478)). ⚠️ **But the flip cannot land alone, and this is a precondition rather than a caveat** — verified on `5fdc519`: under `Halt` the worker does **not** stop (the slice rolls back, the loop keeps ticking, `worker.rs:800-816,688-700`), so `running` stays `true` (`:688`), so `/projector` returns **`200 OK`** (`server/src/lib.rs:1377-1392`) — **and neither Kubernetes probe looks at projection status at all**: projector bins probe `readinessProbe: /health` (the DB+schema gate) and `livenessProbe: /ping` (*"process is up; touches nothing"*) (`deploy/generated/manifests/bins/projector-ordering.yaml:102-111`). **Flipping today produces a projector that wedges permanently and reports itself completely healthy.** Design settled in the ADR: **per-group halt, process stays alive** (process-level would turn one poisoned read model into a scope-wide outage); **readiness, NOT liveness** — projector bins have **no `Service`**, so readiness is a pure signal channel with no side effect, whereas liveness kills and restarts, the restart cannot fix a deterministic schema fault, and the resulting **CrashLoopBackOff stops every sibling group**; re-point readiness to `/projector`; the payload gains a **per-group** breakdown naming the halted group, position, `eventType` and error (today `ProjectionStatus` is per-worker, `projection/mod.rs:13-28`, so it structurally cannot say which group halted); and the missing signal is declared — **`specs/observability.yaml` has no projection contract at all** (`:11`, prose only) |
| **MET-G2** | ⚠️ **Known consequence accepted by flipping now — recorded so nobody rediscovers it**: the role-revocation wedge | `ScopeMembership` is *"the single index every read-side authorization question resolves against, for every role and every surface"* (`specs/database/tables/projection_tables.yaml:801-810`) — **and it is a projection**. If its group halts, read-side authorization freezes: grants stop arriving and **revocations stop applying**, so a removed staff member or deactivated rider keeps access until a human clears the fault. That touches a guarantee already recorded — the §6.4 closure ([ADR-20260810-194548](../adr/ADR-20260810-194548-six-decision-answer-sheet-claim-staleness-closed.md)) decided revocation must be *"explicit and immediate"*. **Accepted, not solved**: under `Skip` the same event is skipped and the index left permanently *wrong*, which is worse in kind for an authorization index. **Quarantine remains the real fix** and stays a tracked follow-up; until then a halted `ScopeMembership` is an **incident, not a ticket**, and its alert must say so |
| **MET-Q7** | The hosted product-analytics SDK row above | ✅ **CLOSED 2026-08-11 — APPROVED AS RECOMMENDED: no hosted SDK.** Ours, server-side. **Plus an addition that matters architecturally**, product owner verbatim: *"We will use a different database from the business database to isolate the activity."* Behavioural data goes in a **separate database from the business data** — which independently lands on the legal lens's instruction and **confirms [PROP-20260811-000946](PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md) D5** (its own store, time-partitioned, so erasure is a partition drop rather than an immutability problem). **Two implications to carry, and one distinction not to conflate**: (a) this is the **behaviour store**, not the metrics store — business metrics stay a fold over `domain_events` in the `bam` schema, because they are business data derived from business facts; (b) the **C4 needs a new container** for the behaviour database plus its edges, and `specs/architecture/*.yaml` is **source DSL, not generated**, so that is an executor spec change when the work lands, not a regeneration |
| **COOP** | The three cooperative properties surfaced unprompted in [PROP-20260811-000946](PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md) D9 | ✅ **CLOSED 2026-08-11 — APPROVED AS RECOMMENDED.** All three are designed in **now**, in the first slice, not deferred to a later project: **(1)** the customer can read their own trail as a screen rendered from the catalog's own `question:` fields; **(2)** the **restaurant** is the beneficiary of the aggregate, not the platform; **(3)** the taxonomy **refuses** things checkably, so it can be published ([#377](https://github.com/TheCaptainCompany/captain-food/issues/377)). The reason they belong in slice 1 is the reason they were raised: each is a property of the **declaration mechanism**, so retrofitting any of them onto an undeclared firehose is a project, while on a declared taxonomy it is a rendering |
| **MET-W** | ⚠️ **A dependency this surfaced, not a question**: `retention:` as a free duration string (`P90D`) contradicts a **recorded legal position** — *"This table IS the written retention schedule CNIL expects — windows declared **once, in the DSL**, feeding both the sweep and the DPIA"* ([legal brief:82](../legal/BRIEF-20260808-account-erasure-two-path.md)). No duration scalar exists (`Duration`/`Retention`/`interval` = zero hits in `specs/common/scalars.yaml`) | ✅ **CLOSED 2026-08-11 — APPROVED AS RECOMMENDED**: a **named catalog of approved retention windows**, `$ref`'d — not a `Duration` scalar with a pattern (a pattern catches `P90DD` but not a well-formed window nobody approved). **Sequenced with the erasure work rather than ahead of it**, so it lands with [#194](https://github.com/TheCaptainCompany/captain-food/issues/194) and the tracking catalog depends on it rather than building it |
| **TRK-ISO** | **Behaviour tracking isolation — decided, and it goes further than the proposal asked** | ✅ **CLOSED 2026-08-11.** Product owner, verbatim: *"The behaviour event tracking will be stored in another database not the business databases the behaviour event tracking will be completely isolated and projected by another projector worker to avoid dependencies between the behaviour event tracking and the business events."* Recorded in [ADR-20260811-120828](../adr/ADR-20260811-120828-behaviour-tracking-isolated-end-to-end-and-a-faulted-worker-pre-diagnoses-itself.md) Decision 1. **Beyond [PROP-20260811-000946](PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md) D5**, which asked only for a separate store: the isolation is now **end to end — separate database AND separate projector worker**. That matters *more* under the halt decision than it did before it: now that a rejected fold halts its group, a shared worker would let a malformed behaviour event wedge a group sitting in the same process as the order read models. Separate workers make it unspellable rather than unlikely. **Settles the distinction**: behaviour events = own database + own worker, written by the UI through a `sink:` mutation, never `domain_events`; **business metrics = the `bam` schema + the `bam` projector**, a fold over `domain_events`, because they are business data derived from business facts. **C4 consequence**: a new container plus edges — and `specs/architecture/*.yaml` is **source DSL, not generated**, so it is an executor spec change when the work lands |
| **HEALTH-2** | **"Any worker must stop and say it in `/health` with a 500; k8s does not need to restart it… we don't need to go on the pods logs. It's a pre diagnostic."** | ✅ **CLOSED 2026-08-11** — [ADR-20260811-120828](../adr/ADR-20260811-120828-behaviour-tracking-isolated-end-to-end-and-a-faulted-worker-pre-diagnoses-itself.md) Decision 2, extending [ADR-20260811-105024](../adr/ADR-20260811-105024-projection-halt-default-and-health-visibility.md). **① Convergence recorded**: *"K8s does not need to restart the worker"* is independently the same conclusion the team reached from the failure analysis — a deterministic fault re-fails after a restart, so liveness gives CrashLoopBackOff and takes sibling groups down. Readiness reports, liveness restarts; the decision says report and do not restart. **② The payload is the deliverable, the status code is the transport** — *"it's a pre diagnostic"* is a constraint on the **body**: a health endpoint returning `{"status":"unhealthy"}` satisfies the code and fails the requirement. The per-group breakdown (group, `haltedSince`, position, `eventType`, stream, error) is therefore the point of the feature. **This is [ADR-20260810-231300](../adr/ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md) "no polling, only pushing" applied one layer up**: the failure pushes its own diagnosis into a watched surface instead of a human polling logs. **On `500`**: k8s treats any non-2xx as a failed probe, so 500 and 503 are identical to the cluster; keep the existing **`503`**, which is also semantically right — nobody should "fix" it to 500 for literal compliance |
| **HEALTH-2a** | ⚠️ **Edge the directive has as stated — it would take the storefront down** | **Verified on `37642cd`**: the monolith runs the API **and** the projection worker in **one process** (`RUN_PROJECTOR`, **default on**, `crates/server/src/lib.rs:641-648`), serves `/{role}/graphql`, **has a `Service`**, and its `/health` is the **deploy interlock** knowing only DB reachability + schema version (`:1503-1526`, ADR-0043). So *"say it in `/health`"* in the monolith would take the **API** unready when a **read model** halts — a degraded projection becomes a **customer-facing outage**, and it would also block deploys of the fix. **The rule is restated so the edge cannot occur**: *"the endpoint a pod's **readiness probe points at** returns non-2xx when a component **that pod is responsible for** is faulted"* — not "`/health` returns 500". Projector bins probe `/projector`; the monolith keeps `/health` on API components only, with its in-process projector observable at `/projector` **which is not its probe**. **Final shape once the cutover lands**: which components a deployable hosts is already declared, so both the probe path and the health composition can be **generated from that declaration** — a process then cannot fail readiness for a component it does not own |
| **HEALTH-2b** | ⚠️ **"Any worker" does not apply unchanged — and the reason is a genuine asymmetry** | **The actor-mailbox workers already solved this the other way.** The mailbox **quarantines**: a repeatedly-failing message hits the delivery-attempts cap and is parked as poison (`specs/database/tables/journals.yaml:69`), **the lane keeps draining**, and an operator requeues it (`specs/common/api.yaml:158,170,202`). Making an actor worker *stop* would remove that and turn a parked message into a **stopped order lane** — the platform's worst failure mode. **The principle worth keeping**: halt is right **where there is no quarantine**, and quarantine is better wherever it exists — projections halt *because* they have none, which is exactly why quarantine stays their tracked follow-up. **What actor workers DO owe** is the pre-diagnostic half: poison data is reachable only through the **admin GraphQL API** today (**no `/mailbox`, no `MailboxStatus` — verified absent**), so the monitoring app cannot see a poisoned lane without admin auth. A `/mailbox` surface is owed, **report-only — it must not gate readiness**, because a poisoned message is a normal recoverable state, not an unhealthy pod |
| **TRK-scope** | ⬅️ **OPEN — with legal, not with the product owner.** Product owner: *"we don't care the person info, just the behaviour"*, and the substantive part — *"using a generated identifier uncorrelated to the person so we will have the tracking without the need to know the person is doing what but a persona."* Plus a clarification that **changes an earlier legal finding**: the "help AI agents" sentence was **internal**, explaining to the team why the data is wanted — **not** a user-facing personalisation feature | **Do not design against this yet.** Legal is working out whether a pseudonymous journey identifier fits the **audience-measurement exemption**, or whether per-journey continuity exceeds it. **The mechanical half, thought about but NOT committed**: if the answer is *"lawful provided the join never happens"*, then **"never joined" must be structural rather than promised** — a pseudonym and a customer id in one database with correlatable timestamps is not a guarantee. Candidate constructions, compiler-first: the **separate database is already decided** (MET-Q7) and does most of the work; **no foreign key** to any business table; **no shared column name** the validator would refuse; and an **`identifierClass` that cannot be `CUSTOMER`** for an anonymous-funnel event — i.e. the join is *unspellable in the DSL* rather than *forbidden in review*. Note this pulls against D8 option A (authenticated, `identifierClass: CUSTOMER`), so the two are alternatives per event kind, not a single answer. **The proposals are deliberately NOT amended until legal reports** |

**What it costs, stated plainly so the confirmation is informed.** Storage grows with grouping-key
cardinality; a projection per metric family means more projectors and checkpoints; a new GraphQL read
surface needs tenant scoping (`metric-query-unscoped`, an ERROR rule — one un-scoped metrics resolver
hands every restaurant's revenue to every other); and **backfill is only as good as the events**.
Measured: `serviceType` appears on `OrderPlaced` and on **no other Order event**, and `OrderExpired`
carries `orderId` alone (`specs/ordering/events.yaml:114-533`). So a projection keyed by `serviceType`
**cannot be decremented by a cancellation** — the new `fold-key-not-on-every-event` rule fails on a
realistic declaration **today**, which is what earns it. Fixing that by adding a field to an event is a
payload shape change, i.e. a **versioning story, not an edit** — free only while the log is empty,
which is the same window `event_version` is waiting in.

Tracking: [#484 "26 of the 29 declared `business_metrics` emit nothing: give business metrics their own catalog, keyed persona x activity, with a bidirectional coverage gate"](https://github.com/TheCaptainCompany/captain-food/issues/484).
Known dependency, stated so slice 1 is not misread as closing the loop: the `asserted_by:` link
cannot point into `specs/tests.yaml` until
[#212 "ADR-0032 completeness cannot reach projectors or read guards"](https://github.com/TheCaptainCompany/captain-food/issues/212)
lands (decided 2026-07-28, unbuilt) — until then the emission test is a convention with two working
examples, not a gate.

---

## 28. Behaviour event tracking declared inside the screens spec — PROP-20260811-000946

Product-owner directive, 2026-08-11: *"We need to integrate the metrics in the spec. And integrate
the behaviour event tracking inside the screens spec."* The **first clause is an endorsement of §27**,
not a new ask — D1–D7 there stand unchanged. This block is the **second** clause, and it is a
different shape: a business metric measures whether a persona achieved an outcome (a fold over
`domain_events`); a behaviour event records what a persona did in the UI, is authored by nobody, can
be rejected by nothing, and is **personal data under a lawful basis**.

Design: [PROP-20260811-000946](PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md).
Tracking: [#485 "Behaviour event tracking has no declaration site, and the one place that knows a component is an allergen filter is the only place that can refuse it"](https://github.com/TheCaptainCompany/captain-food/issues/485).

**The fact that earns the block, verified on `8ee073b`.** Not the absence of tracking — that is
expected. It is this: **`SetCustomerPreferences.dietaryTags` is `array<Tag>`, `Tag` is a free-form
`string` with `maxLength: 80` and no enum, and it is persisted to `View_Customer.preferences` jsonb**
(`specs/customer/commands.yaml:179-182`, `specs/common/scalars.yaml:145-148`,
`specs/database/tables/projection_tables.yaml:337`). `halal` and `kosher` are spellable values
**today**. No screen binds it (zero hits in `specs/screens/`), so nothing is running and nobody did
anything wrong — but the Article 9 exposure this proposal is about is **already declared and already
stored**, and no review caught it because no artifact existed that would make anyone look.

**Two rows below are product-owner-owed. D1–D7 are TEAM-OWNED** under the same delegation as §27
([ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)),
listed for visibility and the ensemble-consent + veto pattern, and **not counted in the
product-owner-owed total**.

| # | Decision | Recommendation |
|---|---|---|
| **Q1** | ⬅️ **PRODUCT-OWNER-OWED.** **Client storage — and therefore whether a consent banner exists at all.** The framing matters: the device identifier **already exists** (`X-SESSION-ID`, *"a client-generated UUID, kept in a cookie / app cache; identifies anonymous users end-to-end"*, `crates/server/src/graphql/session.rs:1-15`). The question is whether a **new purpose** is attached to storage that currently has exactly one | **(A) Authenticated, server-side only** — no new client identifier and no analytics read of the existing one. Plausibly avoids Art. 82 entirely, so **no banner exists**, which is also a conversion saving on the first screen of the funnel. **What we lose**: the pre-cart funnel — `public_user/BrowseForFood` (8 steps) is unattributable, so browse-to-cart conversion is not computable; four discovery screens are `roles: [PUBLIC, CUSTOMER]`. **What softens it**: `CustomerIdentified` already carries `sessionId` so guest carts bind to the customer on identification (`specs/customer/events.yaml:50-70`) — everything from cart onward is attributable with **no** analytics identifier. **(C)** a dedicated analytics device id + banner is the recorded upgrade path. **(B) reusing the cart cookie is the worst option, not the cheapest** — it forfeits the strictly-necessary exemption *for the cart cookie too*, since the exemption attaches to the purpose of the storage. Validator rule R8 refuses `PSEUDONYMOUS_DEVICE` while this row is open, so it cannot be answered by accident in a PR. **VERIFY-FIRST: no counsel review has taken place** |
| **Q2** | ⬅️ **PRODUCT-OWNER-OWED.** **Does the restaurant see its own storefront's behaviour data?** *"23 people opened your menu at 19:40, 4 ordered; these three items are opened most and ordered least."* This is the differentiator, and it is product scope, not plumbing | **Recommended: yes in principle, decided now so the taxonomy is designed for it, built after the DPIA.** Same data, beneficiary changed — and it is the *easier* legal position (the restaurant is controller of its own storefront's traffic, a far simpler legitimate-interest balancing than a platform building cross-restaurant profiles). It is also a thing an independent restaurant cannot buy from Uber Eats at any price. **The tail**: it makes the restaurant a controller or joint controller, needing a controller/processor arrangement that does not exist |
| **D1** | Where a behaviour event is declared | **Split**: a root `specs/behaviour_events.yaml` for the definition (legal fields in ONE place, so a DPIA reads one file), plus a `tracking:` `$ref` binding on screen/action nodes. This is the screens DSL's own idiom — `resolvers`/`actions` are already allowlists that `$ref` definitions living in `api.yaml`. Fully-inline was rejected because a lawful basis is a property of a *processing*, not a widget, and would sit in a file that is *"runtime-editable via Supabase `screen_specs`"* |
| **D2** | **Derived from screen structure, or authored per event?** — the question put to the `ux-designer` lens | **Authored**, with the *binding* proven complete both ways. Derivation fails on measurement: **61 of 121 api operations are screen-bound and 6 of 25 persona activities have no screen-bound operation at all** — `restaurant_owner/ManageCatalog` is 14 steps with zero screens (`grep -c catalog specs/screens/restaurant_backoffice.yaml` = 0), `admin` is 7 activities with 1 screen. It also **over-attributes**: four discovery screens are `roles: [PUBLIC, CUSTOMER]`, and the one operation `ManageCatalog` shares with a screen is `queries/catalog` read by *customers*. And opt-out is the wrong default for personal data — a new screen would start collecting on merge |
| **D3** | What a declaration carries, and which kinds exist | `kind:` is **`VIEW \| INTERACTION` and nothing else**. `IMPRESSION` and session replay are **not values and not concepts** — absent from the grammar, not discouraged in a comment (compiler-first, ADR-20260803-234035). Impression tracking over a *menu* is the mechanism that turns a food catalogue into a health-and-religion inference engine with nobody intending it. Legal fields `purpose`/`lawfulBasis`/`retention`/`identifierClass`/`specialCategoryRisk`/`dpia` are **required with no defaults**; attributes are bounded sets only (§27 D6 parity) |
| **D4** | How it binds to the metrics catalog | **Separate catalogs sharing exactly one thing** — the `activity:` `$ref` into `stories.yaml` — plus one global name-uniqueness namespace (the same constraint §27 D7 gives [#483](https://github.com/TheCaptainCompany/captain-food/issues/483)). That join is the payoff: *"of the 210 who opened the menu, 31 placed an order"* becomes one row of the persona × activity grid. One merged catalog was rejected — half the fields would be meaningless for half the entries, and Evans' one-name-one-meaning rule says *metric* and *event* are two concepts |
| **D5** | Where the records GO | **Their own database, RANGE-partitioned by day; retention is a partition DROP.** **Never `domain_events`** — every row there is a fact an aggregate *decided* in response to a message it could have *rejected*; a behaviour event is neither, so Young's left-fold invariant stops holding (a replay would have to skip rows, and a fold that skips rows is not a fold). Operationally it would size PITR by clickstream, dominate every projection checkpoint, and multiply [#473](https://github.com/TheCaptainCompany/captain-food/issues/473)'s deletion-engine scan bound by ~1000. Not the order path's instance either — that is [#443](https://github.com/TheCaptainCompany/captain-food/issues/443). Not Honeycomb — a trace store has no per-subject erasure API, so Art. 17 has no answer there |
| **D6** | What the validator enforces | **Ten ERROR rules.** Two are load-bearing: **R5 `tracking-on-sensitive-node`** — no `tracking:` binding anywhere under a node marked `sensitivity: SPECIAL_CATEGORY`, unconditional, no override — and **R10 `behaviour-tracking-without-dpia`** — the *emitter* produces nothing while `docs/legal/DPIA-*.md` does not exist. R5 is why the screens spec is the right location: it is the **only** artifact in the repo that knows a `filter_bar` is an allergen filter (the api layer sees an argument, the store sees a string, an SDK sees a payload). R7 encodes *two purposes, two bases*; R9 makes rider productivity-scoring and nudging **unspellable** |
| **D7** | What is the first slice | **The mechanism plus ZERO live events**, then the DPIA, then the first three events (server-side, authenticated). Instrumentation before a DPIA is **processing that should not have started** — the DPIA is a precondition of beginning, not a follow-up. The dated reason to land the mechanism now anyway: **`allergen` has zero occurrences in `specs/catalog/*.yaml`** while the model is decided-and-unbuilt ([ADR-20260808-171056](../adr/ADR-20260808-171056-register-sweep-consent-decisions.md), [#184](https://github.com/TheCaptainCompany/captain-food/issues/184)) — so R5 can be built **before** the control it refuses exists. That window closes when #184 ships |
| **D10** | **The write path — the product owner's own design, evaluated** | ✅ **CONVERGED, and recorded as such.** Their design (withheld until the proposal existed, so the two would be independent) — *"we can name the interaction name and the properties we want to share inside the event, of course the principal context will be sent with the jwt. A mutation should be exposed to send these events"* — matches D1/D3 on the name and properties, and the JWT clause **is D8 option A reached from the other direction**: an authenticated mutation carrying principal context is server-side capture with no analytics identifier and no terminal-equipment read for an analytics purpose. It is also ADR-0041's envelope doctrine (*the acting user is envelope metadata, never a payload field*) applied to a non-domain write **without being asked**. Two independent routes to one answer is the strongest evidence available that A is right, and it narrows Q1 below from *"which option"* to *"do we ever want C's anonymous funnel badly enough to accept a banner"*. **The mutation is also a BETTER answer than the proposal's original "BFF tracking boundary"** — it inherits role-path routing, the ACL and the `op-uncovered-by-story` gate. ⚠️ **One measured blocker**: `op-missing-command` is an **ERROR** (*"mutation declares no command"*) and all **86** mutations bind a command handled by an actor (`tools/codegen-rs/src/validate/core.rs:292,295,301`). So today a mutation **cannot** be a non-command: declaring `recordBehaviourEvent` the only way the validator accepts would enqueue it on the actor mailbox and append it to `domain_events` — exactly what D5 refuses — **silently, with the gate green**. The fix is a small new api.yaml shape: a mutation that declares **`sink:`** where a command declares `command:`, meaning *this write is recorded, not decided* (a command can be rejected; a sink write cannot). Team-owned, but it must land before this half is buildable |

**Not a new row: [#194 "GDPR Article 17 has no technical answer… no DPIA/privacy policy/terms exist"](https://github.com/TheCaptainCompany/captain-food/issues/194)
is open and unchanged.** It is named here only because this work is *sequenced behind it*, and R10
turns that sequencing into a build failure rather than a promise. Filing a duplicate DPIA-ownership
row would inflate the register without adding a question.

**On the "same technique as LinkedIn" framing.** The honest answer is that the pipeline design is
neutral and the purposes are what differ. But three things are design rather than purpose, are only
reachable if decided now, and are properties of the *declaration mechanism*: **(1)** the customer can
read their own trail as a **screen** rendered from the catalog's own `question:` fields, not a GDPR
export ZIP; **(2)** the beneficiary of the aggregate is the **restaurant**, not the platform (Q2);
**(3)** the taxonomy **refuses** things, checkably — and publishing it (adjacent to
[#377 "Build in public"](https://github.com/TheCaptainCompany/captain-food/issues/377)) turns *"we do
not surveil you"* from a claim into a file anyone can read and a gate anyone can run. Retrofitting any
of the three onto an undeclared firehose is a project; on a declared taxonomy it is a rendering.

---

## Maintenance

The `architect` reconciles this file on each daily run: new proposals add rows, answered decisions
move to §5, and a decision open for many runs gets flagged in the report with its age. A decision
nobody is making is the most expensive thing in the backlog, and it will never surface on its own.
