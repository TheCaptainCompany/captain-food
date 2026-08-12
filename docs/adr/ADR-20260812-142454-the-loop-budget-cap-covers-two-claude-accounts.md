# ADR-20260812-142454 — The weekly loop-budget cap covers TWO Claude accounts, so it doubles

- **Status**: Accepted
- **Date**: 2026-08-12
- **Decider**: the founder (Tech CEO), in session
- **Amends**: [ADR-20260808-223000](ADR-20260808-223000-all-day-autonomous-operation.md) — its cap
  figure only. **Unchanged**: [ADR-0014](0014-weekly-loop-budget.md)'s mechanism (a self-imposed
  weekly cap, checked and recorded by a hook) and
  [ADR-20260812-011057](ADR-20260812-011057-loop-budget-is-an-append-only-ledger-and-the-timer-is-never-committed.md)'s
  append-only ledger design, including its 4 h staleness bound.

## Context

The founder's directive, verbatim, 2026-08-12:

> "About this counter it's not appropriate because the same counter is used for 2 Claude accounts.
> Multiply by 2 the numbers."

"This counter" is the weekly agent-time budget: the cap in `.claude/loop-budget.json`, the
append-only usage ledger in `.claude/loop-budget/<ISO-week>/`, and the
`.claude/hooks/loop-budget.sh` guard that reads both.

The ledger is **one shared record for two Claude accounts** working this repository. Every run of
either account appends to the same ISO-week directory, so the recorded usage is the *sum of both
accounts' work* — while `weeklyBudgetSeconds` was sized as *one account's* allowance. The two sides
of the comparison were counting different populations, and the guard compares them directly. The
consequence is not a rounding error: it halves the team's usable week, and it does so **silently**,
by reporting "exhausted" — the one message nobody argues with.

## Decision

**`weeklyBudgetSeconds`: 43200 → 86400** (12 h → 24 h per ISO week) in `.claude/loop-budget.json`,
which remains the single place the number lives and the only file a human edits by hand.

The rule behind the number, which is what prose must state instead of the number: **the cap is the
SUM of the allowances of every Claude account that shares the ledger.** Two accounts today ⇒ twice a
single account's all-day allowance. If the number of accounts sharing the ledger changes, the cap
changes with it, and that is a one-line edit to the config plus a line in an ADR.

**Only the cap is multiplied. Usage is NOT.** Ledger entries are *measured actual time* — a wall
clock between `start` and `stop`, or an honest `--elapsed` with the measurement method in its note.
Doubling a measurement to "match" a doubled cap would invent time nobody worked and steal it from the
next week, and it would corrupt the one artifact the whole guard depends on being true. The ledger
is append-only and immutable precisely so that no correction of the *cap* can ever reach into the
*record*.

Nothing else moves: no new field, no new file, no change to `check`/`start`/`stop`/`audit`, and no
change to the hermetic selftest, whose fixture already derives its cap from a config file it writes
itself rather than restating the repo's.

## What this retires

2026-W33 was reported **exhausted at 725.5m against a 720.0m cap**, and a session stood itself down
to coordinator-only posture on that basis. Against the corrected cap the same 725.5m is **50.4 % of
1440.0m**: the week was **not** exhausted, roughly half of it remained, and the posture adopted in
response was wrong. No override, exception or manual unblock is needed — `loop-budget.sh check`
returns 0 on its own once the cap is right.

The same measurement also produced a *correct* lesson that survives untouched: usage is a per-branch
lower bound until branches merge, and 99.3m of it was invisible to `main`
([docs/claude/loops.md](../claude/loops.md) keeps the cross-branch sum to run near the cap). The
defect was the cap, not the ledger.

`docs/claude/loops.md` pinned "43200s = 12 h/week" in prose — exactly the failure mode CLAUDE.md
warns about for the validator's warning count. It now states the rule and points at
`.claude/loop-budget.json`; the hook's own comment on the 4 h staleness bound likewise stops
restating the cap.

## Alternatives considered

- **Leave the cap and grant per-week overrides when it trips.** Rejected: an override is a decision
  taken under time pressure, by whichever session happens to hit the wall, on the basis of a number
  it cannot verify. The founder's instruction is that the cap itself is wrong.
- **Halve what each account records, so a one-account cap still fits.** Rejected outright: that is
  falsifying measured time. The ledger's value is that it is true.
- **Split into two caps, one per account, enforced per account.** Rejected *for now* — see the open
  option below. It requires attribution the ledger does not carry, and today the pooled cap is the
  behaviour the directive asks for. Doing it would be building a mechanism before there is a question
  it answers.

## Consequences

**Positive.** The guard's verdict is meaningful again for the first time since the second account
started sharing the ledger. Both accounts' all-day operation fits the week that was provisioned for
it. The number is stated in exactly one place, so the next doubling — or halving — is one edit.

**Negative.** A pooled cap is *first come, first served*: one account can spend the whole week and the
other finds the well dry, with nothing in the record to show whose runs did it. `loop-budget.sh
status` shows the spending **branch**, which correlates with a session but not with an account.

**Open option, deliberately not decided here.** If per-account attribution ever matters — an account
starves the other, or a per-account allowance has to be enforced rather than pooled — the finer answer
is to attribute each ledger entry to its account: an `account:` field per entry, or a per-account
subdirectory under the ISO-week directory. Both are compatible with append-only immutability (a new
field on new entries; historical entries stay unattributed). This ADR records the option and the
trigger, and chooses **not** to build it: there is no observed starvation, and a pooled cap with a
correct total is what was asked for. Revisit when the symptom appears, not before.
