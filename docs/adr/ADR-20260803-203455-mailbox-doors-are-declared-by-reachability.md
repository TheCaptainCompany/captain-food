# ADR-20260803-203455 — Every public mailbox door is declared: narrowing the #304 residual class by reachability

- **Status**: Accepted
- **Date**: 2026-08-03
- **Refines**: [ADR-20260803-172654](ADR-20260803-172654-mailbox-port-demands-a-capability-witness.md)
  (the `MailboxAccess` witness), which named this class as its stated limit
- **Realized by**: [#329 "Narrow the #304 residual class: every public mailbox door must be declared"](https://github.com/TheCaptainCompany/captain-food/issues/329)
  (issue retitled from "Close" — the outcome is a narrowing, and the title said otherwise)
- **Defers**: [#331 "Decide whether the mailbox-door rule is worth type resolution (rustc lint / HIR / MIR)"](https://github.com/TheCaptainCompany/captain-food/issues/331)

## Decision

A second guard, `every_public_mailbox_door_is_declared`, NARROWS the class the witness guard cannot
see — it does not close it; see the scope statement below. It seeds on **mints** of `MailboxAccess`, propagates through `actor_client`'s call graph to a
fixpoint, and requires every publicly-reachable tainted function to appear on an explicit **door
list** keyed by `(file, name)` with the reason it exists.

The door list is the whole point, not a side effect: it enumerates the public functions in the
boundary crate that can reach the mailbox — ten entries, of which the **seven non-test ones are the
release surface and the only load-bearing ones**. The other three (`cancel_reminder`,
`schedule_reminder`, `for_tests`) are `test-fixtures`-gated, so `!f.test_only` already excludes them
from the undeclared check AND taint flows straight through them (it stops only at UNGATED doors):
deleting all three leaves the test green. They are listed as documentation of the test-only surface,
nothing more. Each of the seven is a door somebody deliberately opened. Adding an eleventh is an edit to that list, which is the ADR-20260802-170059 posture ("the
declaration is the permission") applied to the crate's own surface rather than to the spec.

## What it does, and the completeness claim I got wrong

ADR-20260803-172654 disclaimed one class: a public in-crate item that mints internally and hands the
capability out through a signature that never names the witness. This guard **narrows** that class;
the first version of this ADR said it *closed* it, and that was false.

The abstract argument is sound as far as it goes. A witness reaches a port method from either a
**parameter** — which names it in a signature, caught by
`every_mailbox_port_method_demands_the_access_witness` — or a **construction**, caught here; a field,
a `const` or a `static` all reduce to one of the two, since something had to mint or receive the
witness to put it there. That dichotomy is real value provenance.

The unsound step was the silent leap from *"a construction exists somewhere"* to *"this scan finds
every construction and every path from it"*. This scan is a **syntactic approximation of the call
graph**: it resolves calls by ident, with no type information. A semantic argument cannot be
discharged by a syntactic tool, and review proved it with four counterexamples that were ordinary
rather than adversarial — `MailboxAccess { 0: () }` (the same construction, a spelling the seed did
not match), a function passed as a VALUE (`let f = MailboxAccess::granted;`, so the ident is never
followed by `(`), a wrapper over a feature-gated door (which does not inherit the door's cargo
feature), and an expression-position macro (the #304 refusal covered only item position).

All four are fixed — the seed and the call graph are read from the AST now, and taint no longer
stops at a *gated* door. But the honest scope is: **sound for constructions the AST recognises as
constructions of the witness, and for call edges resolvable by ident**. A genuinely complete rule
needs type resolution (a rustc lint, or HIR/MIR reachability), which is a scope decision for a
proposal rather than a test — tracked as
[#331 "Decide whether the mailbox-door rule is worth type resolution"](https://github.com/TheCaptainCompany/captain-food/issues/331)
so the deferral is visible in the prioritised backlog rather than living only in this sentence.

## Consequences

- **The seed and the call graph are read from the AST**, not from body text. The witness's own mints
  spell the construction `Self(())` inside an `impl MailboxAccess`, so a type-name seed misses
  `granted` and `for_tests`; and `MailboxAccess { 0: () }` is the same construction as
  `MailboxAccess(())`. Both are constructions in the AST. Neither a rename NOR a respelling of the
  mint blinds it — both verified against a planted exploit. Function bodies are scanned with their
  ATTRIBUTES excluded, so a doc comment naming the mint is documentation rather than a door (the
  text version reported one, with advice whose only real remedy was deleting the docs).
- **Call edges include bare references, not just call syntax.** `let f = MailboxAccess::granted;`,
  `.map(insert_mapped)` and `unwrap_or_else(Self::helper)` all pass a function as a value, so an
  ident-followed-by-`(` scan misses them — and that is a plausible false negative in honest code,
  not only an attack.
- **Taint stops at an UNGATED door only.** Stopping at a door at all is right: a function calling
  `RestaurantClient::send` uses the sanctioned public API every crate has anyway, and without that
  rule the scan reported `OperationStatusBus::publish`, which calls `broadcast::Sender::send`. But a
  GATED door's containment is a cargo feature on its `pub use`, and an in-crate wrapper does not
  inherit it: wrapping `enqueue_inbound_facts` would
  otherwise have re-exposed the untyped bulk door to crates the `bulk-door` manifest guard exists to
  exclude — verified by compiling such a wrapper from `server`, which does not enable the feature.
- **Doors are keyed by `(file, name)`, never by name alone** — for the same reason: `send` is both a
  generated client's write door and `Sender::send`, and a bare-name allowlist would pre-authorise
  any future `pub fn send` anywhere in the crate.
- **Both directions are asserted**, like the entry-construction guard: an undeclared public minter
  fails, and so does a **stale** door entry, which would otherwise pre-authorise a future function
  that happens to take the name.
- **Name-based call resolution over-approximates** (no type information), which flags too much
  rather than too little — the safe direction, and the same posture the witness guard takes on
  module privacy. The cost is that a genuinely new door must be named; that cost is the feature.
  It over-approximates broadly — planting one tainted `Held::new` also flags unrelated `default`
  and `insert_many`, and the failure message says so — but it must not UNDER-approximate, and an
  earlier version did: it excluded call edges whose
  callee shares the caller's name, which sounded like self-recursion protection but could not be
  (the candidate set holds only tainted functions) and silently dropped the ordinary shape "public
  `Facade::new` calls crate-internal minting `Held::new`" — `new` being the commonest ident in Rust.
  Removing the exclusion closes that and leaves the tree green, so it protected nothing.
  String LITERALS inside macros are excluded from the ident harvest, for the same reason doc
  attributes are: `println!("access granted…")` is prose, and flagging it emitted advice
  (`pub(crate)`) whose real remedy was rewording a log line.
- **`unsafe_code = "forbid"` stays load-bearing.** `mem::zeroed::<MailboxAccess>()` would defeat both
  guards identically, so the threat model remains safe Rust.
- **`const`, `static` and associated-const INITIALIZERS are scanned too.** Skipping them was a
  traversal gap, not a scope limit: `const HELD: MailboxAccess = MailboxAccess(());` — the ordinary
  way to stop calling the mint in three places — is a construction the AST recognises, and a public
  `cancel_any` using `HELD` was invisible to BOTH guards. The initializer is scanned like a body and
  the item's ident joins the fixpoint, so nothing about that shape needed type resolution.
- **A `#[derive(..)]` on the witness is a trait impl in one word.** The leak rule saw only
  `Item::Impl`, so `#[derive(Default)]` on `MailboxAccess` handed every crate in the workspace a
  public mint via `Default::default()` — proven from `server`, which holds only the port. Derives
  are now allowlisted (`Debug`/`Clone`/`Copy`/`PartialEq`/`Eq`/`Hash`/`PartialOrd`/`Ord`) and
  anything else refused as a class, which is the rule sessions.md §8b already states: ban a class,
  not a spelling.
- **The anti-blindness assertion names `granted` specifically.** A bare "something is tainted" check
  is satisfied by the test-only `for_tests`, so the PRODUCTION mint could go dark unnoticed.
- What remains outside both guards: any construction or call edge the syntactic scan cannot see (see
  the scope statement above), out-of-crate implementors of the port (contained by the composition
  root), and edits to the boundary crate itself, which are visible in any diff. An expression-position
  macro mentioning the witness is now treated as a mint conservatively, since its expansion is opaque.
