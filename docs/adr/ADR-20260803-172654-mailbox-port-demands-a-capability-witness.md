# ADR-20260803-172654 — The `Mailbox` port demands a capability witness: holding the port is not holding the door

- **Status**: Accepted
- **Date**: 2026-08-03
- **Refines**: [ADR-20260802-170059](ADR-20260802-170059-client-surface-is-spec-gated.md)
  (no method without a usage declaration), [PROP-20260802-130500](../proposals/PROP-20260802-130500-isolation-by-construction.md)
  §5 directive (the system-wide surface audit)
- **Realized by**: [#304 "The Mailbox port surface hole: insert/by_message are pub to any port holder"](https://github.com/TheCaptainCompany/captain-food/issues/304)

## Decision

Every method of the `Mailbox` port takes a `MailboxAccess` witness — a unit struct whose single
field is `pub(crate)` to `actor_client`, minted only by `MailboxAccess::granted()` (also
`pub(crate)`). Outside the boundary crate the witness cannot be constructed, so **no `Mailbox`
method can be called at all**: the generated typed clients (write) and `ActorClient` (read) are the
only paths that reach the mailbox, by compilation rather than by convention.

The one public mint is `MailboxAccess::for_tests()`, compiled only under the D5 `test-fixtures`
feature that `test_fixtures_feature_never_reaches_a_release_artifact` already keeps out of every
release graph — the same posture `EntryFixture` has, for the same reason: an integration test that
seeds a mailbox row *is* the thing behind the door.

Two consumers that read the port directly moved onto `ActorClient::get_operation_status`:
the HubRise connect flow's terminal-status poll, and the generated legacy-arm cross-arm duplicate
check in the PM resolver template. Neither had a reason to hold the port's read side; both
predate the D4 read door.

## Why

The port's methods were `pub`, so any holder of an `Arc<dyn Mailbox>` could address the mailbox
directly. The write methods (`insert`, `insert_many`, `schedule`) were *incidentally* closed
already — a `MailboxEntry` has `pub(crate)` fields, so a caller outside the crate cannot produce
the argument — but that is an accident of one method's signature, not a property of the port. The
two methods keyed by a bare `Uuid` had nothing at all:

- `by_message` — the whole read side, which D4 had just moved behind `ActorClient`. Its own doc
  comment claimed callers "read it through `ActorClient::get_operation_status`, never by naming
  this method"; two callers were doing exactly that.
- `cancel_scheduled` — the wider hole, and the one nobody had named. The client method above it,
  `cancel_scheduling`, is emitted **only for actors that declare `reminders:`**
  (ADR-20260802-170059). The port beneath it would withdraw any scheduled row for anyone. A
  declaration gate one layer up, with an ungated call one layer down, is not a gate.

This is the entry's private fields one level up, and it climbs the same rung of the
PROP-20260802-130500 enforcement hierarchy: level 1 (a doc comment saying "never") becomes level 4
(the compiler). The crossing is still possible — but it takes editing `actor_client` itself, which
is a loud, reviewable diff, not a silent shortcut.

### Options considered

| option | why it won / lost |
|---|---|
| **Capability witness on every port method** ✅ | Closes the read and withdrawal doors outright; makes the write side's closure explicit instead of incidental, so a future method taking only primitives cannot silently reopen the surface; implementors outside the crate still compile (naming a type is not constructing one) |
| Witness only on `by_message` / `cancel_scheduled` | Smaller diff, but leaves the port's guarantee dependent on a signature accident — the next `fn requeue(&self, message_id: Uuid)` reopens it with nobody noticing |
| Narrow the return types instead (opaque `MailboxStatusRow`) | Makes the bypass less *useful*, never impossible; still level 3 |
| A `pub(crate)` extension trait over the port | A public trait's methods cannot be `pub(crate)`, and `PgMailbox` must implement them from another crate — the underlying methods stay public, so nothing closes |
| Textual guard forbidding out-of-crate call sites | Level 3: an alias or a new crate walks past it, which is the failure the door guard's own doc admits |

## Consequences

- `infrastructure::PgMailbox` and the `MemMailbox` double name `MailboxAccess` in their signatures
  and ignore it. An out-of-crate *implementor* is handed a witness when a door calls it, so it
  could retain one — accepted: implementing the port is itself a loud act, and D3's capability
  allowlist governs who may hold `sqlx` at all.
- `every_mailbox_port_method_demands_the_access_witness` (tools/codegen-rs) keeps the rule applying
  to the *whole* surface: a sixth method without the witness compiles fine and would silently
  reopen the hole. The guard also pins the witness's `pub(crate)` field and its single public mint,
  since widening either hands the key back without changing a signature.
- Callers that need the read door now need an `ActorClient`, which needs an `OperationStatusBus`.
  In the monolith that is the real bus. A **standalone adapter has no shared bus** —
  `run_standalone_workers` builds a local subscriber-less one — so the HubRise binary passes a
  default: correct there, because the connect flow only ever pulls the durable row. A future
  caller that wants `watch` in a standalone adapter must thread a real bus first.
- The PROP-20260802-130500 §5 audit row for the `Mailbox` port moves from ❌ hole to ✅ compiler.
  `View_*` read methods ([#305](https://github.com/TheCaptainCompany/captain-food/issues/305)) and
  `PgEventStore` append remain open.
