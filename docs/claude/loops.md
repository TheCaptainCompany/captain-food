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
| **Running timer** | `$(git rev-parse --git-common-dir)/loop-budget-timer.json` | **never** (inside `.git/`) | `start` / `stop` |

The cap is **43200s = 12 h/week** (customer directive 2026-08-08, all-day autonomous operation —
ADR-20260808-223000; the historical default was 1800s = 30 min/week). The week's usage is the **sum**
of that week's ledger files; it resets automatically each ISO week because the next week is a new
directory. Usage is committed so the budget survives ephemeral cloud-routine runners (ADR-0014).

- Guard: `.claude/hooks/loop-budget.sh check|start|stop|status|reset|prune|audit|selftest`
  - `check` → exit 0 if budget remains, **exit 2 if spent** (skip the run). Strictly **read-only**.
  - `start` → check + open the timer. Writes **nothing tracked**, so it can never dirty a tree.
    **Refuses** (exit 3) if a timer is already open; **discards without billing** one older than 4 h.
  - `stop` → close the timer and append the segment. **Refuses** (exit 3) when no timer is open —
    never a silent zero — and when the timer is stale.
  - `stop --elapsed <seconds> --note "…"` → the honest escape hatch when `start` never ran or the
    timer was stale. Use it rather than hand-editing anything.
  - `status` → the week's breakdown; `reset` → drop an open timer without billing it.
  - `audit` → the ~10 ms invariant check the stop-gate runs every turn; `selftest` → the full suite.
- Make targets: `make budget-check` and `make budgeted-loop` (skips cleanly when the week is spent, else
  runs `night-loop` and records the elapsed time). Note `budgeted-loop` prints "weekly budget exhausted"
  for **any** non-zero `start`, so read the guard's own stderr above it — exit 3 means a timer was
  already open, not that the week is spent.

**Commit the new ledger file** your `stop` prints. It is a fresh path every time, so it never
conflicts with a concurrent session's and a merge sums both.

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

What remains true, and is inherent rather than a defect: **usage converges upward**. A checkout sees
the segments its own branch carries, so its total is a lower bound until branches merge — and because
merges add files rather than overwrite a number, merging can only raise it. That is the opposite of
the old behaviour, where a merge could silently discard hours (a naive "take ours" would have thrown
away 9128s — 2.5 h — of another session's recorded time).

**The lower bound bites at the cap** (measured 2026-08-12): a `check` on an up-to-date `main` reported
626.2m OK while three entries on the unmerged
[#500 "#242 Runtime D: retire command_journal"](https://github.com/TheCaptainCompany/captain-food/pull/500)
branch carried another 99.3m — the true week total was 725.5m, already past the cap, and the green
light was false. "Run `check` from a checkout CURRENT with origin/main" is necessary but **not
sufficient**: main is still only a lower bound while any billed branch is unmerged. So, **near the cap,
sum the week across all remote branches before dispatching** — every ledger path is unique and its
content immutable, so unioning is safe:

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
`loop-budget.sh stop --elapsed <difference> --note "carried over from the pre-ledger counter"`. Never
hand-merge the numbers. The stop-gate's `loop-budget.sh audit` fails the turn if the old fields survive.

## Rule

A recurring loop MUST be budget-guarded (`budgeted-loop` or the routine-prompt guard) and MUST commit
the ledger file its `stop` wrote, so the weekly total survives across runs. Never hand-edit budget
state: an unrecorded run defeats the cap, and an invented one steals from the next session. If `stop`
refuses, it is telling you the truth is not knowable from the timer — supply it with `--elapsed`.
See ADR-0014 and ADR-20260812-011057.
