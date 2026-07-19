# 🏭 Codegen roadmap — shrinking the hand-written surface

> Agreed direction (2026-07-19): after the typed-step PM DSL proved that a checkable spec plus
> generation removes misinterpretation, apply the same treatment to the remaining hand-written
> layers. Ranked by misinterpretation-risk removed per unit of effort. Each item follows the same
> recipe: DSL (typed, `$ref`s, validator rules) → emitter → the gate proves spec ↔ code.

| # | Candidate | Today (interpretation surface) | Target |
|---|---|---|---|
| 1 | **Aggregate lifecycle state machines** | Status lifecycles live as comments in scalars.yaml; `rider::can_transition` / `delivery_can_transition` and ~50 rote "require + status-check + append" handlers are hand-written | `lifecycle:` section per aggregate (`{on: command, from: [...], to: ..., throws: ...}`) → generated transition guards + the mechanical handlers; hand-written only genuinely computed logic (pricing, snapshots) |
| 2 | **Behaviour-test harness from tests.yaml** | Every tests.yaml case is hand-mirrored into `crates/application/tests/*_behaviour.rs` — a pure translation step | Generate the runner: given = seed streams, when = dispatch via a generated message→handler table, then/thrown = assert. The gate then EXECUTES the spec instead of checking a translation of it |
| 3 | **PM orchestrator scaffolding** | The four orchestrators hand-implement their DSL legs (ADR-20260719-193500) — deferred "until the shape is proven"; it is now | Generate the step pipeline (state by/expect/set, deliver, send, call, skip/throw plumbing); hand-written only the non-structural guard predicates behind generated hook traits |
| 4 | **Service catalog + configurable binding** | Port traits + adapters + wiring hand-written; local vs http hard-coded | ADR-20260719-214500: `specs/services.yaml` → trait + http client + adapter routes + SPEC-declared binding/exposure (config carries only addresses) (`/services/payment/{request,refund}` → local Stripe adapter or `/adapters/stripe/{intentPayment,refund}` over HTTP; `/external/*` stays the EXTERNAL role's GraphQL path) |
| 5 | **PM state-table rows/stores** | `application/src/pm_state.rs` + `infrastructure/persistence/pm_state.rs` hand-written to conventions | Extend the existing rows.rs/projectors.rs emitter family to `database/tables/process_managers.yaml` |
| 6 | **SDUI → Leptos registry** (ADR-0033) | Deferred with `crates/web` | Generated component registry + resolver/action wiring from `screens/*.yaml` |
| 7 | **Observability middleware assertions** | Contracts in observability.yaml; emission deferred | Generated span/metric assertions at the framework boundaries (`c4-l3` `instrumented` flags) |

Non-goals: generating genuinely computed business logic (pricing formulas, snapshot construction) —
those stay hand-written behind generated seams, with rules.yaml + tests as their contract.
