# ADR-20260830-101500 — The polling saga runner owns a fenced LEG transaction, and a `sends:` command may take the lane

- **Status**: Accepted
- **Date**: 2026-08-30
- **Chunk**: **C2** of
  [ADR-20260829-230418](ADR-20260829-230418-aggregates-own-the-facts-isolation-first.md)
  ("Aggregates own the facts: the isolation subject is resolved FIRST"), executing the founder's
  isolation-first directive.
- **Relates**:
  [#595 "The reclamation replacement birth writes Order-{id} with no transaction and no lane — a second unlaned birth site, reachable today"](https://github.com/TheCaptainCompany/captain-food/issues/595)
  · [ADR-20260816-040239](ADR-20260816-040239-deliver-is-a-lane-enqueue-not-a-foreign-stream-append.md)
  (the semantic ruling and its constraint 1) ·
  [ADR-20260830-012200](ADR-20260830-012200-the-order-birth-routes-through-the-lane.md) (C1: the
  Order-birth route, flipped ON) · [ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)
  (compiler first) · `docs/SPEC-LOG.md` row 2026-08-30.
- **Consulted** (ADR-20260812-143619): banked, no new fan-out — the 13-lens block of
  [ADR-20260829-230418 §Consulted](ADR-20260829-230418-aggregates-own-the-facts-isolation-first.md)
  covers this plan chunk, and the dispatch card carried the per-chunk design inputs from
  **architect** (the odd one: an in-process command CALL, so it needs the fenced-transaction story,
  not a route move), **evans** (the seam is grammar-invisible; make it declared), **vernon**
  (preserve fact-vs-command door semantics), **beck** (golden payload equality before any deletion;
  the clock must be asserted to ARM), **farley** (gate-then-stabilize, per-route not fused) and
  **observability** (declare the route so it inherits the lane watch; state or close the histogram
  gap). Each was verified against the tree before being acted on.

## Context

`ReclamationProcess`'s REPLACEMENT arm called `application::commands::place_replacement_order`
**in process**, from the polling `ProcessManagerRunner`, and that handler appends `OrderPlaced` to
`Order-{id}`. Three facts made this the second unlaned birth site rather than a variant of the
first:

1. it is an in-process **command call**, not a `deliver:` step, so C1's staged-intent seam — which
   lives in the mailbox delivery transaction — could not reach it;
2. the polling runner **owned no transaction at all**: the append and the checkpoint advance were
   two independent commits;
3. the `OrderAcceptanceTimedOut` reminder the spec declares for `(Order, PlaceReplacementOrder)` is
   (re)declared by a **delivery**. With no delivery there was no clock, so a restaurant could go
   silent on a remake for a claim it had already agreed to and nothing would ever say so.

## The decisions

### 1. The polling runner owns a fenced LEG transaction

`commit_leg` opens one transaction per drained position and writes the leg's staged lane enqueues
**and** the checkpoint advance into it. Either the door row exists and the position is consumed, or
neither is true and the leg re-runs next tick — where the frozen door identity makes the re-run a
no-op collision.

The enqueue has no other partner to be atomic with on this route: there is no delivery row to record
a verdict on. Pairing it with the fact that consumes the trigger is what makes both split orders
unnecessary — checkpoint-then-enqueue can lose the replacement outright on a crash (position spent,
nobody enqueued), and enqueue-then-checkpoint is merely safe *by idempotence*. One transaction needs
neither argument. A leg that stages nothing pays a `BEGIN`/`COMMIT` around the single row it already
wrote.

This is what licenses the runner to name `TriggerEnvelope::laned` — the **second** call site of a
constructor whose whole guard was "there is one place to audit". The guard is not relaxed to a count
of two: it becomes an **allowlist of `(file, expected count, audit sentence)`**, the sentence naming
WHICH transaction that file's caller flushes into. It fails in three directions: a call in an
unlisted file, the WRONG NUMBER of calls in a listed file, and a listed file that no longer calls it
at all (`expected 1, found 0` — a stale allowlist is a guard protecting nothing).

The count is the part that carries the constraint, and the first cut of this change **shipped
without it** — a file-granular allowlist, caught in review round 1. That version was strictly weaker
than the `assert_eq!(len, 1)` it replaced: `handler.rs` holds both the audited call and `async fn
prepare(`, so a third call added inside that file would have passed silently, and "the enqueue is
never in `prepare`" is the whole of ADR-20260816-040239 constraint 1. Recorded because the failure
is instructive: *widening* a guard's unit (call site → file) to accommodate a legitimate new caller
reads like a faithful generalisation and is a weakening. When an exact gate must admit one more
case, raise the expected COUNT; never coarsen what is counted.

### 2. The door is a COMMAND door, not an EVENT door

`PlaceReplacementOrder` is a **request the Order may refuse**, not a fact already decided (vernon:
`deliver:` = a fact recorded idempotently, `send:` = a rejectable command). Taking the COMMAND door
keeps that distinction at runtime and buys three things the EVENT door does not:

- the Order's lane worker runs the handler, so a rejection (`OrderNotFound` — the original order is
  gone) lands a **REJECTED verdict on a supervisable row** instead of the `tracing::warn!` the
  in-process arm emitted, which nothing routes;
