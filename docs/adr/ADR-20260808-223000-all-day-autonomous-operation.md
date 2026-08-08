# ADR-20260808-223000 — All-day autonomous operation: weekly loop budget 30 min → 12 h

**Status**: Accepted · **Date**: 2026-08-08 · **Decider**: the customer (product owner), in
session, during the first standing autonomous run (docs/claude/autonomous-run.md).

## Context

The weekly self-imposed loop budget (ADR-0014, docs/claude/loops.md, `.claude/loop-budget.json`)
was 1800 s = 30 min/week — sized for short nightly maintenance loops. On 2026-08-08 the customer
issued three directives in sequence: *"let the team work autonomously and ask my help if needed"*
(the standing kickoff), *"inform me every 5 minutes"* (status cadence, recorded in
docs/claude/autonomous-run.md the same evening), and — confirming the operating shape —
*"you will work all day long without my intervention, right?"*. A 30-minute weekly cap is
incompatible with an all-day supervised run reporting every 5 minutes; the guard would stop the
run mid-morning by design.

## Decision

`weeklyBudgetSeconds` is raised **1800 → 43200** (12 h/week). The guard mechanism itself is
unchanged and still binding: runs check it, record elapsed time, commit the state file, and stop
cleanly when the week is spent.

## Consequences

- All-day runs are in budget; the 5-minute cadence and ~60-min fallback wake-ups operate as
  recorded in docs/claude/autonomous-run.md.
- The **account-level spend cap** (claude.ai usage settings) remains the customer-owned backstop
  outside the repo's control; if it halts a run, that is the guard working, not a failure.
- A run still ends early when every remaining item waits on the customer — idling against the
  budget to "work all day" is expressly not the goal; the run summarizes and stops.
- The customer adjusts the number by editing `.claude/loop-budget.json` (or reverting this ADR).
