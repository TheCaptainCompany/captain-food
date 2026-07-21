# 🏭 Codegen roadmap — shrinking the hand-written surface

> Agreed direction (2026-07-19): after the typed-step PM DSL proved that a checkable spec plus
> generation removes misinterpretation, apply the same treatment to the remaining hand-written
> layers. Ranked by misinterpretation-risk removed per unit of effort. Each item follows the same
> recipe: DSL (typed, `$ref`s, validator rules) → emitter → the gate proves spec ↔ code.

| # | Candidate | Today (interpretation surface) | Target |
|---|---|---|---|
| 1 | **Aggregate lifecycle state machines** — 🟢 **first slice LANDED** (ADR-20260720-004419): `lifecycle:` block per aggregate in actors.yaml (EVENT-keyed, not command-keyed — inbound-fact machines have no command), 8 `lc-*` validator rules + `lc-missing` coverage warning, emitter → `crates/domain/src/generated/lifecycles.rs` (initial/transition/target tables + TERMINAL) + mermaid state diagrams in the generated docs; **Order** wired end-to-end (fold + 7 guarded handlers via `require_order_transition`), Cart/Payment declared | Remaining: dynamic-target DSL extension (event-carried status → Rider/DeliveryJob), Restaurant adoption, rewiring Cart/Payment folds, generating the mechanical "require+guard+append" handlers (best after item 2) |
| 2 | **Behaviour-test harness from tests.yaml** | Every tests.yaml case is hand-mirrored into `crates/application/tests/*_behaviour.rs` — a pure translation step | Generate the runner: given = seed streams, when = dispatch via a generated message→handler table, then/thrown = assert. The gate then EXECUTES the spec instead of checking a translation of it |
| 3 | **PM orchestrator scaffolding** | The four orchestrators hand-implement their DSL legs (ADR-20260719-193500) — deferred "until the shape is proven"; it is now | Generate the step pipeline (state by/expect/set, deliver, send, call, skip/throw plumbing); hand-written only the non-structural guard predicates behind generated hook traits |
| 4 | **Service catalog + configurable binding** — 🟢 **first slice LANDED** (#26, ADR-20260721-043033): `specs/services.yaml` → generated `<Base>Service` traits + typed inputs/outputs + `ServiceCallMeta` envelope (application), `Http<Base>Service` clients + spec-owned `binding` resolvers (infrastructure), expose-gated `/services/*` routes (server); `PaymentGateway`/`DeliveryPartner` migrated at parity and deleted | Remaining: `identity` migration (blocked on a catalog gap — `verify_email_token.output` lacks the proven `email`, `locale` optionality; owner spec change), C4-L3/observability binding to the catalog (ADR-20260719-214500 pt 5), `catalog_sync`/Avelo37 entries with #20/#28 |
| 5 | **PM state-table rows/stores** — 🟢 **LANDED** (#27, ADR-20260721-031734): `database/tables/process_managers.yaml` → `application/src/generated/pm_state.rs` (rows + `by_*` lookup traits + mem doubles) and `infrastructure/src/generated/pm_state.rs` (Pg stores, upsert + `now()` envelope stamp); the hand-written modules are deleted, paths unchanged via re-exports | Remaining: the journal stores (`command_journal.rs` / `inbound_events.rs`, same conventions) as a follow-up slice; fold the `EXTRA_LOOKUPS` seam into the DSL when the paymentStatus resolver bodies are generated |
| 6 | **SDUI → Leptos registry** (ADR-0033) | Deferred with `crates/web` | Generated component registry + resolver/action wiring from `screens/*.yaml` |
| 7 | **Observability middleware assertions** | Contracts in observability.yaml; emission deferred | Generated span/metric assertions at the framework boundaries (`c4-l3` `instrumented` flags) |

Non-goals: generating genuinely computed business logic (pricing formulas, snapshot construction) —
those stay hand-written behind generated seams, with rules.yaml + tests as their contract.
