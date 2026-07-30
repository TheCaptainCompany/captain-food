# ADR-20260730-234918 — The actor runtime is a product-agnostic crate, extraction-ready from day one

- **Status**: Accepted (product owner, 2026-07-30: *"I want someday to reuse this work on another
  product so we will have to centralise it in another repo one day — not today"*)
- **Context**: ADR-20260730-231500 (the actor-mailbox runtime, in build via
  [#242](https://github.com/TheCaptainCompany/captain-food/issues/242) slices 2–4)

## Decision

The actor runtime — mailbox claim/complete, partition leases + `ownership_version` fencing,
checkpoints, the activation shell (micro-mailbox, batched turns, memento, passivation), reminders
promotion, the placement lookup, and the typed-client machinery *shape* — is built as its own
workspace crate (working name `crates/actor_runtime`) with **zero Captain.Food domain
dependencies**, from the first commit:

- it may depend on std/tokio/sqlx/serde and its OWN trait vocabulary (`Mailbox`,
  `PlacementLookup`, an `ActorRuntime`-facing message/actor trait pair) — **never** on
  `crates/domain`, `crates/application`, or any generated Captain.Food type;
- everything Captain.Food-specific reaches it through generics/trait impls emitted by the codegen
  into the APP crates (the typed clients, the per-actor `apply`/`fold`/`requires` glue) — the
  runtime knows "an actor with an identity, a fold and a decide", never "an Order";
- the dependency test is executable, not prose: a codegen/CI check asserts
  `crates/actor_runtime/Cargo.toml` declares no path dependency on the domain-side crates
  (the ADR-0035 dependency-graph gate extended by one edge rule).

Extraction later = `git filter-repo` on one directory + publishing a crate — not surgery.

## Consequence for tests (product-owner directive, same session)

The runtime's test suite takes **Proto.Actor's test ideas as its inspiration list** (repos read at
source, D2.1 of PROP-20260728-152752). The ports worth naming now, so slice 2–4 test plans start
from them:

1. **`ClusterMaintainsSingleConcurrentVirtualActorPerIdentity`** (their flagship, ours made
   provable): a `ConcurrencyVerificationActor`-style probe actor that counts concurrent entries —
   asserted `<= 1` per `actor_id` under load WITH lease steals and worker churn running. Where
   Proto.Actor's own harness comments that duplicated activation "is by design and we don't want
   to report it", ours asserts zero — the fence makes the stronger claim testable.
2. **Their cluster fixture shape**: spawn N workers against one Postgres, churn membership
   (kill/spawn), run invariant probes throughout — our lease/rebalance/failover tests get a
   reusable fixture, not one-off setups.
3. **Handover completeness accounting** (their chunk-index + count reconciliation): after any
   rebalance, per-partition message counts processed exactly once — no loss, no double.
4. **Mailbox discipline tests** (their `defaultMailbox` suite): control-before-user ordering,
   suspension semantics, throughput budget honored, FIFO per sender.
5. **Their gate-the-scanner lesson** (the OVH env-read regression): test the runtime's own test
   fixtures directly, not only through the happy path — a probe that cannot detect a violation is
   indistinguishable from no violations.

## Not today

The extraction itself (separate repo, versioning, publishing) is explicitly deferred — no work is
scheduled. This ADR only makes it CHEAP by forbidding, from the first line, the one thing that
would make it expensive: a domain import inside the runtime.
