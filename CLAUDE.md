# Captain.Food

Local-first food ordering and delivery for independent restaurants and food trucks. V0 target:
product–market fit in **Tours** — mobile-first web UX, backend evolving towards CQRS + event log.

**This file is a resident INDEX, not the whole rulebook** (ADR-20260816-020752). Each topic's
authority is its file in [docs/claude/](docs/claude/); loading is *this snapshot **plus** the topic
file*, never the snapshot alone. Nothing here may be dropped: compressing wording is execution,
removing a rule is a decision reversal needing a [DECISIONS](docs/proposals/DECISIONS.md) row.
What stays resident: **if forgetting a rule produces state a rebuild cannot undo, it is resident**;
a rule whose decision happens after the read may be fetched just in time.

## Domain lens — a food-delivery product, not a generic CRUD app

Apply to every task, whatever its size; it is not derivable from the code.

- **The ETA is the product.** The estimate shown before ordering is the number the customer decides
  on. Degrading or omitting it is a conversion problem, not a polish item.
- **A paid order nobody is told about is the worst failure mode there is** — worse than a crash,
  because the money moved. Anything touching order placement answers: who gets told, and what
  happens if nobody acts?
- **Oversell and un-accepted orders lose both sides of the marketplace at once.**
- **Peak is Friday/Saturday 19:00–21:30.** "Does this hold at peak?" is fair of any change to
  checkout, dispatch, projections or hosting.
- **Some things are legal preconditions in France, not backlog items**: allergen declaration for
  distance selling (EU FIC 1169/2011), VAT and a compliant receipt, GDPR erasure, who holds customer
  funds (payment-agent posture). Flag them; never defer them silently.
- **Availability, stock and orderability are three different things** (see conventions).
- **A control that renders but does nothing is worse than no control.** Screens may declare `gaps`;
  a live widget bound to one is not the same thing.

Audit and next steps: the **`architect`** agent, feeding
[docs/proposals/DECISIONS.md](docs/proposals/DECISIONS.md). **CQRS/ES/DDD doctrine is a separate
voice from that operations role** (ADR-20260815-032912): `young` (read/write separation, folds as
disposable projections, event versioning, set-based validation), `vernon` (aggregate boundaries, one
aggregate per transaction, process managers, Ask vs Tell), `evans` (ubiquitous language, bounded
contexts, ACLs, distillation). Cite the lens that carried a finding.

## Ubiquitous language and conventions

- **English only** in all repository content — docs, code, comments, commits, identifiers.
- **Event payloads are business-only.** Never mix in the envelope (`eventId`, `aggregateType`,
  `aggregateId`, `occurredAt`, the acting `user_id`/`user_type`, `metadata`) — infrastructure adds
  it. The actor who performed an event (`createdBy`/`updatedBy`/…) is envelope metadata on
  `domain_events.user_id` (ADR-0041), not a payload field. A business ROLE that changes semantics
  (`Tipper` = CUSTOMER|RESTAURANT) is business data and stays.
- **Strong typing**: types `$ref` scalars/entities; **one name = one dedicated scalar**.
- **Money** = `{ amountCents, currency }`; convert to/from HubRise's `"9.80 EUR"` only at the
  integration boundary.
- **Availability ≠ stock**: `CatalogItemAvailability` (`AVAILABLE`/`UNAVAILABLE`, manual flag) vs
  derived `StockStatus` (`IN_STOCK`/`LOW_STOCK`/`OUT_OF_STOCK`). Orderable = `AVAILABLE` **and**
  stock > 0.
- **HubRise interop**: `ref` (`ExternalReference`) is the idempotent import key; translation goes
  through an ACL — never let `SKU`/`option_list`/`"9.80 EUR"` reach the domain
  ([specs/integrations/hubrise.md](specs/integrations/hubrise.md)).
- **Slugs**: `^[a-z0-9]+(?:-[a-z0-9]+)*$`.
- **Always name issues/PRs, never bare numbers** — include the title, e.g. `#21 "Frontend:
  Leptos/WASM SDUI renderer"`. In repo markdown it must be a FULL CLICKABLE LINK
  (`[#NN "<title>"](https://github.com/TheCaptainCompany/captain-food/issues/NN)`): GitHub does not
  auto-link bare `#NN` outside issues/PRs/commits.
