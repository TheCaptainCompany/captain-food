# Captain.Food

Local-first food ordering and delivery platform for independent restaurants and food trucks.
V0 target: validate product–market fit in **Tours**, with a mobile-first web UX and a backend
that can evolve towards CQRS + event log.

## Specifications — read before any task

The [specs/](specs/) folder is the **source of truth** for the domain and architecture.
Read the relevant file before implementing or changing anything:

- [specs/ARCHITECTURE_OVERVIEW.md](specs/ARCHITECTURE_OVERVIEW.md) — big picture: V0 goals, domains/subdomains, monorepo structure, CQRS-light + event log backend, integrations, hosting.
- [specs/PRODUCT_SPEC_WEB_CLIENT.md](specs/PRODUCT_SPEC_WEB_CLIENT.md) — web client product spec (user flows, checkout, Stripe payment, order tracking, NFRs, tech constraints).
- [specs/database.md](specs/database.md) — event store schema (`domain_events`) + the `View_*` projection (read) tables, each declared by its source aggregate/events, business filters and columns (the query/UI mapping lives in [specs/traceability.md](specs/traceability.md) §2).
- [specs/scalars.yaml](specs/scalars.yaml) — domain scalar types (IDs, names, money, enums: `OrderStatus`, `RestaurantStatus`, `ServiceType`, `StockStatus`, etc.).
- [specs/entities.yaml](specs/entities.yaml) — value objects and aggregates. HubRise-aligned catalog: `Restaurant`, `Catalog`, `CatalogCategory` (tree), `Product` → `Offer[]` (SKUs), `OptionList`/`Option`, `Cart`/`CartLineItem`, `Order`, `OrderLineItem`. Value objects `Money`, `Stock`, `TaxRate`, `Address`.
- [specs/events.yaml](specs/events.yaml) — **business event** payloads (RestaurantRegistered, ProductAdded, CatalogImported, OrderPlaced...). `*Updated` events carry the full entity (replace semantics).
- [specs/commands.yaml](specs/commands.yaml) — **command payload** catalog (CQRS write side): each command is just its input schema (description + type + properties + required), parallel to events.yaml. Emits/handler → actors.yaml; errors → errors.yaml; persona/use-case/slice → story-map.
- [specs/errors.yaml](specs/errors.yaml) — **anticipated errors** (the old command invariants): each with typed `context` and default `messages.en`/`messages.fr`. Mapped per command in actors.yaml `throws`.
- [specs/actors.yaml](specs/actors.yaml) — **actor-model catalog** (codegen source): aggregates & process managers, each with its inbox of `{ message → emits, throws }`, where every message/event/error is a `$ref` into commands.yaml/events.yaml/errors.yaml (checkable; the ref path encodes kind). Personas/authz live elsewhere (GraphQL `@auth`, story map).
- [specs/story-map.md](specs/story-map.md) — Jeff Patton story map: backbone, actor×story×steps table, V0 walking skeleton, use cases → commands, open gaps.
- [specs/api.yaml](specs/api.yaml) — the **GraphQL API surface** (source of truth): output-type registry, queries, mutations, and the ACL (`roles` → `@auth`/`@public`). The SDL is GENERATED from it to `specs/generated/schema.generated.graphql` (the hand-written `schema.graphql` has been removed). **Role = path**: one master schema served per-role under `/{role}/graphql`, filtered by the `@auth`/`@public` ACL (roles: PUBLIC, CUSTOMER, RESTAURANT_ACCOUNT, RESTAURANT, RIDER, ADMIN, EXTERNAL).
- [specs/traceability.md](specs/traceability.md) — completeness matrix: persona→mutation→actor, persona→query→`View_*`, external→process-manager→actor, + coverage checklist. Derived from the other specs.
- [specs/integrations/hubrise.md](specs/integrations/hubrise.md) — HubRise integration: exposed data, mapping → domain, ACL, gaps, import path.

For a single **navigable, fully detailed view of the whole product** (stories → api → actors → views →
commands → events → entities → scalars → errors, each with its description and cross-links), run the
generator and read [specs/generated/documentation.generated.md](specs/generated/documentation.generated.md)
— it is GENERATED from the specs above (do not hand-edit), so it never drifts from the source of truth.

## CQRS methodology — commands

Commands are **derived from use cases** (business intentions from the story map), **not** from events.
Do NOT mechanically generate one command per event: a command may emit **several events**
(e.g. `PlaceOrder` → `OrderPlaced` + payment), and not all commands have a 1:1 counterpart.
See [specs/story-map.md](specs/story-map.md) for the use case → command derivation.

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

- **Monorepo** (Turborepo/Nx): `apps/web-client` (Next.js, customer), `apps/web-admin` (Next.js, back-office), `apps/api` (Node + GraphQL); `packages/ui`, `packages/types`, `packages/config`.
- **Frontend**: Next.js App Router, React, TypeScript, Tailwind CSS. All backend calls go through **GraphQL**.
- **Backend**: Node/TypeScript (Hono or NestJS), **CQRS-light + event log**.
  - Mutations (commands) validate invariants then write events into the append-only `domain_events` table.
  - Queries read from dedicated **read tables** (`read_orders_by_restaurant`, `read_restaurants_public`...), fed by projections — **never** directly from `domain_events`.
  - No full event sourcing (no snapshots/replay) in V0.
