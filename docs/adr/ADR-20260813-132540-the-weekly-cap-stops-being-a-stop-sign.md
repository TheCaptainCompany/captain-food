# ADR-20260813-132540 — The weekly cap stops being a stop sign (billing continues)

## Status

Accepted and **founder-confirmed** (2026-08-13). The hold is resolved: in direct response to the
held confirm-or-correct question on [PR #541, the PR realizing this ADR](https://github.com/TheCaptainCompany/captain-food/pull/541),
the founder answered, verbatim: **"Continue the work enforcement and split"** (2026-08-13; relayed
~14:05Z, container clock 16:51Z when this resolution was recorded). The provenance chain is
therefore complete: verbatim antecedent 2026-08-12 → labeled elaboration in the 2026-08-13 resume
prompt → verbatim confirmation 2026-08-13. (The hold as originally recorded: the harness's
provenance check flagged the elaborated phrasing below, and the verbatim antecedent alone was
judged sufficient to draft but not to merge a gate-weakening change.) Amends
[ADR-0014 "Weekly time budget for autonomous loops"](0014-weekly-loop-budget.md) for a bounded
period; does not supersede it — the billing machinery, the append-only ledger and every integrity
refusal of [ADR-20260812-011057](ADR-20260812-011057-loop-budget-is-an-append-only-ledger-and-the-timer-is-never-committed.md)
are unchanged.

## Context

The verbatim founder directive on record is from 2026-08-12: **"Don't care about the budget right
now understood?"** (session record, user-message list). The operative operational form — *"do not
gate on the budget — stop reporting loop-budget percentages as a constraint and stop standing work
down for them"* — is the **2026-08-13 scheduled resume prompt's elaboration** of that directive:
its authorship is the prior session's handoff, **not verified founder-verbatim**. It is kept here
because it is the form the guard implements, labeled as what it is.

The concrete cost that forced this record: the elaborated directive arrived ~10:00Z and went
unrecorded. At ~13:20Z the executor dispatched on
[#510](https://github.com/TheCaptainCompany/captain-food/issues/510) ran
`bash .claude/hooks/loop-budget.sh start` as its protocol requires, got exit 2 (W33 stood at
1602.0m against the 1440.0m cap), and stood down exactly as its instructions say to — one full
dispatch round for zero output, against a gate the founder had already lifted. (Timestamps here
are the container clock, `date -u` — nothing else in this repo keeps time; an earlier draft said
~14:20Z, a dispatch-relay error.) The lesson: **a directive that changes a gate is recorded BEFORE
the next dispatch hits the gate.**

## Decision

1. **Over-cap ceases to be a refusal in `check` and `start`.** The switch is a committed config
   field: `.claude/loop-budget.json` gains `"capIsAStopSign": false`. Under `false`, `check`/`start`
   print the over-cap state on stderr EXACTLY as before — the `⛔ weekly loop budget exhausted`
   message is intact, because the override changes only the exit code, never the loudness
   (deleting the message would be the silent-fallback defect class,
   [ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
   clause (b)) — but exit 0. An absent field means `true` (the historical behaviour), so old
   branches parse fine and the flip back is a one-line config edit.
2. **Billing does NOT stop.** `start`/`stop` stay honest, the ledger stays append-only, and every
   integrity refusal is untouched in both flag states: stale timer, double-open, stop-without-timer,
   the audit — the exit-3 family is the fencing-token half of the script, not the budget half, and
   it guards correctness, not spend.
3. **The replacement report**: total minutes per week, stated in `docs/STATUS.md` as a report —
   no percentage, no gate. Business's caveat, verbatim in one sentence: agent time is currently the
   only variable cost with zero offsetting revenue; at the first infrastructure euro the burn line
   becomes a solvency question and the cap's economics change from cost control back to solvency
   control.
4. **Event-bounded, not dated.** The override runs until
   [DECISIONS §35 INV-1](../proposals/DECISIONS.md)'s acceptance criterion is met ("a working
   version", founder-confirmed) AND the first infrastructure euro is spent. **This paragraph is the
   pre-recorded path back**: re-enabling the cap later is executing this recorded path — flip
   `capIsAStopSign` to `true` (or delete the field) in `.claude/loop-budget.json` — not a new
   decision needing a new consult. **The re-arm owner is the architect's run report**: its checklist
   gains "is the first infrastructure euro spent? then flip the cap back." Observability's caveat,
   which is why an owner is named at all: the exit condition is a threshold nobody's alarm watches.

### Design note (beck)

The end condition is not machine-checkable — no script can know that INV-1's "working version" was
confirmed or that an OVH invoice landed — so the flag flip back is a **human act**. The tests
(`loop-budget-selftest.sh` case 9) prove both flag states behave and that the flip is reversible;
they never prove the flag gets flipped. That residual is carried by the named owner above, not by
a gate.

## Consulted

Per [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md),
one line per lens:

- **architect**: a new ADR, not an edit to ADR-0014 (Accepted, historical); a pointer line on
  ADR-0014's Status; event-bounded beats dated; the replacement report is named in the ADR itself.
- **holub**: the cap was never the real constraint — 20.6h billed this week, 76% of it off feature
  branches, and all of it would have passed any plausible budget; the replacements that would bind
  are WIP=1 per the mob rule, unmerged-branch age, and hours-since-last-customer-visible-change on
  the board where the percentage used to be.
- **beck**: extend the hermetic selftest, seen red first, three cases — over-cap under the override
  exits 0 AND still appends; integrity failures still refuse (the load-bearing mutation: the cheap
  wrong implementation is exit-0-for-everything); flag absent restores exit 2, proving
  reversibility.
- **dba**: nothing against; the integrity refusals are the fencing-token half of the script, not
  the budget half — keep them exactly as they are.
- **farley**: the override lands entirely inside `loop-budget.sh` (`Makefile`'s `budgeted-loop`
  branches purely on exit codes — touching callers would be duplication); fix the Makefile's
  "weekly budget exhausted" skip message, which after this change can fire on integrity failures
  and would misreport them; this ADR IS the gate-flip record — no second ADR needed.
- **observability**: over-cap must stay LOUD — change only the exit code, never delete the stderr
  message (the silent-fallback defect class, ADR-20260810-231300(b)); the STATUS minutes line
  suffices as the report because the append-only ledger is the durable reconciliation record;
  caveat: the exit condition is a threshold nobody's alarm watches, hence the named re-arm owner.
- **business**: agent time is currently the only variable cost with zero offsetting revenue; at the
  first infrastructure euro the burn line becomes a solvency question and the cap's economics
  change from cost control back to solvency control — that euro is the re-cap trigger.
- **legal**: nothing in lens; grade-(b) note — once real cooperative funds are spent,
  spend-authorization traceability becomes a statutes/governance question for counsel, not for
  this ADR.
- **graphql-architect**: nothing in my lens.
- **ux-designer**: nothing in my lens.

## Consequences

- Executors and loops no longer stand down on exit 2 from `check`/`start` — under the override
  those paths exit 0, so no protocol text needs a carve-out and no caller changes.
- The weekly minutes keep accumulating in the ledger and are reported in `docs/STATUS.md` as a
  number, not a constraint.
- The `budgeted-loop` skip message no longer asserts "weekly budget exhausted" for what may be a
  timer-integrity refusal (farley's finding).
- When INV-1 closes and the first infrastructure euro is spent, the architect's run report flips
  `capIsAStopSign` back — executing this ADR, not writing a new one.
