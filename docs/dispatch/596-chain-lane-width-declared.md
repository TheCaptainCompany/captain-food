# Dispatch card — [#596 "chain_pm_copy_in_tx reads lane width from a seeded registry and errors at zero — an unseeded worker fails a paid order's saga"](https://github.com/TheCaptainCompany/captain-food/issues/596)

**Read at**: `origin/main` = `830c045`. Every `file:line` is that snapshot; re-stamp if it moves.
**Artifact class**: dispatch card (ADR-20260816-020752).
**Position**: Urgent / tier-1 correctness on the money path. Predates #588 and is independent of the
`ROUTE_ORDER_BIRTH_THROUGH_LANE` flip — it is reachable with the flag OFF.

## 1. Chunk / not in scope

**Chunk**: make the PM fact-chaining hop address lanes from the **declared** routing contract
(`ACTOR_MAILBOXES`, generated from `actors.yaml` `mailbox.partitions`), the same source the routed
deliver already uses, and make the seeded-row-count read **unspellable** at any routing site.

**Not in scope**: the flip; #595 and #601 (other unlaned birth sites); #590's verdict-blind
re-application; any `domain_events` shape or fold change.

## 2. The defect, verified

`crates/infrastructure/src/mailbox/pm_delivery.rs:294-305` derives the lane keyspace width from
`SELECT count(*) FROM mailbox_partitions WHERE actor_type = $1` and returns
`sqlx::Error::Protocol("… has no seeded lanes — start its worker first")` at zero — inside the
completion transaction of a chained fact. `crates/infrastructure/src/mailbox/mod.rs:73-80` does the
opposite, and its comment states the reason verbatim: reading the seeded count "would make a
checkout saga FAIL because a worker had not started yet — a paid order with no birth, the worst
failure mode this product has." Two sites, one contract, opposite sources; the money-path one is
wrong. A second hazard rides along: a *partially* seeded actor yields a non-zero width smaller than
the declared one, so `stable_partition(&actor_id, width)` (`pm_delivery.rs:328`) addresses a
partition the declared routing contract does not have — silent misrouting, no error.

## 3. Approach — compiler-first (ADR-20260803-234035, level 4 floor)

Extract ONE accessor that returns the declared width for an actor type and route both call sites
through it, so no routing path can reach `mailbox_partitions` for a width. Behaviour after the fix:
the chained row is written to the declared partition and simply WAITS for a worker — never an error,
never a misroute. Where the type system cannot reach (a future site re-adding the SQL), add the
gate: a codegen test forbidding `count(*) FROM mailbox_partitions` outside the seeding path.

## 4. Definition of done (ADR-0032)

- Both routing sites derive width from `ACTOR_MAILBOXES`; the zero-width `Protocol` error is gone.
- A test proves a chained fact is written with **no** seeded `mailbox_partitions` rows for the
  target actor, and is drained correctly once the worker seeds.
- A test proves a partially seeded actor still routes to the declared partition.
- Gate added for the removed SQL read; no gate weakened.
- `make rust` green, `make validate` 0 errors, check-drift clean.
- STATUS.md line; SPEC-LOG only if `specs/**` moves (not expected).

## 5. Mob (§44 MOB-COST-1, ADR-20260816-134352)

**Reversibility class: IRREVERSIBLE-ADJACENT (money path).** A paid order's saga failing is the
named class; wider class wins the tie against "reversible internal". → **Briefing roster: FULL**
(whole roster, each lens names what it will catch or excuses itself).
**Checkpoint roster**: lenses that DECLARED a concern at briefing; any lens may opt back in.
Expected declarers: `vernon` (one aggregate per transaction, chained hop inside the completion tx),
`young` (nothing changes in stored events — confirm), `holub`, plus the money/telemetry lenses.

**Checkpoint verification**: `BANKED — NO MISS, and the briefing itself was the value`. The
narrowed CHECKPOINT set (the lenses that declared at briefing) caught nothing the briefing had not
already caught, because the briefing was FULL and it moved the chunk three times: the
reclassification to a one-writer violation (`vernon`/`young`/`dba`, independently), the THIRD
routing site (`dba`/`beck`, independently), and the SPEC-LOG row this card denied was owed (`dba`).
A narrow briefing roster would have shipped a two-site queueing fix with no migration note and no
spec row. The IRREVERSIBLE-ADJACENT → full-briefing sizing is therefore confirmed by outcome, not
just by posture. No lens opted back in at the checkpoint; the red-test evidence matched every
declared concern verbatim.

**HOLD: human** — mailbox runtime. PR stops at ready-for-review for the independent reviewer pass;
no founder wait.

## 6. Branch / touches

