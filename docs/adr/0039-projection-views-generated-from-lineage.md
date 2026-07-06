# ADR-0039 — Projection views generated from event lineage (fold generator)

## Status

Accepted (CTPO, 2026-07-06). Refines ADR-0005 (read models as projections) and ADR-0035 #2 (V0 `View_*`
as SQL views over `domain_events`). Realized incrementally.

## Context

ADR-0035 #2 says a V0 read model is a SQL view over the append-only `domain_events` log. The first one
(`View_RestaurantAccount`) was hand-written — and immediately grew a subtle bug: a naive
`DISTINCT ON (stream) ORDER BY position DESC` fold takes the *latest event's whole payload* for every
column, so **set-once** fields (`default_currency`, `ref` — present only on the creation event, not carried
by later `*Updated` events) went stale/null after any update. The CTPO caught it with a `#TODO`: "be
careful about this kind of situation." Hand-writing one intricate fold per view reproduces that hazard
N times, and the specs already carry the information needed to avoid it — each column declares its
per-event `from` lineage.

At the same time, not every read model is a plain fold: some columns are **computed** (pricing splits,
a category tree, a prospection score, tip sums, the Uber comparison) by application logic that must live
in the tested domain/application layer, not smuggled into SQL.

## Decision

### 1. Each projection view declares a `strategy`
`fold` (default) or `materialized`. A `materialized` view emits a `CREATE TABLE` fed by a projector and
must state `materializedReason`. A `fold` view carries **no hand-written SQL** — the codegen GENERATES its
`CREATE OR REPLACE VIEW` from the per-column `from` lineage. The validator enforces the contract:
`view-fold-ungeneratable` (a fold view whose SQL cannot be generated) and `view-materialized-no-reason`.

### 2. Correct-by-construction fold generation
The generator sources **each column from ITS own lineage**, so the set-once hazard is impossible by
design. Column modes (inferred from `from`, or declared):
- **scalar-latest** (default): the newest event carrying the column's property, scoped to the declared
  carrying event types **and** the property key — a JSON key shared by an unrelated event can't win.
  Set-once fields (property only on the creation event) are read straight from the creation row.
- **occurrence**: a `timestamptz` whose `from` are whole events → `max(occurred_at)`.
- **occurredWhen**: conditional occurrence — `max(occurred_at)` over events matching any
  `{ event, whenPayload? }` clause (e.g. `delivered_at` only when `DeliveryStatusUpdated=DELIVERED`).
- **derive**: status-from-event-type — the latest matching lifecycle event maps to a literal enum value
  or a payload-extracted one.
The row's existence is anchored on the **creation event** (the one carrying the PK); a `tombstone` event
drops the row.

### 3. Applied incrementally
- **Stage A (this ADR):** generator + `View_RestaurantAccount` (drops its hand-written SQL) and
  `View_DeliveryJob` (exercises `derive` + `occurredWhen`). The 4 genuinely computed views
  (`View_Cart`, `View_OrderTracking`, `View_Catalog`, `View_ProspectionPipeline`) → `materialized` with a
  reason; `View_Restaurant`/`View_Customer` → `materialized` pending the modes below.
- **Stage B/C (superseded by ADR-0040):** `Restaurant` (cross-stream currency) and `Customer` (jsonb
  accumulate) turned out cleaner as **incremental materialized tables** than as read-time views — so they
  moved to `tables/projection_tables.yaml` with `projector: trigger` (generated write-time fold) rather
  than extending the read-time fold generator. See ADR-0040.

## Alternatives considered
- **Hand-write every fold**: rejected — repeats the set-once hazard once per view; the lineage already
  encodes the correct per-column source.
- **Express the computed views as SQL too**: rejected — pricing/tree/score/tips arithmetic is business
  logic that belongs in the tested domain/application layer, not a `SELECT`.
- **One materialized projector for everything (skip views)**: deferred, not rejected — ADR-0035 #2 already
  allows a hot fold view to become a materialized table later with no query-API change; `strategy` is the
  switch.

## Consequences
### Positive
- The set-once class of bug is eliminated by construction; folds are derived, not authored.
- "No definition" is no longer ambiguous — every view declares `fold` or `materialized` (+ why).
- Adding a foldable column needs only its `from`/mode, not SQL.
### Negative / risks
- The generator's mode vocabulary must grow to cover real views (cross-stream, accumulate — Stages B/C);
  until then those views are honestly `materialized`.
- Generated SQL is not executed by the validator against a real Postgres — correctness rests on the mode
  semantics + review until an integration harness exists (deferred with `crates/`).

## References
Refines ADR-0005, ADR-0035 #2, ADR-0037. Set-once lesson captured as the `projection-view-set-once-fields`
guardrail.
