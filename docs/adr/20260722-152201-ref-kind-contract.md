# ADR-20260722-152201 — Ref-KIND contract: every `$ref` site declares what it may point at

## Status

Accepted (product-owner directive, 2026-07-22). Extends **ADR-0002** (the validator is our schema) and
the per-site checks introduced by **ADR-20260719-…** (process-manager typed steps),
**ADR-20260722-101500** (`screen-ref-out-of-scope`) and **ADR-0032** (completeness is part of a change).

## Context

`$ref` is how every spec file binds to another: an actor's inbox to `commands.yaml`, a projection column
to an event's property, a process manager to its state table, a screen resolver to a query. Until now the
validator's §1 only proved that a ref **resolves**. Whether it resolved to the **right kind of thing** was
checked ad hoc, wherever someone had written a check:

- `actors.yaml` messages/emits/throws, `guard.throws`, `deliver.event`, `send.to`, screen
  `resolvers`/`actions`, test `rules` — all had hand-written per-site checks.
- `processmanager.yaml` `state_table` was checked only as *"the ref starts with `database/tables/`"* — it
  would have accepted a referential seed table, a journal, a staging mirror or a read-model table as a
  process manager's private state row.
- Nothing checked that a ref site was covered **at all**: adding a new ref-carrying field to the DSL got
  resolve-only validation, silently and forever.

A resolving-but-wrong ref is not a cosmetic problem: the codegen reads these refs to emit SQL, GraphQL SDL,
Rust types and orchestrator scaffolding, so a wrong kind produces confidently wrong generated code.

## Decision

Add **§1b — the ref-kind contract** to the validator (`tools/codegen-rs`), immediately after §1's
referential integrity.

1. **Kinds are finer than files.** A `Kind` classifier maps `(target file, pointer path, node)` to what the
   target *is*, using intra-file discriminators, not just the filename:
   - `database/tables/*`: process-manager state table vs projection table vs referential vs journal vs
     staging vs connection vs event-store table — and any `…/columns/<c>` is a table column;
   - `scalars.yaml`: enum scalar (has `enum`) vs plain scalar;
   - `api.yaml`: query vs mutation vs subscription vs output type vs input type;
   - `actors.yaml` aggregate vs `processmanager.yaml` process manager;
   - `commands.yaml`: a **command** when an actor or process manager receives it, otherwise a
     **payload object** — a shared payload sub-object such as `CartLine` (this mirrors §3's existing
     value-object derivation, and makes "this mutation dispatches something no actor handles" an error).
2. **Every ref site declares its allowed kinds** in one table, `REF_CONTRACT`: `(source-file glob,
   ref-site glob, allowed kinds)`. The site is the ref's location inside its file
   (`*.receives[*].steps[*].read.model`), matched with `*` (one segment) / `**` (any depth).
3. **Fail-closed.** A `$ref` whose site has no contract entry is an **error** (`ref-site-undeclared`),
   reported once per site shape with the concrete location and a suggested contract line. A new
   ref-carrying DSL field therefore cannot land without declaring what it may point at.
4. **The contract lives in the validator, not in `specs/**`.** It is a meta-rule *about* the DSL — a gate,
   like the rest of the validator — and keeping it next to the classifier that reads it means the two
   cannot drift. It is covered by unit tests, not by the spec's own completeness rules.
5. New rules: `ref-kind` (resolves, wrong kind), `ref-kind-unknown` (resolves to something with no
   declared kind), `ref-site-undeclared` (site not covered). All are **errors**.

The existing per-site checks stay: they are more specific (they also build the sets §2–§11 reason over),
and a genuinely wrong ref simply reports twice.

## Consequences

- The kind of every one of the ~3.5k `$ref`s in `specs/**` is now proven, not assumed. `make validate`
  stays **0 errors** (the 26 known view/design-hole warnings are unchanged).
- Two deliberate widenings were recorded rather than silently allowed, because the refs are correct:
  - `services.yaml` `*.operations.*.input.*` may be an **event** — `delivery.offer_job` hands the adapter
    the `DeliveryRequested` birth fact that carries pickup/dropoff, rather than a parallel entity that
    would drift from it;
  - `screens/*.yaml` uses ordered entries (resolver → query, action → mutation, `subscription` →
    subscription) and a final catch-all: every other ref in the free-form UI tree is a translation key —
    which is exactly what `screen-ref-out-of-scope` (ADR-20260722-101500) already asserted.
- Adding a ref-carrying field to any spec now has a second, mechanical step: declare its kinds. That is the
  intended cost — it is what keeps the guarantee exhaustive instead of decaying.
- Kind granularity is limited by what the DSL actually distinguishes. `entities.yaml` has no
  aggregate-vs-value-object discriminator (everything is `type: object`), so all of it classifies as
  `entity`; if that distinction ever matters at a ref site, the DSL needs the discriminator first.
