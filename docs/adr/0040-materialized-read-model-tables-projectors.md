# ADR-0040 — Materialized read-model tables + projector mechanism

## Status

Accepted (CTPO, 2026-07-06). Extends ADR-0039 (fold generator) and ADR-0005 (read models as projections).
Realized incrementally.

## Context

ADR-0039 split read models into generated **fold views** and **materialized** ones (computed columns, or
folds the generator can't yet express). It left two loose ends: the materialized ones still lived in
`projection_views.yaml` (a file of "views" that emitted `CREATE TABLE`s — misleading), and *how* they get
filled was undefined. The CTPO asked for them to be real tables under `tables/`, filled automatically by a
projector, ideally a SQL trigger on `domain_events` (one per projected table) where possible.

## Decision

### 1. Physical form is decided by file (drop the `strategy` field)
- `specs/database/projection_views.yaml` — read models realized as SQL **VIEWS** (the generated folds).
  Named `View_*`.
- `specs/database/tables/projection_tables.yaml` — read models realized as materialized **TABLES**.
  **No `View_*` prefix** (they are real tables). Same metadata as a view (aggregate/slice/fedBy/rules/
  columns with `from` lineage) so the validator still proves API↔read-model both ways.

Naming convention, now an invariant: **`View_*` = a database VIEW; an unprefixed name = a TABLE.** The
codegen emits fold views into `views.generated.sql` and projection tables into `schema.generated.sql`
(column types resolved from the `from` lineage). The validator enforces it (`view-naming`).

### 2. `projector: trigger | app` — how a table is maintained
Business logic must not leak into the database, and the append to `domain_events` (the source of truth)
must never be blocked by a read-model projection. So the mechanism is split by the nature of the columns:

- **`trigger`** — the projection is a **mechanical** incremental fold (scalar-latest / status-from-type /
  jsonb accumulate / cross-stream lookup). The codegen GENERATES an `AFTER INSERT` trigger on
  `domain_events` (UPSERT keyed by PK, `DELETE` on tombstone) **plus a `rebuild_<table>()` function** that
  replays the log in `position` order through the same body (schema/logic changes → rebuild). No
  hand-written SQL, so no business logic and low failure risk. Applies to `Restaurant`, `Customer`.
- **`app`** — columns are **COMPUTED** by business rules (pricing split with clamping, category-tree
  assembly, weighted score, Uber/tip comparison) that must stay in the tested domain/application layer
  (`crates/application`), not in plpgsql. Declared as a **deferred runtime contract** (the projector is
  built when `crates/` lands). Applies to `Cart`, `Catalog`, `OrderTracking`, `ProspectionPipeline`.

### 3. Guardrails
- No business logic in a write-path trigger; a projection error must never abort the event append.
- Every `trigger` table also gets a rebuild function (triggers only cover new inserts).
- N per-table triggers on `domain_events` is fine for a handful of read models; revisit with a single
  dispatch trigger if the count grows.

## Alternatives considered
- **plpgsql triggers for ALL read models** (fully DB-resident V0): rejected for the computed ones —
  untestable with the behaviour harness, duplicates domain logic (drift), violates the dependency rule,
  and couples the event-store write to projection correctness.
- **A Rust projector for everything, no triggers**: viable but throws away the strong-consistency,
  zero-infra win for the mechanical folds, which the generator can produce correctly for free.
- **Keep materialized read models in `projection_views.yaml`**: rejected — a "views" file emitting tables
  is misleading; file = physical form is clearer.

## Consequences
### Positive
- Each generated artifact matches its file; the `View_*`/unprefixed convention is unambiguous.
- Mechanical projections are generated, strongly consistent, and need no separate process.
- Business logic stays testable and in the domain; the event-store write path stays uncoupled.
### Negative / risks
- The trigger generator (Stage 2) must cover cross-stream + jsonb-accumulate to actually fill
  `Restaurant`/`Customer`; until then those tables are declared but unfilled.
- Generated trigger SQL isn't executed against a real Postgres by the validator (deferred with `crates/`).

## References
Extends ADR-0039; refines ADR-0005/0035 #2. Builds on the `tables/` folder from ADR-0037.
