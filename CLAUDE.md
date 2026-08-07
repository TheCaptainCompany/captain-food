# Captain.Food

Local-first food ordering and delivery platform for independent restaurants and food trucks.
V0 target: validate product–market fit in **Tours**, with a mobile-first web UX and a backend
that can evolve towards CQRS + event log.

## Domain lens — this is a food-delivery product, not a generic CRUD app

Apply this judgement to every task, whatever its size. It is what a senior food-and-delivery architect
would bring, and it is not derivable from the code:

- **The ETA is the product.** The estimate a customer sees before ordering is the number they decide
  on. Anything that degrades or omits it is a conversion problem, not a polish item.
- **A paid order that nobody is told about is the worst failure mode there is** — worse than a crash,
  because the money moved. Anything touching order placement must answer: who gets told, and what
  happens if nobody acts?
- **Oversell and un-accepted orders lose both sides of the marketplace at once** — the customer who
  paid and the restaurant that looks incompetent.
- **Peak is Friday/Saturday 19:00–21:30.** "Does this hold at peak?" is a fair question of any change
  to checkout, dispatch, projections or hosting.
- **Some things are legal preconditions in France, not backlog items**: allergen declaration for
  distance selling (EU FIC 1169/2011), VAT computation and a compliant receipt, GDPR erasure, and
  who holds customer funds (payment-agent posture). Flag these rather than deferring them silently.
- **Availability, stock and orderability are three different things** — see the conventions below.
- **A control that renders but does nothing is worse than no control.** Screens here legitimately
  declare `gaps`; shipping a live widget bound to one is not the same thing.

For a full critical audit — findings, triaged issues, proposals, and what to do next — use the
**`architect` agent** (`.claude/agents/architect.md`), which carries this lens plus the review
procedure in `.claude/skills/architecture-review/`. The open-decision queue it feeds is
[docs/proposals/DECISIONS.md](docs/proposals/DECISIONS.md).

## Specifications — read before any task

The [specs/](specs/) folder is the **source of truth** for the domain and architecture.
Read the relevant file before implementing or changing anything:

- [specs/PRODUCT_SPEC_WEB_CLIENT.md](specs/PRODUCT_SPEC_WEB_CLIENT.md) — web client product spec (user flows, checkout, Stripe payment, order tracking, NFRs, tech constraints).
- [specs/database/](specs/database/) — the store schema as DSL: real tables, generated `View_*`
  fold views, event-store functions, journals (the `inbound_messages` mailbox). `View_*` = a SQL
  VIEW; unprefixed = a TABLE. Full taxonomy + generation detail: [docs/claude/dsl.md](docs/claude/dsl.md);
  narrative: [specs/database.md](specs/database.md).
- [specs/scalars.yaml](specs/scalars.yaml) — domain scalar types (IDs, names, money, enums: `OrderStatus`, `RestaurantStatus`, `ServiceType`, `StockStatus`, etc.).
- [specs/entities.yaml](specs/entities.yaml) — value objects and aggregates. HubRise-aligned catalog: `Restaurant`, `Catalog`, `CatalogCategory` (tree), `Product` → `Offer[]` (SKUs), `OptionList`/`Option`, `Cart`/`CartLineItem`, `Order`, `OrderLineItem`. Value objects `Money`, `Stock`, `TaxRate`, `Address`.
- [specs/events.yaml](specs/events.yaml) — **business event** payloads (RestaurantRegistered, ProductAdded, CatalogImported, OrderPlaced...). `*Updated` events carry the full entity (replace semantics).
- [specs/commands.yaml](specs/commands.yaml) — **command payload** catalog (CQRS write side): each command is just its input schema (description + type + properties + required), parallel to events.yaml. Emits/handler → actors.yaml; errors → errors.yaml; persona/use-case/slice → stories.yaml.
- [specs/errors.yaml](specs/errors.yaml) — **anticipated errors** (the old command invariants): each with typed `context` and default `messages.en`/`messages.fr`. Mapped per command in actors.yaml `throws`.
- [specs/actors.yaml](specs/actors.yaml) — **actor-model catalog** (codegen source): aggregates & process managers, each with its inbox of `{ message → emits, throws }`, where every message/event/error is a `$ref` into commands.yaml/events.yaml/errors.yaml (checkable; the ref path encodes kind). Personas/authz live elsewhere (GraphQL `@auth`, story map).
- [specs/stories.yaml](specs/stories.yaml) — the **executable story map** (codegen source): personas → activities → steps, each step a `$ref` into an api.yaml query/mutation. The validator enforces completeness BOTH ways: steps resolve + persona role authorized, AND every mutation/query is reached by ≥1 step (`op-uncovered-by-story`).
- [specs/rules.yaml](specs/rules.yaml) — **business rules / invariants** (ADR-0032): each a readable guarantee. Every behaviour test links to ≥1 rule and every rule is asserted by ≥1 test (bidirectional, validator-enforced). Rules say WHAT we guarantee; [specs/tests.yaml](specs/tests.yaml) says HOW (Given/When/Then). A rule may span several tests.
- [specs/screens/](specs/screens/) — Spec-Driven SDUI apps, one file per audience
  (marketplace / storefront front offices split by host, backoffice, rider, system); each file =
  screens + `resolvers`/`actions` allowlists into api.yaml, validator-proved, gaps explicit.
  Detail: [docs/claude/dsl.md](docs/claude/dsl.md).
