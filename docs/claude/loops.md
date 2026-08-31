# Claude rules — autonomous loops & weekly budget

**Claude Code has no native "minutes per week" or token cap** for `/loop` or `/schedule` routines
(confirmed against the docs). We self-impose one with a committed state file + a guard.

## The weekly budget guard

Three pieces, deliberately separated — a running timer and a durable total have opposite requirements,
and the single mutable `{ secondsUsed, startedAt }` file that used to hold both produced seven distinct
corruptions in one day (ADR-20260812-011057):

| | where | tracked? | who writes it |
|---|---|---|---|
| **Cap** | `.claude/loop-budget.json` — `{ weeklyBudgetSeconds }` | committed | **nothing**; you, by hand |
| **Usage** | `.claude/loop-budget/<ISO-week>/<stamp>-<rand>.json` — one file per recorded run | committed | `stop`, append-only |
| **Running timer** | `$(git rev-parse --git-common-dir)/loop-budget-timer[--<run id>].json` | **never** (inside `.git/`) | `start` / `stop` |

**The cap lives in `.claude/loop-budget.json` and nowhere else** — do not restate it here or in any
other prose, and do not derive it from a number you remember: read the file, or run
`loop-budget.sh check`, which prints `used / cap`. The rule the number expresses: the cap is sized
for **all-day autonomous operation** (customer directive 2026-08-08 — ADR-20260808-223000; the
historical default was 1800s = 30 min/week), and it is the **sum of BOTH Claude accounts'
allowances**, because one shared ledger records the runs of two accounts working this repo
(founder directive 2026-08-12 — ADR-20260812-142454). A cap sized for a single account therefore
halves the team's week silently, and that is a wrong "exhausted" verdict, not a safety margin.

**The cap is currently NOT a stop sign** (founder directive 2026-08-12, operationalized 2026-08-13 —
[ADR-20260813-132540](../adr/ADR-20260813-132540-the-weekly-cap-stops-being-a-stop-sign.md)):
`.claude/loop-budget.json` sets `"capIsAStopSign": false`, so an over-cap `check`/`start` still
prints the exhaustion message but exits 0 — do not stand a run down for it, and do not report the
percentage as a constraint. Billing is unchanged: `start`/`stop` stay mandatory, the ledger stays
append-only, and every integrity refusal (exit 3) still fires. The override is event-bounded (the
ADR records the path back); do not restate the number here — read the ledger or `status`.

The week's usage is the **sum** of that week's ledger files; it resets automatically each ISO week
because the next week is a new directory. Usage is committed so the budget survives ephemeral
cloud-routine runners (ADR-0014). **Doubling the cap never doubles usage**: ledger entries are
measured actual time, and inflating one steals from the next week.

- Guard: `.claude/hooks/loop-budget.sh check|start|stop|status|reset|prune|audit|selftest`
  - `check` → exit 0 if budget remains, **exit 2 if spent** (skip the run). Strictly **read-only**.
  - `start` → check + open **this run's** timer. Writes **nothing tracked**, so it can never dirty a
    tree. **Refuses** (exit 3) if *this run* already has one open; **discards without billing** one
    older than 4 h. Another run's open timer is **reported, not refused** — concurrent runs each
    hold their own timer and each bill their own real time.
  - `stop` → close **this run's** timer and append the segment. **Refuses** (exit 3) when no timer of
    this run's is open — never a silent zero — when the timer is stale, and when the only open timer
    carries **no run id** (see below).
  - `stop --elapsed-seconds <n> --note "…"` → the honest escape hatch when `start` never ran or the
    timer was stale. Use it rather than hand-editing anything.
  - `status` → the week's breakdown **and every open timer**, marking which one is this run's.
  - `reset` → drop **this run's** open timer without billing it.
  - `--run <id>` → address a *named* run's timer (the handover path; `start` prints the id).
    `--adopt` → deliberately bill or discard a timer that carries **no** run id.
  - `audit` → the ~10 ms invariant check the stop-gate runs every turn; `selftest` → the full suite.
- Make targets: `make budget-check` and `make budgeted-loop` (skips cleanly when the week is spent, else
  runs `night-loop` and records the elapsed time). Note `budgeted-loop` prints "weekly budget exhausted"
  for **any** non-zero `start`, so read the guard's own stderr above it — exit 3 means a timer was
  already open, not that the week is spent.

**Commit the new ledger file** your `stop` prints — `.claude/loop-budget/<ISO-week>/<stamp>-<rand>.json`.
It is a fresh path every time, so it never conflicts with a concurrent session's and a merge sums both.
`.claude/loop-budget.json` is **config**; committing it records no time at all.

### Each run OWNS its timer (#821)

`git worktree` does **not** isolate the timer: it is one file in the git **common dir**, shared by
every linked worktree *and* by every concurrent session in the same checkout. A shared anchor is what
lets `stop` **find** a timer; it was never a licence to **bill** one. With a single slot, `stop`
billed whatever it found — the 2026-W36 ledger holds a segment noting *"a concurrent session in this
shared checkout closed the timer I inherited"*, and another with a **33.3-minute unbilled remainder**
after a `stop` billed 3.2 minutes of a ~39-minute run **and printed success**. A silent under-count is
worse than a refusal, because the executor trusting the output records the wrong number.