- **Database**: managed PostgreSQL (e.g. Supabase).
- **Multi-tenant**: restaurant resolution via the `Host` header; pattern `{restaurantSlug}.captain.food` (wildcard `*.captain.food`).
- **Integrations**: Stripe (payments, later Stripe Connect), HubRise (existing restaurant systems), delivery partner (e.g. Avelo37), Supabase Auth (passwordless phone-OTP + email magic-link identity — **wrapped** behind our GraphQL, see [specs/integrations/supabase.md](specs/integrations/supabase.md) / ADR-0015).

## Important conventions

- **Language**: all repository content — docs, code, comments, commit messages, identifiers — is written in **English**. No French.
- **Event payloads** = business only. **Never** mix in the technical envelope (`eventId`, `aggregateType`, `aggregateId`, `occurredAt`, `metadata`) — it is added by infrastructure.
- Types are **strongly typed** and reference scalars/entities via `$ref`; no ambiguous type reuse (one name = one dedicated scalar).
- **Money**: value object `Money` = `{ amountCents, currency }`. Keep this strong typing internally; convert to/from the HubRise string format (`"9.80 EUR"`) **only at the integration boundary**.
- **Availability ≠ stock** (two orthogonal concepts): `CatalogItemAvailability` (`AVAILABLE`/`UNAVAILABLE`, manual UI flag) vs derived `StockStatus` (`IN_STOCK`/`LOW_STOCK`/`OUT_OF_STOCK`). Orderable = `AVAILABLE` **and** stock > 0.
- **HubRise interop**: the `ref` field (scalar `ExternalReference`) is the idempotent import key. HubRise→domain translation goes through an Anti-Corruption Layer; do not let `SKU`/`option_list`/`"9.80 EUR"` leak into the domain.
- Slugs: lowercase, dash-separated (`^[a-z0-9]+(?:-[a-z0-9]+)*$`).

## Operating model (read [docs/PLAYBOOK.md](docs/PLAYBOOK.md))

The project runs on a strict operating model: the **YAML DSL is the source of truth**, everything else
is **generated/derived**, **planning is separate from execution**, and **observability is a contract**.
Topic rules live in [docs/claude/](docs/claude/) — read the relevant one before working:
[dsl.md](docs/claude/dsl.md) · [codegen.md](docs/claude/codegen.md) ·
[observability.md](docs/claude/observability.md) · [c4.md](docs/claude/c4.md) ·
[adr.md](docs/claude/adr.md) · [loops.md](docs/claude/loops.md). Decisions are recorded in
[docs/adr/](docs/adr/).

Generator/reviewer/observability agents are defined in `.claude/agents/`; acceptance gates are wired as
hooks in `.claude/settings.json` (`.claude/hooks/stop-gate.sh`, `validate-generated.sh`). `make help`
lists entrypoints. The validator (`cd tools/codegen && npm run validate`) is the single executable gate —
it covers schema, behaviour-test coverage, observability contracts, and C4 consistency in one run; it
must be **0 errors** (only the known view design-holes warn).

### Non-negotiable rules

- DSL source files (`specs/**`) are **never** modified by autonomous/execution loops — only plan mode
  proposes DSL changes, with approval. C4 (`specs/architecture/*.yaml`) and observability contracts
  (`specs/observability.yaml`) are **source** DSL, not generated.
- Business code (aggregates / pure command handlers) stays **independent of the telemetry SDK**;
  instrumentation lives only in framework/middleware boundaries (see `c4-l3.yaml` `instrumented` flags).
- Every critical workflow must have an observability contract in `specs/observability.yaml`.
- If a **behaviour test** fails, fix the generator/runtime — not the test. If an **observability test**
  fails, fix instrumentation/middleware — not the domain model.
- Review and validation gates are executable and **blocking**; never hand-edit generated output
  (`specs/generated/**`, the `database.md` GENERATED region) — change the spec/emitter and regenerate.
- Every recurring agent/loop failure becomes a new rule, test, or ADR.
- Autonomous loops/routines run under the **weekly time budget** (`make budgeted-loop` or the routine
  guard) — Claude Code has no native cap; see [docs/claude/loops.md](docs/claude/loops.md) / ADR-0014.

## Project status

The repo currently contains the **specs** ([specs/](specs/)), the **codegen** generator/validator
([tools/codegen/](tools/codegen/)), and the **operating-model scaffold** ([docs/](docs/),
`.claude/agents`, `.claude/hooks`, `Makefile`). Application code (apps/, packages/) does **not** exist
yet, so the runtime layers of the playbook — OpenTelemetry emission, Kubernetes probes, BAM projections,
GraphQL operation observability — are specified as **contracts + ADRs** and deferred until then.
