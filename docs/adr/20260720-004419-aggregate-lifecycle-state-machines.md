# ADR-20260720-004419 — Aggregate lifecycle state machines (declared, validated, generated)

## Status

Accepted — DSL (`lifecycle:` per aggregate in actors.yaml) + `lc-*` validator rules + the
`crates/domain/src/generated/lifecycles.rs` emitter landed 2026-07-20; Order adopted end-to-end
(codegen-roadmap item 1). Cart and Payment are declared; Restaurant, Rider and DeliveryJob are
deliberate coverage warnings (see Consequences).

## Context

Aggregate status lifecycles today live as comments in `specs/scalars.yaml` and as hand-written code:
`rider::can_transition`, `delivery_can_transition`, and ~50 rote "require + status-check + append"
command handlers each re-encode which statuses allow which command. The Order machine alone is spread
over eight handlers in `crates/application/src/commands.rs` plus the fold in `crates/domain/src/order.rs`
— three places that must agree by discipline, with nothing executable proving they match the spec
(`rules.yaml#/OrderLifecycleStatusMachine` is asserted only by behaviour tests). This is the
top-ranked misinterpretation surface in docs/codegen-roadmap.md (item 1).

## Decision

Make each aggregate's lifecycle an explicit, validated, generated state machine.

### 1. DSL — a `lifecycle:` block per aggregate in `specs/actors.yaml`

The lifecycle is aggregate metadata, so it lives on the aggregate (the roadmap's framing:
"`lifecycle:` section per aggregate"), next to the inbox it constrains — not in a new file. Shape:

```yaml
Order:
  type: aggregate
  lifecycle:
    status: { $ref: 'scalars.yaml#/OrderStatus' }   # the machine's state enum
    initial:                                        # birth events → entry state
      - event: { $ref: 'events.yaml#/OrderPlaced' }
        to: PLACED
    transitions:                                    # the legal moves, keyed by EVENT
      - { from: [PLACED], event: { $ref: 'events.yaml#/OrderAcceptedByRestaurant' }, to: ACCEPTED }
      # …
    terminal: [DELIVERED, REJECTED, …]              # states with no outgoing transitions
```