So a run has an **owner id** — `--run <id>`, else `$LOOP_BUDGET_RUN_ID`, else `$CLAUDE_CODE_SESSION_ID`,
else none — and the timer **file name carries it**, which makes another run's timer *unaddressable*
rather than merely detected. Consequences worth knowing:

- **Concurrency is normal.** Two sessions both `start`; each bills its own duration. Neither is
  refused, and neither has to estimate with `--elapsed`.
- **A `stop` that cannot prove ownership refuses and says whose timer it found.** If you are handed
  a run to close, use the id `start` printed: `stop --run <id>`.
- **A timer with no run id** (opened before this existed, or by a caller with no identity — plain
  cron, a tarball export) is billable only with `stop --adopt`. Runs with no identity at all share
  the historical single slot and behave exactly as before; set `LOOP_BUDGET_RUN_ID` to separate them.
- `--elapsed-seconds` and `reset` no longer touch another run's timer. Both used to: the escape hatch
  the tool *recommends* on a refusal was itself deleting live timers.
- **A refusal hands over the whole tuple** — started-at, branch, run id **and pid** — because that is
  exactly what an executor reconstructed by hand from the ledger's last segment before deciding
  whether a `stop` was even its to run.

**`exit 2` and `exit 3` mean opposite things and every exit-3 path now says so.** Exit 2 = the week is
spent (stop working); exit 3 = **integrity**, the timer state is wrong (not yours, already open,
stale, missing). They share the "the guard said no" shape, and a run that hit exit 3 reported it
correctly only *because the executor reasoned it through*. The guard holds the fact, so it prints it:

```
⛔ loop-budget: NO RUN TIMER IS OPEN for run 'demo-other' -- …
   Another run DOES hold an open timer. It is not yours to bill:
     started 2026-08-31T17:00:59Z on '821-…', 1.8m ago, run 'a68c75e1-…', pid 25563
   (exit 3 = INTEGRITY, not budget exhaustion -- the week is within cap: 268.1m / 1440.0m used …)
```

#### Why not key the timer by worktree (`--git-dir`)

It was proposed, and it is the smaller change: `git rev-parse --git-dir` instead of
`--git-common-dir` gives each linked worktree its own timer, so overlapping runs in *different*
worktrees stop sharing an object. It was **rejected**, for two reasons that are worth keeping so it
is not re-proposed:

1. **It re-creates [ADR-20260812-011057](../adr/ADR-20260812-011057-loop-budget-is-an-append-only-ledger-and-the-timer-is-never-committed.md)'s
   failure 1** — *"the state resolved from the script's own path, so `start` in one checkout and
   `stop` in another billed different counters"*, measured as six live checkouts holding six
   different weekly totals **simultaneously**. That ADR's decision 1 chose `--git-common-dir`
   precisely so `start` and `stop` are the same timer *whatever checkout each ran in*. Reverting it
   is a decision reversal, and this repo's own workflow (start in the primary checkout, work in a
   linked worktree) walks straight into it.
2. **It partitions on the wrong noun.** What is billed is a **run**, not a directory. One run spans
   worktrees (failure 1); one worktree hosts many runs — and at least one of the observed collisions
   was a *sibling session in the same checkout*, which worktree keying cannot see at all.

Keying on the run id is the same idea applied to the right noun: it makes overlapping runs bill
separately **in any topology**, which is everything worktree keying offered, and it makes a mismatch
**loud** rather than merely rarer. A silent under-count was the defect that actually cost time.

## How to bound each loop type

- **`make budgeted-loop`** (local / CI cron): the simplest enforcement — wraps the night loop in
  start/stop and aborts once the weekly cap is used.
- **`/schedule` (cloud routines)** — survives machine sleep; **min interval is 1 hour**, and there is a
  per-account daily run cap. Bound cost by:
  1. low cadence (e.g. weekly `0 2 * * 1`, or Mon/Wed/Fri), and
  2. starting the routine prompt with the guard, e.g. *"Run `bash .claude/hooks/loop-budget.sh start`;
     if it exits non-zero, stop and report 'weekly budget exhausted'. Otherwise do the work, then run
     `bash .claude/hooks/loop-budget.sh stop` and commit the ledger file it names."*
- **`/loop` (session)** — interval ≥ 1 min, auto-expires after 7 days. Use the same guard in the looped
  prompt/command; press `Esc` to stop early.

## Account-level backstops (set these too)

- **Spend limit**: claude.ai → Settings → Usage (monthly credit cap).
- The **Stop gate** (`stop-gate.sh`) already makes each iteration end as soon as `validate` is green, so
  an idle loop costs almost nothing.

## Worktrees, and what is still worth knowing

