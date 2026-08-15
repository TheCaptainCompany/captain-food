# ADR-20260815-030206 — A process manager is a write-side component and never reads the read side

**Status**: Accepted (the rule and its two carve-outs; the *enforcement grammar* and the
*mailbox-query* reading are OPEN register rows, below) · **Date**: 2026-08-15 ·
**Decider**: the founder / Tech CEO, verbatim below ·
**Register**: [DECISIONS](../proposals/DECISIONS.md) §42 (**PMW-1**, **PMW-2**, **PMW-3**) and the
**STO-9** annotation in §32 ·
**Session**: https://claude.ai/code/session_018WtW3eyd4yWFKHTUEQYJkM

## Enforced by

No `rules.yaml` entry, and that is deliberate rather than an omission: this ADR states a **structural**
guarantee about which side of the system a component reads from, not a business behaviour a customer or
a restaurant can observe, so it has no `Given/When/Then` to pin in `specs/tests.yaml`. Its enforceable
form is a **validator rule** over the PM step DSL — register row **PMW-1** — which does not exist yet
because there is currently no way to *spell* "fold the aggregate's stream" in a `read:` step. Until
PMW-1 lands, this record is prose, and prose is exactly as reliable as CLAUDE.md says prose is. The
money-path consequences it documents are pinned by existing rules
(`rules.yaml#/PaymentCapturedOnFulfilment` and its CRITICAL-1 regression test,
`crates/infrastructure/tests/main/order_projection.rs:474`).

## The directive (verbatim, founder / Tech CEO, 2026-08-15)

> "The actors are always up to date and they are kept in memory for a small amount of time to avoid
> reloading the stream uselessly
> We need a concrete example of process manager to decide the place order is the right one
> Process managers should never use the read side to work it's a write side component
> You will see that if the process manager ask the actors what it needs instead of risky projections
> the code will be simpler and secured in terms of hydratation data"

Earlier in the same exchange:

> "The actors can be queryable ... the process managers will directly ask to the source of truth ...
> We just have to put in place the grpc transport ... I don't think we should involve inbound messages
> table for queries to actors."

## The rule

**Intent, in the founder's own frame — a process manager is a write-side component; it does not read
the read side.** A projection exists to answer a *query*; a process manager exists to *decide*. It must
take its facts from the same place the write side took them: the event log.

**What is actually gated is narrower, and the narrowing is team-found, not a softening:**

> A process manager never reads a **projection** to learn a fact about an **aggregate it can address by
> identity**.

That is the form a validator can enforce and the form every money-path defect below violates. The
broader sentence is the intent; the narrower one is the rule. Two carve-outs make the difference, and
both are load-bearing.

### Carve-out 1 — referential / configuration tables are NOT the read side

`DispatchStrategyRepository` reads `CityDeliveryRanking`, `RestaurantDispatchConfig` and
`DeliveryChannelCatalog`. These are **operator-authored reference data** declared in
`specs/database/tables/referential.yaml` — seeded configuration, not folds of `domain_events`. Reading
them is not "reading the read side"; there is no projector, no lag, no rebuild and no lineage to be
wrong about. `crates/application/src/process_managers/delivery_dispatch.rs:14` already states this in
its module header: *"All of it reads the config tables through `DispatchStrategyRepository`, not the
log."* The rule leaves that read exactly where it is.

### Carve-out 2 — set-shaped / index-shaped reads have no actor to ask

"Ask the actor" presupposes there **is** an actor and that you can name it. A question of the form
*"which open carts exist for this session?"* names no aggregate: it is an index over a set.
`CartBindingProcess` calls `open_by_session` (`cart_binding.rs:17-37`) and **there is no `Session`
aggregate** — verified: no `Session` actor exists in any `specs/*/actors.yaml`. There is nothing to ask.
The same shape covers `place_order`'s server-side repricing, which walks the live catalog through
`CatalogReadRepository` (`crate::pricing::price_cart`, `crates/application/src/commands.rs:2457`): a
cart's lines resolve against a catalog *tree*, not against one addressable aggregate.