- the delivery declares the `schedules:` the spec already attaches to that pair, which is how the
  acceptance clock arms **without any new spec**;
- the birth is appended by the aggregate that owns the fact, past its own serialization point.

`LaneEnqueue` therefore carries a typed `LaneMessageKind` rather than a `kind` string chosen at the
insert (compiler-first: a route cannot pick the wrong door by passing a token).

### 3. `sends:` — the grammar stops lying

The replacement dispatch was **grammar-invisible**: a hand-written call no `$ref` pointed at, so no
rule could see it, and the emitter genuinely cannot express it as a `send:` step (the command's
`orderId` is derived from `reclamationId`, not mapped from a property, and the leg is a 3-way branch
the step DSL has no form for). A PM `receives` entry may now declare `sends:` — a list of
`commands.yaml` `$ref`s its wrapper really sends — on exactly the same footing as the existing
`emits:` escape hatch. Two new validator rules hold the declaration to what a step-derived send
satisfies by construction: `pm-sends-kind` (it must reference a command) and `pm-sends-no-inbox` (an
actor must actually receive it, or the door addresses nothing and the lane row lands FAILED
"unroutable command type" — a replacement order silently never born).

The declaration is not decoration: it feeds the PM's derived `emits` through the target's inbox
(the C4 graph now shows ReclamationProcess reaching the order projections, which it always did) and
it feeds `ROUTED_LANES`, so the route inherits the lane dead-man's switch on its own declaration
rather than because an unrelated route happens to address the same lane.

### 4. Gate-then-stabilize, per route

`ROUTE_REPLACEMENT_BIRTH_THROUGH_LANE`, **default OFF**. Separate from
`ROUTE_ORDER_BIRTH_THROUGH_LANE` on purpose (farley, ADR-20260829-230418 C3): one fused flag would
flip unrelated routes together, which is the un-flippable blast radius a per-route gate exists to
avoid. The polling-runner path is deployed behaviour, so the conversion ships gated; rollback is a
config flip, not a redeploy. **Flipping the default is a separate recorded decision, and deleting
the legacy arm is a separate change again**, with beck's golden payload equality as its
precondition — measured here, not argued: the two routes are asserted to produce a byte-identical
`OrderPlaced` payload on two clean databases.

### 5. Observability: the metric was extended, not invented

The replacement birth **is** an `OrderPlaced`, so the handover it introduces is exactly what
`order_birth_lag_ms` was declared to measure. The emitter's only call site was the inbound-FACT
route, so the COMMAND route would have been silent; it now records there too, with `routed` read
from the **declared** `ROUTED_LANES` table rather than from a config flag — the flag says what the
next enqueue will do, the table says how the row in hand actually got here, which is the honest
answer for a row enqueued before a rollback and delivered after it. A dedicated test binary has
**seen it record** (`replacement order_birth_lag_ms{routed="true"} recorded 14 ms`), because an
emitter no test has observed is the #758 defect class one route later.

## Consequences

- Every PM leg on the polling runner now runs inside a transaction it can stage into. Only the
  replacement route uses it today, and only when its gate is ON.
- `LaneEnqueue.event_type` became `message_type`: the field names a command as often as an event now.
- The next chunk (C3) moves the twelve remaining `deliver:` steps in target-actor groups; nothing
  here pre-empts their per-route gates.
- Rollback stays a config flip until the separate deletion change lands.
