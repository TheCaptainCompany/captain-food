# Claude rules — codegen (`tools/codegen-rs`)

The generator/validator is a **Rust** tool ([`tools/codegen-rs`](../../tools/codegen-rs), bin `generate`).
There is **no LLM in the generation loop** — it is deterministic. It began as a TypeScript tool
(`tools/codegen`), was ported to Rust at parity (all 8 artifacts byte-identical + the same validation issue
set, verified by a differential harness), and the TypeScript codegen was then retired (ADR-0034).

## Commands

Needs a local Rust toolchain (`cargo`, via `rustup`; pinned in `tools/codegen-rs/rust-toolchain.toml`).

- `make validate` — `cargo run … -- --check --specs specs` (validate only, writes nothing).
  (The package carries a SECOND binary since #363: `cargo run --bin determinator -- <affected|hash>`
  — the build-matrix gate (ADR-20260807-223428); `default-run = "generate"` keeps bare
  `cargo run` meaning the generator, so Makefile/hooks/ci.yml are unaffected.)
- `make generate` — `cargo run … -- --specs specs` (validate + write artifacts) then fail on drift.
- `make rust` — `cargo build` + `cargo test` + validate + generate (+ `git diff`) — the full gate.
- `make typecheck` — `cargo build` (the compiler is the type gate).

Every target invokes `$(CARGO)`, which is plain `cargo` on Linux/macOS/CI/Git-Bash. Under **Cygwin**
it becomes `rustup run <channel> cargo`: the rustup proxy mis-detects its own `argv[0]` there and runs
as `rustup`, failing with `invalid value 'build' for '[+toolchain]'`. The same shim (plus a `cygpath -m`
conversion of the paths handed to the native cargo) is in `.claude/hooks/{stop-gate,validate-generated}.sh`.
Override with `make validate CARGO=/path/to/cargo` if your setup needs something else.

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
  emitters target `crates/**/generated` — and, since #373 (ADR-20260807-183024 step 2), WHOLE crates:
  `emit_domain_scope_crates` writes one `domain-{scope}` crate per `specs/{scope}/` under
  `crates/domains/` (kernel = `domain-common`; manifest included, `[dependencies]` DERIVED from the
  fragments' cross-scope `$ref` edges — the §14 DAG makes them acyclic by construction; stale crates
  pruned; workspace membership via the `crates/domains/*` glob), `crates/domain`'s generated modules
  become re-exporting FACADES (same paths, same type identity) keeping the cross-scope artifacts
  (DomainEvent union, global error catalog, states/lifecycles), and `emit_crate_graph` commits the
  derived topology to `specs/generated/crate-graph.generated.json`. Since #382 (step 3),
  `emit_bin_crates` writes ONE BINARY CRATE PER DEPLOYABLE under `crates/bins/` (49 bins:
  `actor-*`/`pm-*`/`projector-{scope}`/`graphql-{scope}`/`gateway-{role}`/surfaces/`bam`;
  manifests = the bin's scope assertion, stale bins pruned, `crates/bins/*` workspace glob), the
  crate-graph `bins` section covers the FULL topology (+ `path` per bin — the #349 input
  contract), and validator §15 (`c4-bin-*`) keeps derived bins ↔ `c4-l2.yaml` containers
  drift-free both ways. NOTE the glob bootstrap order: a workspace glob that matches NOTHING
  fails every cargo command including the generator's own build — introducing a new generated
  crate family means generating once before (or in the same change as) the glob lands. Other Rust
  emitters: domain types (scalars/entities/events/commands/errors/lifecycles),
  projection rows/projectors + PM state stores (app + Pg, item 5), the service catalog (item 4, issue #26:
  `emit_services_application` traits, `emit_services_http_clients` + `emit_service_bindings`
  (infrastructure), expose-gated `emit_services_routes` (server)), and the async-graphql layer.
  **The one emitter that MEASURES** (#491): `emit_app_index` writes `specs/generated/apps.generated.md`
  — the 57-app index (family, boundary, declared vs resolved domain crates, pod grants) — and its
  `resolved` column comes from `measure_workspace_crate_graph` (guppy over `cargo metadata`; normal
  links, workspace members only), not from the model. Hence it runs LAST in `main`, after the domain
  and bin crate manifests this same pass writes: measuring before writing would render the graph as it
  stood before a new deployable existed, and only the NEXT run would agree with itself. It also means
  `make generate` needs a resolvable workspace, and refuses (exit 1) rather than emit a guessed column.
- **main** — orchestration + the coverage report printed by validate/generate.

## Output policy

- Generated artifacts go to `specs/generated/**` (committed; CI verifies they match the specs) and the
  marker-injected `specs/database.md` §2 (between `<!-- GENERATED:views START/END -->`).
  `tools/codegen-rs/out/` is only ephemeral build scratch (gitignored), e.g. Structurizr `.mmd` exports.
- Generated files carry a "GENERATED — do not edit by hand" banner. **Never hand-edit `specs/generated/**`**
  or injected regions; change the spec or the emitter and regenerate.
- `specs/generated/apps.generated.md` is the app index — every deployable with its boundary, its
  declared vs resolved domain crates and its pod grant (#491).
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