These reads are **exempt from the gate, not blessed**. They remain genuine read-side dependencies of the
write path and stay open under register rows **STO-7** (catalog) and **STO-9** (the cart-binding leg).
What this ADR changes for them is that they are now the *named exception* rather than the unremarked
norm.

## The two readings of "ask the actor" — they carry different risk, and only one is adopted

**(i) FOLD the aggregate's own stream, in-process.** The PM loads the stream through the `EventStore`
port it already holds and folds it with the domain's own fold function. No transport, no lease, no
fencing question — it is the same read the command handler on that aggregate performs, and it is
already how two paths work today:
`crates/application/src/process_managers/place_order.rs:47` (folds `Payment-{intentId}` for the
checkout snapshot) and `delivery_dispatch.rs:126` (*"The pickup address — folded from the aggregate's
own stream, like the restaurant command handlers"*). **This reading is ADOPTED.**

**(ii) A QUERY MESSAGE to the actor through the mailbox** — the founder's *"the actors can be
queryable"*, transported over gRPC. This is a genuinely different mechanism and it is **NOT adopted
here**, for two reasons the team raised:

- **Fencing.** The lease fence in `crates/infrastructure/src/mailbox/` is built from a message's
  `message_id`/`position`; a *query* has neither, so there is nowhere to put the guard. An unfenced
  read served by a lease holder can be served by a lane whose lease has already moved.
- **Head-of-line.** A query queued behind commands on the same lane puts an unrelated caller's latency —
  and, on the settlement lane, a Stripe capture — behind whatever the actor is doing.
- **And the obvious mitigation — "the caller re-asserts the served version inside its own fenced
  transaction" — CANNOT CLOSE for any leg with an external effect.** `complete_fenced` runs
  `handler.prepare(message)` **before** `pool.begin()` (`crates/actor_runtime/src/completion.rs:69`), and
  `pm_delivery.rs:61-89` runs the whole `place_order` handler — **Stripe intent creation included** —
  inside `prepare`. So the order of events is: read → **irreversible money movement** → open transaction
  → re-assert → abort. The re-assert converts a *silent* wrong capture into a *loud* one plus a stuck
  `RECEIVED` row. Better; not closed. The compiler-first consequence is recorded as part of **PMW-3**:
  **a validator rule refusing an actor-sourced `read:` step in any leg that also contains a `call:`
  step** — both node kinds already exist in the step DSL, so it is a cheap gate rather than prose.

Reading (ii) is register row **PMW-3**, OPEN. Nothing in this ADR authorises building it. The founder's
own *"I don't think we should involve inbound messages table for queries to actors"* points at the same
problem from the other side and is recorded there.

## Why the rule holds — three independent arguments

- **Young.** A projection is a **disposable, rebuildable fold maintained for query**. That disposability
  is the whole basis of the recorded recovery posture (`replay`, STO-6). If a process manager's
  decisions depend on a projection, then a projection **rebuild changes write-side behaviour** — which
  is precisely what disposability is supposed to guarantee against. You cannot have both.
- **Vernon.** A process manager coordinates aggregates by *sending commands and reacting to events*. Its
  durable state is **its own process-state row**, not another consumer's query model. Reading a query
  model makes the saga a downstream consumer of a component it does not own and cannot version.
- **This repo's dependency rule does not catch it.** The violation is **intra-layer**:
  `OrderReadRepository` is a port declared in `application` (`crates/application/src/queries.rs:356`)
  and the process managers live in `application` too. Outer→inner is satisfied at every site. That is
  exactly how thirteen declared `read:` steps accumulated with the compiler silent — and why the rule
  needs a gate of its own rather than a stricter reading of the existing one.

## The evidence that it already cost money

- **`HookOutcome::Skip` conflates four different worlds into one value.**
  `crates/application/src/process_managers/payment_settlement.rs:85-88` returns `Skip` when the
  `OrderTracking` row is absent — but "the order does not exist", "the projector is behind", "the
  projector crashed" and (post-split) "the table is unreachable" all arrive as the same `None`. And
  `Skipped` is **TERMINAL**: `crates/infrastructure/src/process_manager/runner.rs:350-358` logs a
  `warn!` and the checkpoint advances **unconditionally** at `:376`. A projector **200 ms** behind
  therefore means a delivered order is **never** captured and the ~7-day authorization ages out. The
  same shape sits at `delivery_dispatch.rs:106-111`: a READY order is **never dispatched**, silently.
- **CRITICAL-1 — a money decision that depended on a projector's column lineage.** The projector's
  `OrderPlaced` arm hardcoded `payment_intent_id: None`, so a charging order folded to a NULL intent and
  the capture guard skipped in silence. It is fixed and regression-tested end-to-end through the real
  projector and the real saga runner (`crates/infrastructure/tests/main/order_projection.rs:474`). The
  lesson is not the bug; it is the **dependency shape** that made the bug possible.
- **`reclamation.rs:148` — `if order.payment_status != "CAPTURED"` — a STRINGLY-TYPED guard on a refund
  decision**, because the projection row hands back a `String`
  (`crates/application/src/projectors/order_tracking.rs:121-140` builds it as one). `domain::payment::fold()`
  hands back a `PaymentStatus` **enum**. **The read side erases the type; the actor preserves it.** This
  is the compiler-first argument (ADR-20260803-234035) and it needs no freshness claim at all — it holds
  even if projections were instantaneous.
- **TWO INDEPENDENT FOLDS COMPUTE PAYMENT STATE TODAY, AND THE MONEY DECISION IS TAKEN ON THE WEAKER
  ONE.** `domain::payment::fold` (`crates/domain/src/payment.rs:42-63`) is the aggregate's own fold into a
  typed `PaymentState`. `OrderTrackingProjector::payment_status`
  (`crates/application/src/projectors/order_tracking.rs:121-139`) is a hand-maintained **string** column
  with its own arm list and a `_ => prev` fallthrough. Two folds over the same facts, maintained by
  different people for different purposes, **can disagree** — and the capture/refund guards read the one
  built for a customer tracking screen. This is the cleanest statement of the rule: not "the projection
  might be stale", but *"there are two authorities and we picked the weaker"*.
- **A second, divergent projector exists in the test bed.**
  `crates/application/src/behaviour_support.rs:269-311` hand-writes its own `payment_status` fold for the
  behaviour suite, and it does **not** match the real one: it carries `PaymentAuthorized` and
  `PaymentIntentCreated` arms the real projector (`projectors/order_tracking.rs:121-140`) deliberately
  lacks — the real one seeds `AUTHORIZED` from `OrderPlaced` because the authorization sits at an
  earlier log position than the row's creation. Not a live bug; a **live lie shape** — the suite proves a
  fold nothing in production runs. It **deletes** under this rule, because a PM that folds the stream
  needs no projector double.

## What the rule actually buys, measured — ONE read database of three, not a collapse

The layering rule is often heard as *"and then STO-7, STO-8 and STO-9 all dissolve"*. They do not. The
measured accounting, per read database, counting the `captain_write` readers:

| Read database | Who reads it from `captain_write` | Effect of this rule |
|---|---|---|
| **`read_order`** | Settlement ×4, dispatch, reclamation, cart-binding — **all of them PM legs** | Close them and `captain_write` **drops CONNECT on `read_order` entirely**. **STO-9 closes.** This is the win. |
| **`read_common`** | 2 PM steps (already folds) **plus NINE aggregate command handlers** — `verify_phone`, `request_email_verification`, `request_phone_change`, `confirm_phone_change`, `create_catalog`, `add_product`, `update_product`, `mark_restaurant_as_favorite`, `record_prospect_contact` | The directive says *process managers*; these are **command handlers**. **Removes zero of the nine. STO-8 is untouched.** |
| **`read_catalog`** | `require_orderable_line` (add-to-cart — a plain Cart command, not a PM) and `price_cart` (inside `place_order`) | **At most the checkout leg. STO-7 does not close.** |

**Net: one of three read databases loses its `captain_write` CONNECT.** That is real and worth taking;
it is not the collapse, and anyone planning the physical split on the assumption that it is will plan a
wall that cannot be taken.

## "Ask the Order actor" cannot answer the settlement guard on its own — and the reason is structural

The nine `OrderTracking` reads want **`payment_intent_id` AND `payment_status`**. **Neither is on
`OrderState`** (`crates/domain/src/order.rs:24-43` carries `status`, `restaurant_id`, `customer_id` and
three rate-once flags — no payment field at all). `payment_status` is folded by the **projector** from
`PaymentCaptured`/`PaymentReleased`/`PaymentRefunded`/`OrderPlaced`
(`crates/application/src/projectors/order_tracking.rs:121-139`) — i.e. it is a **cross-aggregate join,
Order ⋈ Payment, performed only by the projector**. So "ask the Order actor" as stated would ask an
aggregate for a fact it does not hold.

The authority for all nine guards is **`PaymentState`** (`crates/domain/src/payment.rs:42-63`), which
carries `order_id`, `status`, `capture_failed`, `refund_opened` and `refund_decision`. What is genuinely
missing is only the **routing key**: `OrderDelivered` carries `{order_id, restaurant_id}` and not the
intent, so a PM reacting to it cannot name the `Payment-{intentId}` stream.

**Recommended sequence, recorded as the cheapest closure: fold `payment_intent_id` onto `OrderState`
from the `OrderPlaced` the Order aggregate already owns.** The field is **already in that payload** — so
this is **no event migration, no new event, one fold field**. The leg then reads: *ask Order → intent;
ask Payment → status* — two by-key folds on terminating streams, both inside `captain_write`.

## Where the team pushed back on the founder — recorded honestly

- **"The code will be simpler" is FALSE on the production lines for the money PMs.** Settlement and
  refund go from **1 read** to **2 folds** — `Order-{id}` for the `paymentIntentId`, then
  `Payment-{intentId}` for the status — plus one more error arm. Net: flat to **+5 lines** on those
  paths. It is TRUE on the **dependency graph** (−1 port type), on the **test bed** (−4 fake classes,
  ≈−375 lines of scaffolding, including the divergent projector above) and on the **system** (**STO-9**
  closes — and *only* STO-9, per the accounting above). **Sell this as correctness, not concision** —
  anyone who lands it expecting a smaller diff on the money path will report the rule as failing.
- **Residency does NOT currently help, as built — and on one actor it actively hurts.**
  `crates/infrastructure/src/mailbox/activation.rs:237-240` routes any `stream_name != self.scoped`
  straight to `self.inner.load(...)` — i.e. it **bypasses the cache for exactly the cross-aggregate loads
  this rule creates** — and the module header says so in words. So the founder's *"kept in memory for a
  small amount of time to avoid reloading the stream uselessly"* describes the design intent correctly,
  and the code has a **scoped-only** restriction that would have to lift first. Three findings make that
  lift a build item rather than a given:
  - **The fence does not generalise.** `guard_freshness_in_tx` (`activation.rs:127-148`) compares ONE
    held version against `MAX(version)` for ONE scoped stream.
  - **Payment activations do not work at all today.** `surrogate_actor_id`
    (`crates/actor_client/src/enqueue.rs:478`) makes the mailbox `actor_id` a UUIDv5 of
    `"Payment:<intentId>"` while the stream is `Payment-pi_xxx` (`crates/domain/src/payment.rs:26`), so
    `scoped` (`activation.rs:87`) never matches — **the aggregate the settlement query needs is precisely
    the one residency cannot serve.** Fixable by keying the cache on the stream the handler asks for.
  - **Catalog makes residency worse, not neutral.** `estimate_bytes`
    (`crates/infrastructure/src/mailbox/activation.rs:200`) sums serialized payload over the WHOLE held
    stream, and `put_locked` (`crates/actor_runtime/src/activation.rs:142-181`) **inserts first, then
    evicts LRU** until under the 64 MB bound. The just-inserted entry has the highest `last_used`, so a
    large Catalog fill **evicts every resident Order, Cart and Payment first**; a Catalog bigger than the
    bound then evicts itself too and `total_bytes` returns to 0. Peak-time hazard: a HubRise import burst
    plus one repricing query makes every subsequent order delivery pay a cold refold — and it is
    currently **invisible**, because `specs/observability.yaml` declares no activation hit-ratio, bytes
    or eviction counters.
  - **Sizing supports the direction anyway**: Order/Payment/Cart at Tours V0 peak ≈ 25 in-flight orders +
    25 payments + ~100 open carts at 5–15 KB/stream = **under 5 MB against a 64 MB bound** — residency is
    over-provisioned by two orders of magnitude for exactly the actors the money path needs. **Catalog is
    the outlier and needs its own answer** (a snapshot at the last full-replace event, or content-hash
    no-op suppression in `import_catalog`, which appends unconditionally today at
    `crates/application/src/commands.rs:3155`). Register row **PMW-2**.
- **This does NOT make settlement transactional, and must never ship described as "race-free".**
  `Payment-{intentId}` is a **different stream**, fed by the Stripe webhook path, with **no ordering
  relation** to `Order-{id}`. Folding it closes the projector-lag window (seconds) down to a
  fold-to-decision window (microseconds) — it does not close it. Double-capture protection remains what
  it is today: **Stripe idempotency keys plus the AUTHORIZED guard**.
- **`PlaceOrderProcess` is the best ARGUMENT and the worst DEMONSTRATION.** The founder asked for a
  concrete example and named place-order; the honest answer is that place-order is **already compliant**
  — `crates/application/src/commands.rs:2391-2394` folds the Restaurant stream (*"folded from ITS
  stream (authoritative, race-free; the saga may read other aggregates' streams through the same
  EventStore port)"*) and `:2419` folds the Cart. The diff on it is **empty**. It proves the rule is
  right and demonstrates nothing.
  **What it did expose is a SPEC↔CODE DRIFT, at the head of the checkout path**:
  `specs/ordering/processmanager.yaml:30-43` declares `PlaceOrderProcess` reading the **Cart and
  Restaurant PROJECTIONS** (`model: { $ref: 'database/tables/projection_tables.yaml#/Cart' }` and
  `#/Restaurant`). The code does not. **The spec is wrong, not the code.** Recorded here, deliberately
  **not fixed** in this change (this is a docs-only record; and the same two lines are being touched by
  in-flight work — see the note below).
- **`place_order`'s repricing is a genuine, remaining read-side dependency** (`price_cart` through
  `CatalogReadRepository`, `commands.rs:2457`) — exempt under carve-out 2, still open under STO-7. The
  checkout path is therefore *not* fully write-side today, and this ADR does not claim it is.

## Consequences

- **Positive.** A settlement decision stops depending on a rebuildable artifact's freshness and column
  lineage; a refund guard stops comparing strings where an enum exists; STO-9 gains an option that costs
  **zero new tables**; the behaviour suite loses a projector double that can silently diverge from the
  real one.
- **Negative / accepted.** Two folds where there was one read on the money path; each fold is a stream
  load, and the residency cache does not serve them today (PMW-2). Neither the settlement race nor the
  catalog/session set-shaped reads are closed by this decision.
- **Bounded, and the bound is measured.** ONE of three read databases (`read_order`) loses its
  `captain_write` CONNECT. `read_common`'s nine command-handler readers and `read_catalog`'s two are
  **outside** this rule, so **STO-7 and STO-8 remain open exactly as they are**.
- **Not decided here.** The DSL grammar that makes the rule spellable (**PMW-1**), the cross-aggregate
  residency lift and its fence (**PMW-2**), and actor queries as a mailbox/gRPC message (**PMW-3**).
- **No code, no `specs/**` edit lands with this ADR.** The spec↔code drift above is **recorded**, not
  fixed. An executor closing it must first check the in-flight
  [#564](https://github.com/TheCaptainCompany/captain-food/issues/564) branch, which already annotates
  those very steps (see below).

### One thing found while writing that sharpens PMW-1

The `source: PROJECTION | EVENT_STREAM` enumeration is **not on `main`**; it exists on the in-flight
`564-mechanical-reader-derivation` branch, where `specs/ordering/processmanager.yaml:32,43` already
carry `source: EVENT_STREAM` on exactly the two `PlaceOrderProcess` steps this ADR names as drifted.
So the drift is real on `main` and its correction is already in flight — PMW-1 is therefore a question
of what the **final** grammar is, on top of an enumeration that is landing, not a green field. That
branch is owned by another session and is untouched by this record.

## Consulted (ADR-20260812-143619 — one line per lens)

**Round 1 — all eight lenses answered:**

- **architect**: The rule is right and the enforceable form is narrower than the sentence; the value is
  that the violation is intra-layer, so nothing existing catches it — record the narrow form or the gate
  cannot be written.
- **dba**: The three dispatch tables are seeded referentials, not folds — carve them out explicitly or
  the first reader of this rule will "fix" a correct read. Cross-stream folds are two loads, not one;
  say so.
- **beck**: The test-bed win is the real one (−4 fakes, ≈−375 lines) and the divergent
  `behaviour_support` projector is a lie shape that deletes under the rule — but do not sell the
  production diff as smaller, it is not.
- **farley**: A projection rebuild changing write-side behaviour is the deployability argument; that is
  what makes this a pipeline concern and not a style preference.
- **graphql**: Nothing in my lens — no schema surface moves; a PM is not a resolver.
- **holub**: **Round 1** — fence it at the one call site that touches money (settlement) and leave the
  rest; a broad rule over thirteen sites is a big-bang refactor wearing a principle's clothes.
- **observability**: `Skipped` being terminal with an unconditional checkpoint advance is the defect
  that makes every one of these silent — the dead-man's-switch on authorization age
  (`payment_authorized_unsettled_age_seconds`) is what catches it today, and it stays owed regardless of
  this rule.
- **ux**: Nothing in my lens (the customer-visible surface of a skipped capture is *nothing happening*,
  which is the point of the observability line above).

**Round 2 — four lenses re-read the drafted rule:**

- **architect**: The two carve-outs and the (i)/(ii) split are what make it landable; PlaceOrderProcess
  must be recorded as the argument-not-demonstration or the next reader will open an empty PR.
- **dba**: **landed after the drafting began and changed three things in it** — (1) the grant accounting
  is **one read database of three**, not the collapse of STO-7/8/9, because `read_common`'s nine readers
  are aggregate command handlers and `read_catalog`'s two are a plain Cart command plus `place_order`'s
  repricing; (2) "ask the Order actor" cannot answer the settlement guard, because neither
  `payment_intent_id` nor `payment_status` is on `OrderState` — `PaymentState` is the authority and the
  cheapest closure is folding `payment_intent_id` onto `OrderState` from the `OrderPlaced` the aggregate
  already owns (no migration, one fold field); (3) the caller-side re-assert cannot close for any leg
  with an external effect (`prepare` runs before `begin`), so the gate is *no actor-sourced read in a leg
  that also `call:`s. Also dropped `lane.ownership_version` from his own earlier "reply carries"
  formulation — stream version is monotonic under append, and the one non-monotonic mover (GDPR stream
  deletion) moves it DOWN and still trips equality, so `(stream_name, served_version)` suffices and the
  lane read is a wasted round trip. Plus the two residency facts above (Payment activations never match;
  Catalog evicts the money actors).
- **beck**: With the enumeration already in flight on #564, the compiler-first answer is likely a ~20-line
  validator rule, not a richer DSL — write PMW-1 so it does not presuppose new grammar.
- **holub**: **REVERSED his round-1 position.** The reframe from *"stop reading projections"* to
  *"a PM never reads a projection to learn a fact about an aggregate it can address by identity"* makes
  the work **smaller**, not bigger — the carve-outs remove most of the thirteen sites from scope, and
  what remains is the money path he wanted fenced anyway. Recorded explicitly because a reversal that is
  not recorded reads as a lens that never objected.
