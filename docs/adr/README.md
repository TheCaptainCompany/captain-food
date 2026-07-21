# Architecture Decision Records

Significant decisions and their rationale. Conventions: `docs/adr/adr.md` (in `docs/claude/`). Template:
[`_template.md`](_template.md). Never delete an ADR — supersede it.

> **ADR identifiers are UTC date-time** (`ADR-YYYYMMDD-HHMMSS`, file `docs/adr/YYYYMMDD-HHMMSS-title.md`)
> so concurrent sessions never collide on a shared counter — see
> [ADR-20260718-135417](20260718-135417-adr-id-scheme-datetime.md). The legacy `0001`–`0047` tables below
> keep their sequential ids; both forms coexist. **New ADRs go in the [date-time section](#newer-decisions-date-time-ids).**

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
| [0029](0029-multi-recipient-tips-model.md) | Multi-recipient tips (`Tip`/`TipOrder`/`OrderTipped`, rider/restaurant/Captain, customer- or restaurant-tipper, additive, separate from the split; realizes kDrive ADR-012) |
| [0030](0030-uber-eats-comparison-model.md) | Uber Eats price-comparison model (single primary `cuisineCategory` + two reference policies, estimate-default with opt-in `basis` for real HubRise prices, `UberComparison` on Offer/Cart/Order computed in projections, no scraping, no new commands/events; realizes 0022/0023/0024/0025) |
| [0031](0031-delivery-bounded-context.md) | Delivery bounded context (`DeliveryJob` aggregate + `DeliveryDispatchProcess`; one lifecycle, two paths — partner INBOUND facts via `avelo37-acl` AND independent-rider commands; PM-emitted `DeliveryRequested`, dual-emitter `OrderDelivered`, `View_DeliveryJob`, RIDER role wired to the context) |
| [0032](0032-business-rules-and-completeness-gates.md) | Business-rules layer (`specs/rules.yaml`) + blocking completeness gates (bidirectional rule↔test linkage; `test-uncovered-*` promoted warning→error; new `op-uncovered-by-story` — every mutation/query anchored to a persona) |
| [0033](0033-spec-driven-sdui-customer-screens.md) | Spec-Driven SDUI (`customer_screens.yaml`) + `translations.yaml` (errors.yaml-style i18n → one `translations.json`); screen reads/writes `$ref`-bound to `api.yaml` (API-meets-UI gate), non-SDUI screens flagged, gaps surfaced; screens+translations rendered in the docs; runtime deferred |
| [0034](0034-full-stack-rust.md) | Full-stack Rust across all platforms (Crux core, Leptos/WASM, Axum BFF, Tauri, UniFFI mobile shells); codegen ported TS→Rust (`tools/codegen-rs`) at parity, then the TypeScript codegen retired. **Amended 2026-07-17**: store apps = **Tauri 2.0 Mobile** hybrid (reuse Leptos SDUI + Crux), storefronts = installable PWAs; UniFFI native shells kept as fallback (SDUI makes the shell swap low-risk) |
| [0035](0035-project-structure-clean-architecture.md) | Clean-Architecture crate layout (`crates/{domain,application,infrastructure,server,shared_types,core,web,desktop}`); serde allowed on domain events/VOs; V0 `View_*` = SQL views over `domain_events` (refines 0005); incremental migrations for stateful tables; ACL/sagas/tenant middleware placed; codegen generation targets defined |
| [0036](0036-domain-topology-single-origin-identity.md) | Domain topology (`{slug}.captain.food` wildcard + `restos/riders/system/api` hosts) & single-origin identity — passkey RP-ID is `captain.food`, so checkout redirects there from restaurant subdomains (extracted from the retired ARCHITECTURE_OVERVIEW.md). **Amended 2026-07-16**: marketing/customer-app split — interim `live.`=app, bare+`www`=marketing; target bare=app, `join.`=marketing; swap plan + reserved-subdomain list |
| [0037](0037-spec-organization-schema-driven-codegen.md) | Specs organized by domain (`specs/database/` = tables/views/functions; `specs/screens/{role}_screens.yaml` with `roles`+`appTypes`); DB DDL generated (not hand-written); enum-as-table `ref_*` lookups reconciled by migration; admin via impersonation; schema-driven codegen as the north star |
| [0038](0038-test-mode-non-production-entities.md) | Test mode — `mode: LIVE\|TEST` on Restaurant/Customer/Order/DeliveryJob; test↔test isolation, excluded from payouts/analytics/notifications; TEST order allowed on a LIVE restaurant (marked, no charge/payout) to validate receipt; test rider runs the real flow safely (Stripe/Uber/HubRise/Deliveroo convention) |
| [0039](0039-projection-views-generated-from-lineage.md) | Projection views generated from event lineage — fold views' `CREATE VIEW` is GENERATED per-column from `from` (scalar-latest / occurrence / occurredWhen / derive + tombstone), killing the set-once hazard by construction; computed read models stay materialized; validator-enforced |
| [0040](0040-materialized-read-model-tables-projectors.md) | Materialized read-model tables — physical form = file (`projection_views.yaml` VIEWs `View_*` vs `tables/projection_tables.yaml` TABLEs, no prefix); `projector: app` = an application-layer (Rust) projector for ALL of them (SQL triggers considered and rejected — business logic stays out of the DB, event-append never coupled to a projection) |
| [0041](0041-acting-user-is-envelope-not-payload.md) | The acting user is envelope metadata (`domain_events.user_id`/`user_type`), not payload — removed `createdBy`/`updatedBy`/`changedBy`/`claimedBy`/`acceptedBy`/`cancelledBy` from events + commands; a business ROLE (e.g. `Tipper`) stays; aggregates keep `createdBy` reconstructed from the envelope |
| [0042](0042-hosting-render-supabase-frankfurt.md) | Hosting: app on **Render** (Frankfurt) + DB/Auth on **Supabase** (Frankfurt, `eu-central-1`) — co-located in the EU for GDPR + low app↔DB latency; PaaS over self-managed K8s (reinterprets P-04's probe contract as Render health-check + SIGTERM drain); portable Axum container, Supavisor pooling |
| [0043](0043-db-migration-release-strategy.md) | DB migration & release strategy — sqlx migrator (`migrations/NNNN_*.sql`, `_sqlx_migrations`, checksummed, append-only) run as Render Pre-Deploy; **expand/contract** (additive-first, cleanup-later; rollback = redeploy previous app); `/health` is a strict readiness gate that embeds the app's migration set and **names missing versioned migrations** (`>=` not `==`, so rollback holds) |
| [0044](0044-coopyleft-license.md) | Licensing — **Captain.Food Coopyleft** (`LICENSE.md`): adopts AGPL v3 but reserves commercial use to social-and-solidarity-economy co-ops/non-profits (CoopCycle-style); un-relicensable copyleft. Source-available, **not** OSI open source; crates use `license-file` (no SPDX id). Needs legal review + contributor terms |
| [0045](0045-sirene-sync-staging-table-and-worker.md) | SIRENE sync — **raw-ingestion staging table `external_sirene_restaurants` + on-app `sync_sirene_worker`** (refines 0019/0020/0027): CI only fetches→UPSERTs raw rows then pings; the ACL+aggregate run on the **deployed** version (kills the CI↔prod version-skew) via a projector-style checkpointed drain. Deletion reuses existing `MarkRestaurantClosed`/`RestaurantMarkedClosed` — detect-by-absence + explicit `F` state, debounced, auto for NON_PARTNER but manual-review for partners; never hard-delete; SIRET-change dedup out of scope. Supersedes the interim direct-write `sirene_sync`; realization is a follow-up |
| [0046](0046-graphql-write-side-command-handlers.md) | GraphQL **write side** — generated `MutationRoot` (one `<Name>Payload`+resolver per api.yaml mutation) over thin CQRS command handlers. Invariant strategy: **rehydrate the aggregate from its event stream** (`EventStore::load` + a pure domain `fold`) for intra-aggregate/state rules (authoritative, no read-lag race), **read-model ports** for cross-aggregate/uniqueness (best-effort), inline for value checks; optimistic-concurrency append at the loaded version (creation idempotent). Every invariant ↔ `errors.yaml` + a `tests.yaml` behaviour test (0032). Fail-closed Google seams (0019/0021). Interim: `Invariant("<Code>: …")` string errors; anonymous PUBLIC actor until the ACL/authn lands (0006). Restaurant aggregate wired; others stubbed |
| [0047](0047-api-auth-supabase-jwt-jwks.md) | API auth & role authz — verify **Supabase JWT via JWKS** (`SUPABASE_JWKS_URL`, public keys, no shared secret) at the `/{role}/graphql` boundary; realizes the deferred ADR-0006 guard over ADR-0015. Business role from `app_metadata.captain_role` (absent ⇒ CUSTOMER); `/public` open, others require a matching role (401/403); fail-closed if JWKS down; asymmetric-only (no HS* `alg`-confusion). First-cut **path** guard shipped in `crates/server` (`auth.rs`); per-field `@auth` + EXTERNAL service tokens are follow-ups |

## Newer decisions (date-time ids)

New ADRs use a UTC date-time id (ADR-20260718-135417) and are appended here, newest last.

| ADR | Title |
|---|---|
| [20260718-135417](20260718-135417-adr-id-scheme-datetime.md) | ADR identifiers are **date-time based** (`ADR-YYYYMMDD-HHMMSS`) — kills the shared-counter collision when concurrent sessions each grab "the next number" (as 0046 was double-allocated 2026-07-17); legacy `0001`–`0047` keep their sequential ids, both coexist |
| [20260718-145856](20260718-145856-inbound-webhook-integration-acl.md) | **Inbound webhook integrations** (Stripe/HubRise) via dedicated REST endpoints (`POST /adapters/{partner}/webhooks`, NOT `/external/graphql`) + **partner-specific auth** (Stripe signature, HubRise token) + **ACL → inbound events** appended in-process, idempotent. `/external/graphql` + `X-External-Api-Key` is the distinct *pull* path (ADR-0047) |
| [20260718-213352](20260718-213352-partner-adapter-crates-and-external-m2m.md) | **Partner adapters = self-contained crates** (`crates/adapters/{stripe,hubrise}`): ACL + axum shell + standalone binary each, mountable into the monolith or **deployable as their own web service** (amends 0035, moved out of `infrastructure`). And **`/external/graphql` = the M2M standard** for external entities to query/mutate/(subscribe later), API-key auth (`X-External-Api-Key`, 0047). Two directions: partner-push webhooks vs external-drive API |
| [20260719-120000](20260719-120000-structured-domain-rejections-graphql-error-contract.md) | **Structured domain rejections** — `DomainError::Rejected { code, context }` (errors.yaml PascalCase code + typed JSON context) replaces the interim `"Code: detail"` string; a generated error catalog owns the wire code + `{placeholder}` en/fr messages; GraphQL maps → `extensions.code` + interpolated message (P-10). Realizes the ADR-0046 follow-up |
| [20260719-014434](20260719-014434-checkout-snapshot-on-paymentintentcreated.md) | **Checkout snapshot on `PaymentIntentCreated`** — the event carries a required `CheckoutSnapshot` frozen by `place_order`, so `OrderPlaced`/`CartCheckedOut` rebuild from the log alone (no external store); non-projected ⇒ zero view impact. Priced items/breakdown + retiring `CheckoutSnapshotSource` ride on server-side pricing |
| [20260719-031136](20260719-031136-write-side-repository-event-sourced-actor.md) | **Write-side `Repository` over the event-store journal** — `domain::Aggregate` (identity + `fold`) + `application::Repository` (`load`/`require`/`events`/`save`/`create`); all 64 handlers + the `ProcessManagerRunner` route through it, never the raw `EventStore`. Functional event-sourced actors; loads are write-side (own stream), never read models. Refines ADR-0035/0046 |
| [20260720-015300](20260720-015300-command-journal-sourcing.md) | **Command sourcing** — every write request persists a `command_journal` row (pk `message_id`, envelope columns, business payload + hash, `RECEIVED→SUCCEEDED\|REJECTED\|FAILED`) BEFORE handling; records rejections; idempotent replay by `message_id`+hash (mismatch = Conflict); `domain_events.cause_id = message_id`; never writes/replays as the event log. Amends 0046, extends 0041, realizes P-02 `message_id` |
| [20260720-015400](20260720-015400-inbound-event-sourcing-adapter-staging.md) | **Inbound event sourcing** — adapters own raw `external_*` staging tables (`external_stripe_events`, `external_hubrise_callbacks`; verify → UPSERT → ACK) and hand off ONLY adapted business events via `inbound_events` (`RECEIVED→DELIVERED\|FAILED`, unique (source, external_id)), drained through the normal write path (`cause_id = inbound_event_id`; aggregate fold stays the authoritative dedupe). HubRise yields commands, not inbound events. Amends 20260718-145856, generalizes 0045 |
| [20260720-015500](20260720-015500-acceptance-first-graphql-envelope.md) | **Acceptance-first GraphQL writes** — ALL mutations async: optional `metadata: MetadataInput` (messageId/correlationId/causeId) + uniform `MutationAcceptance` (effective envelope incl. sessionId/traceId + operationStatus + duplicate); `X-SESSION-ID` header carries the anonymous session; outcomes via PUBLIC ownership-scoped `operationStatus(messageId)` + `operationStatusChanged(messageId)` (`Operation.errorCode` = the async rejection contract); checkout reads move to `paymentStatus(orderId)` (+Changed) off the payment PM row (clientSecret persisted there, nulled on resolve). Amends 0046, 20260719-120000, 20260719-193500 |
| [20260720-213024](20260720-213024-value-first-issue-prioritisation.md) | **Value-first issue ordering** (product-owner directive) — the backlog queue is ordered by value, not effort: tier 1 = foundations & cross-functional/non-functional (contracts, ACL, invariants, observability, retention, the codegen wave), tier 2 = features in value-stream order (customer ordering → restaurant onboarding → delivery). Priorities are defined in the GitHub Project "Prioritized backlog" (Priority = value bucket, Effort = size); the repo records the method (`docs/BACKLOG.md`). Amends 20260720-143000 §4 (sizing/pre-task docs unchanged) |
| [20260721-025159](20260721-025159-journal-mirror-retention-policy.md) | **Journal/mirror retention policy** (#18) — windows live in ONE SQL function, `sweep_retention()`: `command_journal` terminal rows 90 d, `inbound_events` DELIVERED rows 30 d (FAILED kept until resolved), `external_stripe_events`/`external_hubrise_callbacks` processed rows 90 d (the PII cap on verbatim payloads). NEVER swept: `domain_events`/`domain_stream` (forever log), RECEIVED journal rows, unprocessed mirror rows, `external_sirene_restaurants`. Scheduled by the in-process `RetentionSweepWorker` (6 h, `RUN_RETENTION_SWEEP`) or `pg_cron`. Closes the retention follow-ups of 20260720-015300/-015400 |
| [20260721-042018](20260721-042018-claim-time-draft-pr-automerge-supervision.md) | **Claim-time draft PR + supervised auto-merge** (product-owner directive) — claiming an issue = label + claim comment + `NN-slug` branch + an immediate **draft PR** (`Closes #NN`), so issue↔branch↔PR link before any code (draft = the auto-merge interlock); completion = gates green → ready → enable auto-merge → **supervise checks until MERGED**. Records the auto-merge threat model: per-PR arming needs write access (outsider/fork PRs can't arm or merge), the load-bearing config is the `main` ruleset's required `codegen` check. Amends 20260720-233000 |

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
