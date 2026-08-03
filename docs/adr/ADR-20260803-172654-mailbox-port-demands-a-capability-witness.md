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
method can be called by any out-of-crate CALLER**: the generated typed clients (write) and
`ActorClient` (read) are the only paths that reach the mailbox, by compilation rather than by
convention. (An out-of-crate *implementor* is a different case — see Consequences.)

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
| **Capability witness on every port method** ✅ | Closes the read and withdrawal doors outright, and makes the rule MECHANIZABLE: "every method of this trait takes the witness" is checkable by a scan, so the guard below can enforce it forever |
| Witness only on `by_message` / `cancel_scheduled` | Smaller diff, but the rule becomes "methods whose arguments are not otherwise unconstructible" — which no textual guard can check, because it requires reasoning about type constructibility *across cfg configurations*, and that is already configuration-dependent here (`test-fixtures` flips `MailboxEntry` from unconstructible to constructible). A rule you cannot mechanize is a rule you re-litigate at every review |
| Narrow the return types instead (opaque `MailboxStatusRow`) | Makes the bypass less *useful*, never impossible; still level 3 |
| A `pub(crate)` extension trait over the port | A public trait's methods cannot be `pub(crate)`, and `PgMailbox` must implement them from another crate — the underlying methods stay public, so nothing closes |
| Textual guard forbidding out-of-crate call sites | Level 3: an alias or a new crate walks past it, which is the failure the door guard's own doc admits |

## Consequences

- **The boundary is level 4 against CALLERS, weaker against IMPLEMENTORS.** `infrastructure::PgMailbox`
  and the `MemMailbox` double name `MailboxAccess` in their signatures and ignore it. But an
  out-of-crate `impl Mailbox for Decorator(Arc<dyn Mailbox>)` is *handed* a real witness the moment
  any door calls it, and the witness is `Copy` — so it can spend it on any other port method of the
  wrapped mailbox. What contains that is **the composition root**: a decorator only receives calls
  once someone wires it into `crates/server/src/lib.rs`, which is a loud, reviewable diff. It is
  explicitly NOT contained by D3 — a decorator needs no `sqlx` — and saying so would be worse than
  saying nothing, because a wrong justification stops the next reviewer looking. Dropping `Copy`
  would stop retention but not decoration, so it buys nothing real.
- **The guard is a level-3 gate protecting a level-4 property, and only inside the boundary crate.**
  The compiler makes the rule unbreakable from outside `actor_client`; every remaining way to
  reopen the door is an EDIT to that crate, and `every_mailbox_port_method_demands_the_access_witness`
  (tools/codegen-rs) is what catches those. It pins: every port method takes the witness and the
  surface is exactly five methods (fail-closed — it consumes every `fn` token rather than matching
  a list of accepted prefixes, so `unsafe fn` and `#[attr] async fn` are seen); the witness's
  `pub(crate)` field and mint; that the only public mint is `for_tests`, scoped **by span** to the
  cfg-gated fixtures module; that no impl, construction or public function returning the witness
  exists elsewhere in the crate; that `mailbox.rs` declares exactly one public trait, so an
  extension trait with a defaulted method cannot mint internally and hand every port holder an
  ungated door; and that no generated client names the witness at all.
  It works on comment-stripped, whitespace-normalized source **because two independent review
  passes each defeated an earlier version by nothing cleverer than reformatting** — `pub  fn` with
  two spaces, a split signature, an attribute ahead of `async fn`, a comment standing in for a
  parameter, a `From<()> for crate::mailbox::MailboxAccess` matching no literal pattern. Sixteen
  mutation shapes are verified to turn it red against a green baseline. The honest claim is "the
  plausible widenings are caught", not "the surface is proven": this is textual analysis of Rust,
  and the record of this change is that textual analysis loses to anyone actually trying.
- Callers that need the read door now need an `ActorClient`, which needs an `OperationStatusBus`.
  In the monolith that is the real bus. A **standalone adapter has no shared bus** —
  `run_standalone_workers` publishes onto a separate instance nothing outside it can subscribe to —
  so `HubRiseConnectFlow::new` takes `Option<OperationStatusBus>` and the standalone binary passes
  `None`, yielding a pull-only door. Taking the bus rather than a ready-made `ActorClient` also
  removes a hazard the reviewer named: a caller can no longer hand the flow a read door built over
  a *different* mailbox than the one it writes through. To be precise about what that buys **today**:
  the connect flow only ever pulls, so `Some(bus)` and `None` are behaviourally identical for it
  right now — the distinction is forward-looking, and stops a `watch` added later from hanging in
  the standalone topology and nowhere else. No hang was averted on this path; one was made
  impossible on it.
- **No generated per-actor client names the witness.** `{Actor}Client::cancel_scheduling` was the
  one client method that spoke to the port directly; it now delegates to
  `enqueue::cancel_scheduled_mapped` like every other. That matters for
  PROP-20260802-130500 **phase 2**: when each client moves to its own crate, a mint inside a client
  would be the single line that fails to compile, and the tempting "fix" is to widen
  `granted()` to `pub` — trading the compiler (level 4) for a manifest allowlist (level 3). With
  every mint kept in the core module, phase 2 only has to widen the three `pub(crate)` delegates.
  The guard asserts both halves, so the day phase 2 tries to widen the mint it fails loudly — the
  wall becomes a recorded decision instead of a silent slide.
  (The *bigger* phase-2 wall is not this one: per-actor client crates must BUILD entries, so
  `command_entry`/`inbound_entry` and the entry's private fields are the real obstacle. Recorded in
  the proposal's phasing, not here.)
- `ActorClient::pull_only(mailbox)` exists for a caller with no response bus to share. Handing such
  a caller `OperationStatusBus::default()` compiles and then lies: it builds a live broadcast
  channel whose only sender is the client's own field, so a later `watch` awaits forever — never a
  message (nothing publishes), never `Closed` (the client holds the sender). `watch` therefore
  returns `Option<OperationWatch>`, and the posture lives in the type instead of a comment.
- The PROP-20260802-130500 §5 audit row for the `Mailbox` port moves from ❌ hole to ✅ compiler.
  `View_*` read methods ([#305](https://github.com/TheCaptainCompany/captain-food/issues/305)) and
  `PgEventStore` append remain open.