- **Makefile recipe lines are ASCII-only** (`--`, `->`, `|`) — enforced by the
  `makefile_recipe_lines_are_ascii` codegen test; read its failure message for the why.
- **Commands are derived from use cases** (ADR-0004), never one-per-event: a command can be
  REJECTED, while an external fact that already happened is an **inbound (integration) event**
  recorded through the ACL with no command (Stripe, HubRise, delivery partners — 📥 in the story
  map). Doctrine: [docs/claude/dsl.md](docs/claude/dsl.md).

## Specifications — the source of truth, read before any task

Per-scope layout (ADR-20260807-183024, screaming architecture): **`specs/{scope}/{kind}.yaml`** over
`ordering · catalog · network · customer · delivery · payments · comms · common`; the loader merges
each kind's fragments into ONE logical catalog. **`$ref`s are KIND-logical** (`commands.yaml#/X`
names a kind, not a file), so moving an item between scopes rewrites no refs. `specs/common/` is the
kernel. Placement, the cross-scope `$ref` DAG (process managers are exempt bridges), kernel purity
and api nested-intra-scope are validator-enforced — mechanics in
[docs/claude/dsl.md](docs/claude/dsl.md).

Kinds: `scalars` · `entities` · `events` (business payloads; `*Updated` carries the full entity —
replace semantics; lives with its AUTHORING actor) · `commands` (write-side input schemas; lives
with its HANDLING actor) · `errors` (typed `context`, en/fr messages) · `actors` +
`processmanager` (codegen source: each actor's inbox of `{ message → emits, throws }`, all `$ref`s;
the FOLDER declares scope membership) · `rules` (invariants, ADR-0032) · `api` (GraphQL fragments;
**role = path**, one composed schema per role at `/{role}/graphql`) · `configuration` (scope keys;
platform keys in `specs/common/`). Cross-cutting: [stories.yaml](specs/stories.yaml) (executable
story map: personas → activities → steps `$ref`ing api ops) · [tests.yaml](specs/tests.yaml)
(Given/When/Then) · [screens/](specs/screens/) (SDUI per audience, gaps explicit) ·
[translations.yaml](specs/translations.yaml) · [database/](specs/database/) (`View_*` = a SQL VIEW,
unprefixed = a TABLE). Whole-product navigable view:
[documentation.generated.md](specs/generated/documentation.generated.md) (GENERATED).

## Architecture (summary)

**Full-stack Rust** (ADR-0034/0035), Cargo workspace in Clean-Architecture layers: `domains/*`
(per-scope GENERATED type crates + the `domain-common` kernel, deps derived from spec `$ref`s) ·
`domain` (pure DDD facade + hand-written aggregates) · `application` (ports, handlers, PMs,
Repository) · `infrastructure` (event store, `View_*` repos, ACLs, mailbox) · `server` (Axum BFF:
GraphQL, SDUI, tenant middleware) · `actor_runtime` (leases, fencing, head-of-line) ·
`shared_types`/`core`/`web`/`desktop`. Frontend = Leptos→WASM SDUI over GraphQL. Backend =
CQRS-light + event log: mutations enqueue on the actor mailbox (acceptance-first, PENDING), workers
append to `domain_events`, queries read `View_*` — never the raw log. **Self-hosted Postgres —
CloudNativePG on OVH MKS (Paris)**, ≥3 instances, WAL archiving, executed restore drills
(ADR-20260807-002705); operations GitOps-only (Argo CD over GENERATED manifests). Multi-tenant by
`Host` (`{slug}.captain.food`); Stripe, HubRise, delivery partners, Supabase Auth (**wrapped,
identity-only — no business data**). The monolith `server` bin is the DEPLOYED runtime until the
#358 cutover. Dependency rule: outer→inner only.

## Operating model (read [docs/PLAYBOOK.md](docs/PLAYBOOK.md))

The **YAML DSL is the source of truth**, everything else is generated/derived; planning is separate
from execution; observability is a contract. Topic authorities — read the relevant one before
working: [dsl.md](docs/claude/dsl.md) · [codegen.md](docs/claude/codegen.md) ·
[observability.md](docs/claude/observability.md) · [c4.md](docs/claude/c4.md) ·
[adr.md](docs/claude/adr.md) · [loops.md](docs/claude/loops.md) ·
[mermaid.md](docs/claude/mermaid.md) · [sessions.md](docs/claude/sessions.md) (**operational** —
read before a long or exploratory session: gate costs, disk, MCP output volume, unreadable PDFs, and
why an integration's API suite and auth mechanism are established BEFORE any credential is named,
ADR-20260730-032306). Decisions: [docs/adr/](docs/adr/), ids `ADR-YYYYMMDD-HHMMSS` (legacy
`0001`–`0047` keep sequential ids).

