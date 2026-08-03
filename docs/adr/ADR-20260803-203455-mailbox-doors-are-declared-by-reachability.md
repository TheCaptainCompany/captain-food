# ADR-20260803-203455 — Every public mailbox door is declared: closing the #304 residual class by reachability

- **Status**: Accepted
- **Date**: 2026-08-03
- **Refines**: [ADR-20260803-172654](ADR-20260803-172654-mailbox-port-demands-a-capability-witness.md)
  (the `MailboxAccess` witness), which named this class as its stated limit
- **Realized by**: [#329 "Close the #304 residual class: every public mailbox door must be declared"](https://github.com/TheCaptainCompany/captain-food/issues/329)

## Decision

A second guard, `every_public_mailbox_door_is_declared`, closes the class the witness guard cannot
see. It seeds on **mints** of `MailboxAccess`, propagates through `actor_client`'s call graph to a
fixpoint, and requires every publicly-reachable tainted function to appear on an explicit **door
list** keyed by `(file, name)` with the reason it exists.

The door list is the whole point, not a side effect: it is the enumeration of every public function
in the boundary crate that can reach the mailbox — ten today, each one a door somebody deliberately
opened. Adding an eleventh is an edit to that list, which is the ADR-20260802-170059 posture ("the
declaration is the permission") applied to the crate's own surface rather than to the spec.

## Why this closes the class, and why the argument is short

ADR-20260803-172654 disclaimed one class: a public in-crate item that mints internally and hands the
capability out through a signature that never names the witness —
`pub fn cancel_any(&self, id: Uuid) -> Result<bool>` over a held `Arc<dyn Mailbox>`. Seven review
passes established that no signature analysis reaches it, and the disclaimer was the honest answer
at the time.

But the class is not un-checkable, only un-checkable *by signatures*. Calling a port method requires
a witness, and a witness can arrive only two ways:

- **from a parameter** — which names the witness in a signature, and is caught by
  `every_mailbox_port_method_demands_the_access_witness`;
- **from a mint** — which is caught here.

There is no third source in safe Rust. Constructions via a field, a `const` or a `static` all reduce
to one of the two: something had to mint or receive the witness to put it there. So the two guards
compose into a complete rule for the crate, where each alone was open.

## Consequences

- **The seed is structural, not textual.** The witness's own mints spell the construction `Self(())`
  inside an `impl MailboxAccess`, not `MailboxAccess(())`, so a text seed alone misses `granted` and
  `for_tests`. Seeding on "constructs `Self` in an impl on the witness" means **renaming the mint
  does not blind the guard** — verified by renaming `granted` throughout and confirming the guard
  still catches a planted exploit.
- **Taint stops at a declared door.** A function calling `RestaurantClient::send` is using the
  sanctioned public API, which every crate has anyway; it is not a new capability. Without this the
  scan reported `OperationStatusBus::publish` as a door, because it calls `broadcast::Sender::send`
  and the propagation matched the ident `send`.
- **Doors are keyed by `(file, name)`, never by name alone** — for the same reason: `send` is both a
  generated client's write door and `Sender::send`, and a bare-name allowlist would pre-authorise
  any future `pub fn send` anywhere in the crate.
- **Both directions are asserted**, like the entry-construction guard: an undeclared public minter
  fails, and so does a **stale** door entry, which would otherwise pre-authorise a future function
  that happens to take the name.
- **Name-based call resolution over-approximates** (no type information), which flags too much
  rather than too little — the safe direction, and the same posture the witness guard takes on
  module privacy. The cost is that a genuinely new door must be named; that cost is the feature.
- **`unsafe_code = "forbid"` stays load-bearing.** `mem::zeroed::<MailboxAccess>()` would defeat both
  guards identically, so the threat model remains safe Rust.
- What remains outside both guards is unchanged and already recorded: macro expansion (refused as a
  class rather than analysed), out-of-crate implementors of the port (contained by the composition
  root), and edits to the boundary crate itself, which are visible in any diff.