`start` and `stop` bill the **same** timer from any checkout of the repo: it lives in the git *common*
dir, which every linked worktree shares. You no longer have to run them from the same directory. (This
replaced the opposite rule — the old state file followed the script's own path, so `start` in the
primary checkout and `stop` in `/tmp/wt-NNN` billed different counters. Measured 2026-08-12, **six**
live checkouts of the same ISO week held `secondsUsed` of 4844 / 11615 / 13972 / 29078 / 29078 / 30543
*simultaneously*, and whichever branch merged last silently decided the total.)

**But the TIMER is shared and the RECEIPT is not — do not generalise the property above to the
file.** The ledger path is resolved from the SCRIPT's own location
(`ROOT="$(dirname "${BASH_SOURCE[0]}")/../.."`, then `$ROOT/.claude/loop-budget/<week>/`,
`loop-budget.sh:58,96`), not from your cwd or your worktree. So invoking the MAIN checkout's copy by
absolute path from a linked worktree bills the right timer and then writes the receipt **into the
main checkout's working tree**, where the branch that owes it cannot commit it — and a dispatch that
forbids touching the main checkout has implicitly forbidden the one place the guard writes. (Same
resolution, same consequence, for `status`/`check`: they sum the ledger of the script's checkout,
not yours.) Invoke the worktree's OWN copy — `bash "$(git rev-parse --show-toplevel)"/.claude/hooks/loop-budget.sh
stop` — or copy the named file across before committing it. Cost 2026-08-14: ~3 minutes of
branch-state checking, one duplicated file the coordinator had to reconcile by hand, and a ledger
split across two trees.

**On RESUME after a rate-limit cut, re-open `start` before touching the tree.** The timer does not
survive the cut as an open segment you can just `stop` — either it is gone (`stop` exits 3, refusing
to record a silent zero) or it is stale (discarded, unbilled). Working first and reconstructing later
means the resumed segment is invisible to the ledger unless someone remembers `--elapsed-seconds`; that is
how ~45 minutes went unbilled on 2026-08-14.

What remains true, and is inherent rather than a defect: **usage converges upward**. A checkout sees
the segments its own branch carries, so its total is a lower bound until branches merge — and because
merges add files rather than overwrite a number, merging can only raise it. That is the opposite of
the old behaviour, where a merge could silently discard hours (a naive "take ours" would have thrown
away 9128s — 2.5 h — of another session's recorded time).

**The lower bound bites near the cap** (measured 2026-08-12): a `check` on an up-to-date `main`
reported 626.2m while three entries on the unmerged
[#500 "#242 Runtime D: retire command_journal"](https://github.com/TheCaptainCompany/captain-food/pull/500)
branch carried another 99.3m — the true week total was 725.5m, a sixth of the week invisible to every
other checkout. "Run `check` from a checkout CURRENT with origin/main" is necessary but **not
sufficient**: main is still only a lower bound while any billed branch is unmerged. So, **near the cap,
sum the week across all remote branches before dispatching** — every ledger path is unique and its
content immutable, so unioning is safe:

(That same measurement produced a *second*, wrong conclusion worth remembering: 725.5m was read as
past the cap and 2026-W33 was declared exhausted. It was not — the cap was sized for one of the two
accounts sharing the ledger, and the corrected cap makes 725.5m about half the week. A verdict of
"exhausted" is a claim about the cap as much as about the usage; check both before standing a session
down. See ADR-20260812-142454.)

```sh
git fetch origin
week=$(date -u +%G-W%V)
git branch -r | while read -r b; do git ls-tree -r --name-only "$b" -- ".claude/loop-budget/$week/"; done \
  | sort -u | while read -r f; do git branch -r --format='%(refname)' \
  | while read -r b; do git show "$b:$f" 2>/dev/null && break; done; done | grep '"seconds"' \
  | grep -o '[0-9]*' | awk '{s+=$1} END {printf "%.1fm across all branches\n", s/60}'
```

If that total disagrees with `check`, **propagate the missing entries onto `main`**
(`git checkout origin/<branch> -- .claude/loop-budget/<week>/` and commit) — this is not hand-editing
budget state: the files are verbatim hook-written records, the eventual branch merge re-adds identical
paths with identical content (no conflict, no double count), and if the branch dies unmerged the time
was still spent, so main holding the entry is *more* correct, not less.

**If you hit a merge conflict on `.claude/loop-budget.json`**, the branch is old enough to still carry
the retired `secondsUsed`/`startedAt` counter. Resolution: **take `main`'s config-only file**, then
record whatever that branch's counter held above main's migrated seed with
`loop-budget.sh stop --elapsed-seconds <difference> --note "carried over from the pre-ledger counter"`. Never
hand-merge the numbers. The stop-gate's `loop-budget.sh audit` fails the turn if the old fields survive.

## Rule

A recurring loop MUST be budget-guarded (`budgeted-loop` or the routine-prompt guard) and MUST commit
the ledger file its `stop` wrote, so the weekly total survives across runs. Never hand-edit budget
state: an unrecorded run defeats the cap, and an invented one steals from the next session. If `stop`
refuses, it is telling you the truth is not knowable from the timer — supply it with `--elapsed-seconds`.
See ADR-0014 and ADR-20260812-011057.