`BRANCH: 596-chain-lane-width-declared`
`TOUCHES: crates/infrastructure/src/mailbox/pm_delivery.rs`, `crates/infrastructure/src/mailbox/mod.rs`,
`crates/infrastructure/tests/main/*`, `tools/codegen-rs/src/tests.rs`, `docs/STATUS.md`.
Conflict check: #595 and #601 touch neighbouring birth sites — do not dispatch them concurrently.

**RISK**: the declared width and the seeded width disagree for an actor already running in the
deployed monolith, so in-flight rows sit in partitions no current worker claims. Verify the seeding
path derives from the same constant before merging.

## Findings

Six lenses briefed off this card, **all PASS**. Several CORRECTED it; where a lens and §1–§4
disagree, **the lens wins** and the correction is carried in the diff.

### `vernon` / `young` / `dba` — PASS, but this is a ONE-WRITER violation, not a queueing nuisance

Reclassified independently by three lenses, and it is the reason the item outranked everything
else. The lease is keyed by **LANE**, not by stream (`actor_runtime/src/lease.rs:9,35-38`), and
`completion.rs:95` fences on the *lane's* checkpoint — so `stable_partition(actor_id, width)` is the
ONLY thing mapping an aggregate to exactly one lane. Two producers with different widths put the
same `Order-{id}` in two lanes, each with a live lease, each passing its own fence: the mailbox's
serialisation promise breaks **at the addressing function**, upstream of anything a fence can see.
The append's expected-version check demotes silent corruption to a version conflict — but on the
payment leg `prepare` runs *before* `pool.begin()` (`completion.rs:69`), so **the Stripe intent
already exists when the loser rejects**. §2 of this card described only the symptom.

### `dba` + `beck` — THREE sites, not two (§4's "both routing sites" is wrong)

`pm_delivery.rs:294-305` (record-time chaining), the **flip-time backfill** at
`pm_delivery.rs:426-436` (same `count(*)`, same zero-width error), and the correct sibling
`mod.rs:73-80` the fix copies. Both wrong sites are fixed; all three now route through one accessor.

### `beck` — why CI was blind, and the two tests that break the coincidence

Every PM-chain test calls `worker.seed(5)` and **every declared width IS 5**, so seeded count ==
declared count and the two implementations are indistinguishable. Migrations do not seed; only a
worker start does — "unseeded" is honestly reachable by never starting the target worker. Two tests
in `crates/infrastructure/tests/main/pm_prepare_delivery.rs`, authored red-first against the
pre-change HEAD, with the partial-seed case **guarding its data** (`assert_ne!` on the two widths)
so the partition assertion cannot be vacuous.

### `farley` — NO flag, and the reconciliation with his opposite call on #588

On #588 two valid paths made a toggle worth having; **here there is one valid path and the OFF
state IS the paid-order-fails branch**. Gate-then-stabilize gates new behaviour, not the deletion of
an error branch. Rollback is `git revert` + one image redeploy — no schema, no backfill. He also
narrowed the risk in §"RISK": divergence arises only on a width **DECREASE** (seeding is
`ON CONFLICT DO NOTHING`, it never deletes), and there the declared read is the safe side. And:
after the #358 per-bin cutover the exposure window goes from seconds to **indefinite** — a Payment
bin can run while the target actor's bin is not deployed at all.

### `dba` + `young` — two things the fix must not leave behind

- **The fix creates a blind spot.** Today an unseeded lane errors loudly; after the fix the row
  waits — and the supervision query joins *from* `mailbox_partitions`, so an orphan lane shows
  **nothing**. The monitor now starts from the DECLARED grid (`ACTOR_MAILBOXES`) and surfaces
  undeclared lanes carrying rows as well. Detector for the split case:
  `SELECT actor_type, actor_id FROM inbound_messages GROUP BY 1,2 HAVING count(DISTINCT partition) > 1;`
- **A width change is a stored-shape migration, not a config edit** — `partition` is stamped at
  insert, so changing `mailbox.partitions` puts one aggregate in two lanes across the change.
  `young` wanted this executable: a **startup drift check** (declared width ≠ seeded rows ⇒ refuse
  to start). Landed; it is the compiler-first answer to the whole class and it subsumes a text-grep
  gate.
- `dba`: `specs/database/tables/journals.yaml` must say the registry is **not** the routing source
  (its job is lease + `ownership_version` + checkpoint + the ops monitor). That moves `specs/**`, so
  **a SPEC-LOG row IS owed** — §4's "SPEC-LOG only if `specs/**` moves (not expected)" is wrong.

### `vernon` — one DoD line added

A width cutover requires the affected actor's `inbound_messages` backlog **drained** (or a single
worker) before the new width serves. Written into the migration note.

### `business` — out of scope here, filed as [#608](https://github.com/TheCaptainCompany/captain-food/issues/608)

Nothing detects an authorized payment with no order birth, whatever the cause. Its ask on THIS
chunk was one line, answered in the PR: whether any past occurrences exist.
