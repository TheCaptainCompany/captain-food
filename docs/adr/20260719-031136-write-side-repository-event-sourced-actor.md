# ADR-20260719-031136 — Write-side `Repository` over the event-store journal (event-sourced actors)

## Status

Accepted. Realized in the same change (the `Repository` layer + all 64 command handlers + the
`ProcessManagerRunner` routed through it). Refines ADR-0035 (Clean-Architecture layering) and ADR-0046
(write side / command handlers).

## Context

`specs/actors.yaml` models the domain as an **actor model** — aggregates & process-managers, each an actor
with an inbox `{ message → emits, throws }`. The runtime did not express that cleanly: there was **no
write-side repository**. Application code (the 64 command handlers *and* the `ProcessManagerRunner`) used
the low-level `EventStore` port **directly** — `store.load(stream)` + `domain::<agg>::fold(...)` to
rehydrate, `store.append(stream, version, events, actor)` to persist — with the per-aggregate
`load_<agg>`/`require_<agg>` helpers and the `"<Agg>-{id}"` stream `format!`s **duplicated** between
`commands.rs` and `process_managers/mod.rs`.

This is a hexagonal-layering leak: in DDD / actor terms the aggregate **owns emission** and a **repository
(the actor's journal)** owns persistence — the raw event log should live *behind* a repository, and a write
decision must rehydrate the actor's **own write-side stream** (authoritative + carrying the version for the
optimistic-concurrency append), **never** an eventually-consistent read model.

## Decision

**Two layers over one journal.**

1. **`domain::aggregate::Aggregate`** — the event-sourced-actor contract: `type Id`, `category()`,
   `stream(id)`, `fold(events) -> Option<Self>`. Implemented by the 8 aggregate `State` structs (delegating
   to their existing `fold`). Single source of truth for identity + rehydration; **replaces both duplicated
   stream-helper sets**.
2. **`application::repository::Repository`** — a thin layer that WRAPS the `EventStore` journal:
   `load::<A>(id) -> (Option<A>, version)`, `require::<A>(id, nf)`, `events::<A>(id) -> (Vec<DomainEvent>,
   version)` (the raw slice, for decisions that inspect events the folded state doesn't capture — sagas,
   the test-mode scan), `save(stream, version, events, actor)`, `create(stream, events, actor)` (v0 +
   idempotent-on-existing). It subsumes the per-aggregate `load_/require_` helpers + `idempotent_on_existing`.

**Everything write-side now goes through the `Repository`; nothing calls the `EventStore` port directly**
— except the Stripe ACL's `StripeEvent-<id>` v0 append, which is a **non-aggregate idempotency envelope**
(not a domain aggregate) and therefore correctly records its fact through the low-level journal. The
`EventStore` port **keeps its name** (it *is* the journal) — renaming it, and migrating handler signatures
to take `&Repository`, would churn the codegen emitter and were deferred.

**We keep the functional event-sourced-actor (Decider) style** already pervasive here: the actor's
behaviour is a **pure** `fold` + decide function; the shell (handler / runner) loads via the Repository,
invokes the pure decision, and saves the decided events via the Repository. No mutable OO aggregates.

## Alternatives considered

- **Mutable OO aggregates** that raise events on method calls (`order.accept()`), saved by a repository —
  rejected: a paradigm shift against the existing pure functional core (`fold`/decide), worse Rust
  ergonomics and testability.
- **Load process-manager decision state from the read models** — rejected: read projections are
  eventually consistent and carry no stream version; a write decision gated on stale state, with no version
  for its append, is a CQRS correctness bug. Rehydration is always the actor's own write-side stream.
- **Rename `EventStore` → `Journal` + migrate handler signatures to `&Repository`** — deferred (codegen
  emitter + every call site); the handlers wrap `Repository::new(store)` internally for now.

## Consequences

### Positive
- The hexagon reads correctly: **actor decides (pure) → Repository (the actor's journal) persists →
  `PgEventStore` adapter → `domain_events`**. The runner is the imperative shell; it no longer touches the
  event store directly.
- One source of truth for aggregate identity/rehydration (`Aggregate`); duplicated stream helpers and the
  8 `load_/require_` helpers collapse into the generic `Repository`.
- Pure internal restructuring — **same events, streams, versions, idempotency**; all existing behaviour +
  unit tests pass unchanged.

### Negative / caveats
- `Repository::events::<A>` still hands out the raw slice for saga decisions & the test-mode scan (those
  consume events, not folded state); a later step could migrate process-manager decisions to consume folded
  **state** and drop `events`.
- Command-handler *signatures* still advertise `store: &dyn EventStore` (they wrap it in a `Repository`
  internally) — a cosmetic leak until the deferred signature migration.

### Follow-up actions
- Migrate handler signatures to `&Repository` (needs the codegen `emit_server_*` mutation template + call sites).
- Optionally rename `EventStore` → `Journal`; add a first-class `record_fact` for ACL idempotency envelopes.
- Model process-manager cross-aggregate emissions as **commands sent to the target actors** (actor-purity).

## References
Refines ADR-0035 (Clean-Architecture crate layout), ADR-0046 (write-side command handlers); builds on
ADR-0041 (event envelope). Code: `crates/domain/src/aggregate.rs`, `crates/application/src/repository.rs`,
`crates/application/src/commands.rs`, `crates/infrastructure/src/process_manager/runner.rs`.
