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

## `legacyStates:` — a retired lifecycle state, exempt from reachability (#639 part C step 4-i, ADR-20260904-081527 §6)

`legacyStates: [STATE, …]` is an optional key on an `actors.yaml` aggregate's `lifecycle:` block,
a sibling of `transitions:`/`terminal:`. It declares states named ONLY as the target of a retired
entry edge — no LIVE transition produces them anymore, but a pre-existing STORED row may still
carry the value, so its exit edge(s) stay declared (a legacy row must still be able to leave the
state) while the reachability gate stops demanding a live way IN. First use: `RiderStatus.SUSPENDED`
— the four `-> SUSPENDED` entry edges were removed when `RestrictRider`/`ReinstateRider` replaced
`ChangeRiderStatus` as the human-only door, but a historical row minted before that change may still
read `SUSPENDED`, and its one legacy exit (`SUSPENDED -> OFFLINE`) has to stay live for it. The
member must still appear in the scalar's own `enum:` (a `legacyStates` entry is never a licence to
invent a state the type does not have) — the exemption is a declared list the reachability check
consults by name, never a silent absence read as "must be fine". Validated in
`tools/codegen-rs/src/validate/lifecycles.rs`.

## `noTestFixturePossible:` — a DERIVED exemption from `test-uncovered-error` (#639 part C step 4-i round 3, ADR-0032)

`noTestFixturePossible: true` is an optional key on an `errors.yaml` item. It exempts that error
from the BLOCKING `test-uncovered-error` gate (§7c, ADR-0032: every throwable error needs a
`tests.yaml` case asserting it in a `thrown:`) — but the flag is never a bare per-item opt-in a
future error could set to escape the gate silently. It is **DERIVED**, checked by its own ERROR-level
validator rule, `error-exemption-unjustified`
(`tools/codegen-rs/src/validate/core.rs`): the flag is legal **only** when the error is thrown
(`throws:`) by at least one `receives:` entry, and **every** `receives:` entry that throws it names a
command with at least one property whose scalar declares `readOnlyCatchAll` (see above) — ALL, never
ANY: an error co-thrown alongside a catch-all-bearing command by even ONE *other*, ordinary command
is still coverable through a normal `tests.yaml` fixture and owes it real coverage; co-occurrence on
a shared `throws:` list is not the same claim as "this error's only cause IS the catch-all decode".
This is the exact class `readOnlyCatchAll` exists for: the catch-all variant is excluded from the
scalar's own `enum:` by construction, so a `tests.yaml` fixture spelling it fails
`test-invalid-enum-value` before a `thrown:` could even be asserted — the error is structurally
unspellable through the door, not merely un-covered by oversight. A flag that passes derivation still
needs a real proof: the error stays pinned by a named Rust unit test constructing the raw command
from JSON (never a typed literal of the catch-all variant, and never `tests.yaml`). First and only
use today: `errors.yaml#/RiderRestrictionGroundUnrecognised`, thrown solely by `RestrictRider`
(`ground` declares `readOnlyCatchAll: UNRECOGNISED`), pinned by
`crates/application/src/commands.rs::restrict_rider_unrecognised_ground_tests`.

## `whileRestricted:` — the standing carve-out grammar (#639 part C step 4-i, ADR-20260904-081527 §4)

`whileRestricted: [ROLE]` is an optional key on an `api.yaml` query or mutation: a SUBSET of the
operation's own `roles:` that stays reachable even while a caller's standing (see `readOnlyCatchAll`
below and `RiderStanding`) is RESTRICTED. Closed key set (`api_operation_keys.rs`), values validated
by `tools/codegen-rs/src/validate/api_while_restricted.rs`:

- `api-while-restricted-not-subset` — every value must be in the operation's own `roles:`; `roles:`
  omitted is itself an ERROR (nothing to carve out of an already-open operation — operations with
  `roles:` omitted are unaffected by restriction, full stop).
- `api-while-restricted-no-standing-source` — a value must name a role from the closed,
  standing-bearing set (today `{RIDER}`) — a role with no standing to test cannot be carved.
