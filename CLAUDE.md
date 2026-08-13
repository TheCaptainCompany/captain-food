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
Read the relevant file before implementing or changing anything.

**Per-scope layout (ADR-20260807-183024, screaming architecture)**: the domain catalogs live in
**`specs/{scope}/{kind}.yaml`** folders — scopes `ordering · catalog · network · customer ·
delivery · payments · comms · common` — and the loader merges each kind's fragments into ONE
logical catalog. **`$ref`s are KIND-logical**: `commands.yaml#/X` names a kind, never a file
location, so moving an item between scope folders rewrites no refs. `specs/common/` is the kernel
(shared contracts + each kind's doctrine header); the validator enforces placement, the
cross-scope `$ref` DAG (process managers are exempt bridges), kernel purity, and api
nested-intra-scope (see [docs/claude/dsl.md](docs/claude/dsl.md)).

- [specs/PRODUCT_SPEC_WEB_CLIENT.md](specs/PRODUCT_SPEC_WEB_CLIENT.md) — web client product spec (user flows, checkout, Stripe payment, order tracking, NFRs, tech constraints).
- [specs/database/](specs/database/) — the store schema as DSL: real tables, generated `View_*`
  fold views, event-store functions, journals (the `inbound_messages` mailbox). `View_*` = a SQL
  VIEW; unprefixed = a TABLE. Full taxonomy + generation detail: [docs/claude/dsl.md](docs/claude/dsl.md);
  narrative: [specs/database.md](specs/database.md).
- `specs/{scope}/scalars.yaml` — domain scalar types (IDs, names, money, enums: `OrderStatus`, `RestaurantStatus`, `ServiceType`, `StockStatus`, etc.).
- `specs/{scope}/entities.yaml` — value objects and aggregates. HubRise-aligned catalog: `Restaurant`, `Catalog`, `CatalogCategory` (tree), `Product` → `Offer[]` (SKUs), `OptionList`/`Option`, `Cart`/`CartLineItem`, `Order`, `OrderLineItem`. Value objects `Money`, `Stock`, `TaxRate`, `Address`.
- `specs/{scope}/events.yaml` — **business event** payloads (RestaurantRegistered, ProductAdded, CatalogImported, OrderPlaced...). `*Updated` events carry the full entity (replace semantics). An event lives with its AUTHORING actor's scope.
- `specs/{scope}/commands.yaml` — **command payload** catalog (CQRS write side): each command is just its input schema (description + type + properties + required), parallel to events.yaml. Emits/handler → actors.yaml; errors → errors.yaml; persona/use-case/slice → stories.yaml. A command lives with its HANDLING actor's scope.
- `specs/{scope}/errors.yaml` — **anticipated errors** (the old command invariants): each with typed `context` and default `messages.en`/`messages.fr`. Mapped per command in actors.yaml `throws`.
- `specs/{scope}/actors.yaml` + `processmanager.yaml` — **actor-model catalog** (codegen source): aggregates & process managers, each with its inbox of `{ message → emits, throws }`, where every message/event/error is a `$ref` into commands.yaml/events.yaml/errors.yaml (checkable; the ref path encodes kind). The actor's FOLDER is the scope-membership declaration everything else derives from. Personas/authz live elsewhere (GraphQL `@auth`, story map).
- [specs/stories.yaml](specs/stories.yaml) — the **executable story map** (codegen source): personas → activities → steps, each step a `$ref` into an api.yaml query/mutation. The validator enforces completeness BOTH ways: steps resolve + persona role authorized, AND every mutation/query is reached by ≥1 step (`op-uncovered-by-story`).
- `specs/{scope}/rules.yaml` — **business rules / invariants** (ADR-0032): each a readable guarantee. Every behaviour test links to ≥1 rule and every rule is asserted by ≥1 test (bidirectional, validator-enforced). Rules say WHAT we guarantee; [specs/tests.yaml](specs/tests.yaml) says HOW (Given/When/Then). A rule may span several tests.
- [specs/screens/](specs/screens/) — Spec-Driven SDUI apps, one file per audience
  (marketplace / storefront front offices split by host, backoffice, rider, system); each file =
  screens + `resolvers`/`actions` allowlists into api.yaml, validator-proved, gaps explicit.
  Detail: [docs/claude/dsl.md](docs/claude/dsl.md).
- [specs/translations.yaml](specs/translations.yaml) — shared UI i18n catalog + per-surface
  sidecars, merged into one generated JSON. Detail: [docs/claude/dsl.md](docs/claude/dsl.md).
- `specs/{scope}/api.yaml` — the GraphQL surface as PER-SCOPE FRAGMENTS (types, queries,
  mutations, ACL; D8: one domain, one graph); SDL generated. **Role = path**: one composed schema
  served per-role under `/{role}/graphql`, top-level routing from a generated composition table.
- `specs/{scope}/configuration.yaml` — scope-owned runtime keys (D5); platform keys (DB,
  telemetry, identity, mailbox/projector machinery) in `specs/common/configuration.yaml`.
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

**Full-stack Rust** (ADR-0034/0035), Cargo workspace in Clean-Architecture layers — `domains/*`
(per-scope GENERATED type crates + the `domain-common` kernel, deps derived from spec `$ref`s —
ADR-20260807-183024/#373) · `domain` (pure DDD facade re-exporting them + hand-written aggregates,
no outward imports) · `application` (ports, handlers, PMs, write-side Repository) ·
`infrastructure` (event store, `View_*` repos, integration ACLs, the mailbox) · `server` (Axum
BFF: GraphQL, SDUI, tenant middleware) · `actor_runtime` (generic mailbox: leases, fencing,
head-of-line) · `shared_types`/`core`/`web`/`desktop` (UniFFI/Crux/Leptos/Tauri). Frontend =
Leptos→WASM SDUI over GraphQL. Backend = CQRS-light + event log: mutations enqueue on the actor
mailbox (acceptance-first, PENDING) and workers append to `domain_events`; queries read `View_*`
read models, never the raw log. **Self-hosted Postgres — CloudNativePG in-cluster on OVH MKS
(Paris)**: ≥3 instances, anti-affinity, WAL archiving to Object Storage, executed restore drills
(ADR-20260807-002705, superseding the Clever Cloud and OVH-VPS hosting ADRs); operations are
GitOps-only (Argo CD over GENERATED manifests). Multi-tenant by `Host`
(`{slug}.captain.food`); integrations: Stripe, HubRise, delivery partners, Supabase Auth
(**wrapped, identity-only — Supabase holds no business data**). The monolith `server` bin is still
the DEPLOYED runtime until the #358 cutover points traffic at the per-surface/per-actor bins
(ADR-20260807-183024). Dependency rule:
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
**0 errors**. Warnings are a **ratchet the validator owns, not a number in this file**: the per-rule
histogram lives in [`tools/codegen-rs/warning-baseline.json`](tools/codegen-rs/warning-baseline.json)
and `make validate` fails when the live run differs from it **in either direction** — a new kind or a
higher count is a regression, a lower count is an improvement you must bank. So **do not re-measure
anything**: run the gate. If a change legitimately moves the warning surface, run `make
warning-baseline` and commit the refreshed artifact **in the same commit**, and say in the PR body why
an added warning is accepted — the diff (`+1 event-not-projected`) is the record. Never trust a
warning count written in prose anywhere, including here: this paragraph used to pin one, and it went
stale three times (32, then 43, then 37), costing four agents in a single day a full extra validator
run against a pristine `main` worktree apiece before they could claim "no new warning" — three of them
because the pinned number looked wrong and they had to re-derive it. A stale ratchet is now a gate
failure rather than a misleading sentence.

### Non-negotiable rules

- **`specs/**` is the team's work** (founder directive, 2026-08-10, verbatim in
  [ADR-20260810-221840](docs/adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)):
  *"I'm surprise that I read that the spec was untouchable now that we have the team working together
  we don't need to have this constraint anymore … I'm pretty sure the team will ensure the right
  naming and scope. Just keep me informed."* The freeze is **lifted, not narrowed** — execution loops
  may add and amend DSL content **and structure** under the ordinary gates. The boundary is **not**
  content-vs-structure (a file move between scopes rewrites no refs and is free; a one-word type
  change on an emitted event is irreversible). Three questions, in order: **(1) does it contradict or
  create a recorded decision?** (`docs/proposals/DECISIONS.md`, `docs/adr/`) — if yes it is a decision
  reversal, not a spec edit: stop and file a register row, whatever the diff size. **(2) Is the shape
  already emitted, stored or promised?** (`domain_events`, a shipped client, an alert route, a partner
  contract, a legal artifact) — if yes it is a **migration**, and the versioning story is recorded
  before it lands: stored events are immutable, upcasting never mutation. **(3) Otherwise it is the
  team's** — including `specs/common/`, which is a high-fan-out shared kernel, not a no-go zone;
  freezing it would freeze the one place "one name = one dedicated scalar" is enforced. Structure gets
  no separate gate: proportionality already routes any real option space to a proposal + register row,
  which *is* the discussion offered. **Reporting is the obligation that replaces the freeze**: every
  landed spec change writes one sentence in [docs/SPEC-LOG.md](docs/SPEC-LOG.md) — what the product now
  promises differently — in the **same commit**. C4 (`specs/architecture/*.yaml`) and observability
  contracts (`specs/observability.yaml`) are **source** DSL, not generated.
