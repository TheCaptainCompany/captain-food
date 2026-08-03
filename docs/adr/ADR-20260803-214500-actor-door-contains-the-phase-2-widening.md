# ADR-20260803-214500 — The per-actor client crates enqueue through an opaque `ActorDoor`, and a guard contains what that widens

- **Status**: Accepted
- **Date**: 2026-08-03
- **Refines**: [ADR-20260803-172654](ADR-20260803-172654-mailbox-port-demands-a-capability-witness.md)
  (the port demands a capability witness), [PROP-20260802-130500](../proposals/PROP-20260802-130500-isolation-by-construction.md)
  §6 phase 2 (which named the two exits and recommended this one)
- **Realized by**: [#306 "Isolation phase 2: one crate per actor client (aggregates AND process managers)"](https://github.com/TheCaptainCompany/captain-food/issues/306)

## Decision

Phase 2 moves each `{Actor}Client` into its own generated crate (`crates/clients/<actor>`, 17
today). Those crates enqueue through **`actor_client::ActorDoor`**, an opaque facade that takes
primitives, builds the `MailboxEntry` inside `actor_client`, mints the `MailboxAccess` inside
`actor_client`, and returns only an outcome. **Neither `command_entry`/`inbound_entry` nor
`MailboxAccess::granted()` is widened.**

`ActorDoor` may be named by the generated client crates and nowhere else, enforced by the codegen
guard `actor_door_is_named_only_by_generated_client_crates`, which lands in the same change as the
door itself.

## Why

The proposal's §6 named the wall phase 2 hits and its two exits: widen the entry constructors to
`pub`, or expose an opaque facade. Widening is the level-4 → level-3 slide the witness was
installed to prevent five hours earlier — #304's own guard carries a note saying so in as many
words. The facade keeps the two things that matter at compile-time enforcement:

- a `MailboxEntry` still cannot be built outside `actor_client` (private fields);
- a `MailboxAccess` still cannot be minted outside `actor_client` (`pub(crate)` mint).

## What this widens, stated plainly

`ActorDoor` is **string-keyed**: `send_command("Restaurant", 5, id, "MarkRestaurantClosed", …)`
addresses any actor with any message. On the typed path that is impossible — the sealed
`{Actor}Command`/`{Actor}Fact` traits make a non-received message a compile error. Before phase 2
the equivalent capability (`command_entry`) was `pub(crate)` and no such bypass existed at all.

So phase 2 buys a level-4 boundary (which actor a crate may address is now its manifest) at the
cost of a new level-3 one (a public door that could address any of them). That trade is worth
making — the manifest boundary applies to all 17 actors for every consumer, while the door is a
single type one guard watches — but it is a trade, not a free win, and the honest accounting
belongs here rather than in a summary that only lists what improved.

The containment is the guard, and the guard is part of this decision, not a follow-up: naming
`ActorDoor` outside `crates/clients/**` is CI-red. That is the same enforcement tier the repo
already accepts for the `bulk-door` feature grant — level 3, the loud reviewable act — and it is
the best available without a sealed trait, which a generated sibling crate cannot implement by
construction.

## Options considered

| option | verdict |
|---|---|
| **Opaque `ActorDoor` + naming guard** ✅ | Entry and witness stay level 4; the widening is one type, watched by one guard that fails the build |
| Widen `command_entry`/`inbound_entry` to `pub` | Rejected: every holder of `actor_client` could then build rows directly, and #304's witness assertion explicitly calls this the boundary sliding from compiler to allowlist |
| Widen `MailboxAccess::granted()` | Rejected outright: one public mint reopens *every* method of the port for *every* crate in the workspace |
| Feature-gate the door (`client-door`, à la `bulk-door`) | Rejected: Cargo feature unification means any build containing one client crate lights it for all, so the protection is the manifest guard either way — while the dead-code churn when the feature is off is real (`unreachable_pub`/`dead_code` on the delegates). The naming guard gives the same tier with none of the cost |

## Consequences

- The drift guard that proves a typed `send` builds the reference row moved out of the crate
  (`crates/actor_client/tests/drift_guard.rs`) and now runs as a consumer would, through the
  public surface, comparing rows via the D5 `EntryFixture` mirror. This needed
  `enqueue_worker_command` and `enqueue_inbound_fact` exported under `test-fixtures` (dev-only,
  already CI-guarded) and a **dev-dependency cycle** `client-restaurant → actor_client → [dev]
  client-restaurant`, which Cargo permits for exactly this shape.
- A build graph that did not exist before is now routine — `actor_client` with *no* features —
  which surfaced two pre-existing `unreachable_pub`/`dead_code` findings on the `bulk-door` items.
  They are now `cfg_attr`-scoped to the feature-off configuration rather than silenced globally.
- The per-actor crates each own their own `sealed::Sealed`, so an actor's receive set is not
  nameable from a sibling client crate — a small strengthening the split gave for free.
- `server` depends on 15 of the 17 client crates; it addresses neither `Payment` nor
  `CustomerCredit`. That asymmetry is now a fact of the build graph rather than a convention.
