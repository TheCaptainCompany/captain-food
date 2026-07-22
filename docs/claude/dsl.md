# Claude rules — DSL (`specs/**`)

The YAML DSL under `specs/` is the **functional source of truth**. Read the relevant file before
changing anything (see `CLAUDE.md` for the index).

## Conventions

- All content is **English** (identifiers, descriptions, comments). No French except user-facing
  `messages.fr` in `errors.yaml`.
- Reference types with `$ref`, never bare name strings (e.g. `{ $ref: 'scalars.yaml#/OrderId' }`).
  One name = one dedicated scalar; no ambiguous reuse.
- Every `$ref` site is **kind-checked** (§1b, ADR-20260722-152201): resolving is not enough, the target
  must be of a kind the site declares in `REF_CONTRACT` (`tools/codegen-rs/src/main.rs`) — a `state_table`
  must be a process-manager state table, a screen resolver a query, an actor `message` a command or event.
  Adding a **new ref-carrying field** to any spec file therefore also needs a `REF_CONTRACT` line: the
  validator is fail-closed and reports `ref-site-undeclared` (with the suggested line) until you add it.
- Event/command payloads are **business only** — never the technical envelope (`eventId`,
  `aggregateType`, `aggregateId`, `occurredAt`, `metadata`); infra adds that.
- `*Updated` events/commands carry the **full entity** (replace semantics).
- `Money = { amountCents, currency }`. Convert HubRise `"9.80 EUR"` only at the integration boundary.
- Slugs: `^[a-z0-9]+(?:-[a-z0-9]+)*$`.

## Naming

- Scalars/entities: PascalCase. Events: past tense (`OrderPlaced`). Commands: imperative
  (`PlaceOrder`). Errors: PascalCase code. Views: `View_*`. Fixtures (tests): camelCase.

## Versioning (SemVer per file `version:`)

- **MAJOR** breaking structure/semantics · **MINOR** backward-compatible addition · **PATCH** validation
  tightening / doc fix that does not break valid payloads.

## Change classification (state it in any plan)

`breaking` · `backward-compatible` · `generator-only` · `documentation-only` · `observability-only`.

## Hard rules

- **Autonomous/execution loops never modify `specs/**`** — only plan mode proposes DSL changes, with
  approval.
- Commands derive from **use cases** (story map), not mechanically one-per-event (see `CLAUDE.md`).
- If a behaviour test fails, fix the generator or runtime — **do not weaken the test**.
- **Completeness is enforced (ADR-0032), not optional:** a new command/event/error needs a behaviour test
  in `tests.yaml`, and that test needs a `rules: [{ $ref: 'rules.yaml#/<Rule>' }]` link (add the rule to
  `rules.yaml` if new); a new mutation/query needs a story step in `stories.yaml`. `make validate` fails
  otherwise (`test-uncovered-*`, `rule-uncovered`, `test-no-rule`, `op-uncovered-by-story`). Extend the
  specs — never weaken the gate.
- After any DSL change: `make validate` must be green before `make generate`.
