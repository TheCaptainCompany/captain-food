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

**Executor, claim (`7b9e1da`)** — claimed on branch `609-lane-addressing-residue`, draft PR [#613](https://github.com/TheCaptainCompany/captain-food/pull/613), item-2 carry-forward posted on #609.

_(Card defect, minor: the amendment left the `## Findings` heading duplicated — the empty second copy is removed here.)_

### Checkpoint verification (holub's condition) — **MISS**, one, narrow and attributable

Banked honestly rather than conveniently, because a MISS is the outcome that carries information.

**Yes — one thing: the four assertion sites were incidentally pinning their actors' declared
widths**, and Option B removes that pin. Nothing else in the repository pinned Cart's or Order's
width. A declared width is a contract over **stored rows** — changing it re-lanes every
non-terminal `inbound_messages` row, which is why ADR-20260802-220402 needed a migration and only
worked because 5 divides 100. Had it gone unnoticed, Option B would have weakened a gate to get
green while every gate reported green.

**The attribution is SHARED, and weighted to `vernon` — not handed to an absent lens.** This first
read as a roster-width failure, and the review corrected it, correctly. `vernon`'s briefing finding
(c) named these exact literals and observed that they were coupled to the declaration. He was on the
surface he claimed, with the fact in hand, and read the coupling as a **liability** without taking
the one further step to reading it as a **pin**. That is a depth miss by a lens who was present, and
calling it anything else makes roster sizing look like the cause when it was not.

What is genuinely `young`'s — and `young` was not at the briefing — is only the **escalation**: that
a declared width is a contract over stored rows, so removing its pin is a *gate weakening* under
ADR-20260802-220402 and therefore a stop, not a follow-up. Absent that framing this lands as "we
lost some coverage, file it"; with it, it blocks the merge. So the roster width cost the SEVERITY,
and an invited lens missed the FACT.

**Everything else that was wrong was missed by invited lenses on their own turf**: `beck` asserted
the misroute site must hold a second opinion (it never stamps one), and both `vernon` and `beck`
counted the call sites and got both numbers wrong. Inviting `young` would not have fixed `beck`'s
count.

Recommendation, offered rather than taken, and cheaper than reverting the class: `vernon`'s own
generalisation of what he missed — **when a chunk's method is "make X unspellable", every existing
spelling of X is a candidate incidental pin; enumerate what each one was holding before deleting
it.** One line in the card template, and it generalises past this area, where a fourth lens is a
per-chunk cost forever.

### Executor findings — the card was wrong a third time, in three more places

The card's own standing caution asked for this, so it is reported rather than absorbed. All three
were found by verifying a claim before relying on it; the first is the one that would have cost an
executor most.

**(D) Option A DOES NOT COMPILE as the card specifies it.** The card says, emphatically: *"Do **not**
gate the `fn` itself — only the re-export"*. `crates/actor_client/Cargo.toml:74` sets
`unreachable_pub = "deny"` — the boundary crate's own policy that a `pub` item nobody outside uses is
a defect. Gating only the re-export leaves `pub fn stable_partition` unreachable in a release build:

```
error: unreachable `pub` item
  --> crates/actor_client/src/partition.rs:15:1
   | help: consider restricting its visibility: `pub(crate)`
   = note: requested on the command line with `-D unreachable-pub`
```

A working Option A must open with `#[cfg_attr(not(any(test, feature = "test-fixtures")), allow(unreachable_pub))]`
— i.e. suppress the lint that was already arguing for Option B. This does not fire `holub`'s standing
conditional (that was about a caller in a crate without the feature; **every** existing out-of-crate
caller is in a `tests/` target of a crate whose dev-dependencies do enable `test-fixtures` —
`infrastructure/Cargo.toml:78`, `server/Cargo.toml:115`, `actor_client/Cargo.toml:59` — so Option A's
call-site claim was correct). But it moves Option A from "five lines" to "five lines plus a lint
suppression", which is a worse trade than the card priced. Cost: two rebuild cycles.

**(E) The counts are wrong in both directions, and the file count is the one that mattered.** Not
"roughly nineteen out-of-crate call sites" across "roughly twenty test files": **23 sites across 8
files** (22 converted, 1 removed). `holub` was asked to price B at four times his scope; the true
figure is eight files, and the landed diff is **+137 / −54 over 10 files** — the line count he asked
to read at the checkpoint. Under-counting sites while over-counting files by 2.5× made B look like a
sprawl when it is one afternoon of mechanical edits.

**(F, and CORRECTED at the checkpoint) The misroute site does not need to hold a second opinion —
but the first statement of why over-claimed.** The card (following `beck`) says
`pm_prepare_delivery.rs:1447` *"is the **misroute** test and must be able to hold a second opinion — a
bare literal is more honest there than any function call"*. Reading the test: `seeded_two` is never
stamped on any row. It appears in exactly two places — a guard `assert_ne!(declared, seeded_two)` and
a diagnostic message. **The test asserts the ABSENCE of a misroute; it never produces one.** So the
sharper guard needs no second width at all: `declared >= SEEDED_LANES`.

**`beck` corrected the justification at the checkpoint, by computing rather than reasoning, and he
is right.** The first version of this section said the new guard "holds for every id". It does not:
`declared ∈ {0,1,2,3,4}` and two of those five values fail it — he computed FNV-1a-64 over
`uid(ORDER)` and got lane 3, so the assertion is still falsifiable on the id under test and still
has to be re-checked if `ORDER` changes. **What is universal is the IMPLICATION, not the
assertion**: for any id, a producer that believes the keyspace is 2 wide can only stamp 0 or 1, so
*any* declared lane at or above 2 is unreachable under the narrowed keyspace. What the reformulation
actually buys is that it names the seeded KEYSPACE the row must avoid instead of one arbitrary lane
inside it. That distinction is exactly what separates this from #608's negative control, which
genuinely did go vacuous, so the code comment now states it precisely. `vernon`'s caveat was run
over all 23 sites and **no site deliberately stamps a foreign lane**, so the external count went to
**zero**, not to one.

### The thing neither the card nor the issue noticed, and it is a real cost of Option B

**Four assertion sites were INCIDENTALLY pinning their actors' declared widths.**
`assert_eq!(row.partition(), stable_partition(&cart_id, 5))` compares a production stamp to a
literal, so moving Cart's declared width to 7 turns it red. Convert both sides to `declared_lane` and
they move together: the pin is gone. Nothing else in the repo pinned Cart's or Order's width —
`declared_lane_reads_the_declaration_and_refuses_the_undeclared` covers only PlaceOrderProcess and
MailboxSupervision. Since a declared width change is a **migration**
([ADR-20260802-220402](../adr/20260802-220402-mailbox-width-100-to-5.md) had to remap every
non-terminal in-flight row, and only worked because 5 divides 100), losing that silently would have
been Option B weakening a gate to get green.

So B carries `every_declared_width_is_the_standard_one_because_changing_one_is_a_migration` in
`partition.rs`. Not scope creep — the compensation that makes B non-weakening, and a net gain: it
pins all 17 actors instead of 3, names the reason, and runs on every `cargo test` where three of the
four incidental pins needed a Postgres attached. It pins the SHAPE (`5`, except `MailboxSupervision`
at `1`), so a new actor on the standard width costs nothing to maintain.

**Say precisely which half moved, because "replaces the four assertions" overstates it.** Those
assertions were carrying TWO facts: (i) the declared width literal, and (ii) that the production
stamping path agrees with the routing function for this actor type and this id. **The new unit test
restores only (i).** (ii) is not lost — the converted assertions still compare a **real production
stamp** against `declared_lane`, so the producer-independence is intact; what changed is that the
expected side reads the declaration instead of a literal. That is a *better* pin than `5` ever was:
a wrong actor-type string now yields `None` and a panic naming the actor, where the literal would
have produced a plausible wrong lane. `vernon`'s summary at the checkpoint is the accurate one —
**"not the same guarantee, the missing half of it, deliberately placed and widened."**

**Anti-blindness on the new gate — found independently by `vernon` and `beck`, fixed before ready.**
The first version pinned the shape but not the roster, and a loop over a generated slice has two
ways to pass by scanning nothing, both reachable from an ordinary `specs/*/actors.yaml` edit. It now
carries a FLOOR (`declared.len() >= 17`) and a SEED (`any(|(a, w)| *a == "MailboxSupervision" && *w == 1)`)
before the loop, in the idiom this repo already uses at `tools/codegen-rs/src/tests.rs:4157`
(*"if it is renamed, this guard must move with it, never silently scan for nothing"*) and `:3663`.
Neither costs anything when a new standard-width actor is declared.

### Evidence — every mutant measured in an isolated worktree, against a live Postgres

`DATABASE_URL` pointed at a dedicated `cf609` database, `DB_TESTS_REQUIRED=1`, and the baseline run
reported **1252 passed / 0 failed with no DB-skip receipt** — so the copy detector below reached the
DB-gated sites rather than reporting `ok` past them.

**M1 — the frozen routing function drifted.** FNV prime `0x00000100000001b3` → `...b5`.
`golden_values_are_frozen` RED: `assertion left == right failed / left: 93 / right: 21`
(`partition.rs:87`). **The asymmetry is the evidence**: `declared_lane_reads_the_declaration_and_refuses_the_undeclared`,
`stays_in_range` and the new width pin all stay **GREEN** — the first two compare things that move
together, and the third reads the declaration table, not the hash. The golden is the only freeze, and
a width pin is not a hash pin.

**M1b — the copy detector.** `stable_partition` body → `unimplemented!("copy detector")`, signature
kept. Over `actor_client` + `infrastructure` + `server` with the live database: **61 named tests
failed**, and **all 22 tests in the 8 converted files are among them** — every one of
`mailbox_acceptance_timeout` (3), `mailbox_activations` (3), `mailbox_requeue` (1),
`mailbox_retention` (1), `standalone_workers` (1), `pm_prepare_delivery` (12), plus
`typed_send_lands_the_command_entry_row_and_keeps_the_acceptance_contract` (graphql_typed_send) and
`typed_send_builds_the_same_row_as_enqueue_worker_command` / `typed_schedule_parks_a_command_row_and_cancel_withdraws_it_once`
(drift_guard). **No lane-computing test stayed green**, so no converted site is quietly
re-implementing the hash. Sites the detector did NOT reach: none in the converted set. The one
partition unit test that survives is the width pin, correctly — it never calls the hash.

**W1/W2/W3 — the NEW gate seen red, because a brand-new gate nobody has seen fail is not a gate.**
`beck`'s bar applies to the test that replaces a pin as much as to the pin. All three mutate
`ACTOR_MAILBOXES` in the generated slice, which is exactly what the corresponding
`specs/*/actors.yaml` edit emits.

| mutant | before the fix | after the fix | message |
|---|---|---|---|
| **W1** `("Order", 5)` → `("Order", 10)` | red | **red** | `'Order' declares width 10, not 5: changing a declared width re-lanes every in-flight row of that actor, so it needs a migration (ADR-20260802-220402)` |
| **W2** `MailboxSupervision` renamed | **GREEN** — match arm dies, loop asserts `5 == 5` sixteen times | **red** | `no actor named 'MailboxSupervision' declares width 1 any more. If it was RENAMED that is fine — update this seed … do not delete the test` |
| **W3** `ACTOR_MAILBOXES: &[]` | **GREEN** — zero iterations | **red** | `ACTOR_MAILBOXES is down to 0 entries from 17. Removing a mailbox actor is a real decision (its in-flight rows have nowhere to drain)` |

**M2 — the seal, both build modes, both options.** Mutant planted in a **production** source file of
a crate that already depends on `actor_client` and behind no `cfg`:
`crates/infrastructure/src/persistence/mailbox_lanes.rs`, `pub fn second_opinion(id: &uuid::Uuid) -> i16 { actor_client::stable_partition(id, 2) }`.

| | `cargo build -p infrastructure` | `cargo test -p infrastructure` |
|---|---|---|
| **Option B (landed)** | `error[E0425]: cannot find function stable_partition in crate actor_client` | **same error** — `could not compile infrastructure (lib)` and `(lib test)` |
| **Option A (counterfactual, working form)** | `error[E0425]: cannot find function stable_partition in crate actor_client` | **`Finished` — the mutant COMPILES and links** |

**`beck`'s resolver-v2 prediction is CONFIRMED**, and it is the sharpest argument against A: resolver
v2 (`Cargo.toml:8`) unifies the dev-dependency's `test-fixtures` grant into the single `actor_client`
unit the lib links against during a test build, so under Option A anyone verifying the seal with
`cargo test` gets a **false negative**. Option A's defensible claim would have been *"unspellable in
any release artifact; still spellable from the lib of a crate whose dev-dependencies light
`test-fixtures`, under `cargo test`"*. **Under Option B the qualifier is gone, verified rather than
argued**, so the flat claim is the one this PR makes. Filed into
[sessions.md](../claude/sessions.md).

**Theatre avoided, as `beck` listed it**: the caller was NOT planted in a crate that lacks the
dependency (which would give `E0433` and prove nothing), NOT in a `tests/` file, and NOT inside
`crates/actor_client/src/`. No gate was manufactured to delete — `beck` was right that nothing today
polices a width at a call site.

### Fences

- **Routing behaviour is unchanged.** No production source file is touched: the diff is
  `partition.rs` (visibility + docs + one test), `lib.rs` (the re-export) and 8 test files. The
  stored `inbound_messages.partition` column keeps taking exactly the values it took before —
  `declared_lane` is still the sole producer and its body is untouched.
- **The `None` path is intact**: `declared_lane("NotAnActor", ..) == None` still asserted
  (`partition.rs:77`), and nothing acquired a `Default`. **MailboxSupervision is covered twice** now
  — the existing `Some(0)` assertion, plus the new width pin, which is the only test in the repo that
  would notice its width silently becoming 5.
- **Seeding stays disjoint from addressing**: `seed_partitions` (`actor_runtime/src/lease.rs:41`) and
  `standalone.rs:420` read `ACTOR_MAILBOXES` directly and never called `stable_partition`. Confirmed
  unchanged; neither appears in the diff.
- **`crates/infrastructure/src/mailbox/**` untouched** — PR #610 is in independent review there.
  Verified by `git diff --name-only main`.
- **No `specs/**` change.** `make rust` green, `check-drift` clean.

### Adjacent, noted and NOT fixed (for the architect to file or discard)

- `crates/infrastructure/tests/main/mailbox_requeue.rs:90` seeds a poisoned Cart row with a bare
  literal partition `3`, which is not the declared lane for its actor id. It is a listing fixture,
  not a routing one, and it is no longer reachable through `stable_partition` — but it is the same
  literal-lane class. Left alone deliberately (`holub`'s scope-creep concern).
- `crates/actor_client/src/enqueue.rs:22` imports `ACTOR_MAILBOXES` and never uses it — a
  pre-existing `unused_imports` warning on `main`, not introduced here. Verified against `main`
  before assuming.
- [ADR-20260816-165714](../adr/ADR-20260816-165714-lane-addressing-is-declared-not-observed-and-an-unseeded-lane-must-wait.md)
  (#596/#607) was never added to `docs/adr/README.md`. Added in this change alongside the new ADR,
  since the index row was one line and an unindexed ADR is an ADR nobody finds.
- `vernon`'s `Lane(i16)` newtype with a private constructor (make the USE unspellable, not just the
  COMPUTATION) — genuinely the next level and genuinely outside this class. Filed as
  [#612](https://github.com/TheCaptainCompany/captain-food/issues/612).
- The `removed == 3` literal in `pm_prepare_delivery.rs` — the one width literal the reformulation
  did NOT de-literalise, and `beck` is right that it should stay: it is now the last INDEPENDENT
  restatement of PlaceOrderProcess's declared width in that file, so it stays red on a silent width
  change where the converted sites cannot. The fix is one clause of comment saying so, before
  someone "cleans it up". Filed as
  [#617](https://github.com/TheCaptainCompany/captain-food/issues/617).
