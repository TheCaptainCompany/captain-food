# Captain.Food

Local-first food ordering and delivery platform for independent restaurants and food trucks.
V0 target: validate product–market fit in **Tours**, with a mobile-first web UX and a backend
that can evolve towards CQRS + event log.

## Specifications — read before any task

The [specs/](specs/) folder is the **source of truth** for the domain and architecture.
Read the relevant file before implementing or changing anything:

- [specs/PRODUCT_SPEC_WEB_CLIENT.md](specs/PRODUCT_SPEC_WEB_CLIENT.md) — web client product spec (user flows, checkout, Stripe payment, order tracking, NFRs, tech constraints).
- [specs/database/](specs/database/) — the store schema as DSL (ADR-0037/0039/0040): `tables/*.yaml` (real tables, globbed — `tables/eventstore.yaml` = `domain_events` + `domain_stream`; `tables/referential.yaml` = seed/config tables configured once by a repo seed script, not projected; `tables/projection_tables.yaml` = MATERIALIZED read-model tables whose columns are computed, each `projector: app` — an application-layer (Rust) projector over `domain_events` (deferred until `crates/`); `tables/integration_staging.yaml` = ADAPTER-OWNED raw staging (`staging: true` — SIRENE mirror + the verbatim `external_stripe_events`/`external_hubrise_callbacks` webhook mirrors, ADR-0045/ADR-20260720-015400); `tables/journals.yaml` = the WRITE-PATH JOURNALS (ADR-20260720-015300/-015400): `command_journal` (one row per command submission, persisted BEFORE handling — idempotency + operationStatus source, records rejections) and `inbound_events` (adapted inbound BUSINESS events drained through the normal write path) — journals never write `domain_events` and never replay as state; no SQL triggers, so projection/business logic stays out of the DB (ADR-0040)), `projection_views.yaml` (the event-fed `View_*` read models — SQL VIEWS **generated** as a per-column state-fold over `domain_events` from each column's `from` lineage, ADR-0039). **Naming: `View_*` = a SQL VIEW; an unprefixed name = a TABLE.** `functions/*.sql` (event-store functions). Generated to `specs/generated/schema.generated.sql` (tables) + `views.generated.sql` (fold views) (+ one `ref_<enum>` lookup table per scalar enum). [specs/database.md](specs/database.md) is the narrative rationale; query→read-model mapping is the `@reads` binding in [specs/api.yaml](specs/api.yaml).
- [specs/scalars.yaml](specs/scalars.yaml) — domain scalar types (IDs, names, money, enums: `OrderStatus`, `RestaurantStatus`, `ServiceType`, `StockStatus`, etc.).
- [specs/entities.yaml](specs/entities.yaml) — value objects and aggregates. HubRise-aligned catalog: `Restaurant`, `Catalog`, `CatalogCategory` (tree), `Product` → `Offer[]` (SKUs), `OptionList`/`Option`, `Cart`/`CartLineItem`, `Order`, `OrderLineItem`. Value objects `Money`, `Stock`, `TaxRate`, `Address`.
- [specs/events.yaml](specs/events.yaml) — **business event** payloads (RestaurantRegistered, ProductAdded, CatalogImported, OrderPlaced...). `*Updated` events carry the full entity (replace semantics).
- [specs/commands.yaml](specs/commands.yaml) — **command payload** catalog (CQRS write side): each command is just its input schema (description + type + properties + required), parallel to events.yaml. Emits/handler → actors.yaml; errors → errors.yaml; persona/use-case/slice → stories.yaml.
- [specs/errors.yaml](specs/errors.yaml) — **anticipated errors** (the old command invariants): each with typed `context` and default `messages.en`/`messages.fr`. Mapped per command in actors.yaml `throws`.
- [specs/actors.yaml](specs/actors.yaml) — **actor-model catalog** (codegen source): aggregates & process managers, each with its inbox of `{ message → emits, throws }`, where every message/event/error is a `$ref` into commands.yaml/events.yaml/errors.yaml (checkable; the ref path encodes kind). Personas/authz live elsewhere (GraphQL `@auth`, story map).
- [specs/stories.yaml](specs/stories.yaml) — the **executable story map** (codegen source): personas → activities → steps, each step a `$ref` into an api.yaml query/mutation. The validator enforces completeness BOTH ways: steps resolve + persona role authorized, AND every mutation/query is reached by ≥1 step (`op-uncovered-by-story`).
- [specs/rules.yaml](specs/rules.yaml) — **business rules / invariants** (ADR-0032): each a readable guarantee. Every behaviour test links to ≥1 rule and every rule is asserted by ≥1 test (bidirectional, validator-enforced). Rules say WHAT we guarantee; [specs/tests.yaml](specs/tests.yaml) says HOW (Given/When/Then). A rule may span several tests.
- [specs/screens/](specs/screens/) — **Spec-Driven SDUI** apps (ADR-0033/0037, taxonomy refined by ADR-20260722-091500), **one file per audience, named without a `_screens` suffix** (the folder conveys it). Two are customer-facing **front offices** split by host: `captain_frontoffice.yaml` = the **marketplace** (cross-restaurant discovery: `home`/`search` + partner marketing) at `live.captain.food` → bare `captain.food` later; `restaurant_frontoffice.yaml` = a **single restaurant's storefront** (catalog → cart → checkout → tracking, plus the customer account/order screens) at `{slug}.captain.food` (renamed from `customer_screens.yaml`; roles PUBLIC+CUSTOMER). The marketplace was split out of the storefront by ADR-20260722-160000 (#75). Then `restaurant_backoffice.yaml` (staff), `rider.yaml`, `system.yaml` to follow. Each: screens + a **`resolvers`** allowlist (reads → `api.yaml` queries by `$ref`) + an **`actions`** allowlist (writes → `api.yaml` mutations by `$ref`); the SDUI **component registry** is a renderer-level shared allowlist declared once (in `restaurant_frontoffice.yaml`, the source for `crates/web` `registry.rs`). Each screen declares `roles` (⊆ UserType — so PUBLIC+CUSTOMER share the front office) and the file declares `app_types` (web/ios/android/windows); a `system`/admin screen set may follow (refining ADR-0037's impersonation-only stance). The validator (generic over `screens/*.yaml`) proves the **API answers the UI**; UI needs the API lacks are explicit `gaps`; `sdui: false` marks non-SDUI screens. Runtime (renderer/registry/Supabase) is a deferred contract.
- [specs/translations.yaml](specs/translations.yaml) — **SHARED UI i18n catalog** (ADR-0033; sidecars per ADR-20260722-101500), errors.yaml-style: dotted keys + typed `params` + `messages.en`/`fr`. Holds strings shared across surfaces (`common.*`) + future backend/server-rendered text; **surface-specific** strings live in co-located sidecars `specs/screens/<surface>.translations.yaml` (e.g. `restaurant_frontoffice.translations.yaml`). Screens `$ref` the file holding the key (keys globally unique across files). The codegen **merges** `translations.yaml` + every `screens/*.translations.yaml` into one `translations.generated.json`.
- [specs/api.yaml](specs/api.yaml) — the **GraphQL API surface** (source of truth): output-type registry, queries, mutations, and the ACL (`roles` → `@auth`/`@public`). The SDL is GENERATED from it to `specs/generated/schema.generated.graphql` (the hand-written `schema.graphql` has been removed). **Role = path**: one master schema served per-role under `/{role}/graphql`, filtered by the `@auth`/`@public` ACL (roles: PUBLIC, CUSTOMER, RESTAURANT_ACCOUNT, RESTAURANT, RIDER, ADMIN, EXTERNAL).
- [specs/integrations/hubrise.md](specs/integrations/hubrise.md) — HubRise integration: exposed data, mapping → domain, ACL, gaps, import path.

