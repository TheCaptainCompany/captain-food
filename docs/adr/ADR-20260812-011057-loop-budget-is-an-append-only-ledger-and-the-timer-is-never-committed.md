# ADR-20260812-011057 — The loop budget is an append-only ledger, and the running timer is never committed

- **Status**: Accepted
- **Date**: 2026-08-12
- **Amends**: [ADR-0014](0014-weekly-loop-budget.md) (the weekly cap itself is unchanged), and
  supersedes the worktree guidance in [docs/claude/loops.md](../claude/loops.md)

## Context

ADR-0014 self-imposes a weekly time cap on autonomous loops, because Claude Code has no native one.
It was implemented as a single mutable committed file, `.claude/loop-budget.json` =
`{ weeklyBudgetSeconds, week, secondsUsed, startedAt }`, read-modify-written by
`.claude/hooks/loop-budget.sh check|start|stop`.

On 2026-08-11/12 that file produced **seven distinct failures in one day**, each found by a different
agent. They are not seven bugs; they are one design defect with seven faces — *a single mutable
counter, shared by concurrent writers, holding both a durable total and an ephemeral timer*:

1. **The state resolved from the script's own path**, so `start` in one checkout and `stop` in another
   billed different counters. Six live checkouts of the same ISO week were measured holding
   `secondsUsed` of 4844 / 11615 / 13972 / 29078 / 29078 / 30543 *simultaneously*.
2. **`stop` was a silent no-op when `start` never ran** (`if (s.startedAt)`), exit 0 and a cheerful
   summary. An unrecorded run defeats the cap entirely, which is the whole point of the file.
3. **A stale open timer billed enormous phantom time**: one run read a `startedAt` an earlier session
   never closed and billed **261 minutes — 36 % of the weekly cap — for a 16-minute run.**
4. **Writing that back would have *lowered* the committed total** (27262 vs the 29078 already on the
   branch), silently handing back ~30 minutes and burying the stale timer. Caught only by a hand diff.
5. **It conflicted on nearly every concurrent branch, and both "take ours" and "take theirs" were
   wrong** — it is a monotonic counter, so the only correct resolution is
   `base + (ours − base) + (theirs − base)`. One naive "take ours" would have discarded 9128 s (2.5 h)
   of another session's recorded time.
6. **A non-zero `startedAt` got committed**, so an open timer travelled between branches and sessions.
   One was still open on `cutover-local-rehearsal` when this ADR was written.
7. **A hook invocation re-stamped `startedAt` in a checkout doing no loop work**, dirtying an
   unrelated branch's tree and tripping the stop-gate (`check` called `save()` on its read path).

Every one is *silent*. The cap does not crash when it is wrong; it just quietly reports a number
nobody can trust, which is worse — a budget you cannot trust is not a budget.

## Decision

**Split the two things the file was conflating, and make the total append-only.**

| | where | tracked? | who writes it |
|---|---|---|---|
| **Cap** | `.claude/loop-budget.json` = `{ weeklyBudgetSeconds }` | committed | **nothing**; a human, by hand |
| **Usage** | `.claude/loop-budget/<ISO-week>/<stamp>-<rand>.json`, one file per recorded run | committed | `stop`, append-only |
| **Running timer** | `$(git rev-parse --git-common-dir)/loop-budget-timer.json` | **never** | `start` / `stop` |

1. **The running timer lives inside `.git/`.** It is therefore untrackable *by construction*: it
   cannot be committed (6), cannot travel between branches (6), and `start` cannot dirty a working
   tree (7). `--git-common-dir` resolves to the **same** directory from every linked worktree
   (relative `.git` in the main one, absolute in linked ones), so `start` and `stop` are provably the
   same timer whatever checkout each ran in (1).
2. **Usage is an append-only ledger, not a counter.** A run records ONE new file; the week's usage is
   their **sum**. Nothing rewrites a number, so `stop` *cannot* lower the total — monotonicity is
   arithmetic, not a check (4). Two branches recording concurrently create two *different* files, so
   they never conflict and any merge is additive automatically (5). The weekly reset is free: next
   week is a new directory.
3. **`.claude/loop-budget.json` becomes pure config.** No process writes it. A file nothing mutates is
   a file that does not conflict.
