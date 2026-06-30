# Captain.Food Playbook — DSL, ADRs, Loops, Observability, and Agent Contracts

> Adapted for this repo from the source playbook `20260628-claude-code-adr-observability-playbook.md`.
> The operating model is unchanged; file paths and artifact names are mapped to Captain.Food's actual
> layout, and each section is annotated with its **status** (✅ in place · 🟡 partial · ⛔ deferred until
> app code exists).

## Purpose

This document is the operating model for the Captain.Food platform: a YAML **DSL** is the source of
truth; code, architecture views, tests, and runtime instrumentation are **generated/derived** from it;
**planning is separate from execution**; and **observability is a first-class contract**, not an
afterthought. It is mounted as project guidance (see `CLAUDE.md` and `docs/claude/*.md`).

## How the playbook maps onto this repo

| Playbook concept | This repo | Status |
|---|---|---|
| `/domain/*.yaml` DSL | `specs/*.yaml` (`scalars`, `entities`, `events`, `commands`, `errors`, `actors`, `views`, `api`, `stories`, `tests`) | ✅ |
| `behaviour-tests.yaml` | `specs/tests.yaml` (Given/When/Then, codegen-validated incl. coverage) | ✅ |
| `user-story-mapping.yaml` | `specs/stories.yaml` (+ `specs/story-map.md`) | ✅ |
| `observability/*.yaml` | `specs/observability.yaml` (contracts, `$ref`-bound to the model) | ✅ |
| `c4-l2.yaml` / `c4-l3.yaml` | `specs/architecture/c4-l2.yaml` / `c4-l3.yaml` (validated DSL) | ✅ |
| `/schemas/*.schema.json` | **codegen referential validation** (`tools/codegen/src/validate.ts`) instead of JSON Schema (see ADR-0002) | ✅ |
| `/generated/**` | `specs/generated/**` (+ injected `specs/database.md` §2) | ✅ |
| generator / reviewer / observability agents | `.claude/agents/*.md` | ✅ |
| Stop / PostToolUse hooks | `.claude/hooks/*.sh` + `.claude/settings.json` | ✅ |
| `Makefile`, `docs/adr/`, `docs/claude/` | same | ✅ |
| OpenTelemetry / K8s probes / BAM runtime | contracts + ADRs authored; **runtime deferred** until `apps/`/`packages/` exist | ⛔ |

## Core operating principles

### 1. YAML DSL is the functional source of truth ✅

`specs/*.yaml` define the business model and contracts: scalars, entities, events, commands, errors,
actors, API, stories, behaviour tests, and observability contracts. Generated code, tests, diagrams,
SQL, GraphQL SDL, and (later) runtime instrumentation are downstream artifacts.

Consequences:

- The DSL is **read-only for autonomous execution loops**.
- Any change to the DSL goes through a **planning step and explicit approval**.
- Generators validate their own output against the model (`npm run validate`).

### 2. Planning and execution are separate modes ✅ (process)

- **Plan mode**: understand current state, analyze impact, classify breaking vs non-breaking, propose,
  wait for approval. (Use Claude Code plan mode.)
- **Execution mode**: apply approved changes, generate, validate, fix implementation defects.

This prevents the execution loop from silently redefining the business model while fixing code.

### 3. Observability is contractual ✅ (spec) / ⛔ (runtime)

Critical workflows declare an explicit contract in `specs/observability.yaml`: required spans, required
identifiers, attributes, success/error semantics, metrics, latency/error budgets. The codegen validates
the contract's shape and its `$ref` bindings to the domain. Runtime emission is deferred until app code.

### 4. Business code stays clean ⛔ (no app yet — ADR-0016 locks the rule in)

OpenTelemetry must not leak into core business logic. Instrumentation belongs in framework, middleware,
bus decorators, event-store adapters, consumers, projections, gateways, and transport layers. Unit
tests for business logic must pass with no telemetry stack enabled.

## Repository structure (actual)

```text
specs/                         # the DSL — source of truth
  scalars.yaml entities.yaml events.yaml commands.yaml errors.yaml
  actors.yaml views.yaml api.yaml stories.yaml tests.yaml
  observability.yaml           # workflow observability contracts
  story-map.md database.md     # narrative + (generated) schema section
  generated/                   # GENERATED (committed; CI verifies vs the specs): docs, views.sql, schema.graphql, c4
  architecture/
    c4-l2.yaml c4-l3.yaml       # C4 as source-managed DSL

tools/codegen/                 # the generator + validator (TypeScript, tsx)
  src/{load,validate,refs,model}.ts  src/emit/*
  out/                         # ephemeral build scratch (gitignored): Structurizr .mmd exports, etc.

docs/
  PLAYBOOK.md                  # this file
  adr/                         # Architecture Decision Records
  claude/{dsl,codegen,observability,c4,adr}.md   # topic operating rules

.claude/
  agents/{generator,reviewer,observability-agent}.md
  hooks/{stop-gate.sh,validate-generated.sh}
  settings.json                # hooks wiring

CLAUDE.md  Makefile
```

