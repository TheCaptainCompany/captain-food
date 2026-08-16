# Claude rules — observability

Observability is a **contract**, declared in `specs/observability.yaml` and validated by the codegen.
Runtime emission is **deferred until app code exists** (`apps/`/`packages/`), but the contract and rules
are authoritative now.

## Where instrumentation lives (and does NOT)

- **Yes**: GraphQL gateway, command bus, event-store adapter, event publisher, message consumers,
  projection updaters, BAM projector, HubRise ACL, Stripe adapter, middleware (see
  `specs/architecture/c4-l3.yaml` — components with `instrumented: true`).
- **No**: aggregates / pure command handlers (`instrumented: false`). Business unit tests must pass with
  no telemetry stack enabled (ADR-0016).

## Three instrumentation layers

1. Auto-instrumentation: inbound/outbound HTTP, DB, messaging, framework.
2. Framework instrumentation: command bus, event store, publisher, consumer, projection updater,
   GraphQL gateway, BAM projector — where business context is attached to technical spans.
3. Targeted business enrichment (set ONLY in middleware/decorators/adapters): `business.correlation_id`,
   `business.command_type`, `business.actor`, `business.aggregate_id`, `business.result`,
   `business.event_type`, `business.projection_name`.

## Required identifiers (ADR-0018)

`message_id`, `correlation_id`, `cause_id`, `trace_id`, `span_id`, `aggregate_id`.
- `correlation_id` — business-facing, survives the whole causality chain.
- `trace_id` — technical, may rotate across long async boundaries.
- `cause_id` — links a message to its parent; `message_id` uniquely identifies each emitted message.

## Contract shape (`specs/observability.yaml`)

Each critical workflow declares: `workflow` ($ref bindings to saga/command/events — OR a dispatch
`surface: graphql` for a PIPELINE contract binding a whole dispatch surface instead of one
command/saga/aggregate, mutually exclusive with the $ref bindings; ADR-20260721-031127),
`run_identity` (must include `correlation_id` + `trace_id`), `spans` (each with an OTel `kind` in
SERVER|CLIENT|INTERNAL|PRODUCER|CONSUMER and required attributes), `metrics` vs `business_metrics`,
`status_rules` (success | technical_error | business_rejected; `success.required_spans ⊆` declared
spans), and `latency_budget` / `error_budget`. The codegen enforces all of the above.

The `command-acceptance` contract is the surface-bound instance: it instruments the acceptance-first
write pipeline (ADR-20260720-015500) — spans `command.receive`/`command.journal`/`command.dispatch`,
ids `message_id`/`correlation_id`/`trace_id`, metrics `commands_accepted_total{channel}`,
`command_duplicates_total{channel}`, `command_sync_conflicts_total{command_type}`,
`command_completion_ms{status}` (REJECTED/FAILED split). Its latency budget binds the synchronous
acceptance path only; async completion is watched via `command_completion_ms`.

## BAM and GraphQL (runtime, deferred)

- BAM = projections over the same event stream; keep business vs technical observability separate;
  dashboards join to traces via `correlation_id` + workflow/actor/aggregate keys.
- GraphQL: HTTP 200 can still carry `errors[]`. Per operation collect `operationName`, `operationType`,
  `httpStatus`, `hasData`, `errorCount`, `graphql_error_codes`, `duration_ms`, `trace_id`,
  `correlation_id`, `actor`, `tenant_id`. Monitor gateway and application layers separately.

## Rule

If an observability contract test fails, fix instrumentation/middleware — **not the domain model**.

## Reading production telemetry — Honeycomb MCP (moved from CLAUDE.md, 2026-08-01)

Traces/metrics go to **Honeycomb EU (`eu1`)** — a GDPR constraint, not a default
(ADR-20260729-183000: spans carry `customerId`/`orderId`; ADR-0042 pinned data to Frankfurt).
The MCP server is declared in `.mcp.json`, pinned to `https://mcp.eu1.honeycomb.io/mcp`.

**The server is currently DISABLED, on purpose** — `disabledMcpjsonServers: ["honeycomb"]` in
`.claude/settings.json` (ADR-20260816-020752 §11): unauthenticated, its tools cannot run, and no
`apps/` runtime emits spans yet, so it is "not a blinded instrument, it is a broken one" and its
tool list is pure per-session context cost. **Re-auth is the event that re-enables it**: authorize,
then delete that array entry. The definition stays in `.mcp.json` precisely so the `eu1` pin below
is not lost with the server config — and this disablement is recorded in CLAUDE.md too, because
"no Honeycomb server" must never read as "no telemetry concern".

**The region is the trap**: the `honeycomb` plugin ships the US default (`mcp.honeycomb.io`),
and US/EU are separate tenancies — authorizing the US host SUCCEEDS and then returns an empty
environment list, which reads as a broken integration rather than a wrong region. The
project-scoped `.mcp.json` overrides that for everyone; do not "fix" it back.

Auth is per-user OAuth on first use (no secret in the repo); it needs an INTERACTIVE session and
**Honeycomb Intelligence** on the team (an empty tool list after clean auth is usually that
add-on). Headless alternative: a **Management** API key (`<Key ID>:<Secret Key>`, scopes
`Model Context Protocol` + `Environments` read) — the ingest key the app uses to SEND telemetry
cannot read it back.

Query discipline: `get_workspace_context` FIRST; discover fields with `find_columns` /
`get_dataset_columns`; human-readable time ranges ("last 2 hours"), never epoch; name the
environment and dataset in every query; prefer percentiles and `HEATMAP` over `AVG` (an average
hides the Friday/Saturday 19:00-21:30 tail — a P99 checkout regression is invisible in a flat
mean); correlate by `correlation_id`/`trace_id` — the write path is acceptance-first
(ADR-20260720-015500), so a command's interesting half runs AFTER the mutation answered PENDING;
filtering to the GraphQL span alone shows the accept, not the outcome.

