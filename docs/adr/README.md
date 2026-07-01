# Architecture Decision Records

Significant decisions and their rationale. Conventions: `docs/adr/adr.md` (in `docs/claude/`). Template:
[`_template.md`](_template.md). Never delete an ADR — supersede it.

**0001–0015** are technical / operating-model decisions (realized in the repo). **0016–0026** are
**product/business** decisions from the June 2026 sessions (`Status: Proposed` — recorded now, realized
in the DSL in later phases).

The full **Nov–Dec 2025 decision history** (six successive visions — crypto co-op → marketplace →
Tours-focus → white-label SaaS → growth-led product — each restarting the ADR numbering) is integrated
in **[HISTORY.md](HISTORY.md)**: a status-annotated catalog of every historical ADR with its original
source/ID/date and whether it is Active / Superseded / Obsolete / Deferred relative to the decisions
below. The numbered ADRs here are the source of truth for **what holds today**; HISTORY.md is the
lineage behind them.

## Accepted (realized in the repo)

| ADR | Title |
|---|---|
| [0001](0001-dsl-yaml-as-source-of-truth.md) | YAML DSL (`specs/**`) as the functional source of truth |
| [0002](0002-codegen-referential-validation.md) | Codegen referential validation as the DSL schema mechanism (instead of JSON Schema) |
| [0003](0003-semver-per-dsl-file.md) | SemVer versioning per DSL file |
| [0004](0004-commands-derived-from-use-cases.md) | Commands derived from use cases (not one-per-event) |
| [0005](0005-read-models-as-projections.md) | Read side served by `View_*` projections, never the event log |
| [0006](0006-graphql-role-as-path-acl.md) | GraphQL "role = path" ACL with generated SDL |
| [0007](0007-behaviour-tests-in-dsl.md) | Behaviour tests embedded in the DSL with a full-coverage gate |
| [0008](0008-c4-as-source-managed-dsl.md) | C4 L2/L3 as source-managed, validated DSL |
| [0009](0009-observability-contracts-in-dsl.md) | Observability contracts embedded in the DSL |
| [0010](0010-plan-execute-separation-and-gates.md) | Plan/execute separation + validation-gated loop (Stop hook) |
| [0011](0011-agent-topology.md) | Agent topology: generator / reviewer / observability (separation of duties) |
| [0012](0012-domain-infra-observability-separation.md) | Domain / infrastructure / observability separation (no OTel in aggregates) |
| [0013](0013-structurizr-mermaid-c4-generation.md) | Structurizr DSL + Mermaid as generated C4 targets |
| [0014](0014-weekly-loop-budget.md) | Weekly time budget for autonomous loops (loop-state guard) |
| [0015](0015-wrap-supabase-auth-behind-graphql.md) | Wrap Supabase Auth behind GraphQL (synchronous, effect-only auth commands) |

## Product / business decisions (June 2026 — `Proposed`, realized in later DSL phases)

| ADR | Title |
|---|---|
| [0016](0016-proportional-captain-service-fee.md) | Proportional Captain service fee (0% commission on food) |
| [0017](0017-3way-stripe-connect-split.md) | 3-way Stripe Connect split with proportional service fee |
| [0018](0018-transparent-checkout-fee-display.md) | Transparent service-fee display at checkout |
| [0019](0019-restaurant-pre-registration-sirene-google.md) | Restaurant pre-registration via INSEE Sirene + Google Maps |
| [0020](0020-restaurant-sync-cron-prospection.md) | Automated restaurant sync cron + B2B prospection scoring |
| [0021](0021-google-business-profile-order-button.md) | Google Business Profile "Order online" button (**Accepted**) |
| [0022](0022-uber-eats-price-comparison.md) | Uber Eats price comparison in the client (product + cart) |
| [0023](0023-uber-eats-real-prices-hubrise-optin.md) | Real Uber Eats prices via HubRise (restaurant opt-in) |
| [0024](0024-uber-eats-price-estimation.md) | Standardized Uber Eats price estimation (no real data) |
| [0025](0025-amount-split-pedagogical-display.md) | Pedagogical "who gets what" amount-split display |
| [0026](0026-ai-automation-tooling.md) | AI automation tooling: Perplexity Computer + Claude Code |

## Design decisions (realized in the DSL)

Concrete architecture/domain-model decisions taken while building the specs (Accepted; reflected in
`specs/**`). New decisions are recorded here as they are made.

| ADR | Title |
|---|---|
| [0027](0027-restaurant-pre-registration-prospection-model.md) | Restaurant pre-registration & prospection domain model (extend `Restaurant`, generic `externalIdentifiers`, dual-source ACLs, score in read model; rejects a separate `RestaurantDirectory` BC — dark kitchens) |
| [0028](0028-pricing-3way-split-model.md) | Pricing & 3-way split model (`PaymentBreakdown` VO, reference `View_PricingPolicy`, restaurant `marginRate`, breakdown on OrderPlaced/Cart/Order; realizes 0016/0017/0018) |

## Proposed (deferred until app/runtime code exists)

These record decisions whose **rules** are locked in (CLAUDE.md, `docs/claude/observability.md`,
`specs/observability.yaml`) but whose **implementation** waits for `apps/`/`packages/`. Promote to a
numbered Accepted ADR when realized.

- **P-01** OpenTelemetry as the standard telemetry layer (traces, metrics, logs; profiling later).
- **P-02** Identifier propagation contract: `message_id`, `correlation_id`, `cause_id`, `trace_id`,
  `span_id`, `aggregate_id`.
- **P-03** Framework-level instrumentation for CQRS/Event-Sourcing (bus, store, publisher, consumer,
  projection, gateway, BAM).
- **P-04** Kubernetes probe contract (startup/liveness/readiness + SIGTERM drain semantics).
- **P-05** Structured JSON logging everywhere, correlated by the identifier contract.
- **P-06** BAM as an event-sourced projection over the same event stream.
- **P-07** Separation of business observability (BAM) and technical observability.
- **P-08** Workflow-specific SLOs (latency/error budgets per observability contract).
- **P-09** GraphQL operation-level observability (operation/type/errorCount/duration/correlation).
- **P-10** GraphQL error contract: HTTP status vs `errors[]` (200-with-errors).
- **P-12** L4 generation under strong supervision.
