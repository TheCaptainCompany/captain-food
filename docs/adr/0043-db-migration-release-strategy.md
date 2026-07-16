# ADR-0043 — Database migration & release strategy (expand/contract + schema-version readiness gate)

## Status
Accepted

## Context
Production needs a way to evolve the Postgres schema safely, with **zero-downtime deploys** and **rollback**,
on the Render + Supabase (Frankfurt) target (ADR-0042). Two forces shape the design:

- The **codegen emits the full schema** (`specs/generated/schema.generated.sql` + `views.generated.sql`),
  but production cannot `DROP`+`CREATE` the event store — it needs **incremental** deltas (already
  anticipated by ADR-0035).
- The write side is an **append-only event log** and the read side is **projections** (ADR-0005/0039/0040):
  `View_*` are stateless (regenerate freely), materialized projection tables are **rebuildable from events**.
  So the genuinely "hard migration" surface is small: `domain_events`/`domain_stream`, `ref_*` lookups, and
  any stateful table.

We also want a **mis-orchestrated deploy** (app pointed at an un-migrated or wrong-version DB) to be unable
to take the site down.

## Decision

**1. sqlx migrator, files at the repo root.** Migrations are `migrations/NNNN_description.sql`, applied by
sqlx's own migrator (per-file, checksummed, recorded in the append-only **`_sqlx_migrations`** ledger with a
`success` flag; concurrent runs serialize on a Postgres advisory lock). The set is **embedded once** in
`server::MIGRATOR` and shared by the migrate bin and the readiness gate.

**2. Migrations run in Render's Pre-Deploy Command** (`./target/release/migrate`) — *before* the server
starts. The bin exits non-zero on any failure, so a bad migration **blocks the deploy**: the new server is
never promoted and the previous version keeps serving.

*Interim (free tier — Pre-Deploy unavailable):* the server applies migrations **at startup**
(`run_migrations_if_enabled`, gated by `MIGRATE_ON_START`, default on). sqlx's migrator takes a Postgres
advisory lock, so this is safe even with multiple instances; the server serves regardless of the outcome so
`/health` reports the true schema state (a failed migration is held back by the health check, not a
crash-loop). Switch to Pre-Deploy on a paid instance by setting `MIGRATE_ON_START=false`.

**3. Expand/Contract (parallel change).** Schema changes are **additive** during the expand phase
(add columns/objects; never rename/drop/alter in place). Destructive **cleanup is a later forward
migration**, shipped only after the app version that needed the old shape is retired. Rollback is therefore
**"redeploy the previous app"**, not a down-script — the old columns are still there.

**4. Schema-version readiness gate (`/health`).** The server **embeds its migration set** and gates
readiness on *every embedded migration being present-and-successful* in `_sqlx_migrations`. `/health` is
`200` only then; otherwise `503`, and when behind it **names the missing migrations** (`version_description`,
e.g. `0005_view_order_v5`) so the failure is diagnosable at a glance. Because a build requires only **its
own** embedded set, an **older app still passes against a newer DB** — the gate is effectively `>=`, never
`==`, which is what preserves rollback-by-redeploy. (An exact-match gate would make every deploy a
non-rollback-able lockstep — the classic mistake.)

**5. The ledger is append-only.** Never delete rows from `_sqlx_migrations`: sqlx treats an absent version
as *pending* and would re-run it, and it validates checksums of applied ones. "Removing an old version's
support" is achieved by shipping the **contract migration** (a new forward version) and retiring the old
app — not by deleting history.

**6. Naming convention.** Name each migration after the object it introduces, version-suffixing objects
whose structure is versioned (e.g. `0005_view_order_v5.sql` → view `View_Order_v5`), so the `/health`
"missing" output points straight at the absent SQL object. This dovetails with accessing tables **through
versioned views** (ADR-0005): the view is the stable contract; the underlying column change is expand/contract.

**7. Codegen reconciliation.** The generated full schema is the **target/baseline**. `0001_baseline.sql`
marks version 1 with no schema change; the first real domain-schema migration (`0002+`) bootstraps from
`specs/generated/*.sql` once test-applied. `View_*` changes are ordinary forward migrations
(`CREATE OR REPLACE`); event-store/stateful changes are additive.

## Alternatives considered
- **A custom `__DatabaseChangeLog` table** (Liquibase-style, hand-rolled migrator): considered and briefly
  prototyped, then dropped in favour of sqlx's built-in `_sqlx_migrations` — same per-file, checksummed,
  `success`-flagged model with far less code and battle-tested advisory locking.
- **Exact-version gate (`db == app`)**: rejected — kills rollback-by-redeploy (see decision 4).
- **Migrate at app startup** (in-process, every instance): rejected — races across instances; the Pre-Deploy
  step runs once. Advisory locking makes it safe either way, but pre-deploy keeps app boot fast.
- **Separate DB repo**: rejected (ADR-0042 discussion) — schema and app must version together in one commit
  so rollback is a single redeploy.
- **Supabase GitHub auto-deploy of SQL**: not used for now — migrations are owned by our own pipeline
  (Pre-Deploy) for control; can be revisited.

## Consequences
### Positive
- Zero-downtime + real rollback (redeploy previous app); bad migrations can't promote a broken deploy.
- `/health` names the exact missing migration/object → fast diagnosis.
- Small hard-migration surface thanks to the event-sourced/projection design.

### Negative
- Requires discipline: additive-first, cleanup-later; migrations must be transaction-safe (no
  `CREATE INDEX CONCURRENTLY` inside the default per-file transaction without a `no_tx` migration).
- Two artifacts to keep coherent: generated full schema vs. incremental migrations (a future validator can
  diff them).

### Follow-up actions
- Set Render's **Pre-Deploy Command** to `./target/release/migrate`.
- Author `0002` = the domain schema, derived from `specs/generated/*.sql`, once test-applied against Supabase.
- Consider a codegen check that the migrations reproduce the generated schema.

## References
Refines ADR-0035 (incremental migrations for stateful tables); builds on ADR-0042 (Render Pre-Deploy +
health/probe), ADR-0005/0039/0040 (projections), ADR-0006 (GraphQL contract as the client-facing version).
