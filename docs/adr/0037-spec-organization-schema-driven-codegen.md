# ADR-0037 — Spec organization by domain + schema-driven codegen (DB DDL, enum tables)

## Status

Accepted (CTPO, 2026-07-03). Builds on ADR-0034/0035 (Rust codegen, crate layout) and ADR-0005 (read
models). Realized incrementally. **Refined by ADR-20260722-091500** — SDUI screens are named by
**audience** without the `_screens` suffix (`restaurant_frontoffice.yaml` etc.), and §4's "no ADMIN
screen set" is relaxed to allow a future `system` screen set.

## Context

The flat `specs/` folder grew and the codegen hard-codes each file's shape (it *knows* `scalars.yaml` has
enums, `views.yaml` has columns, `customer_screens.yaml` has resolvers…). Two gaps followed: the event-store
schema (tables, functions, triggers, retention) was **hand-written SQL in `database.md`** — not generated,
so not a single source of truth — and the SDUI screens were a single hard-coded `customer_screens.yaml`.
The CTPO wants specs grouped by domain, the DB schema **generated** from declarative specs, and the
generator driven by **declared per-file schemas** rather than baked-in specifics.

## Decision

### 1. Organize specs by domain
```
specs/database/
  tables/              # REAL tables — one yaml per family, globbed by the codegen
    eventstore.yaml    #   domain_events (+ indexes + trigger bindings) + domain_stream (retention caps)
    referential.yaml   #   seed/config tables (View_PhoneCountry/PricingPolicy/Uber*Policy) — configured
                       #   once by a repo seed script, NOT projected from domain_events; keep View_* names
                       #   because queries read them (api.yaml `reads`)
  projection_views.yaml # (was views.yaml) — ONLY the event-fed read models: View_* SQL VIEWS / materialized
                       #   projections over domain_events (ADR-0005/0035)
  functions/*.sql      # one raw .sql per event-store function (ce_events, et_events, all_events, enforce_max_count)
specs/screens/
  {role}_screens.yaml # customer_screens.yaml (already), restaurant_screens.yaml, rider_screens.yaml
```
Naming reflects the reality: `projection_views.yaml` is *derived* (event → column lineage); the `tables/`
folder is *authoritative state* (event store + seeded reference data). Reference data is a table configured
once, not a projection — so it lives under `tables/`, not among the views.

### 2. Generate the database DDL (stop hand-writing it in `database.md`)
The codegen emits `specs/generated/schema.generated.sql` = every `tables/*.yaml` → `CREATE TABLE` + indexes +
trigger bindings; `projection_views.yaml` → `CREATE OR REPLACE VIEW … FROM domain_events` (ADR-0035, where a
`definition:` is given) else materialized `CREATE TABLE`; `functions/*.sql` concatenated in order; plus the
enum tables (below) and the retention trigger/cron. `database.md` becomes the **narrative** that references
the generated SQL — the yaml/sql files are the source of truth. `tables/*.yaml` are declarative like the
views (columns with type/pk/unique/nullable/index, explicit indexes, optional `triggers`/`retention`); column
types validate against `scalars.yaml`. Referential `View_*` tables are valid `reads` targets: the validator's
read-binding check accepts a `View_*` from either `projection_views.yaml` or `tables/*.yaml`.

