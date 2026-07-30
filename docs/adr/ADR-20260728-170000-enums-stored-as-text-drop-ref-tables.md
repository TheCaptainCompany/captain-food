# ADR-20260728-170000 — Enum columns store the TEXT value verbatim; the `ref_<enum>` lookup tables are dropped

## Status

Accepted (product-owner directive, 2026-07-28) — supersedes the enum-storage rule of ADR-0037.

## Context

ADR-0037 stored every enum-typed column as its compact INTEGER declaration-order ordinal, backed by
one generated `ref_<enum>(sort_order, value)` lookup table per `scalars.yaml` enum. In practice the
ordinals proved painful to work with:

- Rows are not self-describing: every manual query, log excerpt, or support session needs a join (or
  a memorized mapping) to know what `status = 2` means — and the answer differs per enum.
- Declaration order became a **frozen storage contract**: values had to be APPENDED forever, retired
  values had to keep their slot (`DeliveryDispatchProcessStatus::FAILED` occupying
  `REOFFER_REQUIRED`'s ordinal), and a well-meaning re-ordering of `scalars.yaml` would silently
  reinterpret every stored row (the `InboundEventStatus` D6 note exists purely to warn about this).
- The ordinal↔value mapping was duplicated in four places that had to agree: the generated seeds,
  `infrastructure::persistence::enum_sql`, hand-written SQL literals in workers/tests, and the
  fold-view CASE expressions — several `EXTERNAL_USER_TYPE: i32 = 6` constants existed only to
  encode one enum member.
- The compactness win is negligible at V0 scale (and Postgres TEXT of a short ASCII token is cheap).

The product owner asked to remove the ref tables and store the text directly.

## Decision

Enum-typed columns (a `scalars.yaml` enum reached via `$ref`, or a projection column whose lineage
is an enum) are stored as **TEXT holding the `scalars.yaml` value verbatim** (`'PLACED'`,
`'EXTERNAL'`, …). Concretely:

- The codegen maps enum scalars to `TEXT` in every table DDL, stops emitting `ref_<enum>` tables,
  and folds views by taking the payload's text value directly (a status derived from an event TYPE
  emits the text literal, validated against the enum).
- The envelope's `user_type` (`domain_events`, `command_journal`, `Actor`, `RequestEnvelope`)
  travels as the UserType TEXT value end to end.
- `infrastructure::persistence::enum_sql` maps enum ↔ TEXT (`EnumText::to_text`/`from_text`); the
  stored string IS the variant name, so reads still fail loudly on an unknown value.
- The conversion ships as the `20260730043000`–`20260730043600` migration set: compact the SIRENE
  mirror first (`VACUUM FULL`, reclaiming the dead payload space), then one transaction per table
  group with the ordinal→value CASE folded into `ALTER … USING` (a single rewrite per table, no
  UPDATE pass), the biggest tables (`restaurant`, `inbound_events`, `command_journal`,
  `domain_events`) each alone, `sweep_retention()` replaced with text predicates, every
  `ref_<enum>` table dropped, and the fold views recreated from the regenerated SQL last.
  (The original single-file `20260728170000_enum_text_storage.sql` rewrote every table in one
  transaction and died on production's 2 GB disk — "no space left on device" — rolling back
  cleanly; it was retired unapplied and replaced by the split set.)
- Declaration order in `scalars.yaml` is no longer a storage contract. **Renaming** a value is now
  the migration-worthy event (a data `UPDATE` plus the code change); adding or reordering values is
  free.

No CHECK constraints are added: writes go through the typed domain enums (serde/`EnumText`), reads
validate on decode, and a CHECK would reintroduce a schema migration for every enum extension —
the exact churn this change removes.

## Alternatives considered

- **Keep ordinals + ref tables (status quo)** — compact, index-friendly; but rows stay opaque, the
  declaration order stays frozen forever, and the mapping stays quadruplicated. Rejected by the
  product owner as too painful to operate.
- **Native Postgres `ENUM` types** — self-describing and compact; but `ALTER TYPE … ADD VALUE` has
  transactional quirks, values cannot be dropped/renamed without a rewrite, and every enum change
  becomes DDL. Strictly more coupling than TEXT for no operational win at this scale.
- **TEXT + CHECK (value IN (…))** — guards against garbage from ad-hoc SQL; but every enum
  extension becomes a migration again. The write path is typed end to end, so the marginal safety
  does not pay for the churn.

## Consequences

### Positive
- Every row is self-describing — `SELECT status FROM ordertracking` answers without a join.
- `scalars.yaml` order is documentation again, not a storage format; the append-only warnings and
  slot-keeping hacks are retired.
- One mapping (`EnumText`, generated from the same variant names) instead of four; the
  `EXTERNAL_USER_TYPE: i32 = 6` constants and ordinal CASE folds are gone.
- Hand-written SQL (workers, tests, retention) reads as business language (`status = 'FAILED'`).

### Negative
- Enum columns are a few bytes wider and compare as text (irrelevant at V0 volumes; indexes on
  `(restaurant_id, status, …)` behave the same).
- `ORDER BY <enum column>` is now alphabetical, not declaration-order (the one consumer that sorted
  by `cuisine_category` only needs a stable order).
- Renaming an enum value now requires a data migration (previously free while the ordinal stayed).

### Follow-up actions
- None open — codegen, crates, tests, docs and the conversion migration land together in this change.