For a single **navigable, fully detailed view of the whole product** (stories → api → actors → views →
commands → events → entities → scalars → errors, each with its description and cross-links), run the
generator and read [specs/generated/documentation.generated.md](specs/generated/documentation.generated.md)
— it is GENERATED from the specs above (do not hand-edit), so it never drifts from the source of truth.

## CQRS methodology — commands

Commands are **derived from use cases** (business intentions from the story map), **not** from events.
Do NOT mechanically generate one command per event: a command may emit **several events**
(e.g. `PlaceOrder` → `OrderPlaced` + payment), and not all commands have a 1:1 counterpart.
See [ADR-0004](docs/adr/0004-commands-derived-from-use-cases.md) and [specs/stories.yaml](specs/stories.yaml) for the use case → command derivation.

### Commands vs inbound (integration) events

**Not every event originates from a command.** A **command** is a *request* to change state that the
system can **reject** — it validates invariants first. But an external system or actor sometimes just
**informs** us that something already happened on their side: there is nothing to validate and nothing
to reject. These are **inbound (integration) events** — recorded as facts directly (through the
Anti-Corruption Layer, idempotently keyed where possible), **without a command**.

Rule of thumb: if the originator can be told *"no"* → **command**. If they are stating a fact that has
**already occurred** → **inbound event** (no command).

