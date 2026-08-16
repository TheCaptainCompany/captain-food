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

**Checkpoint verification**: `PENDING` — at the checkpoint, record whether the narrowed set missed
anything the full roster would have caught. Banked either way; a MISS reverts this class to
full-roster review. An unanswered line here is a run defect.

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

_(empty — filled by the mob)_
