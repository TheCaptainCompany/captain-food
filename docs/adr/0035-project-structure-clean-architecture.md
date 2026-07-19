# ADR-0035 — Project structure: Clean Architecture crate layout for the Rust workspace

## Status

Accepted (CTPO decision, 2026-07-03). Builds on ADR-0034 (full-stack Rust); refines the *implementation* of
ADR-0005 (read side via `View_*`). Realized incrementally as the `crates/` workspace is created.

## Context

ADR-0034 adopted full-stack Rust but did not fix the crate layout. A CTPO draft
(`incoming_news_from_perplexity/20260703-10h37-PROJECT_STRUCTURE.md`) proposed a Clean Architecture
(Martin) + DDD + Crux Ports-&-Adapters structure. Reviewing it against the specs (the real source of truth)
and the retired-TypeScript / now-Rust codegen (ADR-0034) surfaced several drifts and gaps that this ADR
reconciles. The dependency rule is absolute: **outer layers depend on inner layers, never the reverse**,
enforced at compile time by Rust's crate boundaries.

## Decision

### Workspace layout (corrected against the draft)

```
captain-food/
├── Cargo.toml                 # workspace root (lists crates); rust-toolchain.toml; .cargo/config.toml (WASM/aarch64/x86_64)
├── specs/                     # SOURCE OF TRUTH — the real DSL (NOT two files):
│   ├── scalars/entities/events/commands/errors/actors/views/api/stories/rules/tests/
│   │   translations/customer_screens/observability .yaml
│   ├── architecture/c4-l2.yaml c4-l3.yaml
│   ├── database.md            # narrative + injected/derived read-model prose
│   ├── integrations/hubrise.md supabase.md
│   └── generated/             # GENERATED (committed; CI diffs): schema.graphql, views.generated.sql, c4.*, docs, translations.json
├── tools/codegen-rs/          # THE codegen (ADR-0034): validator §1–§11 + all emitters + the Rust generation targets
├── crates/
│   ├── domain/                # ★ pure DDD: aggregates, commands, events, policies, value objects, typed IDs.
│   │                          #   MAY derive serde on events/VOs (see decision 1). deps: serde, uuid, chrono, thiserror.
│   ├── application/           # use cases: ports/ (traits), commands/ (write handlers), queries/ (read handlers),
│   │                          #   process_managers/ (sagas: PlaceOrderProcess, RefundProcess, delivery). deps: domain.
│   ├── infrastructure/        # adapters implementing application ports:
│   │   ├── persistence/       #   event_store (append to domain_events), read_models/ (repos querying View_* SQL views)
│   │   ├── integrations/      #   ACL: hubrise/ stripe/ delivery/ — translate external ↔ domain; inbound-event recording
│   │   └── events/            #   (post-V0) projectors: domain_events → materialized read tables
│   ├── server/                # Axum BFF: main.rs (DI), middleware/{auth,tenant,tracing,rate_limit}, handlers/,
│   │                          #   graphql/ (async-graphql, role-as-path), sdui/ (resolver registry, hydrator, validator)
│   ├── shared_types/          # serde DTOs + sdui_types (GENERATED) + uniffi.udl. deps: serde, uniffi.
│   ├── core/                  # Crux core over domain: model, events, capabilities. deps: crux, domain, shared_types.
│   ├── web/                   # Leptos→WASM: renderer, registry.rs (GENERATED), components/, screens/ (non-SDUI), i18n/
│   └── desktop/               # Tauri 2.0 shell (embeds server + web)
├── ios/  android/             # thin SwiftUI / Compose shells over core via UniFFI (generated bindings — never edit)
├── supabase/
│   ├── migrations/            # INCREMENTAL timestamped SQL for STATEFUL tables (domain_events, auth/tenant) — EF-style
│   └── seed/
└── .github/workflows/         # ci.yml (the codegen gate) + deploy_web / deploy_server
```

Differences from the draft (the "misses"): the real granular `specs/` (not 2 files); the codegen is
`tools/codegen-rs` run by `make generate` (not `scripts/generate.rs` via `pnpm`); an `application/process_managers/`
for sagas; `infrastructure/integrations/` as the ACL incl. **HubRise** (a V0 integration the draft omitted);
`server/middleware/tenant.rs` for `{slug}.captain.food` host resolution; and the read side below.

### Dependency rules