- `api-while-restricted-mutation-derives-actor` — a carved MUTATION must declare
  `derived: { <field>: rider }` (the #865 grammar): the acting identity under the carve-out is
  ALWAYS the caller's own, from `ReadScope`, never a client-suppliable id.

Emitted onto the SDL as `@whileRestricted(roles: [UserType!]!)`, a sibling of `@auth`, omitted when
the key is absent. The generated `guard = "RoleGuard::new(ALLOW_X).and(StandingGuard::new(&[...],
"opName"))"` attribute is emitted on EVERY role-guarded operation, with an EMPTY carve slice when the
key is absent — fail-closed by absence lives in the emitter (`tools/codegen-rs/src/emit/
server_graphql.rs::acl_field_attr`), never in the author's memory. `StandingGuard`
(`crates/server/src/graphql/acl.rs`) reads `ctx.data_opt::<ReadScope>()` only (never a claim — a
claim has no standing) and is chained `.and(..)` after `RoleGuard`, so the two questions (role,
standing) stay orthogonal.

A companion validator, `pm-sends-human-only-command`
(`tools/codegen-rs/src/validate/pm_human_only.rs`), makes it an ERROR for any `processmanager.yaml`
`sends:` to name a command whose `actors.yaml` `requires: acting` carries no `EXTERNAL` key — no
saga may impersonate the human such a door requires. Its EMITS complement, `pm-emits-human-only-event`
(same file, round 3, #639 part C step 4-i), makes it an ERROR for any `processmanager.yaml`
`receives[].emits:` to name an event produced only by such a command's door — no saga may declare
producing the RESULT of a human decision directly, bypassing the door the `sends:` rule guards.

## `readOnlyCatchAll:` — a decode-tolerant enum variant (#639 part C step 4-i, ADR-20260904-081527 §3)

`readOnlyCatchAll: <VARIANT>` is an optional attribute on a `scalars.yaml` enum scalar, a sibling of
`enum:` (never a member of that list). It declares a variant that:

- the generated **domain Rust enum** gains with `#[serde(other)]`, so an unrecognised stored value
  DECODES to it instead of failing the whole aggregate load;
- the **GraphQL SDL enum** (`api.rs#enums_block`, which walks only `enum:`) EXCLUDES — unspellable at
  any write door, by construction;
- the **server-side async-graphql mirror enum** (`emit/server_graphql.rs::emit_server_scalars`) also
  excludes, and the domain→wire conversion becomes a plain function `<scalar>_from_domain(v) ->
  Option<Mirror>` (never a blind `From`, and never `impl From<Foreign> for Option<Foreign>` — that
  violates the orphan rule) so the catch-all renders `null` on the wire, never a panic or a
  fabricated real value — the field carrying it MUST be nullable;
- a **hand-written `EnumText` impl** (never the `enum_text!` macro, which has no tolerant arm) makes
  `from_text` fold an unknown stored string into the catch-all rather than erroring the SQL read.

The raw stored text always stays in the immutable `domain_events.payload` — the catch-all is a
decode-time convenience, never a data-loss. First use: `scalars.yaml#/RiderRestrictionGround`'s
`UNRECOGNISED`.

## `screen-sheet-binding-unknown` — the §25 binding walk extended to a screen's opened sheets (#639 part C step 4-iii-A, ADR-20260904-152807 §6)

§25's `{{ root.path }}` binding walk (`screen-binding-unknown-field`, `tools/codegen-rs/src/validate/screen_bindings.rs`) originally checked only a screen's OWN component subtree — a sheet a screen opens (`open_bottom_sheet`) was invisible to it, so `{{ rider.riderld }}` inside a bottom sheet passed `make validate` at 0 errors and would have dispatched a mutation with an empty id at runtime. `check_screen_bindings` now takes the whole screens FILE `doc` and, after checking the screen body, walks every sheet id `screen_roles::reachable_sheets(doc, screen)` returns (the SAME transitive reachability §26's own role walk already derives — one derivation, not two) against the SAME root map the screen's own `data_requirements` computed: a sheet opened from a detail route reads that route's resolver root (`restrict_rider_sheet` reads `rider.*`, exactly like `rider_detail`, which opens it). A finding inside a sheet reports as `screen-sheet-binding-unknown` — a DISTINCT rule code from the screen-body `screen-binding-unknown-field` (same underlying walk, different location, so a screen-level typo and a sheet-level typo are never conflated in a triage list). Wiring this full-strength on the real corpus found and required fixing two genuinely dead bindings already committed in `restaurant_frontoffice.yaml`'s `rating_sheet` (`order.tipRecipient`, `order.currency` — neither is a declared `Order` property; `recipient` for that widget is a fixed `RIDER` literal and the currency lives at `order.totalAmount.currency`), the SAME "corpus first, wiring last, every commit green" discipline §25's own history states.

## `decisionRow:` — binding a configuration key to its release-gate decision row (#639 part C step 4-iii-A, ADR-20260904-152807 §7)

`decisionRow: <KEY>` is an optional attribute on a `configuration.yaml` key, naming the
`docs/decisions/<KEY>.yaml` row that key's release is gated on. The codegen rule
`decision-row-open-key-must-be-off` (`tools/codegen-rs/src/validate/decisions.rs`,
`validate_decision_row_gated_config_keys`) reads it: while the named row's `status` is `open`, the
key's `deploy.production` value must be EXACTLY `"false"`; a row naming no declared key is itself a
finding; once the row closes (any status other than `open`), the rule is silent — flipping the
production value to anything else IS the recorded decision that closes the row (gate-then-stabilize,
ADR-20260808-144738). This exists because a key and its release preconditions can drift silently
otherwise: `RUN_SIRENE_WORKER`'s own prose said the worker was STOPPED while its `deploy.production`
said `"true"`, unreconciled, discovered only by a human reading both at once
(`PUBLISH-PRECONDITIONS`). First use: `configuration.yaml#/keys/RUN_RIDER_RESTRICTION_DOOR` bound to
[`RIDER-RESTRICTION-PRECONDITIONS`](../decisions/RIDER-RESTRICTION-PRECONDITIONS.yaml).

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
  302s a cookie-less GET there and the client navigates there on a 401 from its role path. **4-ii**
  (#639 part C, ADR-20260904-124600 §2) adds the `unauthenticated:` TWIN, keyed on standing instead
  of session: `restricted: { type: navigate, route }` names a route of the same file that declares
  `while_restricted: true` and carries no `restricted:` of its own (validator
  `screen-restricted-route-unknown`); the client navigates there on a refused read OR a refused Tell
  carrying `extensions.reason == RIDER_RESTRICTED` (`crates/web/src/bounce.rs`'s ONE pure
  `bounce_after` function — no server-side document-GET leg exists yet, ADR §3). A screen that
  declares `while_restricted: true` (or is named as another screen's `restricted:` target) may bind
  ONLY operations carrying `whileRestricted:` for its own role — never `rider_topbar` (its online
  toggle is never carved) — validator `screen-restricted-binds-uncarved-op`. An `inline_error` with
  `for_action: <action>` is where that action's REJECTED verdict renders, in the caller's language
  (the server localizes `Operation.message` from the row's typed context). Bindings support `|
  filter` suffixes on `{{ path | filter }}` (`crates/web/src/renderer.rs::binding_text`):
  `format_currency` (Money objects), `format_datetime` (a UTC instant string → Europe/Paris,
  `fr` — "4 sept. 2026, 14:02"; the event/read model keep the UTC instant, this is presentation-
  only) and `format_address` (an `Address` object → one display line, "12 rue de la Paix, 37000
  Tours" — `line2` only when present, `country` never shown, V0 is Tours-only): an object bound
  with NO filter falls through to `format_currency`'s Money-shaped read and silently renders "" —
  the 4-ii round-2 defect this third filter closes (#882).
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

