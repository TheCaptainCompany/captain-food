# ADR-20260815-030206 — A process manager is a write-side component and never reads the read side

**Status**: Accepted (the rule and its two carve-outs; the *enforcement grammar* and the
*mailbox-query* reading are OPEN register rows, below) · **AMENDED the same day — see
[Correction (2026-08-15)](#correction-2026-08-15--reading-i-is-a-different-destination-not-an-intermediate-step)
before relying on the (i)/(ii) section** · **SCOPE CLARIFIED the same day —
[`place_order` is a command handler, not a PM leg](#scope-clarification-2026-08-15--place_order-is-a-command-handler-not-a-pm-leg);
the rule is unchanged, its reach was being overstated** · **CORRECTED 2026-08-31 — two sections that
were true on 2026-08-15 are false at HEAD; see the corrections in [Enforced by](#enforced-by) and
[One thing found while writing that sharpens PMW-1](#one-thing-found-while-writing-that-sharpens-pmw-1).
**The rule itself is unchanged**; its enforceable form moved to the retirement of `read:`
([ADR-20260831-121957](ADR-20260831-121957-the-pm-read-step-is-retired-source-fixed-the-physics-and-left-the-ownership.md))** ·
**Date**: 2026-08-15 ·
**Decider**: the founder / Tech CEO, verbatim below ·
**Register**: [DECISIONS](../proposals/DECISIONS.md) §42 (**PMW-1**, **PMW-2**, **PMW-3**) and the
**STO-9** annotation in §32 ·
**Session**: https://claude.ai/code/session_018WtW3eyd4yWFKHTUEQYJkM

## Enforced by

No `rules.yaml` entry, and that is deliberate rather than an omission: this ADR states a **structural**
guarantee about which side of the system a component reads from, not a business behaviour a customer or
a restaurant can observe, so it has no `Given/When/Then` to pin in `specs/tests.yaml`. Its enforceable
form is a **validator rule** over the PM step DSL — register row **PMW-1**.

> **CORRECTION (2026-08-31).** The two sentences that stood here were true on 2026-08-15 and are false
> at HEAD; one of them produced a **false negative in a register check on 2026-08-31**, which is the
> concrete cost that earned this note. They read: *"which does not exist yet because there is currently
> no way to* spell *'fold the aggregate's stream' in a `read:` step"* and *"Until PMW-1 lands, this
> record is prose, and prose is exactly as reliable as CLAUDE.md says prose is."*
> **PMW-1 landed.** [PR #566](https://github.com/TheCaptainCompany/captain-food/pull/566) merged
> **2026-08-16** as `b0fd7fdf`; the gate `pm-read-source` is live at
> `tools/codegen-rs/src/validate/process_managers.rs:456`, over the closed `READ_SOURCES` at `:49`.
> So this record has not been prose since 2026-08-16.
> **And the enforcement answer has since been REVERSED** — the founder retired `read:` entirely on
> 2026-08-31 ([ADR-20260831-121957](ADR-20260831-121957-the-pm-read-step-is-retired-source-fixed-the-physics-and-left-the-ownership.md),
> challenge row **PMW-4**). **The rule below is UNCHANGED**; what moved is its enforceable form, from
> *a validator rule over `read:`* to *there is no `read:`* — the level-4 (unrepresentable-state) form
> of this same directive (ADR-20260803-234035), not a change of rule. The retirement needs no new gate:
> the step matcher is already closed, so deleting the `"read"` arm (`:423`) makes the kind unspellable
> by the catch-all that already refuses unknown kinds (`:854`).
> **Nine of the steps this record judges are still standing violations** — eight on the money path
> (`specs/payments/processmanager.yaml:53,70,86,101` settlement, `:132,161,189,219` refund, all on
> `OrderTracking`) and one on dispatch (`specs/delivery/processmanager.yaml:36`).

The
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
own stream, like the restaurant command handlers"*). **This reading is ADOPTED** — but **not** as the
final shape, and **not** as a step on the way to (ii). See
[Correction (2026-08-15)](#correction-2026-08-15--reading-i-is-a-different-destination-not-an-intermediate-step):
it is a **different destination**, it permanently buys shared-database coupling between
independently-deployed pods, and it does **not** deliver the founder's stated in-memory premise.

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

## Correction (2026-08-15) — reading (i) is a DIFFERENT DESTINATION, not an intermediate step

*Added after this ADR landed, on the architect's review of it. Nothing above is deleted; this section
overrides the (i)/(ii) reasoning where the two disagree. The rule itself — a PM never reads a projection
to learn a fact about an aggregate it can address by identity — is **unchanged and still Accepted**.
What was wrong is the framing of the adopted mechanism.*

The section above adopts (i) on the unstated theory that **the `EventStore` port is the final vision and
the adapter behind it is a deployment detail** — i.e. that today's in-process fold becomes tomorrow's
actor query by swapping an adapter. **That theory is false**, and the ADR must not rest on it.

### 1. `EventStore::load` is a STORAGE port, not an actor-query port

`crates/application/src/ports.rs:65`:

```rust
async fn load(&self, stream_name: &str) -> Result<(Vec<DomainEvent>, i64), DomainError>;
```

Its signature is *"give me the rows of this stream"*. **There is no gRPC service it becomes.** Its remote
form is not "the PM asks the Order actor" — it is **"the PM pod opens a connection to the write
database"**. So (i) and (ii) are not the same destination reached at different times: (i) is a
**shared-database read** and (ii) is a **service call**, and adopting (i) permanently chooses
shared-database coupling between independently deployed pods.

That may still be the right call for V0 — it costs zero infrastructure and closes STO-9. But under
**ADR-20260808-235113 (final vision first)** the choice must be *named* rather than glossed, because a
"temporary" mechanism that is actually a terminal one is exactly the intermediate step that directive
forbids shipping unlabelled. **Named here: adopting (i) means `captain_write` keeps a CONNECT from every
PM pod, indefinitely, and the read the PM performs is a database read and not an actor question.**

### 2. Reading (i) does not deliver the founder's own stated premise

The directive's first line is *"the actors ... are kept in memory for a small amount of time to avoid
reloading the stream uselessly"*. `ActivationCache` (`crates/actor_runtime/src/activation.rs:1-21`)
lives **in the owning worker's process**, is **lane-tagged**, and is **dropped on lease loss**. A PM
running in a different pod and folding `Payment-{intentId}` cannot touch it: it re-folds from the log on
**every** settlement leg.

So on the founder's own criterion, **(i) is SLOWER than what he asked for** — it is the "reloading the
stream uselessly" case, one settlement leg at a time. PMW-2 records the residency lift that would fix it;
until that lands, (i) is correct and cold. This is a cost of the adopted reading, not an argument against
the rule.

### 3. The final-vision form already exists one file over, and IS recordable as the target

`specs/services.yaml:16-28` declares **`binding: local | http` as a SPEC-OWNED key** — the topology is a
spec decision, not a code decision. And the generated service types derive serde **unconditionally,
regardless of binding** (`crates/application/src/generated/services.rs:48-60`): `PaymentRequestInput`
carries `Serialize, Deserialize` today even though `payment` is `binding: local`. **The wire shape is
forced whether or not the binding is remote** — which is what makes flipping one key a real option
instead of a rewrite.

**Applied to actors, the final-vision shape is therefore:**

1. an **`answers:` block** on the actor DSL beside its `inbox:` — the queries an actor answers, typed
   like everything else;
2. a **spec-owned `binding:`** on it, exactly as `services.yaml` has;
3. reply types **serde-derived always**, regardless of binding;
4. a typed **`ask`** on the sealed per-actor client (`crates/clients/*`) beside the existing `send`;
5. a **codegen round-trip test per reply type**, so the wire shape cannot silently rot while the binding
   is local.

With that in place, the founder's *"we just have to put in place the grpc transport"* is **true**: it is
one spec key. **Today it is not true** — there is **zero `tonic`, `prost` or `.proto` anywhere in the
tree** (verified). The gap between "one key" and "a rewrite" is items 1–5, and they are buildable
independently of whether a transport is ever added.

### 4. Two things wire-shaping does NOT fix — both stay owed

- **`SettlementHooks` carries cross-call state in mutexes.**
  `crates/application/src/process_managers/payment_settlement.rs:53-58` holds
  `intent: Mutex<Option<PaymentIntentId>>` and `attempted: Mutex<bool>` — state threaded between hook
  calls through interior mutability. **That has no wire form at all.** It must become an explicit value
  passed between steps *before* any serialization question is even askable. Serde on the reply types does
  not touch it.
- **The fencing/ordering hazard is independent of serialization.** PMW-3's objections (i)–(iv) are about
  *when* an answer was true and *who* was holding the lease, not about how it was encoded. The
  compiler-first item recorded there — **a validator rule refusing an actor-sourced `read:` step in any
  leg that also contains a `call:` step** — still stands exactly as written, and would still be needed
  in a fully wire-shaped world.

### 5. What is NOT wire-shaped today — the enumerated list

Every row verified on `main` at the time of writing. "Wire-shaped" = carries `Serialize`/`Deserialize`.

| Shape | Where | Why it blocks a remote binding |
|---|---|---|
| `HookOutcome<T>` | `crates/application/src/generated/process_managers.rs:12-15` | No serde. `Skip(String)` carries **prose** — the very defect §"HookOutcome::Skip conflates four worlds" documents, now also a wire blocker |
| `OrderRead` / `RestaurantRead` / `OpenCartsRead` (5 structs) | `crates/application/src/generated/process_managers.rs:22, 31, 736, 866, 1045` | No serde — and these **are** the query-reply shapes. They are what an `answers:` block would type |
| `DomainError` | `crates/domain/src/shared.rs:23-42` | No serde; two of its three arms (`Invariant`, `Repository`) carry a bare `String`. A remote rejection would arrive as prose |
| `Actor` (the envelope) | `crates/application/src/ports.rs:13-27` | No serde. Every call needs the acting identity + correlation to cross with it |
| `AppendedEvent` | `crates/infrastructure/src/persistence/event_bus.rs:20-31` | `tokio::broadcast`, not `Serialize` — process-local by construction |
| `OperationUpdate` | `crates/actor_client/src/status_bus.rs:26-38` | `tokio::broadcast`, not `Serialize` — process-local by construction. This one is already shipping a user-visible consequence: register row **BUS-1** |

### 6. What IS already a wire contract — so the fear is narrower than it sounds

The instinct that "nothing is serializable" is wrong, and the correction should not overstate it. **Every
command and every event already round-trips serde on every single call, today, with `binding: local`:**

- `crates/infrastructure/src/persistence/event_store.rs:189-212` rebuilds the typed event from
  `(event_type, payload)` — a real deserialize on every load;
- the actor door takes `payload: serde_json::Value` (`crates/actor_client/src/door.rs:57-69`);
- `dispatch_command` takes JSON (`crates/application/src/command_router.rs:39-45`).

So the **write** direction is wire-shaped end to end. What is missing is the **reply** direction —
exactly the six rows in the table above. That is a much smaller, much more tractable statement than "the
codebase is not distributable", and it is the honest one.

### 7. Versioning — which reading implies which doctrine

Greg Young's upcasting doctrine governs **stored** events: a stored event is an immutable contract, so a
schema change is an upcast on read, never a mutation in place. **It does not extend to a query reply** —
there is nothing to upcast, because a reply is *produced fresh* by the current code every time it is
asked for.

A reply needs the **mirror rule**, and it must be recorded separately or someone will apply the wrong
one:

| | Stored event (reading via `EventStore`) | Query reply (reading via an actor `ask`) |
|---|---|---|
| Contract | Immutable; the past is not editable | Produced fresh on every call |
| Change discipline | **Upcast on read**, never mutate | **Additive-only on the producer; tolerant reader on the consumer** |
| Breaking change | A new event type + an upcaster | **A new operation name** — never a silent reshape of the existing one |
| Who pays | Every replay, forever | Only in-flight callers, for one deploy window |

**Consequence for this ADR**: adopting (i) means the PM's read is governed by **event versioning** (it
folds stored events, so it inherits every upcasting obligation of the aggregate it reads). Adopting (ii)
would put it under **reply versioning** instead. These are different maintenance burdens and the choice
between them is part of the choice between the readings — which is another reason (i) and (ii) are
destinations rather than stages.

### What changes in the register

**PMW-3 stays OPEN and stays not-adopted; this correction does not authorise building a transport.**
What it adds is that PMW-3's option (a) — *"do not build it"* — must be read as *choosing shared-database
coupling permanently*, and that items 1–5 of §3 above are the recordable final-vision target whose cost is
mostly **not** the transport.

## Scope clarification (2026-08-15) — `place_order` is a COMMAND HANDLER, not a PM leg

*Added the same day, before any code was dispatched against register §43. **The rule above is
unchanged.** What is corrected is its REACH, which was being overstated in discussion — including by the
session coordinator, and including in this ADR's own "best ARGUMENT and worst DEMONSTRATION" bullet.*

`place_order` is a **command handler** — `crates/application/src/commands.rs:2380`, invoked as the
`PlaceOrder` command leg. `crates/application/src/process_managers/place_order.rs:13` states it verbatim:

> "The COMMAND leg (`commands.yaml#/PlaceOrder`) stays `commands::place_order` (pricing non-goal)."

`PlaceOrderProcess`'s **actual PM legs** are `on_payment_authorized` and `on_payment_failed` — the
reactions to Stripe facts, which is where the process-manager shape lives.

**Therefore the restaurant fold, the cart fold and the catalog read on the checkout path are NOT
governed by this ADR.** They are an aggregate command handler's own reads, which is the same category as
the nine `read_common` and two `read_catalog` command-handler readers this ADR already places **outside**
the rule (see *Consequences*, "Bounded, and the bound is measured"). Three consequences of the
clarification, none of which change the decision:

- The bullet above reading *"place-order is already compliant … the diff on it is empty"* is true but
  for the wrong reason. It is not compliant-by-luck; it is **out of scope**. A future change that made
  `place_order` read a projection would not violate **this** rule — it would need its own argument.
- The spec↔code drift recorded at `specs/ordering/processmanager.yaml:30-43` is still real and still
  worth fixing (the declared model shape does not match what the code reads), but it is a **spec
  accuracy** defect, not a violation of this rule.
- The `read_catalog` row in the STO table (`price_cart` *"inside `place_order`"*) already counted that
  read as a command-handler read. That line was right; the prose around it drifted.

**Why this is worth a dated note rather than a silent edit**: the overstatement was about to justify
work. Register §43's RSO-1/RSO-2 were being reasoned about as if a PM doctrine governed them, which
would have pushed the checkout guards toward an "ask the actor" shape for a component that is not a
process manager. A rule that is cited outside its scope is as expensive as a rule nobody reads.

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
  needs a gate of its own rather than a stricter reading of the existing one. *(Thirteen was the
  2026-08-15 count; **fifteen** at `6b74739b` —
  `grep -rn '^\s*- read:' specs/*/processmanager.yaml`. The two lens lines in **Consulted** that also
  say "thirteen" are kept verbatim as said on 2026-08-15. Corrected 2026-08-31,
  ADR-20260817-105845.)*

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
- **Named, per the Correction above.** The adopted mechanism is a **shared-database read**, kept
  indefinitely, and it does **not** realise the founder's in-memory premise until PMW-2 lands. The
  final-vision target (`answers:` + spec-owned `binding:` + always-serde replies + a typed `ask` + a
  round-trip test per reply type) is recorded in Correction §3 and is mostly **not** transport work.
- **No code, no `specs/**` edit lands with this ADR.** The spec↔code drift above is **recorded**, not
  fixed. An executor closing it must first check the in-flight
  [#564](https://github.com/TheCaptainCompany/captain-food/issues/564) branch, which already annotates
  those very steps (see below).

### One thing found while writing that sharpens PMW-1

**As written on 2026-08-15** (kept verbatim, because it is what this record claimed at the time):
*"The `source: PROJECTION | EVENT_STREAM` enumeration is **not on `main`**; it exists on the in-flight
`564-mechanical-reader-derivation` branch, where `specs/ordering/processmanager.yaml:32,43` already
carry `source: EVENT_STREAM` on exactly the two `PlaceOrderProcess` steps this ADR names as drifted.
So the drift is real on `main` and its correction is already in flight — PMW-1 is therefore a question
of what the **final** grammar is, on top of an enumeration that is landing, not a green field. That
branch is owned by another session and is untouched by this record."*

> **CORRECTION (2026-08-31).** *"Not on `main`"* has been false since **2026-08-16**, when
> [PR #566](https://github.com/TheCaptainCompany/captain-food/pull/566) merged as `b0fd7fdf` — sixteen
> days before this note. **That sentence produced a false negative in a register check on 2026-08-31**:
> read literally at HEAD it says the enumeration is not shipped, so a search for what `main` enforces
> comes back empty from the very record that was supposed to answer it. That is the cost that earned
> this correction, and the general shape of it is worth keeping: **a record that pins a fact to
> "in flight" acquires an expiry date the moment it is written, and nothing detects the expiry.**
> **What is true at HEAD**: the enumeration is on `main`; `specs/ordering/processmanager.yaml:32,43`
> carry `source: EVENT_STREAM`, and so do `:73` and `specs/delivery/processmanager.yaml:46` — **four**
> `EVENT_STREAM` steps against **eleven** `PROJECTION`, over **fifteen** `read:` steps in total
> (`grep -rn '^\s*- read:' specs/*/processmanager.yaml` and
> `grep -rn 'source: ' specs/*/processmanager.yaml` at `6b74739b`).
> **And PMW-1's answer has since been reversed**: `read:` is retired
> ([ADR-20260831-121957](ADR-20260831-121957-the-pm-read-step-is-retired-source-fixed-the-physics-and-left-the-ownership.md),
> challenge row **PMW-4**), so the "final grammar" question this section framed is now a question
> about *what replaces* `read:`, not about what qualifies it.

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

**Round 3 — post-landing review, which produced the Correction above:**

- **architect**: The ADR adopts (i) as though the `EventStore` port were the final vision with the
  adapter behind it a deployment detail. It is not — `load(stream_name) -> (Vec<DomainEvent>, i64)` is a
  **storage** port whose remote form is a database connection, so (i) is a different destination and
  choosing it chooses shared-database coupling permanently. It also fails the founder's own in-memory
  premise, because `ActivationCache` is process-local and lane-tagged. The final-vision shape exists one
  file over in `services.yaml`'s spec-owned `binding:` with unconditional serde, and the ADR should name
  it as the target rather than let (i) read as one.
