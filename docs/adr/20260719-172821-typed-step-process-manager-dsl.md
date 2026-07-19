# ADR-20260719-172821 — Process managers as state-table orchestrators with a typed step DSL

## Status

Accepted

## Context

The process-manager re-architecture (branch WIP, see `docs/process-manager-rearchitecture.md`) had
settled three principles — adapters never write `domain_events`, aggregates own the facts, process
managers orchestrate rather than emit — but expressed each PM's behaviour as free-text `steps`
(comments in YAML). That representation could not be validated, could not generate the sequence
diagrams we align layers with (`docs/sagas.md` was hand-drawn and drifting), repeated the
lock/unlock runtime mechanics in every leg, and forced the plan to *exempt* PMs from the ADR-0032
completeness gate because nothing in a free-text step is checkable.

## Decision

`specs/processmanager.yaml` becomes a typed, code-generation-grade DSL. Each process manager
declares:

- `state_table` — a `$ref` to a **declared private table** (`database/tables/process_managers.yaml`,
  `$ref`-typed columns; process-status columns use dedicated enum scalars).
- `ports` — the outbound port operations it may call (application traits; adapters implement them).
- `receives[]` — one leg per message, each an **ordered list of typed steps** with a closed
  vocabulary mapping 1:1 to a layer: `read` (read model), `guard` (pure decision), `call` (port →
  adapter), `deliver` (hand a fact to the aggregate that records it), `send` (dispatch a command the
  aggregate may reject), `state` (own row: `by` / `expect` / `set` with **explicit columns**).
- Step values are typed: `{ const: ENUM }`, `{ from: $ref …/properties/… }` (projection-lineage
  style), `from_state`, `from_read`, `from_port`, `from_envelope`.

Semantic rules, enforced by the validator (§2b in `tools/codegen-rs`):

- **Errors always throw; only benign alternatives skip.** Every guard declares exactly one outcome.
  `throws` (a typed `errors.yaml` error) marks an ERROR — on a command leg the command is rejected;
  on an event leg the recorded fact stands (facts are never rejected) but the run ABORTS and SURFACES
  the typed error (poison/ops flag) instead of continuing. `skip: true` is reserved for benign
  expected alternatives (idempotent re-delivery, COLLECTION no-op — a failed `state.expect` is such a
  skip) and never for error conditions. A command leg has no benign-skip path: its guards only throw.
  Examples: an orphan Stripe capture/failure throws `PaymentEventOrphaned`; a partner report for an
  unknown dispatch run throws `DeliveryJobNotFound`.
- Every `deliver`/`send` target must actually receive that message per `actors.yaml` (single wiring
  truth); every column, port operation, alias and enum const must exist.
- A PM's `emits`/`throws` are **derived** from its steps (delivered events ∪ the emits of sent
  commands per the target's inbox; guard throws), so the ADR-0032 test gate applies to PMs unchanged
  — no exemption.
- The runtime envelope (row locking / single-flight, checkpointing, correlation & cause propagation,
  poison-message skip, `last_update_utc`) is declared once in the file header and never appears as
  steps — same rule as the event envelope (ADR-0041).

The saga sequence diagrams are now **generated from the steps** (`specs/generated/c4.generated.md`):
each step kind renders as its layer's participant, so the diagram is the layer contract, not an
illustration of it.

## Alternatives considered

- **Free-text steps (status quo)** — documented intent but validated nothing, and required weakening
  the completeness gate.
- **Full behaviour trees / expression language in YAML** — could express arbitrary conditions
  (`!=`, arithmetic) but turns the spec into a programming language; conditions beyond
  `{ const }` equality stay prose in `note`, with the error/rule definition carrying the semantics.
- **PMs as event-sourced actors emitting events** (pre-re-architecture model) — rejected earlier:
  aggregates own the facts; PM state is operational, not domain history.

## Consequences

### Positive
- `make validate` proves PM wiring end-to-end (0 errors on this change, down from 58) and the gate
  got stronger instead of exempted.
- Layer responsibilities are structurally inexpressible to violate (no step kind lets an adapter or
  PM write the log).
- Diagrams, docs, and (next) the orchestrator scaffold generate from one source; `docs/sagas.md`
  narrative no longer drifts from behaviour.
- Refunds are now fully specified as **admin-approved** (`RefundProcess`: pending → Approve/Deny →
  settled), with `RefundNotPending`, `RefundRequiresAdminApproval`, and the `refund_process_manager`
  table.

### Negative
- The step vocabulary is deliberately small; genuinely conditional flows must split into legs or
  PMs, and non-structural guard conditions remain prose.
- The hand-written PM runtime (state-table orchestrators) is still to be reimplemented against this
  spec — the DSL landed first by design.