- [specs/translations.yaml](specs/translations.yaml) — shared UI i18n catalog + per-surface
  sidecars, merged into one generated JSON. Detail: [docs/claude/dsl.md](docs/claude/dsl.md).
- [specs/api.yaml](specs/api.yaml) — the GraphQL surface (types, queries, mutations, ACL); SDL
  generated. **Role = path**: one master schema served per-role under `/{role}/graphql`.
- [specs/integrations/hubrise.md](specs/integrations/hubrise.md) — HubRise integration: exposed data, mapping → domain, ACL, gaps, import path.

For a single **navigable, fully detailed view of the whole product** (stories → api → actors → views →
commands → events → entities → scalars → errors, each with its description and cross-links), run the
generator and read [specs/generated/documentation.generated.md](specs/generated/documentation.generated.md)
— it is GENERATED from the specs above (do not hand-edit), so it never drifts from the source of truth.

## CQRS methodology — commands vs inbound events

Commands are **derived from use cases** (ADR-0004), never one-per-event. A command is a request
the system can REJECT; an external fact that already happened is an **inbound (integration)
event**, recorded through the ACL without a command (Stripe payment facts, HubRise sync,
delivery-partner reports — marked 📥 in the story map). Full doctrine + examples:
[docs/claude/dsl.md](docs/claude/dsl.md).

## Architecture (summary)

**Full-stack Rust** (ADR-0034/0035), Cargo workspace in Clean-Architecture layers — `domain`
(pure DDD, no outward imports) · `application` (ports, handlers, PMs, write-side Repository) ·
`infrastructure` (event store, `View_*` repos, integration ACLs, the mailbox) · `server` (Axum
BFF: GraphQL, SDUI, tenant middleware) · `actor_runtime` (generic mailbox: leases, fencing,
head-of-line) · `shared_types`/`core`/`web`/`desktop` (UniFFI/Crux/Leptos/Tauri). Frontend =
Leptos→WASM SDUI over GraphQL. Backend = CQRS-light + event log: mutations enqueue on the actor
mailbox (acceptance-first, PENDING) and workers append to `domain_events`; queries read `View_*`
read models, never the raw log. Managed Postgres; multi-tenant by `Host`
(`{slug}.captain.food`); integrations: Stripe, HubRise, delivery partners, Supabase Auth
(wrapped, identity-only — ADR-20260731-061609 moved hosting to OVH). Dependency rule:
outer→inner only. Current runtime state: [docs/STATUS.md](docs/STATUS.md).

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
  reader; the title carries the meaning. **In repo markdown files (docs/proposals, ADRs, docs/) the
  reference must be a FULL CLICKABLE LINK** (`[#NN "<title>"](https://github.com/TheCaptainCompany/captain-food/issues/NN)`)
  — GitHub does not auto-link bare `#NN` outside issues/PRs/commits.
- **Makefile recipe lines are ASCII-only** — use `--`, `->`, `|` rather than `—`, `→`, `·`. Native
  Windows GNU Make hands a recipe to Cygwin's `sh` with broken quoting as soon as the line contains a
  byte > 127: `sh` receives the whole recipe as ONE word and reports `$'...': command not found`, so
  the target fails for a reason that has nothing to do with what it does. This bit `check-drift` (an
  em dash in its message made `make rust` fail with **zero** drift). Comments, variables and
  `$(shell ...)` are unaffected — only the tab-indented recipe text. Enforced by the
  `makefile_recipe_lines_are_ascii` codegen test, so it cannot silently come back.

## Reading production telemetry (Honeycomb MCP)

