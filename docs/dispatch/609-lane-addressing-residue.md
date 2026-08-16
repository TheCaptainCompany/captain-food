# Dispatch — #609 "Lane addressing residue after #596: `stable_partition` is still `pub` and `mailbox_address` still carries a vestigial width"

- **Issue**: [#609](https://github.com/TheCaptainCompany/captain-food/issues/609)
- **Base**: `main` @ `2a035ff0966929de5d93c0d0510a1e917ab828aa` ("Lane width comes from the declaration, not a seeded registry (#596) (#607)")
- **Card SHA stamp**: `2a035ff`. Lenses: load this card **at this SHA plus the diff since**. If the tree has moved under you in a way this card does not describe, **discard the card and read the tree** — the card is a snapshot, never an authority over the code.
- **Reversibility class**: **REVERSIBLE INTERNAL** — no stored event shape, no money movement, no legal surface, nothing Tours-facing, no client-visible GraphQL. Routing *behaviour* must not change at all; this chunk is about what remains **spellable**, not about what is spelled.
- **Merge posture**: auto-merge-on-green default. **Not** `HOLD: human` — but see the fence below: if the work turns out to change a routing *value* rather than a routing *surface*, it has left this class and stops for the coordinator.
- **Briefing roster** (3 lenses, per [ADR-20260816-134352](../adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)): `vernon` (lane addressing as a consistency boundary), `beck` (what test fails if the seal does not hold), `holub` (is this the shortest path to value, or named waste).
- **Checkpoint verification** (holub's condition, banked at the checkpoint): the checkpoint goes only to lenses that declared a concern at briefing. At that checkpoint the executor **banks explicitly** whether the narrow set missed anything the full roster would have caught. A MISS reverts REVERSIBLE INTERNAL to the whole roster.

> **Standing caution for this card.** The immediately preceding dispatch (#608) shipped a briefing whose threshold arithmetic was wrong — it derived ~50s from `max_delivery_attempts × retry_spacing_seconds` where the mailbox backoff is exponential (10+20+40+80+160 = 310s). The executor caught it; it was banked as a checkpoint MISS. **Treat every claim in this card as an unverified input.** Where the card asserts a fact about the tree, verify it before relying on it, and say so if it is wrong — a card that is wrong once may be wrong again.

## Why this chunk exists

#596 established the finding that matters: **disagreeing copies of the routing constant break one-writer at the addressing function, upstream of any fence.** [ADR-20260816-165714](../adr/ADR-20260816-165714-lane-width-comes-from-the-declaration.md) made `actor_client::declared_lane(actor_type, actor_id)` the single lane accessor and removed the `width` parameter from every routing site.

What is left are two places the constant can still be **reached**. Neither is reachable by accident today. Both are places a future change could reintroduce a second opinion about the width — and the whole point of #596 was that a second opinion about the width is not a bug you find in review, it is a silent misroute.

This is a **compiler-first** chunk in the exact sense of [ADR-20260803-234035](../adr/ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md): the question is not "can we add a lint that catches a second caller" but "can the type system make a second caller **unspellable**". Level 4 is the floor. If sealing lands, a gate that would have policed the same thing should be deleted, not kept alongside.

## Scope — two independently landable items

### Item 1 — `stable_partition` is still `pub` and re-exported

`crates/actor_client/src/partition.rs`, re-exported from `crates/actor_client/src/lib.rs`. After #596 it is not *spelled* anywhere outside `declared_lane` itself and test code — but `stable_partition(&id, some_width)` remains *spellable* by any crate that depends on `actor_client`.

The tension the issue names honestly, and which this dispatch does **not** pre-resolve:

- Tests legitimately compute an expected lane with it, and its **golden-value freeze test is the guard on the frozen routing function** — that test must keep working, and it must keep testing the real function, not a copy.
- Sealing therefore needs a seam: a `#[cfg(test)]`/`#[doc(hidden)]` export, a test-only feature, a sealed-trait witness, or moving the golden test inside the module. **Which seam is a real decision, not an obvious cleanup.**

**The executor picks the seam and states why**, ranked by the compiler-first ladder (PROP-20260802-130500 §1). Prefer the option where a would-be second caller **fails to compile** over one where it merely fails a lint. If after looking at the tree the honest answer is that no seam is worth its cost, **say that and leave item 1 undone with the reasoning recorded** — a well-argued "not worth it" closes the issue's item as decided, and is a better outcome than a seam nobody can maintain. Do not leave it silently untouched.

### Item 2 — `mailbox_address()` returns a width element nothing reads

Signature today: `Option<(&'static str, Option<&'static str>, u16)>`, emitted by `tools/codegen-rs/src/emit/actor_clients.rs`. The only remaining consumer, the test-only `enqueue_worker_command`, explicitly discards it: `let Some((actor_type, _, _)) = ...`.

Removing the element touches the emitter, the generated table across the client crates, and the codegen test that pins the emitted shape. Mechanical, but it regenerates artifacts — so: **change the emitter and regenerate, never hand-edit `specs/generated/**` or generated crate output.**

A vestigial width in a public return type is the same defect class as item 1 wearing different clothes: it is a width available to a caller who has no business having one. Consider whether the tuple should become a named struct while you are in there — but only if that does not widen the diff past what the codegen test can pin honestly.

## Phases (commit at each — a dead agent costs one phase, not the chunk)

1. **Claim** — `status/in-progress` label, claim comment naming the branch `609-lane-addressing-residue` and this session link. Branch from `main` @ `2a035ff`. Draft PR whose body starts with `Closes #609`.
2. **Item 2 first** (mechanical, proves the regeneration loop is clean before the interesting part): emitter change, regenerate, codegen test updated to pin the new shape. Commit.
3. **Item 1**: the seam, with the ranked justification in the commit message. The golden-value freeze test must still exercise the real `stable_partition`. Commit.
4. **Evidence + records**: mutation-red evidence (below), `docs/STATUS.md` line. Commit.
5. **Gates green → ready for review + auto-merge enabled as one indivisible step → supervise to MERGED.**

If item 1 resolves to "not worth sealing", phase 3 becomes a recorded decision instead of a diff — an ADR if it is a decision without alternatives, or a line in the PR body if it is smaller than that ([proportionality](../../CLAUDE.md), founder directive 2026-07-31). Do not manufacture a proposal for it.

## Gates

- `make rust` — **0 errors**. Warnings are the validator's ratchet: if this legitimately moves the surface, run `make warning-baseline` and commit the refreshed artifact **in the same commit**, and say in the PR body why the added warning is accepted.
- `make test-crates` — no new failures.
- The codegen gate must show **no spec↔generation drift**. Note the operational trap recorded in [sessions.md](../claude/sessions.md): `check-drift` is a whole-tree `git diff --quiet` and **names the wrong cause** when something unrelated is dirty. If it fires, check what is actually dirty before believing its message.
- No `specs/**` change is expected. If you find you need one, that is a scope signal — stop at the checkpoint and say so.

## Evidence required in the PR body

**Mutation-red evidence, named as the semantic edit and the expected failure message — never as line numbers** (beck's rule, `docs/claude/sessions.md`). Both directions: mutant applied → red, mutant reverted → green.

At minimum:

- **"Change the frozen routing function's output for one known input"** — the golden-value freeze test must go red with a value mismatch. This proves item 1's seam did not accidentally point the test at a copy of the function instead of the function.
- **"Reintroduce a second caller that computes a lane with its own width"** — under the chosen seam this should **fail to compile**; quote the compiler error. If it compiles and only a gate catches it, say so plainly: that is level-3 evidence being reported against a level-4 claim, and the PR body must not round it up.
- For item 2: **"Have the emitter emit the old 3-tuple"** — the codegen test must go red naming the shape mismatch.

Measure mutations in an **isolated worktree**. A mutation verdict measured against a tree someone else is editing is not a verdict (recorded this session, the hard way).

## Fences — not yours to move

- **Routing behaviour is frozen.** `declared_lane` must return the same lane for the same `(actor_type, actor_id)` before and after this chunk, for every actor. If any change you make moves a lane value, **stop** — that is a data-routing change, it leaves REVERSIBLE INTERNAL, and it comes back to the coordinator as its own decision.
- **Never weaken a gate, never hand-edit generated output.** If a gate is subsumed by the compiler after item 1, *deleting* it is the correct outcome — but say which gate and why the compiler now covers it.
- Do not touch `crates/infrastructure/src/mailbox/**` in this chunk. PR #610 (#608, birth-gap detector) is in independent review against that path and is not yet merged; overlapping there buys a conflict for no reason.
- No second issue in this run.

## Findings

_(Lenses and the executor append here. Empty is not the same as unread — a lens with nothing to say writes "nothing in my lens".)_
