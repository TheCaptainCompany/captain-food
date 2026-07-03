# Claude rules — codegen (`tools/codegen`)

The generator/validator is TypeScript run via `tsx`. There is **no LLM in the generation loop** — it is
deterministic.

> **Rust port (ADR-0034):** [`tools/codegen-rs`](../../tools/codegen-rs) (bin `generate`) is a faithful
> re-implementation at **parity** — the full validator (§1–§11) + every emitter, producing all 8 artifacts
> byte-identical and the same issue set (both CI-verified). Run it with `make rust` (build + test + validate
> + generate + diff). The **TypeScript codegen below stays the blocking gate** until it is retired; keep the
> two in lockstep — a change to an emitter or validation rule must be made in **both** (or the diff/issue
> parity CI fails). When you touch `src/*.ts` here, mirror it in `tools/codegen-rs/src/main.rs`.

## Commands

- `npm run typecheck` — `tsc --noEmit`.
- `npm run validate` — `tsx src/cli.ts --check` (validate only, writes nothing).
- `npm run generate` — `tsx src/cli.ts` (validate + write artifacts).
- `make rust` — the Rust port: `cargo build` + `cargo test` + `--check` + generate + `git diff` (parity gate).

## Layout

- `src/load.ts` — parse `specs/*.yaml` into the typed model. `SOURCE_FILES` (in `src/model.ts`) is the
  load order; add new spec files there.
- `src/refs.ts` — `$ref` parsing/resolution (cross-file + local `#/...`).
- `src/validate.ts` — referential integrity + semantic checks (this is our "schema"; ADR-0002).
- `src/emit/*` — emitters: `documentation.ts` (md), `documentation-html.ts` (html), `database.ts`
  (views SQL + `database.md` §2 injection), `schema.ts` (GraphQL SDL).
- `src/cli.ts` — orchestration + the coverage report printed by `validate`/`generate`.

## Output policy

- Generated artifacts go to `specs/generated/**` (committed; CI verifies they match the specs) and the
  marker-injected `specs/database.md` §2 (between `<!-- GENERATED:views START/END -->`).
  `tools/codegen/out/` is only ephemeral build scratch (gitignored), e.g. Structurizr `.mmd` exports.
- Generated files carry a "GENERATED — do not edit by hand" banner. **Never hand-edit `specs/generated/**`**
  or injected regions; change the spec or the emitter and regenerate.
- `specs/generated/documentation.generated.{md,html}` is the navigable product doc; `views.generated.sql` the DDL;
  `schema.generated.graphql` the SDL (the hand-written `schema.graphql` was removed);
  `c4.generated.dsl`/`c4.generated.md` the Structurizr/Mermaid views.

## GraphQL conventions (emit/schema.ts)

- **Every query with args takes one generated input class** `<Query>QueryInput` — args are never inlined
  (parallel to mutations' `<Command>Input`). Input is `!` when any arg is required, nullable when all
  args are optional. Entity-typed args pull in their `…Input` value-object types automatically.
- One mutation = one command; result is `<Mutation>Payload` always carrying `correlationId`.

## Validation must stay green

- 0 errors is required. The only accepted warnings are the known view design-holes
  (`view-fedby-unused`, `view-column-no-source` ×3). Any new warning is a real signal — fix or justify.
- When you add a spec concept, add its validation rule in the same change (the model must not be able to
  drift silently). Adding a new source file = add it to `SOURCE_FILES` so its `$ref`s are checked.
- `noUncheckedIndexedAccess` is on: index access is `T | undefined` — guard (`?? {}`, `?? ''`).