Agents live in `.claude/agents/`; gates are hooks in `.claude/settings.json`. `make help` lists
entrypoints; `make test-quiet` / `make rust-quiet` run gates filtered to VERDICTS.
**`make validate` (Rust `tools/codegen-rs`) is the single executable gate for the whole spec** —
schema/refs, actor wiring, api↔model, views, C4, observability, and (ADR-0032) tests/stories/rules
completeness. It must be **0 errors**. Warnings are a **ratchet the validator owns**: never
re-measure or quote a count in prose — run the gate; if a change legitimately moves the surface, run
`make warning-baseline`, commit the refreshed artifact in the SAME commit, and say why an added
warning is accepted.

**Production telemetry**: Honeycomb **EU (`eu1`)** — a GDPR constraint, pinned ON PURPOSE (the US
default "succeeds" and shows an empty environment list; do not "fix" it back). The MCP server is
**deliberately DISABLED** pending re-auth (`disabledMcpjsonServers`, ADR-20260816-020752) — its
absence is a recorded decision, not an absence of telemetry concern, and **re-auth is the event that
re-enables it**. Auth, keys, query discipline:
[docs/claude/observability.md](docs/claude/observability.md).

### Non-negotiable rules

- **`specs/**` is the team's work — the freeze is LIFTED, not narrowed** (founder directive
  2026-08-10, verbatim in
  [ADR-20260810-221840](docs/adr/ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)):
  loops may add and amend DSL **content and structure** under the ordinary gates. Three questions, in
  order: **(1) does it contradict or create a recorded decision?** → a decision reversal: stop and
  file a register row, whatever the diff size. **(2) Is the shape already emitted, stored or
  promised?** (`domain_events`, a shipped client, an alert route, a partner contract, a legal
  artifact) → a **migration**: the versioning story is recorded before it lands, stored events are
  immutable, upcasting never mutation. **(3) Otherwise it is the team's**, including `specs/common/`.
  **Reporting replaces the freeze**: every landed spec change writes one sentence in
  [docs/SPEC-LOG.md](docs/SPEC-LOG.md) — what the product now promises differently — in the SAME
  commit. `specs/architecture/*.yaml` and `specs/observability.yaml` are **source**, not generated.
- **Proposals are committed to the repo** (ADR-20260724-135945): every proposal presented for
  approval lands in [docs/proposals/](docs/proposals/) as `PROP-YYYYMMDD-HHMMSS-<slug>.md` with
  alternatives, the approver's scope choices, and a header linking the realizing PR/ADR. They are
  LIVING (ADR-20260801-020000): the file holds the clean CURRENT design, refinements rewrite it in
  the same change as their ADR, history lives in git — never appended "superseded" blocks; a
  session-local plan file is not a substitute. **GitHub is never the record**: issue/PR bodies carry
  links + a checklist at most, and anything drafted there lands in the repo in the same change.
  **Every proposal MUST include** (founder directive 2026-07-26) per-use-case screen mockups,
  per-flow mermaid sequence diagrams (hexagonal-faithful) and per-option pros/cons
  ([docs/proposals/README.md](docs/proposals/README.md); `PROP-20260726-013207` is the reference),
  **and a tracking issue** named in the header (ADR-20260724-143000). **Proportionality** (founder
  directive 2026-07-31): a real option space → proposal + issue; a decision without alternatives →
  an ADR; a small subject with no real decision → NEITHER, the commit message and a one-paragraph PR
  body are enough.
- **Gate, then stabilize** (founder-approved 2026-07-31): behaviour changing a critical path ships
  BEHIND a gate (env toggle / config flag / spec `activations`); flipping the default is a SEPARATE
  recorded decision after the gated form has been smoked. An unchecked `Concerns` entry mechanically
  blocks `Approved` (validator-enforced).
- **Compiler first; a check is the fallback** (founder directive 2026-08-03, ADR-20260803-234035):
  before writing any gate, ask whether the TYPE SYSTEM can make the mistake unspellable — capability
  witness, sealed trait, private fields, newtype, unrepresentable state. PROP-20260802-130500 §1
  ranks the levels; level 4 is the FLOOR. Write a gate only where types cannot reach (cross-crate
  manifest capability, spec↔generation drift, non-Rust artifacts). Deleting a gate the compiler
  subsumes is a correct outcome.
