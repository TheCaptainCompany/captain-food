# Claude rules — autonomous loops & weekly budget

**Claude Code has no native "minutes per week" or token cap** for `/loop` or `/schedule` routines
(confirmed against the docs). We self-impose one with a committed state file + a guard.

## The weekly budget guard

- State: `.claude/loop-budget.json` — `{ weeklyBudgetSeconds, week, secondsUsed, startedAt }`.
  Current value **43200s = 12 h/week** (customer directive 2026-08-08, all-day autonomous
  operation — ADR-20260808-223000; the historical default was 1800s = 30 min/week); edit
  `weeklyBudgetSeconds` to change. Resets automatically each ISO week. It is **committed** so the budget persists across cloud-routine runs (each run reads/updates it).
- Guard: `.claude/hooks/loop-budget.sh check|start|stop`
  - `check` → exit 0 if budget remains, **exit 2 if spent** (skip the run).
  - `start` → check + stamp a start time.
  - `stop` → add elapsed-since-`start` to the weekly total.
- Make targets: `make budget-check` and `make budgeted-loop` (skips cleanly when the week is spent, else
  runs `night-loop` and records the elapsed time).

## How to bound each loop type

- **`make budgeted-loop`** (local / CI cron): the simplest enforcement — wraps the night loop in
  start/stop and aborts once 30 min/week is used.
- **`/schedule` (cloud routines)** — survives machine sleep; **min interval is 1 hour**, and there is a
  per-account daily run cap. Bound cost by:
  1. low cadence (e.g. weekly `0 2 * * 1`, or Mon/Wed/Fri), and
  2. starting the routine prompt with the guard, e.g. *"Run `bash .claude/hooks/loop-budget.sh start`;
     if it exits non-zero, stop and report 'weekly budget exhausted'. Otherwise do the work, then run
     `bash .claude/hooks/loop-budget.sh stop` and commit `.claude/loop-budget.json`."*
- **`/loop` (session)** — interval ≥ 1 min, auto-expires after 7 days. Use the same guard in the looped
  prompt/command; press `Esc` to stop early.

## Account-level backstops (set these too)

- **Spend limit**: claude.ai → Settings → Usage (monthly credit cap).
- The **Stop gate** (`stop-gate.sh`) already makes each iteration end as soon as `validate` is green, so
  an idle loop costs almost nothing.

## Rule

A recurring loop MUST be budget-guarded (`budgeted-loop` or the routine-prompt guard) and MUST commit
the updated `.claude/loop-budget.json` so the weekly total survives across runs. See ADR-0014.
