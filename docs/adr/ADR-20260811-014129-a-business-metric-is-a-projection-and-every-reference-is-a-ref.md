# ADR-20260811-014129 — A business metric is a projection, and every reference in the DSL is a `$ref`

- **Status**: Accepted
- **Date**: 2026-08-11
- **Source**: product-owner directive (verbatim below), confirming the reversal filed as `DECISIONS` §27bis MET-R
- **Supersedes**: [ADR-20260810-234225 "Business metrics for every feature and every persona, developed with the test and the code"](ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md) — **clauses 4 and the enforcement table only**; its clauses 1–3 are carried forward here unchanged and remain in force
- **Realized by**: [PROP-20260810-234225](../proposals/PROP-20260810-234225-business-metrics-for-every-persona.md) D4/D6/D8/D9 · [PROP-20260811-000946](../proposals/PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md) D3/D10 · [#484 "26 of the 29 declared `business_metrics` emit nothing…"](https://github.com/TheCaptainCompany/captain-food/issues/484) · [#485 "Behaviour event tracking has no declaration site…"](https://github.com/TheCaptainCompany/captain-food/issues/485)

---

## The directive

> *"Confirm the reversal, go with the projections"*
>
> *"But we need to heavily strongly typed the spec no string in it"*
>
> — product owner, 2026-08-11

The first sentence closes MET-R. The second is a **new, separate decision** with a wider blast radius
than the metrics work, and it is recorded here because it arrived in the same breath and because it
lands on a real defect in the grammar it was aimed at.

## Decision 1 — a business metric is a projection

**A business metric is a declared fold over `domain_events`, maintained by the `bam` projector into
the `bam` schema, and read through a tenant-scoped GraphQL query.** It is not a counter emitted at a
call site.

Carried forward unchanged from ADR-20260810-234225: the unit is the persona **ACTIVITY** (clause 1);
a metric declares the **question** it answers (clause 3); declaration is enforced like ADR-0032
(clause 2 — and under a fold this is *strengthened*, because the declaration is what runs).

**Reversed:**

| ADR-20260810-234225 | Now |
|---|---|
| Clause 4 — *"attributes are bounded sets, never entity ids"* | **Grouping keys must have a declared BOUNDED POPULATION**, which permits `restaurantId`. A Postgres row is not a time series. The strict enum-only rule survives for the `alertable:` OTLP subset, which really is one |
| Enforcement table — *"instruments generated into `crates/telemetry/src/generated/`"* | **A generated projector and a generated tenant-scoped query.** Compiler-first is unchanged; it now applies to the projector and query types. The source-text scanner is still deleted, for a stronger reason: a fold has no call site to scan |

**Why**, in the order that decided it — the reasoning is in
[PROP-20260810-234225](../proposals/PROP-20260810-234225-business-metrics-for-every-persona.md) §3 D4
and is not repeated here, but the four load-bearing facts are:

1. **Replay.** `crates/infrastructure/tests/orders_placed_metric.rs:129` asserts the counter does
   *not* fire on a rebuild — by design — so a metric added later would carry **zero history**. A fold
   replays the whole log. The repo's own audit standard ("a `View_*` whose restore path is not replay
   is a finding") rejected the design the repo had recommended.
2. **Ratios and distinct-identity denominators are structurally inexpressible** as monotonic
   pre-aggregated counters. Under a fold they are ordinary, and the plain counter becomes one line.
3. **The C4 already said so**: `specs/architecture/c4-l2.yaml:343,370,484` and `c4-l3.yaml:102-105`
   declare `bam` as a projector with a schema in read-models — a schema with **zero tables**
   (`grep -rn bam specs/database/` = 0).
4. **Erasure.** Identity-bearing metrics are personal data either way; in our Postgres they are inside
   the deletion engine's path rather than in a vendor store with no per-subject deletion API.

**Operational telemetry is not affected and does not move.** Latency, error budgets, span status and
dead-man's switches stay on OTLP/Honeycomb, for a reason that is decisive rather than conventional:
**they must keep working when Postgres is down, which is exactly when a Postgres-backed metric is
blind.** Two mechanisms, two questions, split by failure requirement. A named `alertable:` subset taps
an OTLP counter *as it folds at head* — one declaration, two outputs — so
[#483](https://github.com/TheCaptainCompany/captain-food/issues/483)'s dead-man's switch keeps its
signal.

## Decision 2 — every reference in the DSL is a `$ref`; only a declaration may introduce a bare name

The instruction *"heavily strongly typed the spec, no string in it"* is made precise as **three
categories**, because "no strings" read literally would also forbid `description:`, which would be
theatre:

1. **A DECLARATION introduces a name.** `projections: { OrderOutcomes: … }` and
   `measures: [{ name: orders }]` are where those names come into existence. A bare name here is
   correct and is the only place one is.
2. **A REFERENCE to something declared elsewhere MUST be a `$ref` the loader resolves** — including
   references *within the same file*, which the repo already does (`{ $ref: '#/Order/state/orderId' }`,
   `specs/ordering/actors.yaml:102`). `increment: orders`, `groupBy: [day]` and `value: { sum: orders }`
   were bare names pointing at declarations, and are now `$ref`s.
3. **A VALUE from a closed set stays a bare token** (`type: counter`, `bucket: DAY`,
   `lawfulBasis: CONSENT`) **provided the set is closed in the loader schema**, so a typo is a parse
   error. This is categorically different from (2): the loader knows the whole set.
   **But** — where a **domain scalar already declares that set**, the reference is mandatory and an
   inline restatement is refused. `attributes: [{ values: [DELIVERY, COLLECTION] }]` was a verbatim
   copy of `ServiceType` (`specs/common/scalars.yaml:260-262`) and is now
   `{ $ref: 'scalars.yaml#/ServiceType' }`. That is the "one name = one dedicated scalar" convention
   applied to a value set, and it is what stops the tracking spec silently disagreeing with the domain
   the day a third service type is added.
4. **Free prose for humans stays prose.** `description:`, `question:`, `note:`. Typing these would be
   ceremony with no failure mode behind it.

**Why this is not a style preference**, and the repo has the receipt:
[#413 "Validator gap: a plain-string tombstone declaration is invisible to the parser and every rule"](https://github.com/TheCaptainCompany/captain-food/issues/413)
records exactly this failure — `tombstone: SomeEvent` instead of `tombstone: { $ref: … }` is *"silently
invisible everywhere"*, because the refs walker collects only `$ref` nodes, and even the rule written
for that key *"only sees tombstones the parser recognized"*. A bare name is not checked by the loader;
it is checked only if somebody remembered to write a bespoke rule for that one key — and #413 is the
case where somebody did not.

**Scope of this decision.** It is **binding on new DSL surface** — the metrics fold grammar, the
behaviour-event catalog, and anything added after this date. It is **not** a licence to sweep the
existing spec: the bare-name sites that exist today (`data_requirements:`, `actions_used:`, `roles:`)
are each covered by a bespoke validator rule (`screen-unknown-resolver`, `screen-unknown-role`,
`core.rs:1482,1495`) and are therefore checked, if less structurally. Converting them is a separate,
measured piece of work — filed, not done here.

## Consequences

- `ADR-20260810-234225` is **superseded, not rewritten** — it stays as the record of what was decided
  on 2026-08-10 and why, including the reasoning that turned out to be wrong. That is the point of a
  decision record.
- The `CLAUDE.md` business-metrics bullet drops its "under reversal" flag and states the projection
  design plus the `$ref` rule.
- `specs/business_metrics.yaml` does not exist yet; when it lands it lands in the amended grammar, so
  there is no migration.
- **A retention-window catalog becomes load-bearing.** `retention: P90D` as a free string contradicts
  a position already recorded in
  [docs/legal/BRIEF-20260808-account-erasure-two-path.md:82](../legal/BRIEF-20260808-account-erasure-two-path.md)
  — *"This table IS the written retention schedule CNIL expects — windows declared **once, in the
  DSL**, feeding both the sweep and the DPIA."* A free duration lets an author invent a window counsel
  never approved.
- **No event needs a new field for the metrics work.** See PROP-20260810-234225 §3 D8: keying the fold
  at the **entity grain** (`orderId`, which *every* Order event carries) makes the fold total, and the
  grouping happens at read time. The `serviceType` versioning story is withdrawn.

## Refs

- [PROP-20260810-234225](../proposals/PROP-20260810-234225-business-metrics-for-every-persona.md) — D4 (the fork), D6 (bounded population), D8 (the grammar), D9 (the read)
- [PROP-20260811-000946](../proposals/PROP-20260811-000946-behaviour-event-tracking-in-the-screens-spec.md) — D3 (the declaration fields), D10 (`sink:` mutations)
- [ADR-20260803-234035 "Compiler first; a check is the fallback"](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) — a `$ref` the loader resolves is the compiler; a bespoke rule is the fallback
- [ADR-20260810-221840 "specs/** is the team's work"](ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md) — the "does it contradict a recorded decision" test that made MET-R a filed reversal rather than an edit
- [#413 "Validator gap: a plain-string tombstone declaration is invisible to the parser and every rule"](https://github.com/TheCaptainCompany/captain-food/issues/413) — the receipt for Decision 2
- `specs/common/scalars.yaml:260-262` — `ServiceType`, the scalar the tracking spec was restating by hand
- `specs/ordering/actors.yaml:102` — the same-file `$ref` precedent
- `specs/architecture/c4-l2.yaml:343,370,484` · `c4-l3.yaml:102-105` — `bam` as a projector