- **Final vision first — no intermediate step where the final step can be built** (founder directive
  2026-08-08, verbatim in
  [ADR-20260808-235113](docs/adr/ADR-20260808-235113-final-vision-first-no-intermediate-steps.md)):
  *"always put in place the final step."* Recommendations present the final-vision option FIRST.
  Composes with compiler-first; does NOT overturn gate-then-stabilize (gating decides WHEN a finished
  thing takes over, never licenses a shim). Where staging is externally forced, the intermediate
  ships only with the final step already designed and recorded.
- **Mob programming — every agent is in the dev** (founder directive 2026-08-09, verbatim in
  [ADR-20260809-013142](docs/adr/ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md)):
  (1) the brief goes to the WHOLE roster in parallel **before any code**, each lens naming what it
  will catch (*"nothing in my lens"* is a complete answer); (2) the executor stops at declared
  **checkpoints** and the mob reads the actual diff — any lens may stop the work; (3) the independent
  full-diff review stays, as the THIRD look. **At the BRIEFING the roster is invited by default and a
  lens excuses itself — coordinator-chosen subsets are over there**; the **CHECKPOINT** goes only to
  lenses that DECLARED a concern at briefing (any lens may opt back in), and the chunk's
  **reversibility class** sizes the briefing roster — full mob for money movement, stored event
  shapes, legal surfaces and anything Tours-facing (the `HOLD: human` axis, which wins when the two
  disagree), 2–3 lenses for reversible refactors, generated artifacts and doc sweeps (founder ruling
  2026-08-16,
  [ADR-20260816-134352](docs/adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)).
  Every dispatch card states its class and BANKS at the checkpoint whether the narrow set missed
  anything, **with an attribution** (card defect / invited-lens depth miss / roster width); only a
  miss attributed to **roster width** goes back to the founder, because reverting a class amends his
  ruling. **A MISS no longer reverts a class automatically** — struck 2026-08-17 on n=2 where neither
  miss was a roster-width miss, and replaced by the rule that earned it
  ([ADR-20260817-105845](docs/adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)):
  **a dispatch card may not state a derived number without naming its antecedents, and any bare
  number it does state is marked `UNVERIFIED input`** — because a coordinator-authored number is
  consumed by every lens as established fact, and widening the roster puts more readers in front of
  the same unverified figure.
- **He is the FOUNDER / Tech CEO, and every founder message goes to the whole team before any
  answer** (founder directives 2026-08-12, verbatim in
  [ADR-20260812-143619](docs/adr/ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)):
  no answer is composed and no record lands before the whole roster has been asked. **Carve-outs**:
  an **external-clock fact** (billing suspension, token expiry, partner deadline, opposition window)
  is relayed in the same turn, verbatim from the register, the mob's opinion following; **executing
  an already-recorded rollback/abort path** needs no consult, while going FORWARD through an incident
  is a new decision and does get the mob (*am I executing a recorded path, or inventing one?*);
  **no lens output, and no aggregation of lenses, is legal advice or clearance**; and — a FOURTH,
  added 2026-08-31 by
  [ADR-20260831-204546](docs/adr/ADR-20260831-204546-the-founder-elects-user-invoked-commands-and-direct-question-is-a-fourth-carve-out.md)
  (row `CMD-INVOKE`) — **the founder tagging a message `/direct-question`**, which is him electing
  PER MESSAGE not to spend the fan-out. Unlike the other three it is asked for by no lens and
  predicted by no class, so the skill carries an **escalation duty**: a controlling record the
  question appears to contradict, a `HOLD: human`-axis subject, or *"I do not know and one lens
  would"* means say so and **fan out anyway**. **It skips the MOB, never the REGISTER CHECK.** **Records created
  from a founder directive carry a `Consulted:` block, one line per lens** — a lens never asked is
  indistinguishable from a lens with nothing to say. Historical records keep their vocabulary and
  verbatim quotes stay verbatim. **External artifacts** (mentions légales, partner onboarding,
  filings) must name the capacity the statutes actually confer.
