# ADR-20260830-224500 — An unrecorded declared FACT PARKS; it is never terminally failed

- **Status**: Accepted
- **Date**: 2026-08-30
- **Decider**: the team, in the #780 mob loop. Not a founder decision: no option space reaches him
  here, because the alternative turned out to contradict a recorded rule rather than compete with it.
- **Relates**:
  [ADR-20260830-183000](ADR-20260830-183000-the-actor-inbox-is-a-generated-enum-and-the-match-is-human-owned.md)
  (the typed inbox this extends to the fact half) ·
  [ADR-0004](ADR-0004-commands-are-derived-from-use-cases.md) (a command may be REJECTED; an
  external fact that already happened may not) ·
  [ADR-20260803-143216](ADR-20260803-143216-only-cap-poisoned-mailbox-rows-are-requeueable.md)
  (a handler FAILED is a recorded business decision, so it is not requeueable) ·
  [ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) ·
  [ADR-20260815-030206](ADR-20260815-030206-the-write-side-never-reads-a-projection.md) (dedupe
  folds the aggregate's own stream, never a `View_*`) ·
  `specs/common/rules.yaml#/OnlyCapPoisonedMailboxRowsAreRequeueable`
- **Realized by**: [#780 "The typed-inbox guarantee stops at the fact-record route"](https://github.com/TheCaptainCompany/captain-food/issues/780) /
  PR [#783](https://github.com/TheCaptainCompany/captain-food/pull/783)

- **Consulted** (mob briefing, ADR-20260812-143619 — one line per lens; a lens never asked is
  indistinguishable from a lens with nothing to say):
  - **beck** — build the shape that removes the temptation instead of gating it: a per-actor fact
    sub-enum, because a match over the composite makes `ActorInbox::Payment(_)` compilable AND
    gate-clean. Carried; it is the mechanism, and this ADR is only about the residue it leaves.
  - **young** — the verdict is per-fact, not blanket; `Deferred → Failed(Internal)` is command
    semantics wearing a fact's clothes; the question is *can the source re-emit?*. Carried in its
    reasoning and **overtaken in its conclusion** — see "What we did not do", below.
  - **dba** — the terminal arm's error code: `Internal` is invisible to `poisonedMailboxMessages`
    and refused by `RequeueMailboxMessage`. Carried; it is the finding that decided this ADR, and
    its proposed remedy is the one we did not take.
  - **vernon** — loud, same as UNKNOWN: a deferred route is an unfinished route. Carried.
  - **evans** — a deferral is a MODELLING statement, never a schedule. Carried into the DSL and
    into an executable check.
  - **observability** — the loss is currently zero-signal; the contract must not collapse
    `deferred` into the same series as anything else. Carried.
  - **farley** — the deployed behaviour delta is zero, so the value is a build that refuses a future
    omission; verdict-preservation must be TESTED, not asserted in a comment. Carried.

## The decision

**A DECLARED inbound fact that reaches the mailbox door with no record route PARKS: the delivery
ABORTS, the row stays `RECEIVED`, the attempt counter advances, and at the delivery-attempts cap the
RUNTIME flips it to `FAILED` with `DeliveryInfrastructureError` — the existing poison queue, with
its existing operator recovery. The handler never writes a terminal verdict for it, and never writes
anything at all to `inbound_messages.error`.**

This is the posture the door already takes for an `UndeclaredMessage` ("retry, then park loudly,
rather than bury the row"), applied to the one case that was missing it.

## Why a fact cannot be terminally failed

Three reasons, and each is independently sufficient.

**1. A fact is not refusable.** ADR-0004 is explicit: a COMMAND is write-side input and may be
REJECTED — that is what makes it a command. An inbound FACT already happened somewhere else. There
is no authority under which this system declines it, so "FAILED" is not a verdict about the fact; it
is a verdict about our own missing code, recorded permanently against someone's money.

**2. The terminal verdict is unrecoverable in practice, not merely impolite.** The poison read
(`persistence/mailbox_lanes.rs`), the lane counter and the requeue write ALL filter on
`error->>'code' = 'DeliveryInfrastructureError'`. A handler-written `Internal` is therefore invisible
in `poisonedMailboxMessages`, refused by `RequeueMailboxMessage`
(`MailboxMessageNotRequeueable`), and swept at 90 days. Its only recovery is re-driving the source —
and for a fact the platform emits itself (`DeliveryOfferTimedOut`, `CustomerErasureDue`,
`CartCheckedOut`) there IS no source to re-drive.

**3. "The source can re-emit" does not rescue it even where a source exists.** This is the part that
turned young's per-fact question into a uniform answer. A provider redelivery arrives with the same
idempotent key and is absorbed by the enqueue-side pk dedupe — the reasoning already written into
`handle_recorded_fact`'s `Repository` arm: *"a terminal FAILED would be absorbed by the enqueue-side
pk dedupe when the provider redelivers, permanently losing the payment/delivery fact."* So the
re-emittable/not distinction changes the *cost* of a terminal verdict, never its *correctness*. The
per-fact judgement is still worth having and is recorded in PR #783; it is no longer what picks the
verdict.

## What we did not do, and why

**dba proposed the cheapest fix: have the handler write `code: "DeliveryInfrastructureError"` so the
row lands in the existing poison queue.** It reaches the right destination, needs no migration and
no new status, and we did not take it.

That code is not a free-form label. `specs/common/rules.yaml#/OnlyCapPoisonedMailboxRowsAreRequeueable`,
`specs/common/api.yaml` and `specs/common/commands.yaml` all define it as *"the row the
**delivery-attempts cap** flipped to terminal FAILED"*, and ADR-20260803-143216's whole argument for
excluding handler-FAILED rows from requeue rests on that distinction. A handler minting the cap's
marker makes a routing gap indistinguishable from a transport casualty and silently widens what the
rule means — which dba itself flagged as needing a register row first.

**Parking reaches the same destination through the recorded path.** The row that parks IS
cap-poisoned when it gets there, by the runtime, for the reason the code names. Nothing is
redefined, no rule is amended, no register row is owed, and the operator's recovery is byte-identical
to the one they already have.

The cost is real and bounded: a parked row holds its lane's head for the length of the exponential
retry schedule (~5 minutes at the shipped cap) before the flip unblocks it. That is the same cost
the `UndeclaredMessage` park already accepts, and it is the correct trade against losing a fact.
The drain-loop amplification it interacts with is filed separately as
[#788](https://github.com/TheCaptainCompany/captain-food/issues/788).

## What makes a fact "unrecorded" — the criterion, and why it is not a schedule

Seven declared facts park today. They are not a backlog; they share one property:

> **the receiving aggregate has no fold rule answering "is this re-delivered fact already
> reflected?"**

Every fact that DOES have a record arm has one — `domain::payment::already_records` (all ten Payment
facts), the DeliveryJob lifecycle transition table, `record_inbound_order_*`'s status guards.
Without it there is no idempotency anchor, and recording the fact would let a redelivery append a
second copy — on the money path, a second copy that every downstream fold double-counts.

So the deferral is a statement about the MODEL, not about the schedule: *this aggregate does not yet
model this fact.* **The fold rule is what a route move must add FIRST**, and the route follows it.
`deferred: { reason, issue }` in `specs/*/actors.yaml` carries it, and three checks hold the line:

- `fact_route_gate::every_unrecorded_arm_is_a_declared_deferral` — the Rust arms and the DSL
  declarations are the SAME set, in both directions (closing
  [#781](https://github.com/TheCaptainCompany/captain-food/issues/781) for the fact half);
- `fact_route_gate::the_corpus_declares_exactly_the_argued_deferrals` — an explicit ALLOW-LIST,
  never a count, so an eighth deferral fails a test and must be argued for in the same change;
- `fact_route_gate::a_deferral_reason_is_a_modelling_statement_not_a_schedule` — every reason states
  what the model is missing, and schedule words are refused.

## The record leg carries the recorder's own payload — added in review, and the reason it matters

The first cut of this change modelled the record decision as
`FactLeg::Record { recorder: FactRecorder, event: DomainEvent }`: a NAME beside a widened payload.
The independent reviewer proved, by enqueueing a real `RefundOpened` through the real worker
(`status=RECEIVED attempts=1 appended_on_payment_stream=0`), that the money lane's five newly-wired
facts **did not record**. The route widened the lane's typed fact back to a `DomainEvent` and handed
it to the untyped `record_inbound_payment_event`, whose stream lookup ended in `_ => None` over
exactly those five; the typed door written for them — `record_inbound_payment_fact` /
`intent_of_fact` — had **zero production callers**. The claim that a new Payment `receives:` FACT is
a compile error at the place that knows how to find its stream was, for that reason, unenforced.

Three things make this worth an ADR entry rather than a commit message.

1. **It holed the gate built for it.** `fact_route_gate::a_routed_deliver_target_is_never_a_parked_fact`
   reads the `FactLeg::Unrecorded` arms. A fact that is not parked but nonetheless cannot record is
   invisible to it — so adding `("Payment", "RefundOpened")` to `PM_LANE_ROUTED_DELIVERS`, which
   `specs/payments/processmanager.yaml` already declares as a `deliver:`, would have passed the gate
   GREEN while wedging the money lane head-of-line at every refund open until the attempts cap. That
   is the exact failure the gate's own docstring describes, reached by the one path it does not read.
2. **A struct cannot express the constraint; a sum type can.** Two independent fields make
   "recorder X with a payload X cannot resolve" a spellable value, and nothing was checking it.
   `RecordLeg` — one variant per recorder, each carrying the payload THAT recorder takes — makes the
   pairing unspellable instead of merely absent (ADR-20260803-234035, compiler first). The money lane
   carries its generated `PaymentFactInbox`; the remaining recorders still take a `DomainEvent`, and
   typing each is a change to one variant plus one signature.
3. **A green suite that does not predict production is the defect, not the evidence.** The fact
   suite exercised `PaymentCaptured` and `PaymentAuthorized` — both routed before this change — so
   not one of the five arms it delivered was covered. `fact_delivery.rs` now drives all five through
   the real lane against a real Postgres, and both tests were seen RED against the pre-review money
   path (0 delivered, matching the reviewer's probe) before being seen green.

The untyped `record_inbound_payment_event` survives as a **thin adapter**: it narrows onto
`PaymentFactInbox` and delegates, so both doors share one stream lookup and one idempotency rule,
and all ten declared facts are accepted rather than five. Its refusal message now names the event
TYPE only — it rides `DomainError::Repository` into `inbound_messages.error`, a 90-day durable
column, and a `{event:?}` there wrote a full money payload into it (the #623 leak class this same
change tightens on the sirene path).

## Consequences

- **Deployed behaviour delta: zero.** All seven parking facts are unreachable today — no `deliver:`
  routes any of them, and the enqueue side refuses undeclared pairs. What changes is that the BUILD
  now refuses a future omission, and that a routed `deliver:` landing before its fold rule delays
  its fact instead of destroying it.
- **`mailbox_fact_unrecorded_total{actor_type, message_type, reason}`** makes the residue visible;
  `reason: deferred` must be permanently zero in production, and it is kept in its own series
  because a must-be-zero counter mixed with a routine one is a counter everyone learns to ignore.
- **Rollback is `git revert`**: code-only, no config, no migration, no stored event shape.
- **The five newly-wired Payment facts are the one non-zero behaviour delta**, and only for a
  `deliver:` that does not exist yet: `PM_LANE_ROUTED_DELIVERS` routes `("Order", "OrderPlaced")`
  alone, and the Stripe ACL emits only already-routed facts. Their value is that the NEXT step —
  routing `specs/payments/processmanager.yaml`'s declared `RefundOpened` deliver — now lands on a
  path that records instead of one that wedges.
