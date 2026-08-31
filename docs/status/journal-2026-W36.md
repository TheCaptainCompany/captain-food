# Status journal — 2026-W36

Journal entries for ISO week 2026-W36, newest first, in the order they were written.
Current state: [`../STATUS.md`](../STATUS.md).

> **2026-08-31 — the coordinator gets the register-check gate on its committing surface (#814).**
> Every *agent* has been gated on the ask since 2026-08-21; the **coordinator had no gate on any
> surface**, and in one session produced **nine** failures of exactly the class the gate prevents —
> an option space presented as open that ADR-20260829-230418 had decided, a counsel posture proposed
> without reading BRIEF-20260819 §4.2, a line-range citation (`pm_orchestrators.rs:844-852`) that
> **reads as confirming the claim while showing the opposite**, and a dispatch about to contradict
> PROP-20260815-142349. **Four of the nine were caught by the founder or a lens.** The ninth was
> caught by running the check before dispatching — the proof the discipline works.
> Now a `PreToolUse` hook on the **`Agent`** tool, as Lane D of the *same* `register-check.sh`
> (extended, not forked — the gate-script self-verification set stays at four files, so neither
> guard has to learn a fifth). Two design questions decided structurally rather than by a list:
> the **discriminator** is the target agent's own `tools:` frontmatter — write-capable is gated,
> read-only is not — so lens consults and reviewer passes pass untouched and granting an agent a
> write tool arms the gate for it *in the same commit*, with no exemption list to go stale; and the
> **escape hatch** is shut by requiring a cited record id to RESOLVE to a file under `docs/`, so a
> literal `Register check: none` and a well-shaped invented id are both refused.
> The validator returned the favour mid-write: §23 `record-citation-unresolved` refused the *fake
> ADR id used as an illustration inside the ADR itself* — the same principle one corpus over,
> caught by a gate rather than a reader.
> **Recorded honestly rather than hidden**: a hook gates a TOOL CALL, so the coordinator's prose
> answers to the founder stay ungateable. `.claude/skills/coordinator-register-check/` carries that
> half and is *weaker* — the pre-existing `decision-lookup` skill was invoked **zero** times in the
> session that produced the nine, which is why this one is a hook and not a paragraph, and why the
> right move is routing more coordinator→founder questions through `AskUserQuestion`.
> Records: [ADR-20260831-141500](../adr/ADR-20260831-141500-the-coordinator-gets-the-register-check-gate-on-its-committing-surface.md).
> Proven by selftest cases D1-D12 / LD1-LD3 / W4-W7; three planted mutants (Lane D disarmed, the
> `Agent` settings entry deleted, the resolver stubbed to accept anything) were each observed RED
> before the suite was trusted.

> **2026-08-31 — the `send:` route grammar: four unlaned command sends declared, gated and
> dedup-keyed (#807).**
> `PmStepDef::Send` carried no `to` and no `route_gate` while all four committed `send:` steps
> already WROTE `to:` in the DSL — `pm-send` validated the target and the emitter then discarded
> it, so a `send:` could never reach `ROUTED_LANES` or the `Route` enum. Now it can: three routes
> (the two `MarkOrderDelivered` legs are two triggers for one route), three `specs/common/`
> configuration keys all `default: false`, legacy arms preserved byte-for-byte behind each gate —
> `git diff -w` on the regenerated `process_managers.rs` shows **zero deletions**, the whole diff
> is additive. `pm-route-gate` now covers `send:` steps, and because `to:` is mandatory on a send,
> **every send must declare its route**.
> The find that justified generating before believing: the money path. Keying the routed door on
> the TARGET's identity — the obvious default — would have keyed the credit door on `customerId`
> while `grant_customer_credit` is idempotent per `reclamationId`. One customer receives many
> goodwill credits, so that door would have swallowed every grant after the first: money owed,
> never paid, no error anywhere. A new rule `pm-send-dedup` makes the axis a mandatory declaration
> with **no default**, since the safe axis does not follow from the target.
> Records: [ADR-20260831-093000](../adr/ADR-20260831-093000-the-enumeration-is-deliver-and-send-not-deliver-alone.md)
> corrects ADR-20260829-230418's enumeration (`deliver:` → `deliver:` ∪ `send:` ∪ wrapper-seam
> `sends:`); the property in `specs/common/processmanager.yaml` already covered sends, so this
> executes the recorded decision rather than amending it.
> **Round 2**, after the independent reviewer returned FAIL on two blocking findings. (1) The
> `LaneEnqueue` type's own doc still stated as FROZEN the very rule this branch proves
> catastrophic — *"`external_id` is the TARGET AGGREGATE's id"* — which the generated credit
> route falsifies on the same branch. Both sites now say the axis is DECLARED (`dedup_by:`), and
> that it means **the same request**, not *the key the target handler is idempotent on*:
> `MarkOrderDelivered` REJECTS a repeat rather than absorbing it, so on that route the door is
> the only thing collapsing a partner report racing a rider completion. Its corollary — a door
> minted by a REJECTED first attempt stays minted, closing the route to a later legitimate
> attempt — is a property of `main`'s already-merged C2 door, filed as
> [#811 "A routed COMMAND door is minted at ENQUEUE, so a REJECTED first attempt permanently closes it"](https://github.com/TheCaptainCompany/captain-food/issues/811) and a precondition on both
> flips. (2) `ROUTE_ORDER_DELIVERY_COMPLETION_THROUGH_LANE`'s consequence list gained **(e)**: a
> successful COMMAND-door delivery arms the declared `schedules:`, and `MarkOrderDelivered`
> declares the `OrderExpired` retention clock. Today the saga's in-process arm creates no mailbox
> row, so a completion reported by a PARTNER or by an INDEPENDENT RIDER arms **no** retention
> clock while the same order closed through the `markOrderDelivered` mutation does — a legal
> surface, now named in the text the flip decision is made from.
> Posture `HOLD: human` — PR stays draft. Four non-blocking findings were filed rather than
> fixed here:
> [#810 "`pm-send-dedup` proves a routed send's axis EXISTS, never that it is the RIGHT one — declare the handler's same-request key in the DSL"](https://github.com/TheCaptainCompany/captain-food/issues/810),
> [#811 "A routed COMMAND door is minted at ENQUEUE, so a REJECTED first attempt permanently closes it (blocks the delivery-completion and replacement-birth flips)"](https://github.com/TheCaptainCompany/captain-food/issues/811),
> [#812 "No `pm-deliver-lane` equivalent for routed `send:` steps — a routed send to a mailbox-less aggregate passes validate and fails inside the leg transaction"](https://github.com/TheCaptainCompany/captain-food/issues/812)
> and
> [#813 "`order.lane.enqueue`'s `business.aggregate_id` is bound from `external_id` — on a routed send whose dedup axis is not the aggregate it carries the wrong id"](https://github.com/TheCaptainCompany/captain-food/issues/813).

> **2026-08-31 — four decision rows declared: the three residues #764's ruling left open, plus the
> erasure PM's resume correlation, which cannot be built as approved.**
> Records-only change, straight to `main`. `CREDIT-AT-ERASURE` closed D1-D3 on 2026-08-31
> ([ADR-20260831-033621](../adr/ADR-20260831-033621-customer-credit-is-disposed-of-as-a-leg-of-erasure-goodwill-credit-is-refundable.md))
> and explicitly left D4/D5/D6 open; the recording run could not split them because an executor
> never files an out-of-dispatch decision file
> ([`docs/decisions/README.md`](../decisions/README.md), *"Partial closure = split at close time"*).
> They are now keys:
> **[CREDIT-EXPIRY-WINDOW](../decisions/CREDIT-EXPIRY-WINDOW.yaml)** — 180 days minus a settlement
> margin, or 1 year and adjudicate the gap. Stripe cannot refund a capture indefinitely (~180 days
> in practice), so a credit aged 6-12 months is **traceable and not refundable**, a third state the
> ruling has no branch for; and the tension cuts both ways, because if traceable credit is the
> customer's money then **any** expiry extinguishes it on a timer. The 1-year default
> ([ADR-20260726-163737](../adr/ADR-20260726-163737-reclamation-saga-and-credit-ledger.md)) is
> **chosen but unbuilt**, so the window is free to move today.
> **[CREDIT-DRAIN-ORDER](../decisions/CREDIT-DRAIN-ORDER.yaml)** — promotional first (customer-
> favourable, and the only ordering that cannot be accused of engineering a smaller refund) or
> traceable first. **This row has a clock**: free until the first promotional grant exists, a
> migration after. Verified rather than assumed: `CustomerCreditState` is a single `balance_cents`
> scalar with **no lots at all**, so there is no drain order in the code to preserve — whatever is
> picked is also a decision to give the balance provenance.
> **[CREDIT-LEG-SEQUENCING](../decisions/CREDIT-LEG-SEQUENCING.yaml)** — deliberately **widened**
> past D4's scheduling wording, because it cannot be answered without its two hard preconditions in
> view: (1) `CustomerCreditGranted` carries only `customerId`/`amount`/`reclamationId`
> (`specs/payments/events.yaml:184-195`), so the D1/D2 split is a **stored-event-shape change**;
> (2) the only writer to `CustomerCredit-{customerId}` is the unlaned `send:` at
> `specs/ordering/processmanager.yaml:259`, so the erasure leg would be that stream's **second
> unlaned writer**, separated from the first only by an optimistic version conflict.
> **[ERASURE-PM-RESUME](../decisions/ERASURE-PM-RESUME.yaml)** — new, and the one with a build
> blocked behind it. [PROP-20260829-150752](../proposals/PROP-20260829-150752-customer-erasure.md)
> §3.1 has the parked erasure resume on the blocking order's terminal fact, and **that cannot be
> spelled**: `raw_msg_expr` (`tools/codegen-rs/src/emit/pm_orchestrators.rs:964-972`, called from
> `emit_state` at `:1392`) panics on any `state.by` value that is not a property of the trigger
> message, and none of the four order terminal facts carries a `customerId` — `OrderDelivered`,
> `OrderCancelledByCustomer`, `OrderCancelledByRestaurant` carry `orderId` + `restaurantId`,
> `OrderExpired` carries `orderId` alone. Three options with a real doctrinal split (A `from_read`
> through a projection — young: a projection becomes a write-side correlation input, and under
> projector lag the PM does not resume while a GDPR clock runs; B `customerId` on the four facts —
> replay-neutral but a stored-shape change putting an identifier on four more payloads retained
> 3650 days; C a PM-owned order-to-customer index — vernon-clean, costs a third table and misses
> orders created after the request). Architect recommends **C, fallback A**, recorded as a
> recommendation and not an answer. It blocks the erasure **runtime** chunk of
> [#708](https://github.com/TheCaptainCompany/captain-food/issues/708), which
> [ERASURE-LAUNCH-GATE](../decisions/ERASURE-LAUNCH-GATE.yaml) makes launch-blocking.
>
> **STATUS.md corrected in the same change.** The `Aggregates own the facts` row still read C2 as
> *"built, gated OFF, awaiting review"*; `eda50a63` (*"Closes #595 …"*, PR
> [#762](https://github.com/TheCaptainCompany/captain-food/pull/762)) is an ancestor of `main`, so
> that has been stale since the merge. It now reads **merged, gated OFF**, and says the thing the
> old wording hid: `ROUTE_REPLACEMENT_BIRTH_THROUGH_LANE` defaults `false`
> (`specs/ordering/configuration.yaml:63`), so
> `crates/application/src/process_managers/reclamation.rs:157` still takes the legacy in-process
> path — **a live unlaned birth on merged code**, not on a branch awaiting review.

> **2026-08-31 — the founder ruled on the credit balance that outlives its erased subject: it is
> disposed of as a LEG of the erasure, never a park.**
> ([ADR-20260831-033621](../adr/ADR-20260831-033621-customer-credit-is-disposed-of-as-a-leg-of-erasure-goodwill-credit-is-refundable.md),
> register row [CREDIT-AT-ERASURE](../decisions/CREDIT-AT-ERASURE.yaml), six lenses.) Directive:
> **refund credit traceable to a captured payment, forfeit purely promotional credit, disclose the
> balance at the confirmation step before the irreversible act.** **Escheat** and
> **block-until-zero** both rejected — escheat invents an unowned-funds posture we have no basis
> for, block-until-zero makes a legal right hostage to a marketing balance. Three rulings on the
> branches the directive did not have: **D1 → A**, reclamation **goodwill credit is REFUNDABLE** —
> the third category, and **100% of the credit that can exist at V0** — to the **original captured
> instrument**, capped at the **un-refunded remainder of that capture** (a full refund plus a
> goodwill grant on one claim otherwise pays €35 against a €30 sale). **D2 → A**, forfeiture is a
> rule of **ACCOUNT TERMINATION GENERALLY** — closure, dormancy, the existing one-year expiry and
> erasure alike — because **Art. 12(5)** requires exercising a right to be free of charge and a
> balance extinguished *because* someone asked to be erased is arguable as a charge. **D3 → A**, a
> **failed refund PROCEEDS AND IS RECORDED**: the erasure completes on the **Art. 12(3) clock**, the
> failure lands on the pseudonymous receipt, the amount becomes an ordinary payable — the founder's
> own objection to block-until-zero, applied consistently.
>
> **What the record explicitly does NOT close.** **D4** (does the credit leg ship inside
> [#708](https://github.com/TheCaptainCompany/captain-food/issues/708) or after), **D5** (shorten
> the expiry to ~180 days so *traceable* implies *refundable* by construction) and **D6** (which pot
> drains first when credit is spent — **free only until a promotional grant exists**) are **open**,
> and need keys the coordinator declares. **The three counsel questions on
> [#764](https://github.com/TheCaptainCompany/captain-food/issues/764) are NOT discharged**: legal's
> verdict is **0 discharged, 1 narrowed, 2 untouched**, and **Q2 is now heavier** — both limbs of
> D1/D2 produce an accounting movement someone may have to prove, so "is the credit ledger
> L123-22-retained or shreddable?" now covers more of the ledger, not less. `decided` is a recorded
> founder decision and **not legal clearance**.
>
> **Four lens findings verified against the tree rather than taken on the card's word.**
> `CustomerCreditGranted` carries `customerId`/`amount`/`reclamationId` and **no provenance field**
> (`specs/payments/events.yaml:184-195`) — so the refund/forfeit split is a **stored-event-shape
> change**, and the disclosure block is **absent, not zero**, at V0 (ux). It also carries **no
> `legalRetention:` marker** while `PaymentCaptured` and `PaymentRefunded` both carry the 10-year
> one (`:41`, `:141`) — and the refund arm **creates a new 10-year retained record naming the
> subject as part of erasing them**, which must appear in `retainedUnder` (legal).
> `CustomerCreditBalanceRow` is `customer_id`/`balance_cents`/`currency`/timestamps
> (`crates/application/src/generated/rows.rs:181-187`), so beck's prediction is exact: a classifier
> handed that row applies a **default to 100% of balances**, and `default ⇒ forfeit` silently
> forfeits every refund owed **while every unit test stays green** — the counter-measure is
> compiler-first, a parameter type that row cannot satisfy. And `GrantCustomerCredit` is a `send:`
> step in the PM's own thread (`specs/ordering/processmanager.yaml:259-261`) — **an unlaned
> foreign-stream write on the money path, live today, and not among C3's twelve** (Payment ×7,
> DeliveryJob ×4, Cart ×1).
>
> **The rejected option business had assumed was available is foreclosed by our own design**:
> "let the subject spend the credit first" cannot be offered, because **`re-login-cancels`** means a
> customer who logs in to spend the balance **cancels their own erasure**. Any "use it first" copy
> would be a lie.
>
> Also corrected in the same change: `docs/claude/sessions/environment.md` claimed **executors
> cannot perform GitHub API mutations**. They can — `gh` is absent and MCP is not in a subagent's
> toolset, but `curl -H "Authorization: Bearer $GH_TOKEN"` against `api.github.com` returned `200`
> from this executor. Believing the old sentence makes an executor hand back incomplete work for a
> capability it has.
>
> **Week roll**: this file is new. 2026-08-31 is the **Monday of ISO 2026-W36**, and only
> `journal-2026-W35.md` existed. The budget ledger had already rolled to W36 — which is the trap the
> dispatch named, since inferring the journal's week from the budget file is exactly the wrong
> method; `date +%G-W%V` is the check.
