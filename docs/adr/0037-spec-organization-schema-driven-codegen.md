# ADR-0037 — Spec organization by domain + schema-driven codegen (DB DDL, enum tables)

## Status

Accepted (CTPO, 2026-07-03). Builds on ADR-0034/0035 (Rust codegen, crate layout) and ADR-0005 (read
models). Realized incrementally.

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
  tables.yaml        # REAL tables (domain_events + indexes + trigger bindings, domain_stream)
  views.yaml         # MOVED here — the View_* SQL VIEWS over domain_events (ADR-0035); views, not tables
  functions/*.sql    # one raw .sql per event-store function (ce_events, et_events, all_events, enforce_max_count)
specs/screens/
  {role}_screens.yaml # customer_screens.yaml (already), restaurant_screens.yaml, rider_screens.yaml
```

### 2. Generate the database DDL (stop hand-writing it in `database.md`)
The codegen emits `specs/generated/schema.generated.sql` = `tables.yaml` → `CREATE TABLE` + indexes +
trigger bindings; `views.yaml` → `CREATE OR REPLACE VIEW … FROM domain_events` (ADR-0035); `functions/*.sql`
concatenated in order; plus the enum tables (below) and the retention trigger/cron. `database.md` becomes the
**narrative** that references the generated SQL — the yaml/sql files are the source of truth. `tables.yaml`
is declarative like `views.yaml` (columns with type/pk/unique/nullable/index, explicit indexes, optional
`triggers`/`retention`); column types validate against `scalars.yaml`.

### 3. Enum-as-table
For each `scalars.yaml` enum, generate a lookup table `ref_<enum_snake>(value TEXT PRIMARY KEY, sort_order
INT NOT NULL)` seeded with one row per value — `value` is the verbatim SCREAMING_SNAKE spec value (ADR: enum
1:1 correspondence). `View_*`/tables may FK to these for referential integrity. The Rust `sqlx` migration
**reconciles** rows to the spec on each run (insert new, delete removed, keep `sort_order`) — no manual data
migration when an enum changes.

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
- `views.yaml` move + rekey touches the codegen broadly (parse/validator/emitters) — do it in one pass.
- Enum-table reconciliation must be careful with FKs (delete a value only after dependents migrate).
### Follow-up actions
- ✅ Moved `views.yaml` → `specs/database/`; added `tables.yaml` (real tables, columns may be SQL primitives
  or scalar `$ref`s), `functions/*.sql`, and the `schema.generated.sql` emitter (real tables + `ref_<enum>`
  lookup tables + functions + `$maxCount` trigger). `database.md` §1 is now narrative referencing it.
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