## Claude Code memory and rules

Layered memory: `~/.claude/CLAUDE.md` (global habits) · `./CLAUDE.md` (project contract) ·
`./docs/claude/*.md` (topic rules). The project `CLAUDE.md` carries the non-negotiable rules; the topic
files carry conventions for DSL, codegen, observability, C4, and ADRs.

## Recommended agent topology ✅

- **Generator** (`.claude/agents/generator.md`): reads approved DSL, generates artifacts into
  `specs/generated/` (and the `specs/database.md` GENERATED region); never edits the hand-written DSL
  (`specs/*.yaml`, `specs/architecture/*.yaml`).
- **Reviewer** (`.claude/agents/reviewer.md`): validates generated output against the model, behaviour
  tests, observability contracts, and C4; pass/fail with evidence; never rewrites sources.
- **Observability agent** (`.claude/agents/observability-agent.md`): analyzes runs (traces/logs/
  metrics/BAM), detects contract violations, produces diagnoses; never acts on infra directly.

## Loop model

```text
Spec -> Plan -> Execute -> Review -> Validate -> Publish -> Observe -> Learn
```

| Stage | Main actor | Goal |
|---|---|---|
| Spec | Human + Claude (plan) | Define/evolve source artifacts |
| Plan | Claude (read-only) | Impact + migration + breakage analysis |
| Execute | Claude (execution) | Generate code/implementation artifacts |
| Review | Reviewer agent | Contract + consistency review |
| Validate | Hooks + CI + `npm run validate` | Binary, executable gates |
| Publish | Automation + human gate | Commit, PR, release notes, ADR drafts |
| Observe | Observability stack + agent | Runtime + workflow diagnosis |
| Learn | Human + memory update | Persist lessons into rules/ADRs |

Hard rules: Plan mode may propose DSL changes; execution treats the DSL as frozen; night loops may
regenerate and validate but must not redefine business semantics; review is not optional; gates are
binary and executable.

## Hook strategy ✅

- **Stop hook** (`.claude/hooks/stop-gate.sh`): blocks loop completion unless the acceptance gates pass
  (typecheck + `npm run validate`; behaviour/observability/C4 checks run through the same validator;
  app-level checks are skipped gracefully until they exist).
- **PostToolUse hook** (`.claude/hooks/validate-generated.sh`): after a `specs/**` write, re-runs
  validation and returns contextual feedback (not just an exit code), and forbids hand-edits to
  generated output.

See `.claude/settings.json` for wiring.

## DSL contracts and schema strategy

Minimum source artifacts: the `specs/*.yaml` listed above + `specs/observability.yaml` +
`specs/architecture/c4-l{2,3}.yaml`. **Validation** is the codegen's referential integrity + semantic
checks (ADR-0002), which refuses to generate on a broken model. Schema/DSL versioning uses SemVer
(MAJOR breaking · MINOR additive · PATCH tightening). A plan classifies each change as `breaking`,
`backward-compatible`, `generator-only`, `documentation-only`, or `observability-only`.

## C4 strategy ✅

- L2 (`boundedContexts` + `containers`) and L3 (`components`) are source-managed YAML under
  `specs/architecture/`. The codegen emits **Structurizr DSL** (`specs/generated/c4.generated.dsl`) and **Mermaid**
  (`specs/generated/c4.generated.md`); the HTML doc also has a zero-dependency **interactive drill-down map** (§13).
  See ADR-0013 and the `structurizr` skill.
- L4 is manual or tightly supervised; runtime loops must not invent L4.
- **Consistency checks (codegen):** every aggregate is mapped to a bounded context; every container
  `realizes` / component `handles`/`updates` reference resolves to a real actor/view (no phantom).

## OpenTelemetry strategy ⛔ runtime (contract + ADRs in place)

Concentrate instrumentation at framework boundaries, not in domain objects. Three layers:
auto-instrumentation (HTTP/DB/messaging) · framework instrumentation (command bus, event store,
publisher, consumers, projections, GraphQL gateway, BAM projector) · targeted business enrichment
(`business.correlation_id`, `business.command_type`, `business.actor`, `business.aggregate_id`,
`business.result`, `business.event_type`, `business.projection_name`) set only in middleware/decorators.

Required identifiers (ADR-0018): `message_id`, `correlation_id`, `cause_id`, `trace_id`, `span_id`,
`aggregate_id`. `correlation_id` is business-facing and survives the whole causality chain; `trace_id`
is technical and may rotate across async boundaries; `cause_id` links a message to its parent.

## Kubernetes health and probes ⛔ runtime (ADR-0020)

Distinct liveness/readiness endpoints. **startup** protects slow boot; **liveness** detects
deadlock/unrecoverable failure (may restart); **readiness** gates traffic. On SIGTERM: readiness fails
immediately, liveness stays healthy while in-flight work drains, no new work is accepted, then exit.
For event consumers, readiness reflects the minimum dependencies (broker + critical datastore).

