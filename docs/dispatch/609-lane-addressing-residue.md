# Dispatch — #609 "Lane addressing residue after #596: `stable_partition` is still `pub` and `mailbox_address` still carries a vestigial width"

- **Issue**: [#609](https://github.com/TheCaptainCompany/captain-food/issues/609)
- **Base**: `main` @ `2a035ff0966929de5d93c0d0510a1e917ab828aa` ("Lane width comes from the declaration, not a seeded registry (#596) (#607)")
- **Card SHA stamp**: `2a035ff`. Lenses: load this card **at this SHA plus the diff since**. If the tree has moved under you in a way this card does not describe, **discard the card and read the tree** — the card is a snapshot, never an authority over the code.
- **Reversibility class**: **REVERSIBLE INTERNAL** — no stored event shape, no money movement, no legal surface, nothing Tours-facing, no client-visible GraphQL. Routing *behaviour* must not change at all; this chunk is about what remains **spellable**, not about what is spelled.
- **Merge posture**: auto-merge-on-green default. **Not** `HOLD: human` — but see the fence below: if the work turns out to change a routing *value* rather than a routing *surface*, it has left this class and stops for the coordinator.
- **Briefing roster** (3 lenses, per [ADR-20260816-134352](../adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)): `vernon` (lane addressing as a consistency boundary), `beck` (what test fails if the seal does not hold), `holub` (is this the shortest path to value, or named waste).
- **Checkpoint verification** (holub's condition, banked at the checkpoint): the checkpoint goes only to lenses that declared a concern at briefing. At that checkpoint the executor **banks explicitly** whether the narrow set missed anything the full roster would have caught. A MISS reverts REVERSIBLE INTERNAL to the whole roster.

> **Standing caution for this card.** The immediately preceding dispatch (#608) shipped a briefing whose threshold arithmetic was wrong — it derived ~50s from `max_delivery_attempts × retry_spacing_seconds` where the mailbox backoff is exponential (10+20+40+80+160 = 310s). The executor caught it; it was banked as a checkpoint MISS. **Treat every claim in this card as an unverified input.** Where the card asserts a fact about the tree, verify it before relying on it, and say so if it is wrong — a card that is wrong once may be wrong again.
>
> **It was wrong again.** All three briefing lenses independently found the same three factual errors in the first version of this card, corrected below in **§ Briefing corrections**. Read that section before anything else; it changes the shape of the work.

## Briefing corrections — what the first version of this card got wrong

Three errors, each found independently by `vernon`, `beck` and `holub` against the tree at `18481a6`. Two came from the issue text; one was mine.

**(A) `mailbox_address` has a PRODUCTION consumer.** The card said "the only remaining consumer, the test-only `enqueue_worker_command`". False. There are two destructuring sites and one is live: `declared_identity` at `crates/actor_client/src/enqueue.rs:50` (`let Some((_, identity_prop, _)) = mailbox_address(command_type)`), `pub(crate)`, on the typed-door write path. It also discards the width, so the *conclusion* survives — but an executor who greps for `enqueue_worker_command` fixes one site of two.

**(B) The "real decision" about a seam mostly is not one.** The card (following the issue) said sealing needs a new seam and that the golden freeze test makes it a genuine trade. In fact: `mod partition` is **already private** (`crates/actor_client/src/lib.rs:40`); `golden_values_are_frozen` already lives **inside** `partition.rs` calling `super::stable_partition`, so privacy does not touch it; the single escape is the re-export at `lib.rs:71`; there are **zero** `stable_partition` call sites in any `src/` outside `partition.rs` itself; and the repo already owns the precedent seam — the `test-fixtures` feature (PROP-20260802-130500 D5) held by `test_fixtures_feature_never_reaches_a_release_artifact`. Item 1 is far cheaper than advertised.

**(C) The codegen test this card demanded evidence from does not exist.** The card's phase 2 said "codegen test updated to pin the new shape" and its evidence section demanded "emitter emits the old 3-tuple → codegen test red". **Nothing pins the tuple arity.** What exists is `typed_identity_migration_keeps_generated_runtime_byte_identical` — a *drift* test comparing emitter output to the committed artifact, green for any shape once you regenerate, and its failure message names the wrong migration. Do not write a shape test to satisfy the card. The honest shape gate is already the compiler (see the evidence section).

Minor: "the generated table across the client crates" — `mailbox_address` is emitted into exactly one file, `crates/actor_client/src/generated/addresses.rs`, and re-exported (not re-emitted) elsewhere.

## Why this chunk exists

#596 established the finding that matters: **disagreeing copies of the routing constant break one-writer at the addressing function, upstream of any fence.** [ADR-20260816-165714](../adr/ADR-20260816-165714-lane-width-comes-from-the-declaration.md) made `actor_client::declared_lane(actor_type, actor_id)` the single lane accessor and removed the `width` parameter from every routing site.

What is left are two places the constant can still be **reached**. Neither is reachable by accident today. Both are places a future change could reintroduce a second opinion about the width — and the whole point of #596 was that a second opinion about the width is not a bug you find in review, it is a silent misroute.

**What this chunk is NOT justified by.** The first version of this card called it "a compiler-first chunk in the exact sense of [ADR-20260803-234035](../adr/ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md)". `holub` struck that, correctly: **compiler-first is a HOW directive, not a WHETHER directive.** It governs the method of work already judged worth doing. Read as a standing obligation it generates infinite backlog — every `pub fn` in the workspace is a spellable-but-unspelled path — and "level 4 is the FLOOR" would license an endless sealing programme with no user at the end of it.

**What it IS justified by**, and the justification is narrower and better: one week ago, **in this exact function**, review found a hand-copied literal `5` in all 17 generated client crates. That is not hypothetical risk. It is the observed failure mode of this codebase's actual maintainer, this month, in this code. Structure that makes that shortcut unspellable is a safety requirement at the price named below — and the price turns out to be small.

## Scope — REVISED after briefing

**Item 2 is CUT from this chunk.** The card justified it as "the same defect class as item 1 wearing different clothes". That is wrong, and `holub` gave the deciding fact: `mailbox_address`'s width is emitted from the *same declaration* as `ACTOR_MAILBOXES`, so a caller using it would compute the **correct** lane. The #596 defect was an *observed* source (`count(*) FROM mailbox_partitions`) disagreeing with the *declared* one. A redundant copy of the right answer is not a second opinion. `vernon`'s reason for still wanting it is real but is design hygiene, not defect removal: a width is a property of the address *space*, not of an address, and returning it with the address is the shape that invited every caller in #596 to route for itself.

So item 2 is right, and it is not this chunk's. **Do it as a drive-by the next time something is already touching `emit/actor_clients.rs`.** When someone does, three things travel with it, recorded here so they are not rediscovered: both destructure sites are exhaustive tuple patterns with no `..` rest, so dropping the element gives `error[E0308]` at **both** `enqueue.rs:50` and `enqueue.rs:84` — the compiler is the shape gate and no new test is needed; the comment at `enqueue.rs:82-83` must go with the element it describes; and if the tuple becomes a named struct, `tools/codegen-rs/src/tests.rs:6425` pins the exact re-export string and will move. Post this paragraph as a comment on #609 so the issue carries it.

### Item 1 — the only item. Two options, and the executor picks with the mob at the checkpoint.

The hazard is that `stable_partition(&id, some_width)` is spellable by any crate depending on `actor_client`, via the re-export at `crates/actor_client/src/lib.rs:71`. Everything else about the module is already private.

There is a **second, distinct hazard** the briefing surfaced that the issue never mentions, and it decides between the options. `vernon` and `beck` independently counted roughly nineteen out-of-crate call sites — across `actor_client/tests/drift_guard.rs`, `server/tests/graphql_typed_send.rs` and `infrastructure/tests/main/*` — and **almost every one hard-codes the width as a literal**: `stable_partition(&order, 5)`. Those are copies of exactly the constant #596 was about, wearing test clothing. If a declared width moves from 5 to 7, production routes to `N mod 7` while the fixture is inserted at `N mod 5`, and **the test keeps passing against a lane no worker with the new grid serves.** Green build, wrong lane, no error.

**Option A — the cheap seam (holub's scope).** Split `lib.rs:71` so `stable_partition` rides `#[cfg(any(test, feature = "test-fixtures"))]`. Do **not** gate the `fn` itself — only the re-export; gating the function breaks `declared_lane` in release. Every existing out-of-crate call site is in a `tests/` target of a crate that already enables `test-fixtures`, so no test changes. Roughly five lines, one commit.

**Option B — the final vision (vernon + beck's ranking).** Convert the ~19 sites to `declared_lane("Order", &order)`, which *reads the declaration*. `beck` identified the one site that genuinely needs its own width and must keep it: `infrastructure/tests/main/pm_prepare_delivery.rs:1447` (`seeded_two = stable_partition(&uid(ORDER), 2)`) is the **misroute** test and must be able to hold a second opinion — a bare literal is more honest there than any function call. Drive the external count to that one site, or to zero, and then **`stable_partition` simply stops being `pub`: no cfg, no feature, no gate, nothing to enforce.** Roughly twenty test files, mechanical.

**The card's recommendation is B**, on two grounds and with the counter-argument stated fairly:

- [ADR-20260808-235113](../adr/ADR-20260808-235113-final-vision-first-no-intermediate-steps.md) — no intermediate step where the final step can be built. Option A is the intermediate: it leaves a cfg, an export, and nineteen stale literals, and `beck` shows it needs a *new* level-3 AST assertion to stop one unreviewed line from removing the cfg. Option B **deletes** machinery instead of adding it, and needs no assertion because there is nothing left to assert.
- Option A cannot make an honest unqualified claim; Option B can. See the resolver-v2 finding in the evidence section — under Option A the seal is real only for release artifacts, and anyone verifying it with `cargo test` gets a false negative.
- **Against**: `holub` priced this chunk at five lines and explicitly warned about scope creep at the checkpoint, in an area that has now absorbed four consecutive chunks. Twenty test files is four times his scope. He declared a concern and asked to read the diff line count. **He is right that this is the risk**, and B is only worth it if it genuinely ends the file on this area rather than opening a fifth chunk.

**The executor picks, and BANKS the choice at the checkpoint with all three lenses present** (all three declared concerns). If B turns out to be more than mechanical — a site where `declared_lane` cannot express what the test means, beyond the known misroute one — that is the signal to fall back to A and say so. Note `holub`'s standing conditional: if Option A's `test-fixtures` split does **not** work as described (a caller in a crate that does not enable the feature), his verdict flips to *close the issue as decided-not-worth-it*, and that is a legitimate landing place for this chunk.

**Before touching the ~19 sites under Option B**, `vernon`'s caveat: check each for one that *deliberately* stamps a wrong or foreign partition. Where a test wants a misroute, it must keep one.

## Phases (commit at each — a dead agent costs one phase, not the chunk)

1. **Claim** — `status/in-progress` label, claim comment naming the branch `609-lane-addressing-residue` and this session link. Branch from `main`. Draft PR whose body starts with `Closes #609`. Post the item-2 carry-forward paragraph as a comment on #609.
2. **Decide A or B** against the tree, then execute it. Under B: convert the sites, then drop `pub`. Under A: split the re-export, then add the level-3 AST assertion `beck` requires and mutation-test it. Commit.
3. **Evidence + records**: the mutations below; resize the now-false prose claim at `partition.rs:31-35` (see below); `docs/STATUS.md` line; the resolver-v2 finding into `docs/claude/sessions.md`. Commit.
4. **Gates green → ready for review + auto-merge enabled as one indivisible step → supervise to MERGED.**

`holub` also struck the ceremony in the first version of this card, and he is right in proportion to whichever option lands: under Option A this is a one-commit PR with a one-paragraph body and **no ADR** ([proportionality](../../CLAUDE.md), founder directive 2026-07-31 — a small subject with no real decision gets neither proposal nor ADR). Under Option B the A-vs-B choice is a decision without alternatives once made, so a short ADR is proportionate; still no proposal.

## Gates

- `make rust` — **0 errors**. Warnings are the validator's ratchet: if this legitimately moves the surface, run `make warning-baseline` and commit the refreshed artifact **in the same commit**, and say in the PR body why the added warning is accepted.
- `make test-crates` — no new failures.
- The codegen gate must show **no spec↔generation drift**. Note the operational trap recorded in [sessions.md](../claude/sessions.md): `check-drift` is a whole-tree `git diff --quiet` and **names the wrong cause** when something unrelated is dirty. If it fires, check what is actually dirty before believing its message.
- No `specs/**` change is expected. If you find you need one, that is a scope signal — stop at the checkpoint and say so.

## Evidence required in the PR body — specified by `beck`, and the honesty conditions are not optional

**Mutations named as the semantic edit and the expected failure message — never as line numbers.** Both directions: applied → red, reverted → green. Measure in an **isolated worktree**; a mutation verdict measured against a tree someone else is editing is not a verdict (recorded this session, the hard way).

**M1 — "the frozen routing function drifted".** In `partition.rs`, change the FNV prime `0x00000100000001b3` → `0x00000100000001b5`. Expect `golden_values_are_frozen` red with a value mismatch. **Report the asymmetry, because the asymmetry is the evidence**: `declared_lane_reads_the_declaration_and_refuses_the_undeclared` and `stays_in_range` both stay **green** under this mutant — they compare things that move together. The golden is the only freeze. "The partition tests went red" without naming which one has measured nothing.

**M1b — the copy detector.** M1 cannot detect a copy: point a test at a re-implementation and both sides of a lane comparison move together. Replace the body of `stable_partition` with `unimplemented!("copy detector")`, keep the signature. Every lane-computing test must panic. **Any lane-computing test that stays green is, by construction, not calling the real function.** Honesty condition: most of those sites are DB-gated and open with `let Some(db) = TestDb::acquire(..) else { return }` — they report `ok` with no Postgres. Run with `DB_TESTS_REQUIRED=1` against a live database, **or state plainly which sites the detector did not reach**. "cargo test was green/red" without that variable is not a verdict here.

**M2 — the seal, and the exact line between evidence and theatre.** Plant `pub fn second_opinion(id: &uuid::Uuid) -> i16 { actor_client::stable_partition(id, 2) }` in a **production** source file of a crate that already depends on `actor_client` and is not behind any `cfg` — `crates/infrastructure/src/persistence/mailbox_lanes.rs`. Run **`cargo build -p infrastructure`**, not `cargo test`. Quote the real compiler error.

> **Then run `cargo test -p infrastructure` on the same mutant and report that it COMPILES.** Under Option A it will. Resolver v2 (`Cargo.toml:8`) unifies the dev-dependency's `test-fixtures` grant into the single `actor_client` unit the lib links against during a test build. So Option A's true, defensible claim is: *unspellable in any release artifact; still spellable from the lib of a crate whose dev-dependencies light `test-fixtures`, when built under `cargo test`.* That is level 4 for the shipped binary, backstopped at level 3 by the D5 manifest guard. **It is not "unspellable" flat, and the PR body must not round it up.** Anyone verifying the seal with `cargo test` gets a false negative. Under Option B the qualifier disappears, because the export does.

**Theatre `beck` expects and rejects, in likelihood order**: planting the caller in a crate that does not depend on `actor_client` (`domain`, `domains/*`, `web`) — you get `E0433`, which would have fired identically before this chunk, and it is the most likely fake; planting it in a `tests/` file — that compiling is the seam working as designed, not a breach; planting it inside `crates/actor_client/src/` — in-crate access to a private module was always legal.

**No gate is subsumed, so do not manufacture one to delete.** `beck` searched `tools/codegen-rs/src/tests.rs` and `.claude/settings.json`: nothing today polices a width at a call site. The card's earlier "delete the gate the compiler subsumes" line had no referent.

**One prose claim becomes false and must be RESIZED, not upgraded.** `partition.rs:31-35` records that `stable_partition` "stays `pub` for tests … remains SPELLABLE … the claim is kept at its true size here" — itself the residue of #596's review catching an over-claim. After this chunk it is false. Under Option A, resize it to the release-artifact claim above and name `test_fixtures_feature_never_reaches_a_release_artifact` as what holds it. Under Option B, delete it — there is nothing left to qualify.

## Fences — not yours to move

- **Routing behaviour is frozen**, and `vernon` found the first version of this fence stated correctly but **incompletely**. Three additions:
  - The durable contract is the **stored column**, not the function. The failure mode is not "`declared_lane` returns a different number" but "an in-flight `inbound_messages.partition` becomes unreachable". Identical today because `declared_lane` is the sole producer — but stating it against the column is what makes it checkable, and `lease.rs:61-62` already ships the detector (`GROUP BY actor_type, actor_id HAVING count(DISTINCT partition) > 1`).
  - **The `None` path is routing behaviour too.** `declared_lane("NotAnActor", ..) == None` is asserted at `partition.rs:71` precisely so a wiring bug is not a silent lane 0. **The undeclared outcome is frozen, not only the `Some` value.** Any refactor that acquires a `Default` turns undeclared into lane 0 for free — a system that looks perfectly healthy under every single-aggregate test.
  - **"For every actor" needs teeth.** `MailboxSupervision` (width 1) is the only actor that can distinguish "reads the declaration" from "says 5". A before/after run over the width-5 actors proves nothing; the evidence must include MailboxSupervision.
- **Seeding must stay disjoint from addressing.** `seed_partitions(pool, actor_type, width)` (`actor_runtime/src/lease.rs:41`) decides which lanes *exist* and is the supervisor's business — it reads `ACTOR_MAILBOXES` and never `mailbox_address`. If the width feeding seeding ever diverged from the width feeding routing you would route to lane 4 with lanes 0..2 seeded: no lease covers it, no worker drains it, the message sits forever and **nothing errors**. Verified disjoint today; confirm it stays that way.
- **Never weaken a gate, never hand-edit generated output.**
- Do not touch `crates/infrastructure/src/mailbox/**`. PR #610 (#608, birth-gap detector) is in independent review against that path and is not yet merged. Note `standalone.rs:420` pulls a `width` from `ACTOR_MAILBOXES` in production — that is consumer-side seeding, it does not call `stable_partition`, and item 1 neither breaks it nor gives you a reason to cross this fence.
- No second issue in this run.

## Findings

_(Lenses and the executor append here. Empty is not the same as unread — a lens with nothing to say writes "nothing in my lens".)_

**Briefing, `18481a6`** — all three lenses DECLARED A CONCERN, so all three are at the checkpoint.

- **`vernon`** — declared. Wants at the checkpoint: (i) which option landed and whether the ~19 test-site width literals survived it; (ii) the `None`-path and MailboxSupervision coverage in the frozen-routing evidence. Both cheap to get wrong and invisible in a green build. Also notes the genuinely level-4 form of this whole area, priced but not scoped here: make the *use* unspellable rather than the *computation* — `declared_lane -> Lane(i16)` newtype with a private constructor and `MailboxEntry.partition: Lane`, after which a hand-rolled FNV compiles and is unusable. **Likely exceeds this class; worth a follow-up issue rather than silent loss.**
- **`beck`** — declared. Watching for two evidence-honesty failures: the `cargo build` / `cargo test` split in M2 being rounded up to "unspellable", and byte-identity being reported as a shape pin. Both are the failure class this card's own standing caution is about.
- **`holub`** — declared. Watching for scope creep: item 2 getting done anyway because a card once listed it, the tuple becoming a struct, or a `#[doc(hidden)]`/new-gate variant being built when the existing seam covers it. Wants to read the diff line count. Standing conditional above. Flow observation banked: **four consecutive chunks (#588, #596, #598, #609) have all been in `actor_client`/`mailbox`** — the runtime has absorbed the team's last four dispatches, and the next release should answer a question with a Tours restaurant or rider at the end of it. He could not reach the backlog from his sandbox and declined to guess an alternative; **that is a coordinator action item, not his.**

## Findings

_(Lenses and the executor append here. Empty is not the same as unread — a lens with nothing to say writes "nothing in my lens".)_