Captain.Food inbound events:
- **Stripe** webhooks: `PaymentCaptured`, `PaymentFailed`, `PaymentRefunded` — Stripe reports the outcome; we record it.
- **HubRise**: inventory sync (`OfferStockUpdated`) and externally-channeled order updates.
- **Delivery partner** (e.g. Avelo37): `DeliveryStatusUpdated`, `DeliveryAcceptedByPartner` (post-V0).

Note the request/report split: a refund is **requested** by a command (`RejectOrder`, `CancelOrder*`),
but the `PaymentRefunded` **fact** is **reported** by Stripe (inbound). Contrast `ImportCatalog`, which
stays a **command** even though the data comes from HubRise — we orchestrate it and can reject it via ACL
validation. In the story map, inbound events are marked 📥.

## Architecture (summary)

- **Full-stack Rust** (ADR-0034 — supersedes the earlier Next.js/Node stack). **Cargo workspace** in Clean-Architecture layers (ADR-0035): `crates/domain` (pure DDD — aggregates/commands/events/policies/value-objects + the `Aggregate` trait (event-sourced-actor identity + `fold`); may derive `serde` on events/VOs but no serialization *logic*), `crates/application` (use cases — `ports/` traits, command/query handlers, `process_managers/` sagas, and a write-side `Repository` over the event-store journal so handlers/runner never touch the raw `EventStore` — ADR-20260719-031136), `crates/infrastructure` (adapters — event store, read-model repos over `View_*`, `integrations/` ACL for HubRise/Stripe/delivery), `crates/server` (Axum BFF — GraphQL, SDUI, `middleware/tenant`), `crates/shared_types` (serde + UniFFI), `crates/core` (Crux app-shell over `domain`), `crates/web` (Leptos → WASM SDUI renderer), `crates/desktop` (Tauri 2.0). Mobile = thin SwiftUI/Compose shells over the core via UniFFI. Dependency rule: outer→inner only; `domain` imports nothing else.
- **Frontend**: **Leptos** (Rust→WASM), SSR+hydration; the SDUI screens (ADR-0033) render via a generated Leptos component registry. All backend calls go through **GraphQL**.
- **Backend**: **Rust** — Axum + Tokio + SQLx (compile-time-checked) + async-graphql, **CQRS-light + event log**.
  - Mutations (commands) validate invariants then write events into the append-only `domain_events` table.
  - Queries read the dedicated **`View_*` read models** — **never** raw `domain_events`. In **V0** these are Postgres **SQL views defined over `domain_events`** (projection-on-read, ADR-0035); a hot view can later become a materialized table fed by a projector with no query-API change (refines ADR-0005).
  - No full event sourcing (no snapshots/replay) in V0.
- **Database**: managed PostgreSQL (e.g. Supabase).
- **Multi-tenant**: restaurant resolution via the `Host` header; pattern `{restaurantSlug}.captain.food` (wildcard `*.captain.food`).
- **Integrations**: Stripe (payments, later Stripe Connect), HubRise (existing restaurant systems), delivery partner (e.g. Avelo37), Supabase Auth (passwordless phone-OTP + email magic-link identity — **wrapped** behind our GraphQL, see [specs/integrations/supabase.md](specs/integrations/supabase.md) / ADR-0015).

## Important conventions

