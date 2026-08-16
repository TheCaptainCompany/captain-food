# ADR-20260816-194428 — The lane width is UNSPELLABLE, not merely unspelled

- **Status**: Accepted
- **Date**: 2026-08-16
- **Issue**: [#609 "Lane addressing residue after #596: `stable_partition` is still `pub` and `mailbox_address` still carries a vestigial width"](https://github.com/TheCaptainCompany/captain-food/issues/609)
- **Supersedes nothing**; completes [ADR-20260816-165714](ADR-20260816-165714-lane-addressing-is-declared-not-observed-and-an-unseeded-lane-must-wait.md)
- **Dispatch card**: `docs/dispatch/609-lane-addressing-residue.md`

## Context

[#596](https://github.com/TheCaptainCompany/captain-food/issues/596) established that disagreeing
copies of the routing width break one-writer **at the addressing function**, upstream of any fence:
the lease is keyed by lane and the completion fence checkpoints per lane, so two producers with two
widths give one aggregate two lanes, each with a live lease, each passing its own fence.
ADR-20260816-165714 made `actor_client::declared_lane(actor_type, actor_id)` the sole accessor and
removed the `width` parameter from every routing site.

It removed the **parameter**, not the **possibility**. `stable_partition(&id, some_width)` remained
`pub`-exported from `actor_client`, and the records said so at that true, smaller size — itself a
review correction on #596, where the first draft asserted the larger claim while a hand-copied
literal `5` still sat in all 17 generated client crates.

Two facts decided what to do about the residue, and neither is in the issue text:

1. **The residue was not idle.** Roughly twenty-two out-of-crate call sites, across eight test files,
   spelled `stable_partition(&order, 5)` — copies of exactly the constant #596 was about, wearing
   test clothing. A fixture stamped at `N mod 5` against a declaration that has moved to 7 lands on
   a lane the new grid's producers never use, while the worker still drains it: green build, wrong
   lane, no error.
2. **The hazard is observed, not hypothetical.** One week earlier, in this exact function, review
   found the hand-copied literal in every generated client crate. It is this codebase's actual
   maintainer failure mode, this month, in this code.

## Decision

**`stable_partition` is private to `crates/actor_client/src/partition.rs`.** Only `declared_lane`
crosses the crate line. Every out-of-crate caller — production and test alike — reads the
declaration through the accessor; there is no `pub`, no feature, no `cfg`, and therefore nothing for
a gate to police.

The rejected alternative and the reason are recorded below, because the difference between them is
the difference between a level-4 claim and a level-3 one.

Two obligations travelled with the conversion and are part of the decision:

- **The width pin becomes deliberate.** Four assertion sites were *incidentally* pinning their
  actors' declared widths by comparing a production stamp against a literal `5`. Those assertions
  carried TWO facts — the width literal, and that the production stamping path agrees with the
  routing function — and only the first is at risk here: the converted assertions still compare a
  REAL production stamp against `declared_lane`, so producer-independence is intact, and a wrong
  actor-type string now yields `None` and a panic rather than a plausible wrong lane. The missing
  half moves into `partition.rs` as
  `every_declared_width_is_the_standard_one_because_changing_one_is_a_migration`, which names the
  reason (ADR-20260802-220402 had to remap every non-terminal in-flight row for 100 -> 5, and only
  because 5 divides 100) and runs on every `cargo test` — where three of the four needed a Postgres.
  It pins the SHAPE, not the roster: a new actor on the standard width costs nothing, a changed
  width or a second exception stops the build and asks for the migration story. **Not the same
  guarantee — the missing half of it, deliberately placed and widened from 3 actors to 17.**
  A FLOOR on the slice length and a SEED asserting the `MailboxSupervision` entry sit before the
  loop, in this repo's existing anti-blindness idiom, so the generated slice cannot be emptied or
  lose its only non-5 actor and leave a loop scanning nothing.
- **The misroute guard stops needing a second opinion.** `a_partially_seeded_actor_still_routes_to_the_declared_partition`
  guarded its data with `declared != stable_partition(id, 2)`. It now asserts
  `declared >= SEEDED_LANES`. **Precisely**: the IMPLICATION is universal — a producer that believes
  the keyspace is 2 wide can only ever stamp 0 or 1, so any declared lane at or above 2 is
  unreachable under the narrowed keyspace — while the ASSERTION is not, since `declared` is one of
  `{0,1,2,3,4}` and two of those values would fail it. What the reformulation buys is that it names
  the seeded KEYSPACE the row must avoid instead of one arbitrary lane inside it. The test asserts
  the ABSENCE of a misroute and never stamps one, so it never needed to be able to compute one.

## Alternatives considered

**A — the cheap seam.** Keep the export but gate it: `#[cfg(any(test, feature = "test-fixtures"))]
pub use partition::stable_partition;`. Five lines, no test changes, and it was the smaller-scope
recommendation. Rejected on measured evidence, not taste:

- **It does not compile as specified.** `crates/actor_client/Cargo.toml` sets
  `unreachable_pub = "deny"` — a boundary crate's own policy that a `pub` item nobody outside uses
  is a defect. Gating only the re-export makes `pub fn stable_partition` unreachable in a release
  build: `error: unreachable pub item`. A working Option A needs a
  `#[cfg_attr(not(...), allow(unreachable_pub))]` — i.e. it starts by suppressing the lint that
  already agreed with Option B.
- **Its seal is real only for release artifacts.** With the same production mutant planted in
  `infrastructure`, `cargo build -p infrastructure` fails with `E0425` but
  `cargo test -p infrastructure` **compiles**: resolver v2 unifies the dev-dependency's
  `test-fixtures` grant into the single `actor_client` unit the lib links against during a test
  build. Anyone verifying the seal with `cargo test` gets a false negative. Both measured; see the
  PR body and `docs/claude/sessions.md`.
- It leaves nineteen-plus stale width literals and needs a **new** level-3 assertion to stop one
  unreviewed line from deleting the `cfg`. Option B deletes machinery instead of adding it.

**C — do nothing, close as not-worth-it.** A standing option, and correct if Option A had turned out
unworkable *and* Option B expensive. Option B measured at +137/-54 over ten files, entirely
mechanical, so the argument did not arise.

**D — `vernon`'s level-4 form of the whole area**: `declared_lane -> Lane(i16)` newtype with a
private constructor and `MailboxEntry.partition: Lane`, after which a hand-rolled FNV compiles and
is *unusable*. This ADR makes the COMPUTATION unspellable; that would make the USE unspellable.
Out of this chunk's class and filed as follow-up work rather than lost.

## Consequences

### Positive

- The claim is now flat and needs no qualifier: `stable_partition(id, some_width)` does not compile
  anywhere outside its module, under `cargo build` and `cargo test` alike.
- No cfg, no feature, no gate, no doc caveat. `compiler first, a check is the fallback`
  (ADR-20260803-234035) applied to a decision already judged worth making — the type system, not a
  grep.
- The width pin got stronger and cheaper: one deliberate test with a stated reason, runnable without
  a database, replacing four incidental ones that mostly needed Postgres.

### Negative

- Twenty-two test call sites now read `declared_lane("Order", &id).expect("Order declares a mailbox")`
  rather than a two-argument call. Wordier, and a test that genuinely wanted a foreign lane would
  now have to write the integer it means. None does today; that is a feature of the current suite,
  not a guarantee about the next one.
- A future test that legitimately needs a lane under a hypothetical width must add a
  `test-fixtures`-gated accessor and argue for it. That is the intended cost.

### Follow-up

- **Not** re-opening item 2 (`mailbox_address`'s vestigial third tuple element). Cut at briefing
  because its width comes from the same declaration as `ACTOR_MAILBOXES`, so a caller using it
  computes the CORRECT lane — a redundant copy of the right answer, not a second opinion. Carried
  forward on #609 as a drive-by for the next change to `tools/codegen-rs/src/emit/actor_clients.rs`.
- `vernon`'s `Lane(i16)` newtype (alternative D) is filed as
  [#612](https://github.com/TheCaptainCompany/captain-food/issues/612).
- The `removed == 3` literal in `pm_prepare_delivery.rs` is the last INDEPENDENT restatement of a
  declared width in that file, and is therefore valuable rather than residual — it must be
  commented, not converted:
  [#617](https://github.com/TheCaptainCompany/captain-food/issues/617).
- `crates/infrastructure/tests/main/mailbox_requeue.rs` seeds a poisoned Cart row with a bare
  literal partition `3` that is not any declared lane for its id. It is a listing fixture, not a
  routing one, and no longer reachable through `stable_partition` — noted, not changed.
