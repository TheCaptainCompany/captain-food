# ADR-0014 — Weekly time budget for autonomous loops (loop-state guard)

## Status
Accepted (realizes the former backlog item P-13).
Amended by [ADR-20260813-132540](ADR-20260813-132540-the-weekly-cap-stops-being-a-stop-sign.md)
(the cap is not a stop sign until [DECISIONS §35 INV-1](../proposals/DECISIONS.md) is met and the
first infrastructure euro is spent; billing and the ledger unchanged).

## Context
Autonomous loops/routines must not quietly consume quota. Claude Code has **no native cap** to limit a
`/loop` or `/schedule` routine to a wall-clock time or token budget per week (verified against the
docs): `/loop` only auto-expires after 7 days; `/schedule` enforces a 1-hour minimum interval and a
per-account daily run cap, but no weekly time/token budget. We want a hard, auditable "≤ 30 min/week".

## Decision
Self-impose the budget with operational memory + a guard:
- **State** `.claude/loop-budget.json` (`weeklyBudgetSeconds` default 1800, `week`, `secondsUsed`,
  `startedAt`) — committed, so it persists across cloud-routine runs; resets each ISO week.
- **Guard** `.claude/hooks/loop-budget.sh check|start|stop` — `check`/`start` exit 2 when the week's
  budget is spent; `stop` adds the elapsed time to the weekly total.
- **Make** `budgeted-loop` wraps `night-loop` in start/stop and skips cleanly when exhausted;
  `budget-check` is the standalone gate. `/schedule` routines call the guard from their prompt and
  commit the updated state. Account-level spend limits remain the backstop, and the Stop gate keeps idle
  iterations near-zero cost. See `docs/claude/loops.md`.

## Alternatives considered
- Rely on a native cap — does not exist.
- Cadence only (run weekly) — bounds frequency, not the duration of a single runaway run.
- Trust the model to self-limit — not auditable; easy to forget.

## Consequences
### Positive
- Hard, auditable weekly ceiling that works for `/loop`, `/schedule`, and CI cron; survives cloud runs
  via the committed state file.
### Negative
- `stop` must actually be called to account for time; a killed run under-counts (conservative bias is
  acceptable). The guard measures wall-clock, not tokens.
### Follow-up actions
- Optionally also track tokens/cost in the same state file if a spend signal becomes available to hooks.