- **Language**: all repository content — docs, code, comments, commit messages, identifiers — is written in **English**. No French.
- **Event payloads** = business only. **Never** mix in the technical envelope (`eventId`, `aggregateType`, `aggregateId`, `occurredAt`, the **acting user** `user_id`/`user_type`, `metadata`) — it is added by infrastructure. In particular the actor/user who performed an event (`createdBy`/`updatedBy`/`changedBy`/…) is **envelope metadata** recorded on `domain_events.user_id` (ADR-0041), not a payload field — just like `occurredAt`. (A business ROLE that changes semantics — e.g. `Tipper` = CUSTOMER|RESTAURANT — is business data and stays.)
- Types are **strongly typed** and reference scalars/entities via `$ref`; no ambiguous type reuse (one name = one dedicated scalar).
- **Money**: value object `Money` = `{ amountCents, currency }`. Keep this strong typing internally; convert to/from the HubRise string format (`"9.80 EUR"`) **only at the integration boundary**.
- **Availability ≠ stock** (two orthogonal concepts): `CatalogItemAvailability` (`AVAILABLE`/`UNAVAILABLE`, manual UI flag) vs derived `StockStatus` (`IN_STOCK`/`LOW_STOCK`/`OUT_OF_STOCK`). Orderable = `AVAILABLE` **and** stock > 0.
- **HubRise interop**: the `ref` field (scalar `ExternalReference`) is the idempotent import key. HubRise→domain translation goes through an Anti-Corruption Layer; do not let `SKU`/`option_list`/`"9.80 EUR"` leak into the domain.
- Slugs: lowercase, dash-separated (`^[a-z0-9]+(?:-[a-z0-9]+)*$`).
- **Always name issues/PRs, never bare numbers**: whenever referring to a GitHub issue or PR in any
  user-facing message, commit, or doc, include its **title** alongside the number — e.g.
  `#21 "Frontend: Leptos/WASM SDUI renderer"`, not just `#21`. A bare number is not memorable to a human
  reader; the title carries the meaning.
- **Makefile recipe lines are ASCII-only** — use `--`, `->`, `|` rather than `—`, `→`, `·`. Native
  Windows GNU Make hands a recipe to Cygwin's `sh` with broken quoting as soon as the line contains a
  byte > 127: `sh` receives the whole recipe as ONE word and reports `$'...': command not found`, so
  the target fails for a reason that has nothing to do with what it does. This bit `check-drift` (an
  em dash in its message made `make rust` fail with **zero** drift). Comments, variables and
  `$(shell ...)` are unaffected — only the tab-indented recipe text. Enforced by the
  `makefile_recipe_lines_are_ascii` codegen test, so it cannot silently come back.

## Operating model (read [docs/PLAYBOOK.md](docs/PLAYBOOK.md))

The project runs on a strict operating model: the **YAML DSL is the source of truth**, everything else
is **generated/derived**, **planning is separate from execution**, and **observability is a contract**.
Topic rules live in [docs/claude/](docs/claude/) — read the relevant one before working:
[dsl.md](docs/claude/dsl.md) · [codegen.md](docs/claude/codegen.md) ·
[observability.md](docs/claude/observability.md) · [c4.md](docs/claude/c4.md) ·
[adr.md](docs/claude/adr.md) · [loops.md](docs/claude/loops.md) · [mermaid.md](docs/claude/mermaid.md). Decisions are recorded in
[docs/adr/](docs/adr/).

Generator/reviewer/observability agents are defined in `.claude/agents/`; acceptance gates are wired as
hooks in `.claude/settings.json` (`.claude/hooks/stop-gate.sh`, `validate-generated.sh`). `make help`
lists entrypoints. The validator (`make validate`, the Rust `tools/codegen-rs`) is the single executable gate for
the **whole spec** — schema/refs, actor wiring, api↔model, views, C4, observability, and (ADR-0032)
**tests, stories and rules completeness**: every message/event/error is exercised by a test, every
mutation/query is reached by a story step, and every test↔rule link holds both ways. It must be
**0 errors** (only the known view design-holes warn).

### Non-negotiable rules

- DSL source files (`specs/**`) are **never** modified by autonomous/execution loops — only plan mode
  proposes DSL changes, with approval. C4 (`specs/architecture/*.yaml`) and observability contracts
  (`specs/observability.yaml`) are **source** DSL, not generated.
- Business code (aggregates / pure command handlers) stays **independent of the telemetry SDK**;
  instrumentation lives only in framework/middleware boundaries (see `c4-l3.yaml` `instrumented` flags).
- Every critical workflow must have an observability contract in `specs/observability.yaml`.
- If a **behaviour test** fails, fix the generator/runtime — not the test. If an **observability test**
  fails, fix instrumentation/middleware — not the domain model.