Allowed: `server → application, infrastructure, shared_types`; `application → domain`; `infrastructure →
application, domain`; `core → domain, shared_types`; `web → shared_types, core`; `desktop → server, web`.
Forbidden: `domain → anything`; `application → infrastructure` (use ports); `application → server/web/desktop`;
any circular dep. Ports & Adapters: a use case that needs I/O declares a `trait` in `application/ports/`,
`infrastructure` implements it, `server/main.rs` injects it. **Amendment to the draft: `domain` MAY depend on
`serde`** — see decision 1.

### The four locked decisions

1. **serde in `domain` is allowed** (decision, overrides the draft's ban). Domain events and value objects
   derive `Serialize`/`Deserialize` because they are serialized into the append-only `domain_events` log and
   cross the UniFFI/Crux boundary; a "pure" ban would force a mirror DTO + mapper per type for no benefit.
   Serialization *logic* (wire formats, HubRise `"9.80 EUR"` parsing) stays out of the domain, in the ACL.
2. **CQRS read side = event store + read-model repos; V0 `View_*` are Postgres SQL views over
   `domain_events`** (projection-on-read), not projector-fed tables. This *refines ADR-0005*, it does not
   reverse it: queries still target `View_*` (never raw `domain_events`), so the read-side contract holds;
   only the backing implementation changes. `views.yaml` `columns.from` (event→column lineage) becomes the
   view's SELECT projection. Evolution: swap a hot `View_*` to a materialized table + projector later with no
   query-API or `views.yaml` change.
3. **i18n**: `translations.yaml` stays the source of truth (typed params, en/fr, validated). The codegen emits
   the JSON bundle now; it will additionally emit Fluent `.ftl` per locale (for `leptos_i18n`) once locale
   plural/format rules are actually needed. Format change ⇒ emitter change only, no source change.
4. **Migrations**: incremental, timestamped SQL (EF-style) for **stateful** tables (`domain_events`, auth/
   tenant) — append-only, so usually additive. `View_*` are regenerated idempotently as `CREATE OR REPLACE
   VIEW` on every deploy (not deltas). Rollback uses the view layer as an indirection: repoint / replace a
   view to a prior definition — zero data migration, and since the log is immutable any view version is
   rebuildable.

### Codegen generation targets (realizes ADR-0034 follow-up #3)

From the specs, `tools/codegen-rs` will emit: `shared_types` serde DTOs + `sdui_types`; Crux/domain scaffolds
from `actors`/`commands`/`events`; the `View_*` **`CREATE OR REPLACE VIEW … FROM domain_events`** DDL (V0) +
incremental migrations for stateful tables; the Leptos SDUI `registry.rs` from `customer_screens.yaml`;
`#[test]`s from `tests.yaml`; and (later) `.ftl` from `translations.yaml`. All remain drift-checked by CI.

## Alternatives considered

- **Draft as-is** (2-file specs, `scripts/generate.rs`, serde-free domain, state repositories): rejected —
  contradicts the real DSL, the retired TS codegen, event sourcing, and Crux.
- **Projector-fed read tables in V0**: deferred — more moving parts (async workers, lag, rebuild tooling)
  than Tours-scale needs; SQL-views-on-log is simpler and consistent, and the API contract lets us upgrade
  per-view later.
- **Single mega-crate**: rejected — loses the compile-time dependency enforcement that is the point.

## Consequences

### Positive
- Compile-time-enforced Clean Architecture; domain stays framework-free (bar serde-as-data); one core across
  web/desktop/mobile; read side trivially rebuildable from the immutable log; cheap, safe read-schema rollback.
### Negative
- SQL-views-on-log re-fold events per query (un-indexed, slower) — acceptable at V0 scale, upgradeable per-view.
- More crates = more `Cargo.toml` wiring; the domain/core split must be kept clear (domain = DDD, core = Crux).
### Follow-up actions
- Create the `crates/` workspace to this layout (skeletons first, dependency rules enforced).
- Build the ADR-0034 #3 generation targets, starting with `shared_types` + the `View_*` view DDL.
- Update CLAUDE.md's architecture summary (domain/core split; crate list) and `docs/adr/README.md` index.

## Influences
Clean Architecture (Martin), DDD bounded contexts + ACL (Evans), Ports & Adapters / Hexagonal (Cockburn,
Crux), CQRS + event log (Young). Complements ADR-0033 (SDUI) and ADR-0034 (full-stack Rust).