- **Proposals are committed to the repo** (ADR-20260724-135945, founder directive): every
  proposal presented for approval lands in [docs/proposals/](docs/proposals/) as
  `PROP-YYYYMMDD-HHMMSS-<slug>.md` — the proposal as presented, alternatives considered, the
  approver's scope choices, status header linking the realizing PR/ADR. Proposals are LIVING
  documents (ADR-20260801-020000): the file always holds the clean CURRENT state of the design —
  refinements rewrite it in the same change as their ADR; history lives in the file's git log,
  never as appended sections or superseded blocks. A session-local plan file is NOT a substitute — the rationale must survive the session.
  **GitHub is never the record**: issue bodies, PR bodies and PR/issue comments carry LINKS into
  docs/proposals + docs/adr (and a tracking checklist at most) — never the design content itself;
  content drafted in a GitHub surface must land in the repo in the same change.
  **Proportionality** (founder directive, 2026-07-31): the record matches the size of the
  decision — a real option space needing arbitration gets a proposal (+ tracking issue); a
  decision without alternatives gets an ADR (inline "options considered" at most); a small
  subject with no real decision needs NEITHER — the commit message and the PR's one-paragraph
  body are enough. Issues and PR bodies stay one paragraph + links + a checklist.
- **Gate, then stabilize** (Rust-RFC import, founder-approved 2026-07-31): behavior that
  changes a critical path ships BEHIND a gate (env toggle / config flag / spec `activations`),
  and flipping the default is a SEPARATE, recorded decision (a one-line ADR) after the gated
  form has been smoked — never the same change. **Named concerns**: a proposal's header may
  carry a `Concerns` checklist; an unchecked concern mechanically blocks `Approved` (see
  docs/proposals/README.md) — enforced by the validator's proposal-hygiene rules.
  **Every proposal MUST include** (founder directive, 2026-07-26): **per-use-case screen
  mockups**, **per-flow sequence diagrams** (mermaid, hexagonal-faithful), and **per-option pros/cons**
  for every decision it surfaces (a bare "A vs B" without trade-offs is incomplete) — see
  [docs/proposals/README.md](docs/proposals/README.md); `PROP-20260726-013207` is the reference example.
  **Every proposal has a tracking issue** (ADR-20260724-143000): create it before/with the proposal
  if missing, name it in the header, keep the two in step — an issue-less proposal is invisible to
  the prioritised backlog and gets lost.
