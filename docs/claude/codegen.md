# Claude rules — codegen (`tools/codegen-rs`)

The generator/validator is a **Rust** tool ([`tools/codegen-rs`](../../tools/codegen-rs), bin `generate`).
There is **no LLM in the generation loop** — it is deterministic. It began as a TypeScript tool
(`tools/codegen`), was ported to Rust at parity (all 8 artifacts byte-identical + the same validation issue
set, verified by a differential harness), and the TypeScript codegen was then retired (ADR-0034).

## Commands

Needs a local Rust toolchain (`cargo`, via `rustup`; pinned in `tools/codegen-rs/rust-toolchain.toml`).

- `make validate` — `cargo run … -- --check --specs specs` (validate only, writes nothing).
- `make generate` — `cargo run … -- --specs specs` (validate + write artifacts) then fail on drift.
- `make rust` — `cargo build` + `cargo test` + validate + generate (+ `git diff`) — the full gate.
- `make typecheck` — `cargo build` (the compiler is the type gate).

## Layout

Single crate, one binary (`src/main.rs`), organized in sections that mirror the old TypeScript modules:

- **loading** — `load_model` parses `specs/*.yaml` into `Model { defs }`; `SOURCE_FILES` is the load order
  (add new spec files there so their `$ref`s are checked). File-level `version`/`description` are stripped.
- **refs** — `parse_ref` / `resolve_ref` / `ref_target_file` / `collect_refs` (`$ref` parsing/resolution,
  cross-file + local `#/…`; `collect_refs` locations are dot-joined).
- **validate** — `fn validate` runs §1–§11 (referential integrity + all semantic checks; this is our
  "schema", ADR-0002) and returns the `Issue` set + `Coverage`.
- **emitters** — `emit_translations_json`, `emit_views_sql` + `emit_views_markdown` (the `database.md` §2
  injection), `emit_structurizr` + `emit_mermaid` (C4), `emit_schema` (GraphQL SDL), `emit_documentation`
  (md) + `emit_documentation_html` (html); `build_context_map` is the bounded-context engine. Rust-code
  emitters target `crates/**/generated`: domain types (scalars/entities/events/commands/errors/lifecycles),
  projection rows/projectors + PM state stores (app + Pg, item 5), the service catalog (item 4, issue #26:
  `emit_services_application` traits, `emit_services_http_clients` + `emit_service_bindings`
  (infrastructure), expose-gated `emit_services_routes` (server)), and the async-graphql layer.
- **main** — orchestration + the coverage report printed by validate/generate.

## Output policy

- Generated artifacts go to `specs/generated/**` (committed; CI verifies they match the specs) and the
  marker-injected `specs/database.md` §2 (between `<!-- GENERATED:views START/END -->`).
  `tools/codegen-rs/out/` is only ephemeral build scratch (gitignored), e.g. Structurizr `.mmd` exports.
- Generated files carry a "GENERATED — do not edit by hand" banner. **Never hand-edit `specs/generated/**`**
  or injected regions; change the spec or the emitter and regenerate.
- `specs/generated/documentation.generated.{md,html}` is the navigable product doc; `views.generated.sql` the DDL;
  `schema.generated.graphql` the SDL (the hand-written `schema.graphql` was removed);
  `c4.generated.dsl`/`c4.generated.md` the Structurizr/Mermaid views.
- An emitter change must keep output stable-or-intentional: CI regenerates and fails on any drift, so
  commit the regenerated `specs/generated/**` in the same change.

## GraphQL conventions (`emit_schema`)

- **Every query with args takes one generated input class** `<Query>QueryInput` — args are never inlined
  (parallel to mutations' `<Command>Input`). Input is `!` when any arg is required, nullable when all
  args are optional. Entity-typed args pull in their `…Input` value-object types automatically.
- One mutation = one command; result is `<Mutation>Payload` always carrying `correlationId`.

## Validation must stay green

- 0 errors is required. The only accepted warnings are the known view design-holes
  (`view-fedby-unused`, `view-column-no-source` ×3). Any new warning is a real signal — fix or justify.
- When you add a spec concept, add its validation rule in the same change (the model must not be able to
  drift silently). Adding a new source file = add it to `SOURCE_FILES` so its `$ref`s are checked.
- Prefer total access on the YAML `Value` tree: `.get(...).and_then(...)` with explicit fallbacks over
  unchecked indexing, so a missing/mistyped node surfaces as a validation error, never a panic.
