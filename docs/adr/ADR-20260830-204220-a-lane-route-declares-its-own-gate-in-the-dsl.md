# ADR-20260830-204220 — A lane route declares its own gate in the DSL, and the routed step consults that route

## Status

Accepted

## Consulted

<!-- ADR-20260812-143619: a record created from a founder directive carries one line per lens. This
     chunk's briefing is [#780](https://github.com/TheCaptainCompany/captain-food/issues/780)'s;
     its reversibility class is 2–3 lenses (reversible refactor, codegen-only, no route moves, no
     stored shape, no wire change), so the two lenses that spoke on this at that briefing are the
     two carried here. -->

- **farley** — *"The routed step must consult its own flag, not 'a flag'."* Named the defect
  precisely: it is not merely fusion, it is **a false claim of non-fusion sitting at the exact line
  a reader checks** (`runner.rs`'s "the gate is per ROUTE, not per runner"), which the first C3
  route PR would have read as a discharged obligation. Set the shape: generate the enumeration from
  the spec so that **adding a route without adding its key is a build failure**, and make bare sink
  presence unspellable rather than discouraged.
- **vernon** — the gate names **which aggregate boundary is being closed**, so sharing one boolean
  means flipping the fence for `Payment` also flips it for `DeliveryJob`. Blast radius, not config
  naming.

## Enforced by

n/a — no behavioral guarantee. This ADR changes no route's position and no observable behaviour; it
records the FORM the per-route gate takes. The mechanical guarantees it does carry are executable,
not prose: `ref-site-undeclared` on `route_gate:` (`make validate`), `RouteGates`'s absent `Default`
(E0063 at every construction site), the absence of any route-blind accessor on `TriggerEnvelope`
(E0061), and `route_gates_are_not_fused` in `tools/codegen-rs/src/tests.rs`.

## Context

[ADR-20260829-230418](ADR-20260829-230418-aggregates-own-the-facts-isolation-first.md) chunk C3
refuses a fused flag in as many words: *"one fused flag would make the twelve routes flip together,
which is exactly the un-flippable blast radius the gate exists to avoid."* The code did not deliver
that.

`crates/infrastructure/src/process_manager/runner.rs` built ONE trigger envelope and handed ONE lane
sink, gated on the single boolean `route_replacement_birth_through_lane`. Every routed step then
read only sink PRESENCE — `if let Some(lanes) = env.lane_sink()` in both the generated pipeline and
the hand-written reclamation seam. So the route-selection predicate was `sink.is_some()`, and the
only producer of `Some` on that runner was the reclamation key. The gate was **per runner, spelled
per route in a comment**, and the comment claimed the opposite of what the code did.

Two consequences, one latent and one blocking:

- **Latent today.** `runner.rs` unconditionally filters `PaymentAuthorized|PaymentCaptured|
  PaymentFailed|PaymentRefunded` out of every group, so `place_order::on_payment_authorized` on the
  runner is unreachable dead code. One key, one reachable consumer, so the read was *accidentally*
  correct. There was no live bug.
- **Blocking for C3.** All of C3's event-leg routes except `CartCheckedOut` are runner-hosted. Add
  any one of them and `ROUTE_REPLACEMENT_BIRTH_THROUGH_LANE=true` silently flips it too. Rolling the
  new route back would then require rolling the reclamation birth back with it — a second, unrelated
  behaviour change made under incident pressure. **A rollback that changes something you did not
  intend to change is not a rollback.**

The forces on the FORM of the fix:

- 13 `deliver:` steps exist; all thirteen are candidates for routing. One configuration key each is a
  real surface, and `config-gates-restates-default` (landed `cd865d95`) already polices how much
  prose a key may carry.
- `crates/application` cannot see `crates/server`'s generated `Config` without inverting the
  dependency rule, so nothing in the saga can look a key up by name.
- `ROUTE_ORDER_BIRTH_THROUGH_LANE` (default `true` since
  [ADR-20260830-012200](ADR-20260830-012200-the-order-birth-routes-through-the-lane.md)) and
  `ROUTE_REPLACEMENT_BIRTH_THROUGH_LANE` (default `false`) must keep their exact effective behaviour.
  This changes plumbing, never a route's state.

## Decision

**A lane route declares its own gate in the DSL, and the generated step consults that route by name.**

1. A routed `deliver:` step, and a routed wrapper-seam `sends:` entry, carries
   `route_gate: { $ref: 'configuration.yaml#/keys/<KEY>' }`. **Presence of that `$ref` is what makes
   the step routed** — the codegen's hand-kept `PM_LANE_ROUTED_DELIVERS` / `SENDS_LANE_ROUTED` consts
   are deleted and the routed set is read from the model. A routed `sends:` also names its `to:`
   target, which was a hardcoded pair in the codegen before.
2. The codegen emits, into `application::generated::process_managers`, a `Route` enumeration (one
   variant per declared route, carrying `config_key()`, `actor_type()`, `message_type()`) and a
   `RouteGates` struct with one `bool` field per route.
3. `TriggerEnvelope::lane_sink()` is **deleted**. The only accessor is
   `lane_sink_for(route: Route)`, returning `Some` only when a sink is attached **and** that route's
   own gate is on. `laned` takes the `RouteGates` alongside the sink.
4. Composition roots build a `RouteGates` literal, feeding each field from its own configuration key.

**One key per route is kept, deliberately.** The key count is the number of independently rollbackable
behaviours, which is precisely what "per-route gate" means; a route that exists without its own lever
is the thing this ADR removes.

Four levels of enforcement, in the order PROP-20260802-130500 §1 ranks them:

| Mistake | What refuses it |
|---|---|
| Route names a key that does not exist | `ref-site-undeclared` at `make validate` — the gate is a `$ref` the refs walker resolves, not a string looked up at runtime |
| Staging without naming your route | E0061 — there is no route-blind accessor to call |
| Declaring a route and forgetting a construction site | E0063 — `RouteGates` derives no `Default`, so every literal must name every field |
| Feeding route A's field from route B's key (copy-paste fusion) | `route_gates_are_not_fused` — a `syn` scan of every `RouteGates { .. }` literal in `crates/**`, asserting the PROPERTY (a field naming any declared route's key must name its own) and refusing `..rest` |

Only the fourth is a check, and only because the compiler cannot see which config field *means* which
key. Compiler first; a check is the fallback (ADR-20260803-234035).

`runner.rs`'s comment stays, because the code now delivers what it claims: the sink is handed over
UNCONDITIONALLY (owning the fenced transaction is a property of the runner, true on every leg) and
`route_gates` decides, per route, who may use it. With every gate off, the envelope is laned and no
route stages — byte for byte what the withheld sink used to give.

## Alternatives considered

- **One config key per route, with the binding left in the codegen (status quo + a second boolean).**
  Rejected: it is the shape that produced this defect. Threading a second boolean fixes today's two
  routes and reinstates the same trap for the third, because nothing binds a route to a key except a
  developer remembering to.
- **A single map-valued key** (`ROUTED_LANES=Order.OrderPlaced:on,…`). Rejected on three counts. It
  is stringly-typed at the boundary, so a typo in a route name disables nothing and reports nothing —
  the failure mode is silence, under incident pressure, on the money path. It cannot carry per-route
  `gates:` prose, and that prose is the rollback runbook: the two existing keys document genuinely
  different consequences (envelope provenance, one lane hop of latency, whether a rejection lands a
  supervisable verdict). And it trades 13 keys for one key with 13 meanings, which is not a smaller
  surface, only a less legible one.
- **Reusing the `activations:` grammar.** Rejected: `mailbox.activations` is per-ACTOR held-state
  policy, a different axis entirely — a route is a `(process manager, message, target)` triple, and
  two routes to the same lane must be separately flippable.
- **A validator rule instead of a type** ("every routed step must name a gate"). Rejected as the
  primary mechanism under ADR-20260803-234035: the `$ref` + the missing `Default` already make three
  of the four mistakes unspellable, and a rule that restates what the compiler enforces is a rule
  that will drift.
- **Leaving `PM_LANE_ROUTED_DELIVERS` in the codegen and adding only the key binding.** Rejected: two
  places would then say which steps are routed, and the whole defect is two places disagreeing about
  which switch drives which route.

## Consequences

### Positive
- The routed set moves from a Rust const into the DSL, where the refs walker can see it. #598's
  `ROUTED_LANES` and #780's `routed_fact_targets` oracle are both derived from the model now.
- The C3 route moves become mechanical: add `route_gate:` and its key, and the compiler names every
  site that must decide.
- `runner.rs`'s comment is true. The specific harm farley named — a false claim of non-fusion at the
  line a reader checks — is gone rather than annotated.

### Negative
- One configuration key per route, and C3 adds up to eleven more. Accepted: that count IS the number
  of independently rollbackable behaviours.
- The `sends:` entry grammar changed shape (`{ command:, to:, route_gate: }` instead of a bare
  `$ref`), so `c4.rs` and the `pm-sends-*` rules read it through one shared parser now.

### Follow-up actions
- None required. The route moves themselves are C3's own chunks, each with its own recorded decision.
- Noted, **not fixed here** (out of scope, flagged for the architect): `runner.rs`'s unconditional
  `Payment*` trigger filter makes `place_order::on_payment_authorized` at that dispatch arm
  unreachable dead code. It is harmless today and deleting it is a separate decision — but a routed
  step behind an unreachable arm is a route nobody can smoke.
