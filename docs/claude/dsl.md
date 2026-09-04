# Claude rules — DSL (`specs/**`)

The YAML DSL under `specs/` is the **functional source of truth**. Read the relevant file before
changing anything (see `CLAUDE.md` for the index).

## Per-scope spec folders (ADR-20260807-183024 D1/D5/D8, #375)

The domain catalogs are split into **`specs/{scope}/{kind}.yaml`** fragments — scopes
`ordering · catalog · network · customer · delivery · payments · comms · common` — for kinds
`scalars/entities/events/commands/errors/actors/processmanager/rules` (flat, one item per
top-level key) plus `api.yaml` (sections `types/inputs/queries/mutations/subscriptions`) and
`configuration.yaml` (`keys`). The codegen **loader merges** every fragment into ONE logical
catalog per kind, with per-item ORIGIN tracking; a duplicate item name across files mapping to
one catalog is a validation error (`scope-duplicate-item`).

- **`$ref`s are KIND-logical and never change**: `commands.yaml#/X` names the KIND (per
  `REF_CONTRACT`), not a file path. Moving an item between scope folders rewrites no refs.
- **The actor's folder is the scope declaration** everything else derives from. Validator §14:
  - *Placement* (`scope-placement-*`): a command lives with its handling actor; an event with its
    authoring actor (an aggregate re-emitting the event it received RECORDS it — echo-records do
    not author; pure inbound facts fall back to the handling scope); an error with its throwing
    actor; an api mutation with its command, a type with its read-model's aggregate, an operation
    with its return type. `specs/common/` is ALWAYS a legal home (kernel promotion = a
    cross-scope contract); a WRONG scope is not; multi-scope derivation REQUIRES common.
  - *Cross-scope `$ref` DAG* (`scope-cycle`): non-PM-sourced cross-scope refs must be acyclic.
    **Process managers are declared bridges** — an orchestrator legitimately closes loops between
    scopes; its refs ARE its dependency list, realized by #373 in the generated
    `specs/generated/crate-graph.generated.json` `bins` map (e.g. `pm-place-order` →
    ordering+payments+common), which step (3)'s bin emitter turns into the pm bins' Cargo deps.
  - *Kernel purity* (`scope-kernel-purity`): a `common/` item references only `common/` items.
  - *API nesting* (`api-nested-cross-scope`, D8): an api type nests only its own scope's or
    kernel types — cross-scope data appears at top level or pre-joined in a projector-owned view.
- `specs/common/{kind}.yaml` also carries each kind's original doctrine header (the catalog-wide
  description); scope fragments carry a short generated header.
- Structural dirs (`architecture/ database/ screens/ generated/ integrations/`) are never scopes.
- **Moving/deleting a spec file? Grep `crates/` for `include_str!`/path reads FIRST** — the P2
  split broke a compile-time `include_str!("../../../specs/configuration.yaml")` in
  `crates/telemetry` (cost: one red CI round).

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

- **`specs/**` is ordinary work** (founder directive 2026-08-10,
  [ADR-20260810-221840](../adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)) —
  the freeze is lifted; execution loops may add and amend DSL content **and structure**. Three
  questions before any edit lands: **(1)** does it contradict or create a recorded decision
  (`docs/proposals/DECISIONS.md`, `docs/adr/`)? → stop, file a register row. **(2)** Is the shape
  already emitted, stored or promised (`domain_events`, a shipped client, an alert route, a partner
  contract, a legal artifact)? → it is a **migration**: record the versioning story first (upcasting,
  never mutation). **(3)** Otherwise it is the team's, `specs/common/` included. Every landed spec
  change writes its one-sentence row in [docs/SPEC-LOG.md](../SPEC-LOG.md) in the **same commit**.
- Commands derive from **use cases** (story map), not mechanically one-per-event (see `CLAUDE.md`).
- If a behaviour test fails, fix the generator or runtime — **do not weaken the test**.
- **Completeness is enforced (ADR-0032), not optional:** a new command/event/error needs a behaviour test
  in `tests.yaml`, and that test needs a `rules: [{ $ref: 'rules.yaml#/<Rule>' }]` link (add the rule to
  `rules.yaml` if new); a new mutation/query needs a story step in `stories.yaml`. `make validate` fails
  otherwise (`test-uncovered-*`, `rule-uncovered`, `test-no-rule`, `op-uncovered-by-story`). Extend the
  specs — never weaken the gate.
- After any DSL change: `make validate` must be green before `make generate`.

## The specs index — full detail (moved from CLAUDE.md, 2026-08-01)

CLAUDE.md keeps the one-line index; the load-bearing detail lives here:

