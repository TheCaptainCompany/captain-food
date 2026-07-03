# ADR-0034 — Full-stack Rust for all product code

## Status

Accepted (CTPO decision, 2026-07-03) — supersedes the Next.js/React/Node stack assumptions in earlier
context (CLAUDE.md architecture summary, `c4-l2.yaml` container technologies, `customer_screens.yaml`
tech header, and the "React runtime" note in ADR-0033).

## Context

The project began with a TypeScript/Next.js assumption. After exploration the CTPO adopted **full-stack
Rust** across every platform. Rationale (from the decision note): in an agentic, spec-driven model the
compiler is the cheapest, strictest reviewer — the borrow checker + exhaustive matching + `sqlx`
compile-time query checking catch structural errors before runtime, and a single Rust core is compiled to
web, desktop, iOS and Android. "The spec is the contract, the compiler is the enforcer, Claude Code is the
implementer."

## Decision

**All product code is Rust.** Do NOT generate TypeScript/JavaScript except at explicit integration
boundaries (and never hand-edit UniFFI-generated Swift/Kotlin bindings).

| Platform | UI | Business logic | Bridge |
|---|---|---|---|
| Web (customer) | **Leptos** (Rust→WASM) | Rust core via **Crux** | direct (WASM) |
| Desktop (restaurant) | Leptos in **Tauri 2.0** | Crux core | same process |
| iOS (future) | SwiftUI (thin shell) | Crux core | **UniFFI** → Swift |
| Android (future) | Jetpack Compose (thin shell) | Crux core | **UniFFI** → Kotlin |
| Backend BFF | — | Rust core | **Axum + Tokio + SQLx** |

Per-concern: HTTP `axum`, async `tokio`, DB `sqlx` (compile-time checked), core `crux`, GraphQL
`async-graphql`, validation `garde`/`validator`, serde `serde`, auth `jsonwebtoken` + Supabase REST, i18n
`leptos_i18n`, tests built-in `#[test]` + `rstest`. Config = TOML/`Cargo.toml`; migrations = SQL via
`sqlx migrate`; CI = shell/YAML. Workspace of crates: `core` (Crux, pure), `shared_types`, `web` (Leptos),
`server` (Axum), `desktop` (Tauri).

**The spec stays the permanent source of truth** and is renderer-agnostic: the domain specs
(`events`/`commands`/`actors`/`views`/`api`/`rules`/`tests`) map 1:1 onto Crux core modules; the SDUI
`customer_screens.yaml` + `translations.yaml` drive a generated Leptos component registry instead of a React
one. Swapping the renderer only changes the component registry — spec, backend, core and SDUI JSON are
unaffected.

## Consequences
### Positive
- One shared Rust core across all platforms; compiler-enforced correctness suits the agentic workflow; the
  existing tech-agnostic domain/SDUI specs carry over unchanged.
### Negative / risks
- Leptos has no `shadcn/ui` equivalent — all components are generated from the SDUI spec (accepted; the spec
  fully defines structure/props/behaviour). Smaller ecosystem + talent pool than React/Node; WASM bundle/SSR
  considerations. The existing **TypeScript `tools/codegen`** must either be kept as the spec toolchain that
  now emits Rust, or ported to Rust (`scripts/generate.rs`) — open (see follow-ups).

## Follow-ups (reconciliation — separately tracked)
1. **Update tech references in the specs** to the Rust stack: `c4-l2.yaml` container technologies + relationships,
   `CLAUDE.md` architecture summary, `customer_screens.yaml` tech header, ADR-0033 deferred-runtime note
   (renderer → Leptos). Domain specs unchanged.
2. **Codegen language decision** (DECIDED): rewrite the codegen in Rust (`tools/codegen-rs`, bin `generate`),
   ported incrementally to parity. Local toolchain via `rustup` (pinned `rust-toolchain.toml`); `make rust`
   runs build + test + validate + generate(+diff) locally, and the CI `rust-codegen` job runs the same —
   **validate + generate + git-diff**, so a spec↔generation mismatch fails the build (same guarantee as the
   TS job). The TypeScript `consistency` job stays the blocking gate until the Rust tool reaches parity.
   Ported so far (byte-identical, verified by generate+diff): spec loading + meta-strip, `$ref` referential
   integrity, the actor + view + API models, and the emitters for `translations.generated.json`,
   `views.generated.sql`, `c4.generated.dsl` + `c4.generated.md` (Structurizr + Mermaid), and
   `schema.generated.graphql` (the full GraphQL SDL), the **`database.md`** §2 read-model injection, and the
   **`documentation.generated.md`** Markdown docs (the bounded-context engine `buildContextMap` + stories +
   every kind rendered with cross-links, ~6.4k lines). Remaining: the HTML documentation emitter, then the
   other validation gates — then flip CI to Rust and retire the TS codegen.
3. **Generation targets**: what the codegen emits for Rust — `shared_types` (serde), Crux core skeletons from
   actors/commands/events, `async-graphql` schema, `sqlx` migrations from `views.yaml`, the Leptos SDUI
   registry from `customer_screens.yaml`. Currently it emits GraphQL SDL + SQL + C4 + docs (renderer-agnostic).

## Influences
Ports-and-Adaptors / Hexagonal (Crux). Same DDD/CQRS lineage as ADR-0033 (Evans, Young, Vernon, Beck, Patton).
"Agentic engineering, not vibe coding" (post-Karpathy Feb-2026).

## References
Perplexity `RUST_ARCHITECTURE_DECISION` note (2026-07-03). Complements ADR-0033 (SDUI, renderer-agnostic).