**Transitions are keyed by EVENT, not by command** (the roadmap sketch said `on: command`; the event
is the right alphabet). The command → event mapping already lives in the aggregate's `receives`
(actors.yaml is the single wiring truth), the fold's alphabet IS events, and inbound facts
(Payment's whole machine, the Order birth delivered by PlaceOrderProcess) have **no** command. A
command-keyed machine could not describe them; an event-keyed one covers both, and the handler for a
command simply consults the table with the event it is about to emit. The rejection error on an
illegal move is likewise not re-declared per transition: it is already each command's `throws`
(`InvalidOrderStatus`, …).

Events that do not change the status (OrderRated, RefundOpened, cart line edits…) are simply not in
the machine — the fold treats them as status no-ops, exactly as before.

### 2. Validator — `lc-*` rules (new §2c in tools/codegen-rs, mirroring §2b/§2d)

- `lc-shape` — `lifecycle` must be a mapping with `status`, non-empty `initial`, and `transitions`.
- `lc-status` — `status` must `$ref` a scalars.yaml **enum** scalar.
- `lc-state` — every state in `initial[].to` / `transitions[].from|to` / `terminal` is a member of
  that enum.
- `lc-event` — every `initial[].event` / `transitions[].event` must be a resolving `$ref` into
  events.yaml.
- `lc-event-not-emitted` — every event claimed by the machine is actually emitted by THIS aggregate
  per its `receives[].emits`.
- `lc-ambiguous` — no two transitions from the same state on the same event (and no duplicate
  initial event): the machine is deterministic.
- `lc-terminal-outgoing` — a `terminal` state has no outgoing transition.
- `lc-unreachable` — every state the lifecycle names is reachable from an initial state.
- `lc-missing` (**warning**) — an aggregate whose status scalar exists (`<Aggregate>Status`, with a
  trailing `Job` stripped so DeliveryJob ↔ `DeliveryStatus` is found) but that declares no
  `lifecycle`. A warning, not an error, so adoption is incremental.

### 3. Emitter — `crates/domain/src/generated/lifecycles.rs`

One module per declaring aggregate (`lifecycles::order`, `::cart`, `::payment`), plain Rust
data/match, no SDK, no I/O — the domain stays dependency-free:

- `initial(&DomainEvent) -> Option<Status>` — the state a birth event enters;
- `transition(from: Status, &DomainEvent) -> Option<Status>` — the transition table:
  `Some(next)` iff legal (`None` = illegal move or not a lifecycle event) — the handlers'
  append-time guard;
- `target(&DomainEvent) -> Option<Status>` — the state an event drives the machine to irrespective
  of the current state (emitted only for single-target events) — the fold's apply step, because at
  fold time the recorded fact wins (event-sourcing: guards run at append time, rehydration trusts
  the log);
- `TERMINAL: &[Status]` + `is_terminal(Status) -> bool`.

The generated documentation (`documentation.generated.{md,html}`) embeds a per-aggregate mermaid
`stateDiagram-v2` in the actor section, exactly like the PM sequence diagrams.

### 4. Adoption

- **Order** end-to-end: spec block; `domain::order::fold` births status via `initial` and applies
  recorded facts via `target` (the fact wins at fold time — legality is an append-time concern); the
  seven transition handlers (accept / start-preparation / ready / delivered / reject / cancel ×2)
  consult `transition` and reject with the existing typed `InvalidOrderStatus` when it returns
  `None`. The feedback commands (rate / tip / refund-request) are not lifecycle transitions and keep
  their DELIVERED / not-terminated guards. New rule `rules.yaml#/OrderLifecycleIsExplicit` with
  behaviour tests (skipped-transition + terminal-state rejections) and Rust tests.
- **Cart**, **Payment**: declared (trivially static machines matching their folds); folds/handlers
  not rewired yet.
- **Restaurant, Rider, DeliveryJob**: left as `lc-missing` warnings. Rider (`RiderStatusChanged`)
  and DeliveryJob (`DeliveryStatusUpdated`, `DeliveryPartnerStatusUpdated`) have **event-carried
  target states** (the event's `status` property names the target), which the static `to:` shape
  cannot express — extending the DSL (e.g. `to: { from_property: status }`) is the natural follow-up.
  Restaurant's machine also encodes idempotent no-ops (re-activating ACTIVE emits nothing), a
  handler-level concern to model when it is adopted.

Note: `OrderStatus::OUT_FOR_DELIVERY` is intentionally NOT in the Order machine. No Order event
enters it — it is a read-side presentation status the OrderTracking projection derives from delivery
facts; the old hand-written `MarkOrderDelivered` guard listed it defensively but it is unreachable
in the write-side fold.

## Alternatives considered

- **A dedicated `specs/lifecycles.yaml`** — rejected: the machine constrains one aggregate's inbox
  and reuses its emits; splitting it from actors.yaml would re-create the cross-file drift this
  change removes.
- **Command-keyed transitions (`on: command`, the roadmap sketch)** — rejected: cannot express
  event-born aggregates or inbound-fact machines (Payment), and duplicates the command→event wiring
  already in `receives`.
- **Generating the whole mechanical handler** (require + check + append) — deferred: that is the
  rest of roadmap item 1, best landed after the behaviour-test harness (item 2) can prove handler
  parity spec-side. This change deliberately generates only the transition table and wires the
  hand-written handlers to consult it.

## Consequences

### Positive
- One declaration in the spec now drives the fold, the handler guards, the docs diagram, and the
  validator — the Order machine cannot silently drift across its three implementations.
- Illegal-jump and terminal-state semantics are executable spec (`lc-*` at validate time, the
  generated table at run time, behaviour tests at test time).

### Negative
- Three accepted `lc-missing` warnings (Restaurant, Rider, DeliveryJob) until the dynamic-target
  extension lands — the warning list is no longer only the known view design-holes.
- The static `to:` shape cannot express event-carried target states yet (see follow-ups).

### Follow-up actions
- Extend the DSL with event-carried targets (`to: { from_property: <prop> }`) and adopt Rider +
  DeliveryJob (replacing `rider::can_transition` / `delivery_can_transition`).
- Adopt Restaurant (modelling its idempotent no-op commands) and wire Cart/Payment folds to their
  generated tables.
- Roadmap item 1 remainder: generate the mechanical "require + guard + append" handlers from
  `receives` × `lifecycle`.
