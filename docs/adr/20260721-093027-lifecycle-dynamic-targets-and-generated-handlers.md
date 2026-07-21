# ADR-20260721-093027 — Lifecycle completion: dynamic targets (`via`), full adoption, generated require+guard+append handlers

## Status

Accepted — completes codegen-roadmap item 1 (issue #23), building on ADR-20260720-004419
(first slice: `lifecycle:` DSL, `lc-*` rules, `lifecycles.rs` emitter, Order wired end-to-end).

## Context

The first lifecycle slice left the codebase split-brain: Order transitions are machine-checked
against the declared spec, while Restaurant/Rider/DeliveryJob transitions live in hand code the
validator cannot see (`rider::can_transition`, `delivery_can_transition`), and the Cart/Payment
folds still move their status through hand `match` arms. Two machines — Rider and DeliveryJob —
did not fit the DSL at all: their central events (`RiderStatusChanged`, `DeliveryStatusUpdated`,
`DeliveryPartnerStatusUpdated`, and the `RiderRegistered` birth) **carry the target state in the
payload**, so one event drives the machine to several states and the static `event → to` mapping
cannot describe them. Finally, the lifecycle-guarded command handlers are rote
"require + guard + append" Rust, hand-repeated per command.

## Decision

### 1. Dynamic-target DSL extension — `via: <payloadField>`

A lifecycle entry (initial or transition) may declare `via: <field>`: the event carries the
machine's target state in that payload field.

```yaml
Rider:
  lifecycle:
    status: { $ref: 'scalars.yaml#/RiderStatus' }
    initial:
      - event: { $ref: 'events.yaml#/RiderRegistered' }
        to: OFFLINE          # the canonical birth state (diagram + reachability)
        via: status          # …but the recorded fact wins: the fold births from the payload
    transitions:
      - { from: [OFFLINE, ON_DELIVERY], event: { $ref: 'events.yaml#/RiderStatusChanged' }, to: AVAILABLE, via: status }
      - { from: [AVAILABLE],            event: { $ref: 'events.yaml#/RiderStatusChanged' }, to: ON_DELIVERY, via: status }
      # …
```

Semantics:

- **Transition entry with `via`**: the entry legalizes the move `from × {to}` **when the event's
  `via` field equals `to`**. One dynamic event may therefore appear in several entries (one per
  target) and the machine stays deterministic: the event *instance* picks exactly one arm.
  Generated `transition(from, event)` matches `(from, event)` guarded by `event.<via> == to`;
  generated `target(event)` returns `Some(event.<via>)` — at fold time the recorded fact wins,
  exactly as for static targets.
- **Initial entry with `via`**: the birth state is event-carried — generated `initial(event)`
  returns the payload field. `to` remains required as the **canonical** birth state (what the
  command handlers emit; anchors reachability analysis and the `[*] -->` diagram edge).
- Validator: new `lc-via` rule — the field must exist on the event's events.yaml payload, be
  **required** (a nullable/optional target cannot drive a machine), and `$ref` the same scalar as
  `lifecycle.status`; an event must use one consistent `via` across all its entries (mixing
  static and dynamic arms for the same event is `lc-ambiguous`). Determinism for dynamic entries
  keys on `(from, event, to)` instead of `(from, event)`.
- Mermaid state diagrams label dynamic edges `Event(field)` so event-carried transitions are
  visible in the generated docs.

### 2. Full adoption — Restaurant, Rider, DeliveryJob declared; every fold rewired

- **Restaurant** (`RestaurantStatus`): `RestaurantRegistered → DRAFT`;
  `RestaurantActivated: DRAFT|INACTIVE → ACTIVE`; `RestaurantDeactivated: DRAFT|ACTIVE → INACTIVE`;
  `RestaurantRemoved` / `RestaurantMarkedClosed: any → INACTIVE` (recording a removal/closure on an
  already-INACTIVE restaurant is legal — the fact still lands). No terminal state (INACTIVE can be
  re-activated). Activate/Deactivate keep their **ensure-command** semantics in the handlers
  (already-there → idempotent no-op, no event): idempotency short-circuits are command policy, not
  transitions, so they stay out of the machine.
- **Rider** (`RiderStatus`): dynamic machine as above, including the `SUSPENDED → SUSPENDED`
  self-loop (suspension is admin-imposed from anywhere, idempotently re-suspendable) and
  reinstatement only through OFFLINE. `domain::rider::can_transition` is **deleted**; the guard is
  the generated table consulted with the event about to be appended.
- **DeliveryJob** (`DeliveryStatus`): static edges for the operational facts
  (accept/assign/unassign/pickup/complete/cancel/dispatch-failed) + dynamic edges for
  `DeliveryStatusUpdated` and `DeliveryPartnerStatusUpdated` (both `via: status`), reproducing
  `delivery_can_transition` exactly: forward along PENDING → ASSIGNED → PICKED_UP →
  OUT_FOR_DELIVERY → DELIVERED with the PICKED_UP → DELIVERED hand-over shortcut, and
  CANCELLED/FAILED early exits. The hand code disagreed with itself on one edge — `cancel_delivery`
  allowed cancelling a FAILED job while `delivery_can_transition` treated FAILED as fully terminal;
  the declared machine resolves it **preserving both behaviours**: `DeliveryCancelled` (the manual
  command, rules.yaml#/DeliveryCancellableBeforeCompletion — FAILED jobs are surfaced for manual
  handling) has `FAILED` in its `from`, the dynamic status-report events do not. Terminal =
  `[DELIVERED, CANCELLED]`. `delivery_can_transition` is **deleted**.
- **Folds rewired** (Cart, Payment, Restaurant, Rider, DeliveryJob — Order already was): status
  moves ONLY through `lifecycle::initial` / `lifecycle::target`; hand `match` arms keep only
  payload extraction and non-status fields. Payment keeps its duplicate-birth guard (a re-delivered
  `PaymentIntentCreated` never resets an existing state).

### 3. Generated "require + guard + append" handlers

New emitter → `crates/application/src/generated/handlers.rs`: for each command whose whole handler
is mechanical — rehydrate, build the single emitted event from the command by name, consult the
lifecycle table, append — the function is generated and `commands.rs` re-exports it (call sites and
behaviour tests unchanged). Covered now: the seven Order lifecycle commands, `ChangeRiderStatus`,
`UpdateDeliveryStatus`, `UpdateDeliveryPartnerStatus`.

Emitter conventions (folded into the DSL if a second consumer appears, per the PM-pipeline
precedent ADR-20260721-053456):

- **Event construction**: each event payload field maps from the same-named command field, else
  `None` for an optional field (a required unmappable field aborts generation).
- **Seams** stay hand-written in `commands.rs` (`pub(crate)`), named per aggregate in an emitter
  table: the `require_*` rehydration (existence + tenant scoping), the stream naming, and the
  rejection expression for an illegal move (`InvalidOrderStatus` context, the Rider
  `currentStatus`/`targetStatus` context, the DeliveryJob `expectedStatus` diagnostic via
  `canonical_predecessor`) — error-context construction is per-aggregate policy, not mechanics.
- The **hand behaviour suite is the parity gate** (#24's generated harness has not landed; when it
  does, it takes over as the gate — the sequencing the issue anticipated). Handlers with business
  checks beyond the machine (`DeliveryAlreadyAssigned` arbitration, rider-identity checks,
  ensure-command idempotency, cross-aggregate invariants) stay hand-written, now guarding through
  the generated `transition` table where they guard at all.

## Alternatives considered

- **`to: [list]` on one dynamic entry** instead of one entry per target — fewer lines but loses the
  per-target `from` sets (Rider's targets each have different legal sources) and complicates the
  diagram emitter; rejected.
- **Marking dynamism on the event** (events.yaml annotation) — the machine is aggregate metadata;
  events.yaml stays a pure payload catalog; rejected.
- **Generating ALL lifecycle-adjacent handlers** (accept/decline/pickup/…): their
  `DeliveryAlreadyAssigned` vs `InvalidDeliveryStatus` arbitration and identity checks are business
  logic; generating them would need a guard-DSL that is not warranted yet; deferred.
- **Waiting for #24 before generating any handler** — the hand behaviour suite already pins every
  covered command (same assertions the generated harness would make), so the mechanical subset is
  provable today; the issue's "best after #24" is satisfied by keeping the non-mechanical rest out.

## Consequences

### Positive
- Illegal transitions are validator errors across ALL five status-carrying aggregates; the three
  `lc-missing` warnings are gone (zero remaining).
- One transition regime: every fold and every guard consults the generated tables; the two hand
  transition functions (and their drift — the FAILED-cancel disagreement was real) are deleted.
- Ten command handlers become generated code; every future lifecycle command on a declared machine
  is one emitter-table row.
- Every aggregate's state diagram is generated into the docs for free.

### Negative
- The seam table (require/reject/stream expressions) is emitter-embedded convention, not DSL —
  acceptable at one consumer, same trade-off as the PM pipelines.
- Dynamic entries make the DSL more verbose for dense machines (DeliveryJob declares 12 dynamic
  rows); the explicitness is the point.

### Follow-up actions
- #24: swap the parity gate to the generated behaviour harness once it lands.
- Fold the handler seam table into the DSL if a second consumer appears.
- The `RestaurantNotReadyForActivation` invariant on `ActivateRestaurant` remains TODO (needs a
  catalog read-model port) — unchanged by this ADR.
