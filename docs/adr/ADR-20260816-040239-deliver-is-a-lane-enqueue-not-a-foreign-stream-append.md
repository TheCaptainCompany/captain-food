# ADR-20260816-040239 — `deliver:` is a lane ENQUEUE, not a foreign-stream append

- **Status**: Accepted (team ruling under
  [ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md);
  the option space collapsed under an already-merged fence, so an ADR and not a proposal —
  proportionality, founder directive 2026-07-31)
- **Date**: 2026-08-16
- **Realizes**: [#588 "The normal checkout path never enqueues OrderPlaced onto the Order lane — the
  acceptance clock cannot start for saga-appended births"](https://github.com/TheCaptainCompany/captain-food/issues/588)
  · dispatch card [`docs/dispatch/588-order-lane-birth-enqueue.md`](../dispatch/588-order-lane-birth-enqueue.md)
- **Relates**: [ADR-20260816-020752](ADR-20260816-020752-the-loops-context-budget-a-dispatch-card-snapshot-semantics-and-phase-commits.md)
  (dispatch card; phase commits) · [#167](https://github.com/TheCaptainCompany/captain-food/issues/167)
  (acceptance-timeout auto-cancel — schedules apply on the `Recorded`/`Cancelled` arm only, merged)
  · [#590](https://github.com/TheCaptainCompany/captain-food/issues/590) (verdict-blind
  `AlreadyRecorded` re-application — this decision deliberately takes **no** dependency on it)
- **Scope of this record**: the SEMANTIC ruling and the enumeration that bounds its blast radius.
  The emitter change, the config flag and the observability amendment land on
  `588-order-lane-birth-enqueue` under this ADR.

## Context — the code contradicts the spec, and the spec is right

`specs/ordering/processmanager.yaml:112` says:

```yaml
- deliver:
    event: { $ref: 'events.yaml#/OrderPlaced' }
    to: { $ref: 'actors.yaml#/Order' }
    note: "Birth of the Order, materialized from the frozen checkout snapshot; the Order records it idempotently."
```

That is a Tell: `deliver … to: <actor>`. The emitter reads it as a Tell in its own comment —
`crates/application/src/generated/process_managers.rs:656` renders
`// deliver OrderPlaced → Order (the aggregate records the fact)` — and then, five lines below, does
the opposite: `format!("Order-{}", …)` + `Repository::new(store).save(…)`. The PM appends to the
Order's stream itself. **The code is the falsehood; the spec is the truth** (evans).

Two consequences, both live today:

1. **The acceptance clock can never arm on a real order.** `apply_schedules_in_tx` keys the
   acceptance-deadline row off a mailbox delivery whose verdict is `Recorded`
   (`crates/application/src/commands.rs:1088` `record_inbound_order_placed`). No production path
   enqueues `OrderPlaced` onto the Order lane, so `record_inbound_order_placed` never runs for a
   real order and `ENFORCE_ACCEPTANCE_TIMEOUT` gates an unarmed clock — it would read as shipped and
   change nothing (already written down at `specs/ordering/configuration.yaml:64`).

2. **Two aggregates, one transaction — worse than the dispatch card stated.**
   `on_payment_authorized` (`crates/application/src/generated/process_managers.rs:661` and `:673`)
   saves **`Order-{id}` AND `Cart-{id}`**, and `crates/infrastructure/src/mailbox/handler.rs:763`
   flushes both staged appends into ONE delivery transaction. Neither write passes the Order's own
   mailbox, so the birth is not serialised against the Order's writer at all.

## Decision

**A `deliver:` step whose target actor declares the event in its `receives` becomes a mailbox lane
ENQUEUE.** The process manager stops calling `Repository::save` on the foreign stream. It stages an
enqueue intent that `handler.rs` converts, through the typed door, into an `inbound_messages` row
**inside the same delivery transaction**. The target actor's lane worker performs the append, its
aggregate absorbs it idempotently, and the delivery's `Recorded` verdict is what
`apply_schedules_in_tx` keys the schedule on.

### The principle this rests on

> **Being the birth AUTHORITY licenses the DECISION, never the APPEND.**

`PlaceOrderProcess` legitimately decides that an Order shall exist — it holds the frozen checkout
snapshot and the payment outcome. It has no licence to write the Order's stream. Deciding is the
saga's; appending is the aggregate's, behind its own mailbox, which is the serialization point for
that aggregate's writer. One aggregate per transaction (vernon) is what this buys.

### Rejected alternatives

- **B — enqueue alongside the direct append (dual write).** The birth is always already on the
  stream when the lane runs, so the clock could only arm via the verdict-blind `AlreadyRecorded`
  arm #590 flags as "safe today only because the sole route uses `keep`" — a hazard made
  load-bearing on the money path — plus a genuine dual write whose enqueue failure leaves the clock
  silently unarmed. **Rejected.**
- **C — B, made atomic.** Removes the dual-write hole but keeps both the `AlreadyRecorded`
  dependency and a PM writing a foreign aggregate's stream. **Rejected.**

### Binding constraints on the realization

1. **Never in `prepare`.** `crates/actor_runtime/src/completion.rs:69` re-runs `prepare` with **no
   transaction open**, and re-runs it on redelivery. The enqueue happens in `handle`, inside the
   delivery transaction.
2. **The door insert MUST use the passed `&mut Transaction`**, never a pool handle off
   `self.deps`. Get 1 or 2 wrong and the atomicity is a fiction.
3. **Dedup at two layers, both pinned.**
   - Door: `inbound_message_id = UUIDv5(source:external_id)`
     (`crates/actor_client/src/enqueue.rs:188-190`), where **`external_id` MUST be the ORDER ID,
     never the triggering message id** — a redelivered `PaymentAuthorized` carries a different
     mailbox row id and would mint a second birth message. This derivation is **FROZEN**, the same
     treatment `surrogate_actor_id` carries at `crates/actor_client/src/enqueue.rs:484`: changing it
     re-mints the identity of every in-flight and future birth message.
   - Aggregate: `record_inbound_order_placed` already returns `AlreadyRecorded` / `NoChange`
     (`crates/application/src/commands.rs:1102-1108`).
   - **A duplicate enqueue is a SUCCESS outcome to the process manager**, never an error. The PM
     must not fail, retry or skip because the door deduped.
4. **The routing predicate ships behind a config flag, unconditionally** (farley, overturning
   dispatch-card §4's "only if P0 finds more than the Order pair"): gate-then-stabilize does not
   price a money-path commit-path change by blast-count. This turns rollback from a redeploy into a
   flip. The existing `ENFORCE_ACCEPTANCE_TIMEOUT` does **not** cover it — that gates the timeout
   append, not the birth's path. Flipping the default is a separate recorded decision.
5. **No DSL keyword change and no rename** (evans). The DSL already says `deliver … to:`; the spec
   note already says "the Order records it idempotently". Only the mechanics were lying. The
   compiler-first counterpart: a `deliver:` target that does NOT declare the event in its `receives`
   would be the real semantic split, and that becomes a **validator error** rather than a new word
   (see §"Receives-declaration test" — no such step exists today, so the rule lands clean).
6. **Not a migration** (young). Payload, event type and stream name are unchanged; no upcaster.

### What legitimately changes: the ENVELOPE

The lane route stamps `cause_id` from the mailbox row and takes `user_id`/`user_type` from it
(`crates/infrastructure/src/mailbox/handler.rs:344-351`), where the PM append stamps the saga's own
actor. Post-change `OrderPlaced` rows therefore carry a different envelope from historical ones.
This is **legal and permanent — never backfilled**: stored events are immutable and the envelope
faithfully records who actually appended. See the verification below for why nothing reads it.

## P0 findings

### 1. Enumeration — every `deliver:` step and the receives-declaration test

Predicate: does the target actor declare that event in its `actors.yaml` `receives`? (The generated
tabulation is `ACTOR_INBOUND_FACTS`, `crates/actor_client/src/generated/addresses.rs:146`.)

| # | `deliver:` step | Event → target | In target's `receives`? | Generated site |
|---|---|---|---|---|
| 1 | `specs/ordering/processmanager.yaml:82` | `PaymentIntentCreated` → `Payment` | yes | hand-written leg (`process_managers/place_order.rs`) |
| 2 | `specs/ordering/processmanager.yaml:112` | `OrderPlaced` → `Order` | yes | `process_managers.rs:656` |
| 3 | `specs/ordering/processmanager.yaml:118` | `CartCheckedOut` → `Cart` | yes | `process_managers.rs:668` |
| 4 | `specs/delivery/processmanager.yaml:50` | `DeliveryRequested` → `DeliveryJob` | yes | `process_managers.rs:113` |
| 5 | `specs/delivery/processmanager.yaml:119` | `DeliveryDispatchFailed` → `DeliveryJob` | yes | `process_managers.rs:266` |
| 6 | `specs/delivery/processmanager.yaml:163` | `DeliveryDispatchFailed` → `DeliveryJob` | yes | `process_managers.rs:372` |
| 7 | `specs/delivery/processmanager.yaml:207` | `DeliveryDispatchFailed` → `DeliveryJob` | yes | `process_managers.rs:478` |
| 8 | `specs/payments/processmanager.yaml:140` | `RefundOpened` → `Payment` | yes | `process_managers.rs:1103` |
| 9 | `specs/payments/processmanager.yaml:168` | `RefundOpened` → `Payment` | yes | `process_managers.rs:1183` |
| 10 | `specs/payments/processmanager.yaml:196` | `RefundOpened` → `Payment` | yes | `process_managers.rs:1263` |
| 11 | `specs/payments/processmanager.yaml:226` | `RefundOpened` → `Payment` | yes | `process_managers.rs:1343` |
| 12 | `specs/payments/processmanager.yaml:258` | `RefundApproved` → `Payment` | yes | `process_managers.rs:1418` |
| 13 | `specs/payments/processmanager.yaml:280` | `RefundDenied` → `Payment` | yes | `process_managers.rs:1474` |

`specs/common/processmanager.yaml` declares no `deliver:` step.

**Result: 13 of 13 qualify.** The routing predicate catches **every** `deliver:` step in the DSL,
not just the Order/`OrderPlaced` pair — twelve of them on the money path or the dispatch path. This
is the empirical confirmation of constraint 4: the flag is not optional, and the change must ship
routing **only the Order/`OrderPlaced` pair** initially, with the remaining twelve behind the same
flag and flipped as separately recorded steps.

**Receives-declaration test: NO `deliver:` step fails it.** Zero counter-examples, so evans'
"if any target does not declare it, that is the real split" branch is not taken, and the validator
rule making it an error can be added without grandfathering anything.

### 2. Envelope-heterogeneity verification (owed to young, who did not audit the fold set)

**No BAM fold, projection or SQL view groups or filters on `user_type` or `cause_id` for
`OrderPlaced` — or for any event.**

- `specs/business_metrics.yaml` contains no occurrence of `user_type`, `cause_id`, `user_id` or
  `causation`. The three `OrderPlaced` folds key on `orderId` and `restaurantId`
  (`specs/business_metrics.yaml:45,47,58-64`) — payload properties only.
- `specs/database/` mentions the two columns only in their own DDL
  (`tables/eventstore.yaml:19,21`, `tables/journals.yaml:52,54`); no `View_*` selects or groups on
  them.
- No projector reads them: `crates/infrastructure/src/projection/**` contains no occurrence of
  `user_type` or `cause_id`. Every other Rust hit is on the WRITE side (the envelope's own
  construction in `actor_client`, `mailbox`, `graphql/generated/mutation.rs`) or a scalar
  declaration.

Conclusion: the envelope change is invisible to every read model. It is recorded here so a future
fold author knows `OrderPlaced.user_type` is heterogeneous across the flip date by design.

**Read paths at mutation return**: nothing assumes `Order-{id}` exists when a mutation returns. The
birth is *already* asynchronous w.r.t. the `placeOrder` mutation today — it happens on the Stripe
`PaymentAuthorized` webhook, not on the checkout call — so Option A adds one lane hop, not a new
asynchrony class. The only stream-name reader outside `application`/tests is the GraphQL
subscription filter `crates/server/src/graphql/generated/subscription.rs:115`, which waits for
events rather than asserting the stream exists.

### 3. The reclamation second unlaned birth site — LIVE, and it cannot use this seam

The dispatch card recorded `ProcessManagerRunner`'s liveness as unverified. **It is live**: spawned
at `crates/server/src/lib.rs:793`, default-on at `crates/server/src/generated/config.rs:867`.
`runner.rs:319-332` unconditionally filters out `PaymentAuthorized`/`PaymentCaptured`/
`PaymentFailed`/`PaymentRefunded`, so `runner.rs:414` is dead for the checkout birth — but
**`ReclamationResolved` is NOT filtered**, so `runner.rs:487` →
`crates/application/src/process_managers/reclamation.rs:104` → `commands::place_replacement_order`
**is reachable today**. It writes `Order-{id}` with **no transaction and no lane**, from the
runner's polling tick, and therefore **cannot use the staged-intent seam** (the runner owns no
delivery transaction to stage into).

Consequence: after #588 lands, the replacement-order birth remains an unlaned foreign-stream append
and its acceptance clock stays unarmed. A follow-up issue tracks moving the reclamation route onto
the mailbox; it is explicitly out of #588's scope. **A replacement order that no restaurant is told
about is the domain lens's worst failure mode**, so this is a known, recorded gap and not an
accepted end state.

### 4. Not anticipated by the dispatch conditions: three staged-set consumers strand

`handler.rs:771-787` runs three things off the PM delivery's `staged` set, all of which currently
see `OrderPlaced` because the PM appended it there:

- `super::record_order_placements(&staged)` — the #456 "a stranger paid us" BAM counter. It is
  called at **`handler.rs:771` ONLY**, i.e. exclusively on the PM-fact route; the inbound-fact route
  (`handler.rs:441-455`) does not call it. Option A moves `OrderPlaced` out of that staged set, so
  **this counter silently goes to zero** unless the realization also records placements on the
  inbound-fact route.
- `DeliveryActivation::promote_after_commit(&self.activations, None, &staged)` — whose own comment
  names "the saga leg's appends (OrderPlaced on the Order stream)" as what stales out held copies.
- `self.fanout_delivery(&staged, None, promote)` — the subscription fanout.

The latter two are already invoked on the inbound-fact route and follow the append automatically;
the counter is the real hole. Recorded here so the realization treats it as in-scope, not as a
regression discovered by a dashboard going flat.

## Consequences

- **Positive**: one aggregate per transaction on the checkout saga's most expensive leg; the Order's
  mailbox becomes the serialization point for its own birth; the acceptance clock arms on the
  canonical `Recorded` arm with **no dependency on #590**; spec and code stop contradicting each
  other; rollback becomes a config flip.
- **Cost**: the Order birth gains one lane hop of latency after the payment authorization commits.
  Tolerable under the recorded acceptance-first PENDING model, and measured rather than assumed —
  the realization adds a birth-lag histogram (enqueue → `Recorded`), which nothing measures today.
- **Observability contract must be amended in the realizing PR** (farley): `specs/observability.yaml:226`
  makes `event.store.append` a REQUIRED span for `place-order` success. Option A moves that append
  into a different mailbox delivery, so both the success rule and the 800 ms p95 budget silently
  change meaning — a green dashboard for a saga that no longer does the measured thing. The append
  becomes non-required there, and the birth-lag histogram is added. Peak-load coverage is already
  provided by #586's gauges.
- **Reversibility: hard to reverse.** Reversible in code, not in state — once a birth has ridden the
  lane in production those rows exist. Money-path saga commit path + mailbox runtime ⇒ **`HOLD: human`**
  at ready-for-review per
  [ADR-20260815-115220](ADR-20260815-115220-auto-merge-on-green-by-default-hold-human-for-the-named-class.md)
  as amended by
  [ADR-20260815-134655](ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md).

## Standing fences the realization must not break

- No double-append of `OrderPlaced` — `commands.rs:1102` stays the absorber; never a second
  `Repository::save`.
- Idempotent birth under at-least-once redelivery — dedup at the door, keyed on the order id.
- Enqueue inside the delivery transaction; never after commit, never in `prepare`.
- The GDPR retention/expiry schedule is untouched; no new route may key it.
- No new dependency on #590's verdict-blind arm.

## Consulted

Per [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)
and the mob-programming directive
([ADR-20260809-013142](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md)); whole
roster invited, cheap excuse allowed. Five lenses returned load-bearing findings, all PASS:

- **vernon** (aggregate boundaries): PASS. Found the violation is two-aggregates-one-transaction,
  not one; supplied the birth-authority principle; pinned the `prepare`/transaction and door-id
  constraints and the duplicate-enqueue-is-success rule; corrected the card's "runner not live" —
  the reclamation route IS reachable.
- **young** (write side): PASS. Payload/type/stream unchanged ⇒ not a migration, no upcaster; the
  envelope change is legal and never backfilled; required the fold-set verification he had not
  performed, delivered in §2.
- **farley** (release path): PASS with an overturn of card §4 — the flag is unconditional, and the
  observability contract is amended in the same PR or the dashboard lies.
- **evans** (ubiquitous language): PASS. No keyword split, no rename; the code was the falsehood.
  Compiler-first condition: undeclared `deliver:` targets become a validator error, not a new word.
- **beck** (proof): PASS, conditional on red-first evidence — the named failing test plus three
  data-only mutations, authored and proven red at the pre-change HEAD before any emitter diff.
