# ADR-20260816-165714 — Lane addressing is DECLARED, never observed; an unaddressable lane WAITS, it does not fail

**Status**: Accepted · **Date**: 2026-08-16 ·
**Deciders**: the team (mob, FULL briefing roster — reversibility class IRREVERSIBLE-ADJACENT,
money path), executor-authored ·
**Realizes**: [#596 "chain_pm_copy_in_tx reads lane width from a seeded registry and errors at zero — an unseeded worker fails a paid order's saga"](https://github.com/TheCaptainCompany/captain-food/issues/596) ·
**Dispatch card**: [`docs/dispatch/596-chain-lane-width-declared.md`](../dispatch/596-chain-lane-width-declared.md) ·
**Related**: [ADR-20260803-234035 "Compiler first; a check is the fallback"](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) ·
[ADR-20260730-234918 "actor_runtime is extraction-ready; the routing function is a frozen contract"](ADR-20260730-234918-actor-runtime-extraction-ready.md) ·
[ADR-20260810-231300 "No polling, only pushing"](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md) (the monitoring carve-out this leans on) ·
[#608 "Nothing detects an authorized payment with no order birth"](https://github.com/TheCaptainCompany/captain-food/issues/608) ·
[#609 "Lane addressing residue after #596"](https://github.com/TheCaptainCompany/captain-food/issues/609) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## Lead with the worst of it: this produced a permanently-failed authorization

`chain_pm_copy_in_tx` derived the target actor's lane keyspace width from
`SELECT count(*) FROM mailbox_partitions WHERE actor_type = $1` and returned
`sqlx::Error::Protocol` at zero — **inside the completion transaction that records an inbound
Stripe fact**. The registry is a runtime artifact: nothing writes it but `MailboxWorker::seed` at
worker startup, and migrations never touch it. So "the target actor's worker has not started" made
the recording of an authorization Stripe had already accepted fail.

That much the issue said. What it did **not** say, and what the red tests surfaced, is where that
error goes. A completion transaction that fails takes the **poison path** (`worker.rs`):

- **below the attempt cap** it returns `Err` and holds the **Payment lane** head-of-line — every
  subsequent payment fact on that lane waits behind it;
- **at the cap** it flips **the authorization row itself** to terminal `FAILED`.

So the defect did not merely delay a saga hop. It converted "a worker has not started" into a
**paid customer whose order can never be born, even after the worker comes up** — the row that
would have chained is terminal, and nothing re-derives it. The money moved, the hold is on the
customer's card, and the only remaining evidence is a `FAILED` mailbox row. That is the product's
named worst failure mode, reached from an operational non-event.

The residue this leaves — an authorized payment with no order birth, sitting in a state nothing
watches — is exactly what **[#608](https://github.com/TheCaptainCompany/captain-food/issues/608)**
exists to detect. #608 is not scoped here; this ADR is one of the causes it would have caught.

## And it was a one-writer violation, not a queueing nuisance

Reclassified independently by `vernon`, `young` and `dba` at the briefing, and this is the reason
the item outranked everything else in the backlog.

The lease is keyed by **LANE**, not by stream (`actor_runtime/src/lease.rs`:
`Lane { actor_type, partition, .. }`), and `completion.rs` fences on the **lane's** checkpoint. So
`stable_partition(actor_id, width)` is the **only** thing mapping an aggregate to exactly one lane.
Two producers with different widths put the same `Order-{id}` in two lanes, **each with a live
lease, each passing its own fence**. The mailbox's serialisation promise breaks *at the addressing
function*, upstream of anything a fence can observe — a fence can only defend the lane it is given.

The event store's expected-version check demotes the resulting corruption from silent to a version
conflict. That is a real mitigation and it is not sufficient here, because on the payment leg
`prepare` runs **before** `pool.begin()` (`completion.rs`): the Stripe intent already exists by the
time the losing writer is rejected.

A *partially* seeded actor — a half-applied seed, an older narrower binary, a width change in
flight — reaches this with no error at all: a non-zero count smaller than the declared width, and a
silently different keyspace.

## Decision

**1. Lane addressing comes from the DECLARATION. There is one accessor and it takes no width.**

`actor_client::declared_lane(actor_type, actor_id) -> Option<i16>` reads `ACTOR_MAILBOXES`
(generated from `actors.yaml` `mailbox.partitions`) and applies the frozen routing function.

**Every routing site calls it, and none of them takes a `width` parameter.** Reaching that took
more than the three sites the dispatch card named — review blocker B1 caught the first draft of
this ADR *asserting* the property while five other places still took or hand-rolled a width, and
using the assertion to drop the card's grep gate. What it actually took:
`ActorDoor::send_command`/`schedule_command`, `command_entry`, `inbound_entry` and the reminder
scheduler all lost their `width` argument, and the **emitter** stopped writing a literal width into
the generated typed clients — `send_command("Order", 5, …)` in generated code was the one genuinely
independent copy of the routing constant, replicated once per actor crate.

**What remains true and is not more than that**: `stable_partition` is still `pub` and still
re-exported, because tests legitimately compute an expected lane with it and the golden-value
freeze lives on it. So the wrong two-step is still *spellable*; after B1 it is simply not *spelled*
anywhere outside `declared_lane` itself and test code. That is level 4 for the parameter and
nothing stronger for the function, and this ADR now says so rather than rounding it up. The residue
— that, plus `mailbox_address`'s vestigial width element, which nothing reads any more — is filed
as [#609 "Lane addressing residue after #596"](https://github.com/TheCaptainCompany/captain-food/issues/609)
rather than left as an unstated boundary.

Because the parameter is gone, the text-grep check the dispatch card proposed (§3) is not written:
decision 3 below subsumes the runtime half, and a gate the compiler subsumes should not exist. That
conclusion is only load-bearing *given* B1 was actually done — it was not a licence to skip the
work.

The width is not configuration. It is an **addressing contract**, and the registry is an
*observation* of what some worker did at some past startup. An observation may be absent, partial
or stale; a contract may not.

**2. A lane nobody can drain yet is a lane the message WAITS on.** The hop is written to its
declared partition and sits `RECEIVED` until a worker claims it. "No worker has started" is an
operational state, not a business outcome, and it must never abort a transaction that is recording
money. `flush_lane_enqueues_in_tx` already said this in a comment; it is now the rule.

**3. A width CHANGE is a stored-shape migration, and `seed_partitions` refuses to start on drift.**

`inbound_messages.partition` is stamped at INSERT, so changing `mailbox.partitions` while rows
exist puts one aggregate in two lanes across the change. The registry's single writer therefore
refuses to start when a **non-empty** registry describes a different keyspace than the declared
one — widening, narrowing or partial.

**An EMPTY registry is a first boot, not drift**: it seeds and proceeds. Getting that backwards
would turn every fresh environment — and, after the #358 per-bin cutover, every newly deployed
bin — into a crash loop, which is a worse outage than the defect the check exists to prevent. The
failure message states the distinction explicitly, because the remedy for drift (clear the
registry) is catastrophic if applied to a first boot.

**Cutover procedure** (`vernon`), carried in that message: drain the affected actor's
`inbound_messages` backlog to empty — or run exactly ONE worker for it — **before** the new width
serves; then delete the stale registry rows and restart. With no in-flight rows there is no
aggregate whose stamped partition can disagree with the new addressing.

**Pre-deploy check, because this decision can refuse a start that used to succeed.** Before
deploying this change, confirm the live registry already matches the declaration:

```sql
SELECT actor_type, count(*) FROM mailbox_partitions GROUP BY 1 ORDER BY 1;
-- must equal ACTOR_MAILBOXES: 5 for every actor, 1 for MailboxSupervision
```

It does, and the reason is specific rather than general. The argument from `ON CONFLICT DO NOTHING`
alone is **not sufficient** — it says seeding never deletes, which is why a *narrowing* leaves
stale rows behind. What makes live databases safe is that the one narrowing this system has ever
performed cleaned up after itself: migration `20260802220000_mailbox_width_100_to_5.sql` ends with
`DELETE FROM mailbox_partitions WHERE partition >= 5`, so every actor present at that time is
`[0..4]`. And `MailboxSupervision`, the only actor whose declared width is not 5, was introduced on
2026-08-03 by #315 — **after** that migration — and has only ever declared `partitions: 1`, so it
can only ever hold `[0]`. Both cases satisfy the check, so no live worker refuses to start on this
deploy. A future narrowing that does *not* carry its own `DELETE` would break this, which is
exactly what decision 3 now catches at startup instead of at 20:30 on a Friday.

The check lives in `actor_runtime`, which carries **no path dependency into the workspace**
(extraction-readiness, `tests/dependency_rule.rs`). It therefore validates the registry against the
width it was **given**, not against `ACTOR_MAILBOXES` — the declared width arrives from the caller,
and the crate stays a `git mv` away from extraction.

**4. The supervision monitor is driven by the DECLARATION too** (`dba`).

The fix creates a blind spot if this is not done in the same change: before it, a hop to an unseeded
lane errored *loudly*; after it, the row waits *quietly* — and `mailboxLanes` joined **from**
`mailbox_partitions`, so a lane with no registry row rendered **nothing at all**. The one screen an
operator opens to ask "is anything stuck?" would have answered "no" over a backlog of paid orders.
**Trading a loud wrong failure for a silent right one is not an improvement.**

The lane population is now the declared grid `UNION` anything actually carrying work, with the
registry `LEFT JOIN`ed in.

Visible is not the same as diagnosable, and the first draft of this ADR overstated it (review
follow-up): a declared-but-never-seeded lane and a seeded-but-merely-unclaimed one render
`ownershipVersion 0` / `claimedBy null` / `checkpoint 0` — **byte-identical** — while only one of
them will ever be drained. So `MailboxLane` gains a **`registration`** field
(`scalars.yaml#/MailboxLaneRegistration`: `SEEDED` / `DECLARED_UNSEEDED` / `UNDECLARED_ORPHAN`),
additive on the GraphQL type, and the ADMIN page reads it as its first badge. *With* that field the
three states are distinguishable; the pair-identity is now pinned by an assertion in
`crates/server/tests/mailbox_lanes.rs` so the field cannot quietly stop being load-bearing.

The screen copy moved with it, because prose that teaches the opposite of the code is not something
`make validate` can see: the empty state no longer says *"Lanes appear when a mailbox worker seeds
the partition registry on startup"* (the exact belief this change destroys, on a state that is now
unreachable), and the reading guide names the never-seeded case and says plainly that `pending > 0`
there is a paid order waiting on a worker that was never deployed.

Split-lane detector, for the state that predates decision 3:

```sql
SELECT actor_type, actor_id FROM inbound_messages
GROUP BY 1, 2 HAVING count(DISTINCT partition) > 1;
```

## Explicitly NOT behind a flag (`farley`)

`farley` ruled no toggle, and reconciled it against his opposite call on #588: **there** two valid
paths made a flag worth having; **here there is one valid path, and the OFF state IS the
paid-order-fails branch**. Gate-then-stabilize gates *new behaviour*; it does not gate the deletion
of an error branch. Rollback is `git revert` plus one image redeploy — no schema change, no
backfill, no data migration.

He also narrowed the risk the dispatch card flagged: divergence between declared and seeded widths
arises only on a width **DECREASE**, because seeding is `ON CONFLICT DO NOTHING` and never deletes.
On a decrease, the declared read is the **safe** side — it is the side every other producer already
uses.

## The exposure window becomes indefinite after #358

In the deployed monolith the composition root seeds **every** `ACTOR_MAILBOXES` entry at startup
before serving, so the unseeded window is a few seconds at boot. After the **#358 per-bin cutover**
that stops being true: a **Payment bin can run while the target actor's bin is not deployed at
all**. The window goes from seconds to **indefinite**, and every authorization arriving in it would
have poisoned itself under the old code. This ADR is a precondition of that cutover, not an
independent tidy-up.

## Past occurrences: none, and here is why that is a real answer

#608's one-line ask on this chunk was whether any past occurrences exist — authorizations with no
`OrderPlaced` on the corresponding order stream.

**Production has no real customer orders.** V0 is pre-PMF; production carries **1 of 1**
restaurants, registered by the smoke script itself (`docs/STATUS.md`), and the only money-path
traffic is `tools/smoke/prod-smoke.sh`'s L4 leg in **Stripe TEST mode** (`sk_test_` is enforced —
the script refuses to move live money). There is therefore no real authorization that could have
been poisoned, and the question is discharged rather than deferred. Structurally the exposure was
also near-zero: the monolith seeds every declared actor before it serves.

No production query was run from this session, and none was needed for that answer. **This
discharges #608's ask on #596 only** — it says nothing about #608 itself, which is about having a
detector at all, for any cause, before there *are* real orders. That ordering is the right one.

## Consequences

- Every routing site shares one implementation, so the two that disagreed cannot drift apart again.
  The correct sibling (`flush_lane_enqueues_in_tx`) was converted along with the two wrong ones for
  exactly that reason, though it needed no fix.
- The ADMIN lane page now lists the whole declared topology (one row per declared lane) rather than
  only the seeded rows. That is a deliberate surface change: an operator cannot notice a lane is
  missing from a list they have never seen complete.
- `seed_partitions` can now fail a worker start. That is intended, is loud, and is bounded to the
  case where the alternative is two writers on one aggregate.
- **Three routing sites existed, not two.** The dispatch card said "both routing sites"; the
  flip-time backfill carried an identical `count(*)` and an identical zero-width error, found
  independently by `dba` and `beck`. It mattered *more* there: a rescue pass for facts nobody
  reacted to, running at startup, that refused to run when the system was cold.
- **And that site shipped its first draft with no negative test** (review blocker B3): reverting
  the backfill to the seeded `count(*)` left all 84 infrastructure tests green, because its only
  test seeds the PM lanes first — the very coincidence this branch indicts, reproduced inside the
  fix for it. `backfill_enqueues_to_the_declared_partition_with_the_target_unseeded` is the cold
  case, and it is verified red against the pre-fix body. The lesson generalises: a test written
  against a system in its *warm* state cannot see a defect that only exists when it is cold, and
  startup code is exactly where cold is the normal condition.

## Consulted

Not a founder directive, so no `Consulted:` block is owed. The mob's verdicts — six lenses, all
PASS, several correcting the card — are recorded in the dispatch card's `## Findings` block, with
the checkpoint verification banked there as `NO MISS`.
