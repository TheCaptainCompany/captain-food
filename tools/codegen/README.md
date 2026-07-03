# @captain-food/codegen

Deterministic generator: reads the `specs/*.yaml` model, **validates** its referential integrity,
and **emits** derived artifacts. The yaml specs are the source of truth; edit them, re-run, and the
outputs follow — no LLM in the loop. (Claude is used only to author/extend the emitter templates.)

> **Rust port (ADR-0034):** [`../codegen-rs`](../codegen-rs) is a faithful re-implementation at **parity** —
> the same full validator and emitters, producing all artifacts byte-identical and the same validation
> issue set (both CI-verified; run `make rust`). It stays in lockstep with this TypeScript codegen, which
> remains the **blocking** gate until it is retired. Any change here must be mirrored in `codegen-rs`.

## Usage

```bash
cd tools/codegen
npm install
npm run validate     # load + check the model only (no files written); exits non-zero on error
npm run generate     # validate, then emit into ./out
```

Flags (via `tsx src/cli.ts`): `--check` (validate only), `--specs <dir>`, `--out <dir>`.

## How it works

```
load.ts      read specs/*.yaml → one in-memory Model (file metadata stripped)
refs.ts      parse/resolve `$ref`, deep-walk the tree to find every reference
validate.ts  referential integrity + actor wiring + coverage (the real value)
emit/        pure functions: Model → artifact string (TS template literals)
cli.ts       load → validate → (emit)
```

### Validation rules

- **ref-format / ref-dangling** — every `$ref` anywhere parses and resolves to a real definition.
- **actor-message / actor-emits / actor-throws** — an actor's `message` targets commands/events,
  `emits` targets events, `throws` targets errors.
- **command-unhandled** (warning) — a command with no actor handler.
- **event-orphan** (warning) — an event never emitted nor consumed.
- **op-no-authz / op-unknown-usertype** — every Query/Mutation declares `roles` (→ `@auth`/`@public`), with user types that exist in `scalars.yaml#/UserType`.
- **op-missing-command / op-missing-reads** — every mutation declares `@command`, every query declares `@reads`.
- **mutation-unknown-command / mutation-command-unhandled / command-duplicate-mutation / command-no-mutation**
  — the `@command` target is a defined, handled command; no command is declared twice; every handled
  command is declared by exactly one mutation.
- **view-unknown-aggregate / view-column-type / view-index-column / view-no-pk / view-fedby-unproduced**
  — each view's `aggregate` is a real aggregate, every column `type` resolves (SQL primitive or
  scalars.yaml type), indexes reference real columns, the view has a PK, and `fedBy` events are produced.
- **reads-unknown-view / view-no-query** — every view a type binds via `reads` exists in `views.yaml`;
  every non-internal view is bound by some output type (queries inherit the binding from their return type).
- **event-not-projected** (warning) — an emitted event that feeds no `View_*` and is not declared
  under `nonProjectedEvents`.