### 3. Enum-as-table — integer-keyed, enums stored as their ordinal
For each `scalars.yaml` enum, generate an **integer-keyed** lookup table
`ref_<enum_snake>(sort_order INT PRIMARY KEY, value TEXT NOT NULL UNIQUE)` seeded with one row per value —
`value` is the verbatim SCREAMING_SNAKE spec value (ADR: enum 1:1 correspondence), `sort_order` is its
declaration ordinal. **By principle, an enum column is ALWAYS stored as its `INTEGER` ordinal** (that
`sort_order`), never the TEXT value — a ref table always exists to resolve it, so `sql_type(<enum>)` →
`INTEGER` uniformly (tables and fold views alike; a fold view maps the payload's TEXT enum to its ordinal
via a generated `CASE`). No FK constraint is emitted (kept lean; the ref table is available for joins/UI).
Because the ordinal is **persisted** (incl. in the append-only `domain_events` log, e.g. `user_type`), enum
values must stay **append-only** — never reordered/renumbered. The Rust `sqlx` migration **reconciles**
rows to the spec on each run (insert new, delete removed, keep each value's `sort_order` stable) — no manual
data migration when an enum grows. (Read-model Rust row types keep the enum newtype; the INTEGER↔enum
mapping is a deferred `sqlx` concern.)

### 4. Screens are role-tagged, admin uses impersonation
Each screen declares `roles: [...]` ⊆ {PUBLIC, CUSTOMER, RESTAURANT, RIDER} and `appTypes: [web, ios,
android, windows]`. A screen may be `[PUBLIC, CUSTOMER]`, so the customer app serves anonymous + signed-in
screens with **no separate public app**; the role=path ACL (ADR-0006) gates per role. There is **no ADMIN
screen set** — admin is a back-office plus **impersonation** ("view as", scoped/time-boxed/audited token for
a target role's path), the industry-standard pattern (Stripe Sudo, Shopify staff login-as). The codegen
globs `specs/screens/*.yaml` and validates/renders them generically.

### 5. Schema-driven generator (north star)
Remove hard-coded per-file specificity incrementally: each spec file declares its kind/shape and the
generator dispatches off that declaration. Applied first to the database + screens work above; existing
emitters converge onto the declarative model over time.

## Alternatives considered
- **Keep DB DDL hand-written in `database.md`**: rejected — violates "spec is the source of truth"; drifts.
- **Postgres native `enum` types / free-text**: rejected in favour of `ref_*` tables — FK integrity, queryable, label/order-able, migration-reconcilable.
- **A 4th ADMIN screen set / ADMIN in every screen's roles**: rejected — duplication + weak audit; impersonation is cleaner and standard.

## Consequences
### Positive
- One source of truth for the whole DB schema; `database.md` stops drifting; enum integrity via FKs.
- Screens handle PUBLIC/CUSTOMER overlap and multi-platform without app duplication.
- Generator gets progressively more declarative → new spec files need less bespoke code.
### Negative / risks
- `views.yaml` → `projection_views.yaml` move + the `tables/` glob + rekey touch the codegen broadly
  (parse/validator/emitters) — done in one pass; the `reads`-binding check had to union projection views
  with referential-table `View_*`.
- Enum-table reconciliation must be careful with FKs (delete a value only after dependents migrate).
### Follow-up actions
- ✅ Moved `views.yaml` → `specs/database/projection_views.yaml` (renamed for what it is); the real tables
  live under a globbed `specs/database/tables/` folder: `eventstore.yaml` (domain_events + domain_stream) and
  `referential.yaml` (seed/config `View_*` tables — configured once by a repo seed script, not projected).
  Columns may be SQL primitives or scalar `$ref`s. Added `functions/*.sql` and the `schema.generated.sql`
  emitter (real tables + `ref_<enum>` lookup tables + functions + `$maxCount` trigger, per-column `index`).
  `database.md` §1 is now narrative referencing it.
- ✅ Screens `roles` (⊆ UserType) + `app_types` (⊆ web/ios/android/windows): added to `customer_screens.yaml`
  and validated; §11 is generic over `screens/*.yaml` (no hard-coded `customer_screens`).
- ✅ Recorded test mode as ADR-0038.
- ⬜ **Remaining**: generalize the two *docs emitters* (MD + HTML) to render all `screens/*.yaml` files
  (deferred — only needed once a 2nd screens file exists; must stay byte-identical for the single-file case).
- ⬜ Enum-table row **reconciliation** in the `sqlx` migration (insert/update/delete on enum change) — lands
  with the migration generator (once `crates/` migrations exist).
- ⬜ Realize ADR-0038 (test mode) in the domain specs.

## References
Stripe test-mode/Sudo, HubRise sandbox, EventStoreDB stream metadata. Complements ADR-0005/0006/0034/0035.