- **specs/database/** (ADR-0037/0039/0040) — the store schema as DSL: `tables/*.yaml` are the real
  tables, globbed. `tables/eventstore.yaml` = `domain_events` + `domain_stream`;
  `tables/referential.yaml` = seed/config tables (repo seed script, not projected);
  `tables/projection_tables.yaml` = MATERIALIZED read-model tables, each `projector: app` (a Rust
  projector over `domain_events`); `tables/integration_staging.yaml` = ADAPTER-OWNED raw staging
  (`staging: true` — SIRENE mirror + the verbatim `external_stripe_events` /
  `external_hubrise_callbacks` webhook mirrors, ADR-0045 / ADR-20260720-015400);
  `tables/journals.yaml` = the WRITE-PATH JOURNALS (ADR-20260720-015300/-015400): the
  `inbound_messages` mailbox (+ `mailbox_partitions`) and the residual `command_journal` —
  journals never write `domain_events` and never replay as state. No SQL triggers (ADR-0040).
  `projection_views.yaml` = the event-fed `View_*` read models — SQL VIEWS **generated** as a
  per-column state-fold over `domain_events` from each column's `from` lineage (ADR-0039).
  **Naming: `View_*` = a SQL VIEW; an unprefixed name = a TABLE.** `functions/*.sql` = event-store
  functions. Generated to `specs/generated/schema.generated.sql` + `views.generated.sql`; enum
  columns store the scalars.yaml TEXT value verbatim (ADR-20260728-170000 — no `ref_<enum>`
  lookups). `specs/database.md` is the narrative rationale; the query→read-model mapping is the
  `@reads` binding in the api fragments (`specs/{scope}/api.yaml`).
- **specs/screens/** (ADR-0033/0037, taxonomy ADR-20260722-091500) — Spec-Driven SDUI apps, one
  file per audience (folder conveys the `_screens` suffix). Customer-facing front offices split
  by host (ADR-20260722-160000): `captain_frontoffice.yaml` = the marketplace at
  `live.captain.food`; `restaurant_frontoffice.yaml` = a single restaurant's storefront at
  `{slug}.captain.food` (roles PUBLIC+CUSTOMER; also the source of the shared SDUI component
  registry for `crates/web` `registry.rs`). Then `restaurant_backoffice.yaml`, `rider.yaml`,
  `system.yaml`. Each file: screens + a `resolvers` allowlist (reads → api.yaml queries by $ref)
  + an `actions` allowlist (writes → api.yaml mutations by $ref); screens declare `roles`
  (⊆ UserType) and files declare `app_types`. The validator proves the API answers the UI; UI
  needs the API lacks are explicit `gaps`; `sdui: false` marks non-SDUI screens. **R1** (#639
  part C 2c-ii, PROP-20260831-180622 §5): a screen may declare `graphql_role: <UserType>` to address
  `/{role}/graphql` instead of its surface's role (the rider sign-in door speaks to
  `/public/graphql`); validator §26 refuses the declaration unless the role is one of the screen's
  `roles`, `PUBLIC` comes with `requires_auth: false`, and EVERY operation the screen binds (its
  tree, its reads, the sheets it opens) admits that role — a role-refused control is a control that
  renders and does nothing. A `requires_auth` screen may declare
  `unauthenticated: { type: navigate, route }` naming an open route of the same file: the server
  302s a cookie-less GET there and the client navigates there on a 401 from its role path. An
  `inline_error` with `for_action: <action>` is where that action's REJECTED verdict renders, in
  the caller's language (the server localizes `Operation.message` from the row's typed context).
- **specs/translations.yaml** (ADR-0033; sidecars ADR-20260722-101500) — SHARED UI i18n catalog,
  errors.yaml-style (dotted keys + typed `params` + `messages.en`/`fr`) for cross-surface
  strings (`common.*`) + future backend text; surface-specific strings live in co-located
  sidecars `specs/screens/<surface>.translations.yaml` (keys globally unique across files); the
  codegen merges everything into one `translations.generated.json`.
- **specs/{scope}/api.yaml** — the GraphQL surface as per-scope fragments (output-type registry, queries, mutations, ACL
  `roles` → `@auth`/`@public`); SDL GENERATED to `specs/generated/schema.generated.graphql`.
  **Role = path**: one master schema served per-role under `/{role}/graphql` (PUBLIC, CUSTOMER,
  RESTAURANT_ACCOUNT, RESTAURANT, RIDER, ADMIN, EXTERNAL).
- **specs/stories.yaml** — the executable story map (personas → activities → steps, each step a
  $ref into api.yaml); the validator enforces completeness BOTH ways (steps resolve + persona
  authorized, and every mutation/query reached by ≥1 step, `op-uncovered-by-story`).
- **specs/{scope}/rules.yaml** (ADR-0032) — business rules/invariants; every behaviour test links ≥1
  rule and every rule is asserted by ≥1 test (bidirectional, validator-enforced). Rules = WHAT,
  `specs/tests.yaml` = HOW (Given/When/Then).
- **specs/{scope}/actors.yaml** — the actor-model catalog (codegen source): aggregates & process
  managers, each with typed `identity`, optional `mailbox`, `reminders:`/`deletion:`
  (ADR-20260731-214500), and an inbox of `{ message → emits, throws, schedules }` where every
  ref is kind-checked.

## CQRS methodology — commands vs inbound events (moved from CLAUDE.md, 2026-08-01)

Commands are **derived from use cases** (ADR-0004), never mechanically one-per-event: a command
may emit several events (`PlaceOrder` → frozen checkout + payment intent) and not all commands
have a 1:1 counterpart.

**Not every event originates from a command.** A command is a request the system can REJECT; an
external system sometimes just INFORMS us of a fact that already happened — nothing to validate,
nothing to refuse. Those are **inbound (integration) events**, recorded directly through the
Anti-Corruption Layer (idempotently keyed), without a command. Rule of thumb: originator can be
told "no" → command; stating an already-occurred fact → inbound event. Captain.Food inbound
events: Stripe `PaymentAuthorized`/`PaymentCaptured`/`PaymentReleased`/`PaymentFailed`/`PaymentRefunded`; HubRise inventory sync +
externally-channeled order updates; delivery partners' `DeliveryStatusUpdated`/
`DeliveryAcceptedByPartner`. Note the request/report split: a refund is REQUESTED by a command
(`RejectOrder`, `CancelOrder*`) but the `PaymentRefunded` FACT is REPORTED by Stripe. Contrast
`ImportCatalog`: stays a command even though the data comes from HubRise — we orchestrate it and
can reject it. In the story map, inbound events are marked 📥.

