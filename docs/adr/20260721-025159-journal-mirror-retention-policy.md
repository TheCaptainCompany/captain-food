# ADR-20260721-025159 — Retention policy for write-path journals and adapter webhook mirrors

## Status

Accepted — closes the "retention policy" follow-up actions of ADR-20260720-015300 (command
journal) and ADR-20260720-015400 (inbound events + `external_*` mirrors). Extends the
`$maxAge` sweep pattern of `specs/database.md` §1 to the journal/staging table categories.
Realizes issue #18.

## Context

`command_journal`, `inbound_events` and the verbatim webhook mirrors (`external_stripe_events`,
`external_hubrise_callbacks`) grow **unbounded**: every checkout, webhook and worker tick adds
rows, and nothing ever deletes them. Unlike `domain_events` — the append-only forever log that
keeps projections rebuildable (ADR-0005) — these tables have a **usefulness window**: a journal
row backs an `operationStatus` poll measured in minutes and a support lookup measured in weeks;
a delivered inbound row is only a delivery receipt once its fact is in the log; a mirror row
exists for replay/backfill. On the managed Postgres low tier this is a slow-motion disk
incident, the journal idempotency lookup sits on the hot write path, and the mirrors retain
**raw webhook payloads** (Stripe payloads embed customer payment metadata) indefinitely — a
GDPR storage-limitation exposure (Art. 5(1)(e)) for a French consumer product.

## Decision

One SQL function — **`sweep_retention()`**, declared as source in
`specs/database/functions/sweep_retention.sql` and assembled into the generated schema — is the
single place the windows live. It deletes, per table:

| Table | Swept | Window (aged from) | Never swept |
|---|---|---|---|
| `command_journal` | terminal rows (`SUCCEEDED`/`REJECTED`/`FAILED`) | **90 days** (`completed_at`) | `RECEIVED` rows, at any age |
| `inbound_events` | `DELIVERED` rows | **30 days** (`delivered_at`) | `FAILED` (kept until resolved) and `RECEIVED` (pending work) rows |
| `external_stripe_events` | processed rows (`processed_at` set) | **90 days** (`processed_at`) | unprocessed rows (`processed_at IS NULL`) |
| `external_hubrise_callbacks` | processed rows (`processed_at` set) | **90 days** (`processed_at`) | unprocessed rows |

Out of scope, permanently:

- **`domain_events` / `domain_stream` are NEVER touched** by this sweep. The event log's only
  trimming remains the opt-in per-stream `$maxAge`/`$maxCount` machinery (ADR-0005,
  `specs/database.md` §1) — this function does not even reference the table.
- **`external_sirene_restaurants`** is exempt: it is a full mirror whose detect-by-absence
  deletion semantics (ADR-0045) require the complete row set, and it holds company (not
  consumer) data.

### Why these windows

- **`command_journal` 90 days** — the top of the 30–90 band proposed in #18: a journal row is
  the **only** trace of a rejected command (ADR-20260720-015300), so it serves support/audit and
  payment-dispute reconstruction well past the minutes-scale `operationStatus` window; one row
  per command keeps 90 days of volume modest. `RECEIVED` rows are never age-swept here — the
  existing stale-`RECEIVED` sweep (10 min, drain-worker tick) marks crashed runs `FAILED`
  first, and those then age out through the terminal window.
- **`inbound_events` 30 days** — once `DELIVERED`, the business fact lives forever in
  `domain_events` with `cause_id = inbound_event_id`; the row itself is only short-term delivery
  forensics. After deletion that `cause_id` dangles by design (cause ids are envelope pointers,
  not foreign keys); the raw mirror still holds the payload for 60 more days for deeper
  archaeology.
- **Mirrors 90 days** — longer than Stripe's own 30-day resend window, so we remain our own
  replay source well past the provider's; projections rebuild from `domain_events`, never from
  mirrors, so replay is the mirrors' only long-tail use. 90 days is also the PII cap (below).

### Mechanism

- The **windows are encoded once**, in `sweep_retention()` (status names resolved through the
  `ref_*` enum lookup tables, not hard-coded ordinals). The function returns
  `(swept_table, deleted)` per table for observability.
- **Scheduling**: the in-process **`RetentionSweepWorker`** calls the function every **6 hours**
  (first pass at boot), env-gated `RUN_RETENTION_SWEEP` (default on) like the other workers, and
  logs a summary line per non-empty pass. A `pg_cron` nightly
  `SELECT * FROM sweep_retention();` is the documented alternative for environments that prefer
  DB-side scheduling — either way the deletion logic (and the windows) stay in the one function.
- The tables' YAML specs (`journals.yaml`, `integration_staging.yaml`) carry a documentary
  `retention:` block pointing here, so the DSL reader sees the policy next to the columns.

### PII governance note

The mirrors store **verbatim** webhook payloads: Stripe events embed customer payment metadata
(names, partial card data, addresses in `metadata`/`charges`), HubRise callbacks embed catalog
and account contact data. The 90-day cap is therefore not only disk hygiene but the
storage-limitation bound for raw third-party payloads: past the replay window there is no
purpose justifying retention. Domain events keep **business fields only** (ADR-0041 envelope
discipline), and `command_journal.payload` is the business command; their PII (customer
addresses on orders) is governed by the event log's own (separate, future) GDPR
erasure strategy — out of scope here.

## Alternatives considered

- **`pg_cron` as the primary mechanism** — rejected for now: the managed tier's availability of
  the extension is not guaranteed, the app already operates four in-process workers with the
  same env-gate pattern, and a worker tick is testable in CI with a plain Postgres. The
  function-based design makes switching to `pg_cron` a one-line schedule, not a rewrite.
- **Windows declared per-table in YAML and codegen-emitting the sweep** — rejected: each table's
  predicate differs in kind (terminal status set vs single status vs high-water-mark column), so
  the YAML would grow a predicate mini-language for four rows. The YAML keeps a documentary
  `retention:` block; the executable truth is the one SQL function.
- **`DELETE` from the app in Rust per table** — rejected: four statements' windows would drift
  from the documented policy across code sites; the function keeps policy atomic and callable
  from any scheduler.
- **Partitioning by month + `DROP PARTITION`** — over-engineering at V0 volumes; revisit if
  `DELETE` sweeps ever show up in the metrics (#16).

## Consequences

### Positive

- Journals and mirrors are bounded; the journal idempotency lookup stops degrading with age.
- Raw-payload PII has a defined, enforced storage limit (GDPR storage limitation).
- `FAILED` inbound rows can never be silently reaped — unresolved failures stay visible until
  an operator resolves them (retry → `DELIVERED`, which then ages out).
- The forever-log guarantee is executable: the DB-gated test proves a sweep pass leaves
  `domain_events`, pending/`FAILED` rows and the SIRENE mirror untouched.

### Negative

- A delivered `inbound_events` row older than 30 days no longer resolves its
  `domain_events.cause_id` pointer (accepted — documented above).
- Replay from mirrors is only possible within 90 days; a backfill discovered later must
  re-request data from the provider (Stripe: gone after 30 days on their side too).
- The windows are constants in a SQL function — changing them is a migration
  (`CREATE OR REPLACE FUNCTION`), not a config flip. Acceptable: retention is policy, and
  policy changes should leave a migration trail.

### Follow-up actions

- Sanity-check the windows against #16's table-size/latency metrics once they exist.
- The event log's own GDPR **erasure** strategy (customer PII inside business payloads —
  crypto-shredding or payload redaction) is a separate, larger decision; this ADR only bounds
  the raw third-party copies.