- **Team ownership — sessions start by themselves, the coordinator never authors the diff** (founder
  directive 2026-08-10, verbatim in
  [ADR-20260810-011500](docs/adr/ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)):
  a session begins working WITHOUT asking — CLAUDE.md → STATUS → the **architect** names the next
  chunk from the prioritised backlog → claim → the mob loop, with the **executor** writing EVERY
  phase of the diff (code, specs under recorded approval, records). The lead is a COORDINATOR only:
  briefs, checkpoints, relaying, GitHub mechanics. The only thing brought to the founder is the
  **decision queue** (genuine option spaces, external/legal actions, admin-gated provisioning) with
  options + trade-offs + a recommendation — never "shall I proceed?". Signature: mob evidence in the
  PR body, executor-authored commits, coordinator pushes limited to claim commits and GitHub
  surfaces. The unasked start is always accompanied by a compact action plan shown to the founder
  (chunk, phases, checkpoints, gates, fences, anticipated decisions) as transparency, never a
  permission request ([ADR-20260810-114242](docs/adr/ADR-20260810-114242-loop-start-action-plan.md)).
- **No polling, only pushing — polling is a graceful fallback until pushing works again** (founder
  directive 2026-08-10, verbatim in
  [ADR-20260810-231300](docs/adr/ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)).
  Push is the primary transport for every state change one component must learn from another. A poll
  is legitimate ONLY as a fallback that is (a) a **declared** degraded mode, (b) **observably**
  degraded under a `specs/observability.yaml` contract (`mailbox_push_down_total{reason}` is the
  reference shape) and (c) has a **path back that something actively detects** — a silent fallback
  turns a loud outage into a permanent invisible latency tax. **"Pushing works again" is never
  detected by the absence of an error**: detection must be a **positive liveness proof on the push
  path** (the `mailbox_wake.rs` self-notify canary with a required echo). **Scope**: state-change
  *propagation*, not *time-triggered* work (reminder promotion, TTL expiry, retention sweeps are
  outside it — there the discipline is *sleep until the next due row*). Tiebreaker: **does any
  component know this fact before the clock does?** Applies to the team's own loop: completions
  arrive as push, so never reintroduce a polling status cron. **Carve-out — MONITORING keeps a poll,
  permanently, with no exit**, even where push works: for a monitor silence is ambiguous ("healthy,
  nothing to report" vs "dead, reporting nothing") and it has no durable record to reconcile against.
  **Defect class**: a monitoring path that can only fire when a signal ARRIVES — a threshold alert
  goes quiet exactly when it should scream; liveness needs a dead-man's-switch.
- **Business code (aggregates / pure command handlers) stays independent of the telemetry SDK**;
  instrumentation lives only at framework/middleware boundaries (`c4-l3.yaml` `instrumented`).
  **Every critical workflow has an observability contract** in `specs/observability.yaml`.
- **A business metric IS A PROJECTION** (founder directive 2026-08-11,
  [ADR-20260811-014129](docs/adr/ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md),
  superseding ADR-20260810-234225 in part). Every feature, for every persona, carries at least one;
  the unit is the **persona ACTIVITY** in `specs/stories.yaml`, never the step (a step is an
  operation call, an activity is an outcome). A metric **declares the question it answers** and is a
  **declared `fold:` over `domain_events`** maintained by the `bam` projector into the `bam` schema,
  read through a tenant-scoped GraphQL query — *not* a counter at a call site: a fold **replays**,
  and ratios and distinct-identity denominators are inexpressible as counters. Grouping keys need a
  **declared bounded population**. **Operational telemetry does not move** — latency, error budgets,
  span status, dead-man's switches stay on OTLP/Honeycomb, because they must work when Postgres is
  down. An `alertable:` subset taps a counter as it folds at head. Grammar and open rows:
  [PROP-20260810-234225](docs/proposals/PROP-20260810-234225-business-metrics-for-every-persona.md).
- **Every reference in the DSL is a `$ref`; only a declaration may introduce a bare name** (founder
  directive 2026-08-11, same ADR): **(1)** a **declaration** introduces a name — the only correct
  bare name; **(2)** a **reference** to something declared elsewhere is a `$ref`, **including inside
  the same file**; **(3)** a **value from a closed set** stays a bare token (`type: counter`)
  *provided the set is closed in the loader schema* — **except** where a domain scalar already
  declares that set, where the `$ref` is mandatory (`{ $ref: 'scalars.yaml#/ServiceType' }`, never a
  hand-copied `[DELIVERY, COLLECTION]`); **(4)** **prose stays prose**. Structural, not stylistic: a
  plain-string reference is invisible to the refs walker, so no rule can see it. **Binding on NEW DSL
  surface**; converting existing bare-name sites is separate, sequenced work
  ([DECISIONS §27bis MET-T2](docs/proposals/DECISIONS.md)).
- **If a behaviour test fails, fix the generator/runtime — not the test. If an observability test
  fails, fix instrumentation/middleware — not the domain model.** Gates are executable and
  **blocking**: **never weaken a gate**, and **never hand-edit generated output**
  (`specs/generated/**`, the `database.md` GENERATED region) — change the spec/emitter and regenerate.
- **Completeness is part of every change (ADR-0032)**: a new command/event/error also needs a
  behaviour test (+ its `rules:` link); a new mutation/query also needs a story step; a new business
  rule also needs a test. `make validate` blocks otherwise — extend the specs, never the gate.
- **Independent review before ready-for-review** (founder directive 2026-08-01): a PR is marked ready
  only after a reviewer-agent pass over the FULL branch diff by eyes that did not write it
  (payments, migrations, erasure get the multi-lens fan-out). After any decision that renames or
  reshapes something, grep the OLD term across `specs/**`, `docs/**` and open issue/PR bodies before
  the turn ends. Prefer ONE SESSION PER WORK CHUNK: the repo carries the state, fresh context is
  cheap, long-context error rates are real.
- **A review round ends in a decision, not another round** (founder directive 2026-08-26, verbatim in
  [ADR-20260826-084500](docs/adr/ADR-20260826-084500-one-review-pass-per-presentation-and-findings-are-triaged-not-chased.md)):
  *"I'm worried that we cannot finish the work we are in an infinite loop."* A review always finds
  something — that is what it is for — so review-on-every-push is a cycle with **no terminating
  condition**, and on #679 it ran a night and 114 commits for a one-step deliverable while the last
  four passes each found no blocking defect and the rounds themselves introduced three regressions.
  **A pass fires on PRESENTATION, not on push** — and since the founder retired the CI auto-review
  (2026-08-28, [ADR-20260828-091500](docs/adr/ADR-20260828-091500-the-ci-auto-review-is-retired-the-team-reviews-its-own-work.md):
  *"It cost ai usage for each commit and unnecessary because we are doing the code review
  ourselves"*), the pass IS the team's independent reviewer-agent read of the full branch diff,
  in-session, before ready-for-review — an implementation shift, the review pattern unchanged;
  `@claude` on a PR stays as the founder's on-demand look. A fresh look after a rewrite costs one
  deliberate re-presentation. Findings are
  **triaged, never chased**: blocking (fix here) · non-blocking (one linked issue) · not-a-finding
  (reply, change nothing), and **a PR ships when no BLOCKING finding remains**, never "when the
  reviewer is satisfied". **Three rounds is a CEILING**: at a third, stop and bring the founder what
  shipped, what is open and a recommendation. Procedure and buckets:
  [.claude/skills/review-triage/SKILL.md](.claude/skills/review-triage/SKILL.md). This changes the
  review's CADENCE and never its existence — the rule above stands, `HOLD: human` is untouched, and
  no gate is weakened.
- **Every session records what it learned** (ADR-20260730-034635), in the SAME change as the work —
  not just failures, not only on the second occurrence. Operational findings →
  [docs/claude/sessions.md](docs/claude/sessions.md) or the topic file; decisions → an ADR; option
  spaces → a proposal; durable state → `STATUS.md`; dated status history → the top of the current
  `docs/status/journal-YYYY-Www.md`. **Prefer executable over prose** — a validator rule, test
  or hook beats a bullet, because prose can be ignored and a gate cannot. Record only what is **not
  derivable from the code** and would **cost the next session time**, with the cost that earned it;
  sharpen an existing rule rather than appending a near-duplicate. **Writing nothing is a valid
  outcome.** Every recurring agent/loop failure becomes a rule, test or ADR.
- **Record every substantive change in the same change** — durable state in
  [docs/STATUS.md](docs/STATUS.md), the dated entry at the top of the current
  `docs/status/journal-YYYY-Www.md` — and land cross-cutting **decisions as ADRs in the same
  change** — so concurrent sessions never diverge.
- **Respect the prioritised backlog — and the team now sets it** (founder directive 2026-08-10,
  [ADR-20260810-215503](docs/adr/ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md)):
  priorities live in the GitHub Project "Prioritized backlog" (Priority field + row order) — pick
  from the top; skipping the top item needs a stated reason. Bucket and order are the team's to set;
  the founder may override either at any time, without justification. NOT delegated: genuine option
  spaces, external/legal/admin-gated matters, `specs/**` approval — **a `Priority` is not an
  approval; ranking an AMBER item `Urgent` does not make it dispatchable** — and **the method**, now
  **binding**: [docs/BACKLOG.md](docs/BACKLOG.md) defines value (value-first, ADR-20260720-213024:
  foundations/cross-functional/non-functional first, then features in value-stream order) and every
  ranking must be justifiable under it. **An agent must never change a Priority bucket or row
  position to make an item dispatchable, or its own recommendation legitimate**: a blocked top item
  is reported blocked, never re-ranked. Every bucket change or material row move is stated in the
  architect's run report with the method clause justifying it; a re-ranking that reverses a
  previously stated order also gets a dated line at the top of the current
  `docs/status/journal-YYYY-Www.md` — `STATUS.md` changes only when durable state does.
- **Spec- and docs-only changes go straight to `main`** (founder directive): commit and push directly
  — no branch, no PR, no claim ceremony — for changes confined to `specs/**`, `docs/**`, ADRs,
  `CLAUDE.md`, `STATUS.md` and the artifacts they regenerate. **Keep `main` green**: run `make rust`
  locally before pushing anything touching `specs/**` (a docs-only edit that regenerates nothing may
  skip it). A spec change that moves the warning surface also carries
  `tools/codegen-rs/warning-baseline.json` — part of a spec change's footprint, and it does NOT turn
  the change into code work. The flow below applies to **code/feature work** (`crates/**`, `tools/**`
  other than that artifact, CI, deploy).
- **Issue workflow — claim ⇒ draft PR immediately; finish ⇒ supervised auto-merge**
  (ADR-20260720-233000 + -20260721-042018 + -20260721-044613, method in
  [docs/BACKLOG.md](docs/BACKLOG.md)): FIRST claim the issue (`status/in-progress` label + a claim
  comment naming the `NN-slug` branch and carrying the session link
  `https://claude.ai/code/session_<id>` — the claim predates any commit, so it must be traceable to
  its run), create `NN-slug` from `main`, open a **draft PR** whose body starts with `Closes #NN`.
  **Never enable auto-merge there** — a claim-time diff passes CI trivially. When the work is done
  and `make rust` is green, **the COORDINATOR** — never the executor, which physically cannot: both
  are GraphQL-only mutations and the endpoint is 403-pinned in executor sessions
  ([ADR-20260831-183847](docs/adr/ADR-20260831-183847-the-ready-flip-is-the-coordinators-step-and-always-was.md),
  restoring [ADR-20260810-011500](docs/adr/ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
  §2) — marks the PR **ready for review** and **enables auto-merge together, as one
  indivisible step**, then **supervises the checks until MERGED** (fix + push on failure; never end at
  "pushed, CI pending"). **The executor hands back at green with the PR still in draft.** That
  allocation is fixed; what a dispatch's posture selects is the MERGE CONDITION, not the actor. That
  is the **default posture**; a dispatch marks **`HOLD: human`** for the
  named class — stored event shapes / fold semantics / migrations, payments / funds / erasure, legal
  surfaces, non-additive GraphQL changes, the mailbox runtime, the merge machinery itself — and those
  PRs stop at ready-for-review until the TEAM's independent reviewer pass, never a founder wait;
  after review PASS + green gates the coordinator merges
  ([ADR-20260815-115220](docs/adr/ADR-20260815-115220-auto-merge-on-green-by-default-hold-human-for-the-named-class.md),
  amended by [ADR-20260815-134655](docs/adr/ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md)).
- **Autonomous loops run under the weekly time budget** (`make budgeted-loop` or the routine guard) —
  Claude Code has no native cap; [docs/claude/loops.md](docs/claude/loops.md) / ADR-0014.

## Project status

Live state: [docs/STATUS.md](docs/STATUS.md) — deliberately not duplicated here. Toolchain:
`tools/codegen-rs` (bin `generate`) runs the whole validator + every emitter; `make validate` /
`make generate` / `make rust`; CI's `codegen` gate fails on any spec↔generation drift.