- **Compiler first; a check is the fallback** (founder directive, 2026-08-03,
  ADR-20260803-234035): before writing any gate, ask whether the TYPE SYSTEM can make the mistake
  unspellable — a capability witness with a `pub(crate)` constructor, a sealed marker trait, private
  fields, a newtype, an unrepresentable state. The enforcement hierarchy in PROP-20260802-130500 §1
  ranks the levels; level 4 is the **floor**, not an achievement. Write a gate only where types
  genuinely cannot reach (cross-crate manifest capability, spec↔generation drift, non-Rust
  artifacts). Deleting a gate the compiler subsumes is a correct outcome, not a regression. Earned
  by [#329](https://github.com/TheCaptainCompany/captain-food/issues/329): seven review rounds and
  ~191 lines hardening a source-text scanner over a boundary the compiler already enforced, every
  gap in it found by a reviewer rather than by the scanner.
- **Final vision first — no intermediate step where the final step can be built** (founder
  directive, 2026-08-08, verbatim in
  [ADR-20260808-235113](docs/adr/ADR-20260808-235113-final-vision-first-no-intermediate-steps.md)):
  *"do not choose the easy path, choose the final clean vision … always put in place the final
  step."* When an option space contains a cheap intermediate and the final clean shape, build the
  final shape directly; recommendations present the final-vision option FIRST. Composes with
  compiler-first (the type-level answer IS the final vision) and does NOT overturn
  gate-then-stabilize — gating decides WHEN a finished thing takes over, never licenses a shim.
  Where staging is externally forced, the intermediate ships only with the final step already
  designed and recorded.
- **Mob programming — every agent is in the dev** (founder directive, 2026-08-09, verbatim in
  [ADR-20260809-013142](docs/adr/ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md)):
  *"everyone is involved in the dev so … everyone will be able to detect issues during the dev."*
  A dispatch is a MOB: (1) **mob briefing** — the brief goes to the WHOLE roster in parallel before
  any code, each lens naming what it will catch and what the executor must know ("nothing in my
  lens" is a complete answer and costs one line); (2) **mob checkpoints** — the executor stops at
  declared phase boundaries and the mob reads the actual diff, any lens may stop the work;
  (3) the independent full-diff review stays, now as the THIRD look. Coordinator-chosen lens
  subsets are over — the roster is invited by default and a lens excuses itself. Earned by
  [#424](https://github.com/TheCaptainCompany/captain-food/issues/424), where a post-hoc UX pass
  found the built checkout state **could not render at all** — a finding that would have changed
  the work, for free, if the lens had been in the briefing.
- **He is the FOUNDER / Tech CEO, and every founder message goes to the whole team before any answer**
  (founder directives, 2026-08-12, verbatim in
  [ADR-20260812-143619](docs/adr/ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)):
  *"Stop calling me product owner. I'm the founder / Tech CEO."* and *"When I say something ask the
  team for answers never answer directly without asking the whole team."* The mob principle above
  extends from **dispatches to founder messages**, and coordinator-never-authors below from **the diff
  to the answer**: no answer is composed and no record lands before the whole roster has been asked;
  *"nothing in my lens"* stays a complete one-line answer. **Three carve-outs**: an **external-clock
  fact** (a billing suspension, a token expiry, a partner deadline, an opposition window) is relayed
  in the same turn, verbatim from the register, with the mob's opinion following; **executing an
  already-recorded rollback/abort path** needs no consult — the mob's involvement happened when the
  path was written — while going FORWARD through an incident (a hotfix migration, flipping a gate to
  escape) is a new decision and does get the mob (*am I executing a recorded path, or inventing
  one?*); and **no lens output, and no aggregation of lenses, is legal advice or clearance** —
  agreement between lenses never upgrades a hedged finding to a settled one. **A record created from a
  founder directive carries a `Consulted:` block, one line per lens** (*"nothing in my lens"* is a
  valid line), because a lens that was never asked is indistinguishable from a lens with nothing to
  say — silence is ambiguous, the same defect class ADR-20260810-231300 records for monitoring. The
  rename sweeps the LIVING operating docs only; **historical ADRs and proposals keep their vocabulary
  and verbatim quotes stay verbatim**, and "product ownership" remains the name of the FUNCTION the
  team holds (ADR-20260808-144738). **External artifacts** (mentions légales, partner onboarding,
  association filings) must name the capacity the statutes actually confer — "founder / Tech CEO" is a
  repo-internal title, not a French corporate mandate.
- **Team ownership — sessions start by themselves, and the coordinator never authors the diff**
  (founder directive, 2026-08-10, verbatim in
  [ADR-20260810-011500](docs/adr/ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)):
  *"every next session start to work by itself without asking permission because the team has the
  ownership of the product"* — *"never do the job yourself, only the team agents have the ownership
  of the product, you are playing the role of assistant."* A session begins working WITHOUT being
  asked: CLAUDE.md → STATUS → the **architect agent** names the next chunk from the prioritised
  backlog → claim → the full mob loop above, with the **executor agent** writing EVERY phase of the
  diff (code, specs under recorded approval, records). The session lead is a COORDINATOR only:
  briefs, checkpoints, relaying, GitHub mechanics — never the product diff. The only thing brought
  to the founder is the **decision queue** (genuine option spaces, external/legal actions,
  admin-gated provisioning), presented with options + trade-offs + a recommendation — never
  "shall I proceed?". The observable compliance signature: mob evidence in the PR body,
  executor-authored commits, coordinator pushes limited to claim commits and GitHub surfaces.
  The loop still starts unasked, AND its start is always accompanied by a compact action plan
  shown to the founder — chunk, phases, checkpoints, gates, out-of-scope fences,
  anticipated decision points — as transparency, never as a permission request
  ([ADR-20260810-114242](docs/adr/ADR-20260810-114242-loop-start-action-plan.md)).
- **No polling, only pushing — polling is a graceful fallback until pushing works again**
  (founder directive, 2026-08-10, verbatim in
  [ADR-20260810-231300](docs/adr/ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)):
  *"no polling only pushing, polling as graceful fallback until pushing works again."* Push is the
  primary transport for every state change one component must learn from another. **The second clause
  is the usable half**: a poll is legitimate ONLY as a fallback that is (a) a **declared** degraded
  mode, (b) **observably** degraded — an operator can tell from telemetry that it is polling rather
  than pushing, under a `specs/observability.yaml` contract
  (`mailbox_push_down_total{reason}` is the reference shape) — and (c) has a **path back that
  something actively detects**. A poll with none of those is just a poll with an excuse, and a
  *silent* fallback is worse than the poll it replaced: it turns a loud outage into a permanent
  invisible latency tax. **"Pushing works again" is never detected by the absence of an error** — a
  `LISTEN` through a transaction-mode pooler is accepted and then delivers nothing, so `recv()` never
  fails and a connection-error-driven flag stays `true` forever. Detection must be a **positive
  liveness proof on the push path itself**: `mailbox_wake.rs`'s 30 s self-`pg_notify` canary with a
  required echo is the reference implementation. **Scope**: this governs state-change *propagation*,
  not *time-triggered* work — nobody can `NOTIFY` "a deadline passed", so reminder promotion, TTL
  expiry and retention sweeps are outside it (there, the discipline is *sleep until the next due row*,
  not *scan on a fixed interval*). The tiebreaker at the boundary: **does any component know this fact
  before the clock does?** If yes, it is propagation and must be pushed. Applies to the team's own
  loop too — agent completions arrive as push, so **do not reintroduce a polling status cron**.
  **Second carve-out — MONITORING keeps a poll, permanently** (founder refinement, same ADR):
  *"Monitoring could be excluded from this principle if we cannot design it pushable. In any case for
  monitoring will have a polling as fallback."* Still try to make it pushable; it may poll where push
  cannot be designed; and it **keeps a poll in every case, even where push works** — this clause is
  *stronger* than the general principle and has **no exit**, inverting condition (c). The reason is not
  frequency, it is that **for a monitor, silence is ambiguous**: a push-only monitor cannot tell
  "healthy, nothing to report" from "dead, reporting nothing". Every other push consumer resolves that
  with a durable backstop to reconcile against (`domain_events` + `projection_checkpoint`,
  `inbound_messages` + status); a monitor watching a black box has none, because the thing it watches
  is the thing that would tell it. **Narrow test**: the observer is outside what it observes and has no
  durable record to reconcile against — it does NOT license polling in a monitor that could subscribe
  and reconcile. `mailbox_wake.rs`'s canary is this clause already implemented (a push mechanism driven
  on a timer); `tools/smoke/prod-smoke.sh`'s `wait_for` is correct under it. **New defect class**: a
  monitoring path that can only fire when a signal ARRIVES — a threshold alert goes quiet when export
  stops, which is exactly when it should scream. Liveness needs a dead-man's-switch, not a threshold.
- Business code (aggregates / pure command handlers) stays **independent of the telemetry SDK**;
  instrumentation lives only in framework/middleware boundaries (see `c4-l3.yaml` `instrumented` flags).
- Every critical workflow must have an observability contract in `specs/observability.yaml`.
- **A business metric IS A PROJECTION** (founder directive, 2026-08-11: *"Confirm the reversal,
  go with the projections"*; [ADR-20260811-014129](docs/adr/ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md),
  superseding [ADR-20260810-234225](docs/adr/ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md)
  in part). Every feature, for every persona, carries at least one — the unit is the **persona
  ACTIVITY** in `specs/stories.yaml` (8 personas, 25 activities), never the story step, because a step
  is an operation call and an activity is an outcome. A metric **declares the question it answers**,
  and it is a **declared `fold:` over `domain_events`** maintained by the `bam` projector into the
  `bam` schema, read through a **tenant-scoped GraphQL query** — *not* a counter emitted at a call
  site. Two reasons that decide everything downstream: **a fold replays** (a counter does not, so a
  metric added later would carry zero history), and **ratios and distinct-identity denominators are
  inexpressible as counters** but ordinary as queries. Grouping keys need a **declared bounded
  population** — `restaurantId` is fine, a Postgres row is not a time series. **Operational telemetry
  does not move**: latency, error budgets, span status and dead-man's switches stay on
  OTLP/Honeycomb, because they must work when Postgres is down, which is exactly when a
  Postgres-backed metric is blind. An `alertable:` subset taps a counter as it folds at head — one
  declaration, two outputs. Grammar, rules and the open rows:
  [PROP-20260810-234225](docs/proposals/PROP-20260810-234225-business-metrics-for-every-persona.md) D4/D6/D8/D9.
- **Every reference in the DSL is a `$ref`; only a declaration may introduce a bare name**
  (founder directive, 2026-08-11: *"we need to heavily strongly typed the spec no string in
  it"*; [ADR-20260811-014129](docs/adr/ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md)
  Decision 2). Four categories, in order: **(1)** a **declaration** introduces a name (`measures:
  [{ name: orders }]`) — correct, and the only place a bare name is; **(2)** a **reference** to
  something declared elsewhere is a `$ref` the loader resolves, **including inside the same file**
  (`{ $ref: '#/Order/state/orderId' }`, `specs/ordering/actors.yaml:102`); **(3)** a **value from a
  closed set** stays a bare token (`type: counter`, `bucket: DAY`) *provided the set is closed in the
  loader schema* — **except** where a domain scalar already declares that set, where the `$ref` is
  mandatory (`{ $ref: 'scalars.yaml#/ServiceType' }`, never a hand-copied `[DELIVERY, COLLECTION]` —
  "one name = one dedicated scalar" applied to a value set); **(4)** **prose stays prose**
  (`description:`, `question:`) — typing it would be theatre. Why it is structural and not stylistic:
  [#413](https://github.com/TheCaptainCompany/captain-food/issues/413) — a plain-string
  `tombstone: SomeEvent` is *"silently invisible everywhere"*, because the refs walker collects only
  `$ref` nodes and the rule written for that key *"only sees tombstones the parser recognized"*.
  **Binding on NEW DSL surface.** It is **not** a licence to sweep: the existing bare-name sites
  (`data_requirements:`/`actions_used:` 40, `roles:` 112) each have a bespoke rule today and their
  conversion is separate, sequenced work ([DECISIONS §27bis MET-T2](docs/proposals/DECISIONS.md)).
- If a **behaviour test** fails, fix the generator/runtime — not the test. If an **observability test**
  fails, fix instrumentation/middleware — not the domain model.
- **Completeness is part of every change (ADR-0032):** a new command/event/error also needs a behaviour
  test (+ its `rules:` link); a new mutation/query also needs a story step; a new business rule also needs
  a test. `make validate` blocks otherwise — do not weaken the gate, extend the specs.
- Review and validation gates are executable and **blocking**; never hand-edit generated output
  (`specs/generated/**`, the `database.md` GENERATED region) — change the spec/emitter and regenerate.
- Every recurring agent/loop failure becomes a new rule, test, or ADR.
- **Independent review before ready-for-review** (founder directive, 2026-08-01): a PR is
  marked ready only after a reviewer-agent pass over the FULL branch diff by eyes that did not
  write it (high-stakes changes — payments, migrations, erasure — get the multi-lens fan-out;
  the #270 five-lens review found six criticals in fully-gated work and is the model). After any
  decision that renames or reshapes something, grep the OLD term across specs/**, docs/** and
  open issue/PR bodies before the turn ends — staleness the compiler cannot catch is caught by
  the sweep, not by the founder. Prefer ONE SESSION PER WORK CHUNK: the repo carries the
  state (proposals, ADRs, checklists), so fresh context is cheap and long-context error rates
  are real.
- **Every session records what it learned** (ADR-20260730-034635, founder directive), in the
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
- **Respect the prioritised backlog — and the team now sets it** (founder directive,
  2026-08-10, [ADR-20260810-215503](docs/adr/ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md):
  *"Don't care about the project field anymore the team decides without me"*): priorities live **in the
  GitHub Project "Prioritized backlog"** (Priority field + row order) — pick work from the top;
  skipping the top item needs a stated reason. The **`Priority` bucket and row order are the team's
  to set**; the founder may override either at any time, without justification, and the team
  adopts it immediately. What is NOT delegated: genuine option spaces
  ([docs/proposals/DECISIONS.md](docs/proposals/DECISIONS.md)), external/legal/admin-gated matters,
  `specs/**` approval — **a `Priority` is not an approval; ranking an AMBER item `Urgent` does not
  make it dispatchable** — and **the method**, which is now **binding rather than descriptive**:
  [docs/BACKLOG.md](docs/BACKLOG.md) records how value is defined (value-first, ADR-20260720-213024) —
  foundations/cross-functional/non-functional first, then features in value-stream order — and every
  ranking must be justifiable under it. **An agent must never change a Priority bucket or a row
  position in order to make an item dispatchable, or to make its own recommendation legitimate**: a
  blocked top item is reported blocked, never re-ranked. Every bucket change or material row move is
  stated in the architect's run report with the method clause that justifies it; a re-ranking that
  reverses a previously stated order also gets a line in `docs/STATUS.md`.
- **Spec- and docs-only changes go straight to `main`** (founder directive): commit and **push
  directly to `main`** — no branch, no PR, no claim ceremony — for changes confined to `specs/**`,
  `docs/**`, ADRs, `CLAUDE.md`, `STATUS.md`, and the generated artifacts they regenerate. **Keep `main`
  green**: run the same gate CI would (`make rust`) locally **before** pushing anything that touches
  `specs/**` (a docs-only edit that regenerates nothing may skip it). A spec change that moves the
  warning surface also carries **`tools/codegen-rs/warning-baseline.json`** (refreshed by
  `make warning-baseline`): it is part of a spec change's footprint even though it sits under
  `tools/` and `make generate` never writes it, so it does NOT turn the change into code work. The
  claim → draft-PR → supervised-merge flow below applies to **code/feature work** (touching
  `crates/**`, `tools/**` *other than that artifact*, CI, deploy), not to pure spec/doc edits.
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