## Business observability and BAM ⛔ runtime (ADR-0022/0023)

Keep technical observability (why it's slow/failing) and business observability/BAM (whether business
activity succeeds, and how fast) separate. BAM is one or more **projections** over the same event
stream: commands succeeded/rejected, lead time between milestones, saga compensation rate, backlog,
throughput by tenant/actor/channel/context. BAM dashboards must join to traces via `correlation_id`,
workflow type, actor, and aggregate keys.

## GraphQL query observability ⛔ runtime (ADR-0025/0026)

GraphQL HTTP status does not fully describe business success (200 + `errors[]`). For each operation
collect: `operationName`, `operationType`, `httpStatus`, `hasData`, `errorCount`, `graphql_error_codes`,
`duration_ms`, `trace_id`, `correlation_id`, `actor`, `tenant_id`. Two layers: gateway/ingress
(2xx/4xx/5xx, upstream/timeouts) vs application (operation correctness, error semantics, performance).

## Observability contract — example

The full, validated example for the **place-order** saga lives in `specs/observability.yaml`. It binds
the saga/command/events by `$ref`, declares the mandatory ids, the required spans (with OTel kinds),
metrics vs business_metrics, success/technical_error/business_rejected rules, and SLOs. A `refund`
contract is also defined. A workflow contract turns observability into something testable and
reviewable — a workflow can be asserted to be not only implemented but diagnosable.

## Testing strategy

- **Behaviour tests** (`specs/tests.yaml`): Given/When/Then over the actor model, codegen-validated
  (data shapes, handler, `then ⊆ emits`, `thrown ⊆ throws`, full coverage). ✅
- **Business unit tests** must run with no OpenTelemetry enabled (deferred — no app). ⛔
- **Observability contract tests**: in-memory exporter asserts required spans/attributes/metrics,
  error→tag mapping, propagation across messaging boundaries (deferred — no app). ⛔
- Validation categories: schema (codegen) · behaviour · generator regression · observability contract ·
  C4 consistency · lint/compile/package.

## ADR backlog

See `docs/adr/README.md` for the full index. ADRs recording decisions already realized in this repo are
**Accepted**; the rest (runtime: OTel/K8s/BAM/GraphQL observability, Structurizr generation) are
**Proposed**.

## Suggested initial implementation order

1. ✅ Freeze the minimal DSL (`specs/*.yaml`).
2. ✅ Validation (codegen referential + semantic checks).
3. ✅ Project `CLAUDE.md` + topic rule files.
4. ✅ Generator/reviewer/observability agents.
5. ✅ Stop hook + file-write validation.
6. ✅ Generate Structurizr DSL + Mermaid from C4 YAML.
7. ⛔ OpenTelemetry at framework boundaries (needs `apps/`).
8. ✅ One critical-workflow observability contract (place-order).
9. ⛔ Observability contract tests.
10. ⛔ BAM projection for one workflow.
11. ⛔ GraphQL operation-level logs/traces/metrics.
12. 🟡 First ADR batch (the accepted ones are written).

## Non-negotiable rules summary

- DSL source files (`specs/**`) are never modified by autonomous execution loops.
- Business code remains independent from telemetry SDK calls.
- Every critical workflow must have an observability contract.
- Runtime success requires both technical and business-level observability.
- Review and validation gates must be executable and blocking.
- Every recurring agent failure becomes an explicit rule, test, or ADR.

## Minimal prompts

**Plan mode (DSL evolution):** "Analyze the current `specs/**` (DSL) and `specs/architecture/*` (C4).
Propose a plan for the requested change. Classify each change as breaking, backward-compatible,
generator-only, documentation-only, or observability-only. List impacted model files, validator rules,
generated artifacts, C4 views, ADRs, and migration steps. Do not modify any file."

**Execution mode (generation):** "Use the approved `specs/**` as frozen input. Change generator logic
under `tools/codegen` and regenerate into `out/`. Do not modify `specs/**` semantics. Run `npm run
typecheck && npm run validate && npm run generate`. If validation fails, fix the generator, not the DSL."

**Review:** "Review generated artifacts against the DSL, the validator, behaviour tests, observability
contracts, and C4. Produce a pass/fail report with precise evidence and required corrections."

**Observability agent:** "Analyze workflow runs using traces, logs, metrics, and BAM. Detect violations
of the workflow observability contract in `specs/observability.yaml`. For each incident produce: symptom,
probable root cause, evidence, impact radius, confidence score, recommended next action. Do not act on
infrastructure directly."

## Final note

Without strict boundaries between source-of-truth artifacts, generated artifacts, validation, and
observability, a sleep-running loop becomes a drift-amplifying machine. With those boundaries it becomes
a controlled system that can build, ship, observe, and improve safely over time.