4. **`stop` refuses rather than inventing or dropping time.** No open timer → exit 3 saying the run
   would "silently vanish from the weekly cap" (2). Timer older than **4 h** → discarded, *not*
   billed and *not* clamped, exit 3 (3). Both name the honest escape hatch,
   `stop --elapsed <seconds> --note "…"`. `start` refuses a live timer (double-billing) and discards a
   stale one with a warning.
5. **`check` is strictly read-only** (7).

**Why 4 h is the staleness bound.** It is chosen against the cap, not against a feeling: the weekly
budget is 12 h, so a single unclosed segment beyond 4 h would consume a third of the week in one
write — which is exactly the observed failure (261 min ≈ 4 h 21 m). Measured real segments in this
repo are tens of minutes. A genuinely longer run is still recordable, but only through an explicit
`--elapsed`, because inventing 4 h+ of budget deserves a human assertion.

**Why not a `.gitattributes` merge driver** (the obvious fix for 5): custom merge drivers are
**local-only and never run in GitHub's server-side merge**, which is where this repo's PRs actually
land, and they require per-clone `git config` registration that no fresh CI checkout has. It would
have been a resolution that silently did not apply exactly when it mattered. Making the state
non-colliding is the final shape; a driver is the intermediate one (ADR-20260808-235113).

**Gates, because prose can be skipped and a gate cannot** (CLAUDE.md):

- `loop-budget.sh audit` — ~12 ms, run by `stop-gate.sh` on **every** turn. Fails if tracked budget
  state contains `startedAt` (a committed open timer) or `secondsUsed` (a resurrected counter, the
  likely outcome of a careless merge on an in-flight branch).
- `loop-budget.sh selftest` — 39 assertions in a hermetic `git init` fixture, incl. a real
  `git worktree` proving cross-worktree billing. Run by `stop-gate.sh` when the diff touches
  `.claude/hooks/loop-budget*`. Each numbered case reproduces one of the seven failures and asserts it
  is now refused; every guard was watched to fire against a negative control before being trusted
  (#292).

## Alternatives considered

- **Keep the counter, add a `.gitattributes` additive merge driver.** Rejected: does not run in
  GitHub's server-side merge (above), and leaves failures 2/3/4/6/7 to be fixed by guards that a
  future edit can weaken. It treats the symptom (merging) rather than the shape (mutability).
- **Move the total into `.git/` as well**, dissolving the merge problem completely. Rejected: it would
  no longer survive a fresh clone or an ephemeral cloud runner, which is precisely what ADR-0014
  needs. A cap a new checkout forgets is not a cap.
- **Clamp a stale timer to 4 h instead of refusing it.** Rejected: 4 h of clamped phantom time is
  barely better than 4 h 21 m of unclamped phantom time. Refusing forces the truth to be supplied.
- **Let `stop` keep silently recording zero, and document the requirement to call `start`.** Rejected
  outright: that is the prose-instead-of-gate failure mode this repo has a standing rule against.

## Consequences

**Positive.** Six of the seven failures become *unrepresentable* rather than guarded: there is no
committed timer to go stale, no number to lower, no shared file to conflict on. Concurrent sessions
stop fighting over one counter. `start` can no longer dirty an unrelated tree. The ledger is also an
audit trail — `loop-budget.sh status` now shows which branch spent what.

**Negative.** Usage is a *lower bound* per checkout until branches merge, since a checkout only sees
its own branch's segments; merges can only raise it, never lower it, so the number converges upward.
One small file per run is committed (~50/week; `loop-budget.sh prune` keeps 6 weeks). Branches created
before this change will conflict once on `.claude/loop-budget.json`; the resolution is recorded in
`docs/claude/loops.md` and enforced by `audit`.

**Follow-up.** The pre-ledger total on `origin/main` (29078 s) was migrated as the ledger's first
entry, so no recorded time was lost. In-flight branches carrying a *higher* counter than main's seed
(measured: one at 30543 s, i.e. 1465 s above) must record the difference with `stop --elapsed` when
they merge — `audit` will refuse the old fields, which is the prompt to do it. The stale open timer
committed on `cutover-local-rehearsal` is neutralised without touching that branch: the new hook never
reads `startedAt` from tracked state, so it is inert, and it disappears when that branch takes main's
config-only file.