- **Completeness is part of every change (ADR-0032):** a new command/event/error also needs a behaviour
  test (+ its `rules:` link); a new mutation/query also needs a story step; a new business rule also needs
  a test. `make validate` blocks otherwise — do not weaken the gate, extend the specs.
- Review and validation gates are executable and **blocking**; never hand-edit generated output
  (`specs/generated/**`, the `database.md` GENERATED region) — change the spec/emitter and regenerate.
- Every recurring agent/loop failure becomes a new rule, test, or ADR.
- Keep **`docs/STATUS.md`** current with every substantive change, and land cross-cutting **decisions as
  ADRs in the same change** — so concurrent sessions never diverge on state or intent. ADR ids are
  **date-time** (`ADR-YYYYMMDD-HHMMSS`) to avoid collisions (ADR-20260718-135417); legacy `0001`–`0047`
  keep their sequential ids.
- **Respect the prioritised backlog**: priorities are defined **in the GitHub Project
  "Prioritized backlog"** (Priority field + row order) — pick work from the top; skipping the top item
  needs a stated reason. Re-prioritising is a **product-owner decision made in the project**, never by
  an agent. [docs/BACKLOG.md](docs/BACKLOG.md) records the process and how value is defined
  (value-first, ADR-20260720-213024): foundations/cross-functional/non-functional first, then features
  in value-stream order.
- **Spec- and docs-only changes go straight to `main`** (product-owner directive): commit and **push
  directly to `main`** — no branch, no PR, no claim ceremony — for changes confined to `specs/**`,
  `docs/**`, ADRs, `CLAUDE.md`, `STATUS.md`, and the generated artifacts they regenerate. **Keep `main`
  green**: run the same gate CI would (`make rust`) locally **before** pushing anything that touches
  `specs/**` (a docs-only edit that regenerates nothing may skip it). The claim → draft-PR →
  supervised-merge flow below applies to **code/feature work** (touching `crates/**`, `tools/**`, CI,
  deploy), not to pure spec/doc edits.
- **Issue workflow — claim ⇒ draft PR immediately; finish ⇒ supervised auto-merge**
  (ADR-20260720-233000 + ADR-20260721-042018 + ADR-20260721-044613, method in
  [docs/BACKLOG.md](docs/BACKLOG.md)): when asked to work an issue, FIRST claim it
  (`status/in-progress` label + claim comment naming the `NN-slug` branch), create the `NN-slug`
  branch from `main`, and open a **draft PR** whose body starts with `Closes #NN` — branch, PR and
  issue are linked before any code is written. **Never enable auto-merge at this point** — a
  claim-time PR is a near-empty diff and would pass CI trivially. When the work is done and local
  gates are green (`make rust`), mark the PR **ready for review** and **enable auto-merge**
  **together, as one indivisible step**, and **supervise the checks until the PR is MERGED** (fix +
  push on failure; never end at "pushed, CI pending"). The merge closes the issue and ends the claim.
- Autonomous loops/routines run under the **weekly time budget** (`make budgeted-loop` or the routine
  guard) — Claude Code has no native cap; see [docs/claude/loops.md](docs/claude/loops.md) / ADR-0014.

## Project status

The repo currently contains the **specs** ([specs/](specs/)), the **codegen** generator/validator
([tools/codegen/](tools/codegen/)), and the **operating-model scaffold** ([docs/](docs/),
`.claude/agents`, `.claude/hooks`, `Makefile`). The Rust workspace (`crates/`) does **not** exist yet
(ADR-0034), so the runtime layers of the playbook — the Crux core, Leptos/Axum apps, OpenTelemetry
emission, Kubernetes probes, BAM projections, GraphQL operation observability — are specified as
**contracts + ADRs** and deferred until then. The codegen is a **Rust** tool
([tools/codegen-rs/](tools/codegen-rs/), bin `generate`, ADR-0034): it runs the full validator (§1–§11)
and every emitter (translations, views SQL + the `database.md` §2 injection, C4 Structurizr/Mermaid,
GraphQL SDL, and the Markdown + HTML docs). It began as a TypeScript tool (`tools/codegen`), was ported
to Rust at parity (all 8 artifacts byte-identical + the same validation issue set), and the TypeScript
codegen was then **retired**. Run it with `make validate` / `make generate` (needs a local Rust
toolchain; `make rust` = build + test + validate + generate). CI's single `codegen` gate does the same and
fails on any spec↔generation drift.
