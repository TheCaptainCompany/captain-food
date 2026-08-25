# ADR-20260825-005323 — `claude-review` hardens while still required: the bypass, chosen over the recorded path

**Status**: Accepted · **Date**: 2026-08-25 ·
**Decider**: the **FOUNDER / Tech CEO**, ruling on the sequencing question put to him after the mob
briefing, **against the team's recommendation** ·
**Relates to**: [DECISIONS §45 **REV-1**](../proposals/DECISIONS.md) (answered 2026-08-17, **not
executed**) · [ADR-20260807-235930](ADR-20260807-235930-main-ruleset-required-checks.md) (amendment
box) · [#593 "The claude-review bot gate blocks every merge when it cannot run"](https://github.com/TheCaptainCompany/captain-food/issues/593) ·
**Realized by**: [#680](https://github.com/TheCaptainCompany/captain-food/pull/680), issue
[#677](https://github.com/TheCaptainCompany/captain-food/issues/677) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted. **The exposure below is knowingly carried, not removed.**

## Context

[#677](https://github.com/TheCaptainCompany/captain-food/issues/677): the claude-code-action
performs its own workflow validation and, when a PR modifies its own workflow file, **refuses to run
and exits 0**. `claude-review` is a **required** status check in ruleset `19179892`, so such a PR
reports a green review gate with no review performed — it clears its own gate. The same false green
appears whenever the action cannot produce a verdict at all: credit exhaustion, model outage,
permission denials.

The founder directed **option 2**: fail the job unless the reviewer actually posted its verdict.

The mob briefing then surfaced the problem with the ORDER, not the fix. `farley`'s register check
found **REV-1**: on 2026-08-17 the founder decided, *against the team's own recommendation*, that
`claude-review` comes **out** of the required checks — recorded as a knowing trade, the compensating
control being that the independent reviewer pass stays mandatory. **It was never executed**: the
ruleset `PATCH` returned 403 from the session's agent proxy (an egress block, not a GitHub denial),
and it has been an open action on #593 ever since.

So `claude-review` is **decided-out but still required**. Hardening it in that state means:

1. **Every "the reviewer could not post" becomes a repo-wide merge stop** — which is #593 verbatim,
   the failure that produced REV-1. The workflow file itself records `api_error_status: 429` /
   out-of-credits runs from 2026-08-24.
2. **It is self-blocking.** If the gate reds wrongly, the revert PR must itself pass `claude-review`
   to merge. The gate locks the door with the key inside, and the other key is the admin ruleset
   path that 403s.
3. **The fix trips its own gate**, because it edits the reviewer workflow. `beck`'s ruling: keep the
   self-red — a bootstrap carve-out ("skip the assertion when this workflow is edited") **is** the
   hole under a nicer name.

## Decision

**The founder chose the one-time admin bypass over executing REV-1 first**, having been shown the
cost twice and reaffirming it. `claude-review` stays **required** and becomes **able to fail**.

The team's recommendation was the opposite: execute REV-1 first — it is already decided, so
executing it needs no new decision, only an admin-capable actor — and then merge the hardening, so
the gate reds loudly on a *non-required* check and a credit-exhaustion day stops nothing. That
recommendation is recorded here as declined, not as unstated.

**REV-1 remains open on #593.** Executing it later removes the exposure without touching the
workflow file, and is still the recommended next step.

## Consequences — the exposure, stated plainly

- Any run where the reviewer cannot post — 429, model outage, permission denials, the action's own
  self-skip — **blocks every PR in the repository** until it is resolved or bypassed.
- A revert of #680 would itself need `claude-review` green.
- Both were true of the pre-REV-1 state that #593 documented. This ADR exists so the next session
  reading a repo-wide merge stop finds the cause immediately, and knows the remedy is REV-1 rather
  than a workflow edit.
- **A workflow file cannot make itself required or un-required**, and cannot stop the ruleset
  accepting `skipped`. #680 is a *verdict-honesty* fix only. Reading #677 as "option 2 solved
  requiredness" is a misreading.

## Alternatives considered

- **Execute REV-1 first, then merge** — the team's recommendation. Removes the merge-stop exposure
  entirely, needs no new decision, and costs one admin action. **Declined by the founder.**
- **Leave #680 unmerged at `HOLD: human`** — keeps #677's hole open: any PR editing the reviewer
  workflow keeps clearing its own required review gate unreviewed. Declined.
- **A bootstrap carve-out in the workflow** — skip the assertion when this file is edited. Rejected
  on `beck`'s reading: it re-creates the exact hole #677 names, under a friendlier name.

## Consulted

Records created from a founder directive carry one line per lens (ADR-20260812-143619).

- **farley** — surfaced REV-1 and the ordering defect; called the reversibility class wrong
  (a self-blocking merge gate is not "reversible" because its diff is small) and named the merge
  machinery a `HOLD: human` class whatever the diff looks like. **Adopted**: #680 was held, and the
  question went to the founder rather than being merged on the team's own judgement.
- **beck** — owned the self-red: keep it, it is the gate correctly seeing itself red first; refused
  the bootstrap carve-out; named the stale-comment and human-comment vacuous-green shapes that the
  marker now closes. **Adopted.**
- **reviewer** (independent, full-diff) — found the `pipefail` SIGPIPE false red, the merge-ref sha
  mismatch, and that the gate's claim outran what it proves. **Adopted**; all corrected before merge.
- **legal-specialist** — nothing in this lens: no personal data, no external artifact, no capacity
  statement. Recorded so a lens never asked is not mistaken for a lens with nothing to say.
- **architect** — not separately convened: no backlog re-ranking, no new audit finding; the
  sequencing turns on REV-1, which is already a register row.