Honeycomb **EU (`eu1`)** — a GDPR constraint; `.mcp.json` pins the EU host ON PURPOSE (the
plugin's US default "succeeds" and then shows an empty environment list — do not "fix" it back).
Auth, key kinds, and query discipline (percentiles over averages, correlate by
`correlation_id`): [docs/claude/observability.md](docs/claude/observability.md).

## Operating model (read [docs/PLAYBOOK.md](docs/PLAYBOOK.md))

The project runs on a strict operating model: the **YAML DSL is the source of truth**, everything else
is **generated/derived**, **planning is separate from execution**, and **observability is a contract**.
Topic rules live in [docs/claude/](docs/claude/) — read the relevant one before working:
[dsl.md](docs/claude/dsl.md) · [codegen.md](docs/claude/codegen.md) ·
[observability.md](docs/claude/observability.md) · [c4.md](docs/claude/c4.md) ·
[adr.md](docs/claude/adr.md) · [loops.md](docs/claude/loops.md) · [mermaid.md](docs/claude/mermaid.md) ·
[sessions.md](docs/claude/sessions.md). Decisions are recorded in
[docs/adr/](docs/adr/).

**[sessions.md](docs/claude/sessions.md) is operational, not conceptual** — read it before a long or
exploratory session: which gate is cheap vs expensive, why `df` lies about the disk allowance (and what
deleting `target/debug` costs you afterwards), how to keep GitHub MCP output from dwarfing every file you
read, that PDFs cannot be read in this container at all, and why a third-party integration's API suite and
auth mechanism must be established **before** any credential is named (ADR-20260730-032306 — getting that
order wrong cost two wrong key sets and four mis-named repository secrets).

Generator/reviewer/observability agents are defined in `.claude/agents/`; acceptance gates are wired as
hooks in `.claude/settings.json` (`.claude/hooks/stop-gate.sh`, `validate-generated.sh`). `make help`
lists entrypoints. The validator (`make validate`, the Rust `tools/codegen-rs`) is the single executable gate for
the **whole spec** — schema/refs, actor wiring, api↔model, views, C4, observability, and (ADR-0032)
**tests, stories and rules completeness**: every message/event/error is exercised by a test, every
mutation/query is reached by a story step, and every test↔rule link holds both ways. It must be
**0 errors**. Warnings are a **baseline to compare against, not a clean slate**: `main` carries **43**
(re-measured 2026-08-07 at `0e18f03`; it was 32 before ADR-20260804-154700 added the screen-action
gate, so this number MOVES — measure, never quote) — `command-no-mutation` ×13 and
`event-not-projected` ×11 dominate, then `action-missing-required-input` ×10, `action-unknown-input`
×7, `view-fedby-unused` ×1, `identity-property-not-on-command` ×1. The rule for a change is therefore **0 errors and no NEW
warning**: diff the count and kinds against `main` (`make validate` prints
`checks: N error(s), M warning(s)`), and never read a non-zero count as a regression you caused.
Three independent reviewer passes on [#304 "The Mailbox port surface hole"](https://github.com/TheCaptainCompany/captain-food/issues/304)
each had to stop and re-derive this because the old wording said otherwise — re-measure rather than
trust the numbers above if they look off.

### Non-negotiable rules

- DSL source files (`specs/**`) are **never** modified by autonomous/execution loops — only plan mode
  proposes DSL changes, with approval. C4 (`specs/architecture/*.yaml`) and observability contracts
  (`specs/observability.yaml`) are **source** DSL, not generated.
- **Proposals are committed to the repo** (ADR-20260724-135945, product-owner directive): every
  proposal presented for approval lands in [docs/proposals/](docs/proposals/) as
  `PROP-YYYYMMDD-HHMMSS-<slug>.md` — the proposal as presented, alternatives considered, the
  approver's scope choices, status header linking the realizing PR/ADR. Proposals are LIVING
  documents (ADR-20260801-020000): the file always holds the clean CURRENT state of the design —
  refinements rewrite it in the same change as their ADR; history lives in the file's git log,
  never as appended sections or superseded blocks. A session-local plan file is NOT a substitute — the rationale must survive the session.
  **GitHub is never the record**: issue bodies, PR bodies and PR/issue comments carry LINKS into
  docs/proposals + docs/adr (and a tracking checklist at most) — never the design content itself;
  content drafted in a GitHub surface must land in the repo in the same change.
  **Proportionality** (product-owner directive, 2026-07-31): the record matches the size of the
  decision — a real option space needing arbitration gets a proposal (+ tracking issue); a
  decision without alternatives gets an ADR (inline "options considered" at most); a small
  subject with no real decision needs NEITHER — the commit message and the PR's one-paragraph
  body are enough. Issues and PR bodies stay one paragraph + links + a checklist.
- **Gate, then stabilize** (Rust-RFC import, product-owner approved 2026-07-31): behavior that
  changes a critical path ships BEHIND a gate (env toggle / config flag / spec `activations`),
  and flipping the default is a SEPARATE, recorded decision (a one-line ADR) after the gated
  form has been smoked — never the same change. **Named concerns**: a proposal's header may
  carry a `Concerns` checklist; an unchecked concern mechanically blocks `Approved` (see
  docs/proposals/README.md) — enforced by the validator's proposal-hygiene rules.
  **Every proposal MUST include** (product-owner directive, 2026-07-26): **per-use-case screen
  mockups**, **per-flow sequence diagrams** (mermaid, hexagonal-faithful), and **per-option pros/cons**
  for every decision it surfaces (a bare "A vs B" without trade-offs is incomplete) — see
  [docs/proposals/README.md](docs/proposals/README.md); `PROP-20260726-013207` is the reference example.
  **Every proposal has a tracking issue** (ADR-20260724-143000): create it before/with the proposal
  if missing, name it in the header, keep the two in step — an issue-less proposal is invisible to
  the prioritised backlog and gets lost.
- **Compiler first; a check is the fallback** (product-owner directive, 2026-08-03,
  ADR-20260803-234035): before writing any gate, ask whether the TYPE SYSTEM can make the mistake
  unspellable — a capability witness with a `pub(crate)` constructor, a sealed marker trait, private
  fields, a newtype, an unrepresentable state. The enforcement hierarchy in PROP-20260802-130500 §1
  ranks the levels; level 4 is the **floor**, not an achievement. Write a gate only where types
  genuinely cannot reach (cross-crate manifest capability, spec↔generation drift, non-Rust
  artifacts). Deleting a gate the compiler subsumes is a correct outcome, not a regression. Earned
  by [#329](https://github.com/TheCaptainCompany/captain-food/issues/329): seven review rounds and
  ~191 lines hardening a source-text scanner over a boundary the compiler already enforced, every
  gap in it found by a reviewer rather than by the scanner.
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
- **Independent review before ready-for-review** (product-owner directive, 2026-08-01): a PR is
  marked ready only after a reviewer-agent pass over the FULL branch diff by eyes that did not
  write it (high-stakes changes — payments, migrations, erasure — get the multi-lens fan-out;
  the #270 five-lens review found six criticals in fully-gated work and is the model). After any
  decision that renames or reshapes something, grep the OLD term across specs/**, docs/** and
  open issue/PR bodies before the turn ends — staleness the compiler cannot catch is caught by
  the sweep, not by the product owner. Prefer ONE SESSION PER WORK CHUNK: the repo carries the
  state (proposals, ADRs, checklists), so fresh context is cheap and long-context error rates
  are real.
- **Every session records what it learned** (ADR-20260730-034635, product-owner directive), in the
  **same change** as the work — not just failures, and not only on the second occurrence. Operational
  findings (environment limits, tool behaviour, gate costs, workflow traps) go to
  [docs/claude/sessions.md](docs/claude/sessions.md) or the relevant topic file; decisions to an ADR;
  option spaces to a proposal; state to `STATUS.md`. **Prefer executable over prose** — a validator
  rule, test or hook beats a bullet point, because prose can be ignored and a gate cannot
  (`makefile_recipe_lines_are_ascii` is the model). Record only what is **not derivable from the code**
  and would **cost the next session time**, with the concrete cost that earned it; sharpen an existing
  rule rather than appending a near-duplicate. **Writing nothing is a valid outcome** — a session diary
  is not a lesson, and padding lowers the odds the real rules get read.
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
  (`status/in-progress` label + claim comment naming the `NN-slug` branch **and carrying the session
  link** `https://claude.ai/code/session_<id>` — a claim comment is the first artifact of an issue and
  predates any commit, so it must be traceable to its run), create the `NN-slug`
  branch from `main`, and open a **draft PR** whose body starts with `Closes #NN` — branch, PR and
  issue are linked before any code is written. **Never enable auto-merge at this point** — a
  claim-time PR is a near-empty diff and would pass CI trivially. When the work is done and local
  gates are green (`make rust`), mark the PR **ready for review** and **enable auto-merge**
  **together, as one indivisible step**, and **supervise the checks until the PR is MERGED** (fix +
  push on failure; never end at "pushed, CI pending"). The merge closes the issue and ends the claim.
- Autonomous loops/routines run under the **weekly time budget** (`make budgeted-loop` or the routine
  guard) — Claude Code has no native cap; see [docs/claude/loops.md](docs/claude/loops.md) / ADR-0014.

## Project status

The live state (what exists, what is gated, what is next) is
[docs/STATUS.md](docs/STATUS.md) — kept current with every substantive change, and deliberately
NOT duplicated here. Toolchain: the Rust codegen `tools/codegen-rs` (bin `generate`) runs the
whole validator + every emitter; `make validate` / `make generate` / `make rust`; CI's `codegen`
gate fails on any spec↔generation drift.