- **persona-no-role / persona-unknown-role / story-unknown-op / story-role-not-authorized** — the
  story map (`stories.yaml`): every persona declares a `personaRole` that is a `scalars.yaml#/UserType`,
  every activity step `$ref`s an existing api query/mutation, and the persona's role may actually call
  that op (the op is `@public`, i.e. its roles include `PUBLIC`, OR the role is in the op's `roles`).

**Two read-side primitives** (closing the "every event → a query-facing view" oversimplification):
- **Internal views** (`internal: true` in views.yaml) — read models consumed by *command handlers /
  auth resolution*, not a GraphQL query (e.g. `View_Customer`: phone-unique idempotency for
  `RegisterCustomer` + auth→Customer link). Exempt from `view-no-query`.
- **Non-projected events** (`nonProjectedEvents:` in views.yaml) — transient/saga-internal facts whose
  data is returned in the mutation payload, never read via a projection (e.g. `PaymentIntentCreated`).
  Exempt from `event-not-projected`.

A command referenced only from `properties` is classified as a *command value object* (e.g. `CartLine`),
not an unhandled command. `npm run validate` prints a "validated against specs" report so the coverage
(refs, actor wiring, schema↔model, views) is visible, not silent.

### The GraphQL ↔ domain link is DECLARED, not inferred

The mutation→command and query→view links are **explicit** in `api.yaml` (a mutation names its
`command`; a type names its `reads`), never a naming convention guessed by the generator. They become
the `@command`/`@reads` directives in the emitted SDL:

```graphql
registerRestaurant(input: ...): ... @auth(requires: [ADMIN, RESTAURANT_ACCOUNT]) @command(name: "RegisterRestaurant")
restaurants: [...] @public @reads(views: ["View_Restaurant"])
```

`validate` enforces both directions of these declared links (see rules above), so traceability is
derived from what the spec *states*, not from what the tool *assumes*. `UserType` is declared in
`scalars.yaml`; the emitted SDL mirrors it.

The SDL is now PURE OUTPUT — generated to `specs/generated/schema.generated.graphql` from `api.yaml`
(+ scalars/entities/commands/views). The hand-written `schema.graphql` has been removed.

### `api.yaml` — the API surface (source of truth)

The GraphQL API is declared in **`api.yaml`**, not hand-written SDL (JSON-Schema vocabulary: `properties`,
`array`, `$ref`). It reuses scalars/entities/commands/views by `$ref` and declares only what is
GraphQL-specific:
- **types** — the OUTPUT-TYPE REGISTRY. Each entry is a resolver declaring its SHAPE **inline** via
  `properties` (the read/API shape — DECOUPLED from entities.yaml, the write shape, which may differ;
  the bound view is the source of truth) and its backing read model (`reads: [{ $ref: 'views.yaml#/View_*' }]`
  → `@reads`). Reads are declared on the TYPE, not the query. Properties reference scalars and shared
  value objects by `$ref`; value objects/sub-types with no API divergence (Address, Money, Category,
  OrderLineItem, …) are emitted from entities.yaml and referenced, not re-declared.
- **queries** — `args`, `returns` (a registered output type, e.g. `{ $ref: '#/types/Restaurant' }`),
  `roles`, `slice`. The query's `@reads` is **inherited** from its return type's binding.
- **mutations** — `command` (commands.yaml dispatch → input type derived from it), `roles` (→ `@auth`/
  `@public`), `slice`, and a minimal `payload` (the generator always adds `correlationId: CorrelationId!`).

Navigation between read models is declared in **`views.yaml` as foreign keys** (`fk: "View_X.col"`), not by
adding GraphQL fields to entities — entities stay pure domain shapes. The old SDL regex parser is retired;
the hand-written `schema.graphql` has been removed — the SDL is generated to `specs/generated/schema.generated.graphql`.

### `stories.yaml` — the story map (validated source)

`stories.yaml` is the product story map: **personas → activities → steps**, where each step `$ref`s the
`api.yaml` query/mutation that realizes it and each persona declares a `personaRole` (a
`scalars.yaml#/UserType`). The generator validates that every step ref resolves to a real op and that the
persona's role may actually call it (op `@public`, or role ∈ op `roles`) — so the story map can't drift
from the API or its ACL. It emits a **persona → activity → operation** matrix into
`documentation.generated.md`. It declares no new operations — only how personas use the existing surface.

## Status / roadmap

1. ✅ Loader + validator + `documentation.generated.md` — persona → mutation → command → handler → emits,
   now sourced from `api.yaml`.
2. ✅ `views.yaml` → `database.md` §2 + `specs/generated/views.generated.sql`, with FK navigation.
3. ✅ `api.yaml` is the API source; validated against the domain (command→handled, reads→views,
   roles→UserType, return types, payload/arg types). SDL parser retired.
4. ✅ Generate the full SDL from `api.yaml` (+ scalars/entities/commands/views) → `specs/generated/schema.generated.graphql`:
   scalars, enums, directives, output types **with FK-derived navigation**, input types, payloads
   (always `correlationId`), `Query`/`Mutation` with `@auth`/`@command`/`@reads`. This is now the only
   SDL — the hand-written `schema.graphql` has been removed.
   - **IDs are client-generated**: every create command carries its aggregate id (UUID), so creates are
     idempotent and the client reads the projection by that id with no round-trip. Inputs therefore
     include ids; payloads shrink to `correlationId` (exceptions: `RegisterCustomerPayload` = customerId
     + created on a returning phone; `PlaceOrderPayload` = Stripe paymentIntentId + clientSecret).
   - **Input curation**: server-derived fields are marked `readOnly: true` in entities.yaml (e.g.
     `Stock.status`) and the input emitter skips them — so input types stay intent-only.
5. ✅ Hand-written `schema.graphql` removed — `specs/generated/schema.generated.graphql` is the single SDL.
6. ✅ Rust port at parity (ADR-0034): [`tools/codegen-rs`](../codegen-rs) runs the full validator (§1–§11)
   + every emitter, byte-identical output + same issue set, CI-verified (`rust-codegen` job). Next: flip CI
   to make it the blocking gate and retire this TypeScript codegen.
7. ⬜ Rust generation targets (once `crates/` exists): `shared_types`, Crux handler scaffolds from
   `actors.yaml`, `sqlx` migrations, the Leptos SDUI registry.
