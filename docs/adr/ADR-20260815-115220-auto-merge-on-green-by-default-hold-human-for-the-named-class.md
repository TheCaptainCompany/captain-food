# ADR-20260815-115220 — Auto-merge-on-green by default; `HOLD: human` for the named class

- **Status**: Accepted (founder delegation, 2026-08-15; roster consulted per
  [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md))
- **Date**: 2026-08-15
- **Amended by**: [ADR-20260831-183847](ADR-20260831-183847-the-ready-flip-is-the-coordinators-step-and-always-was.md)
  (2026-08-31) — **this ADR settles *when* the ready + auto-merge step is taken versus withheld; it
  never settled *who* takes it.** The `Supersedes` note below records that
  `.claude/agents/executor.md` step 7 was "rewritten accordingly in the same commit", and that
  rewrite put the step into the EXECUTOR's voice as a side effect — contradicting
  [ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
  §2, which had already assigned "GitHub mechanics … ready + auto-merge" to the coordinator and is
  listed under `Relates` below for a different clause. **The flip and the arming are the
  coordinator's**; the executor hands back at green with the PR in draft. The decision recorded
  here — auto-merge-on-green by default, `HOLD: human` for the named class — stands unchanged, and
  applies to the coordinator's step.
- **Supersedes (in part)**: the auto-merge reading of
  [ADR-20260721-042018](20260721-042018-claim-time-draft-pr-automerge-supervision.md) and
  [ADR-20260721-044613](20260721-044613-auto-merge-never-armed-before-completion.md) — their
  claim-time draft-PR interlock and the "ready + arm as one indivisible step" sequencing stand
  unchanged; what this ADR settles is *when* that step is taken versus withheld, adding the
  `HOLD: human` class and the bound conditions below. `.claude/agents/executor.md` step 7 is
  rewritten accordingly in the same commit.
- **Relates**: [ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
  (executor writes every phase) ·
  [ADR-20260810-221840](ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md)
  (the specs freeze is lifted — also swept from executor.md in this commit)

## Context — two operating documents contradicted each other

`CLAUDE.md`'s issue-workflow bullet has said, since ADR-20260721-042018/-044613:

> When the work is done and local gates are green (`make rust`), mark the PR **ready for review**
> and **enable auto-merge** **together, as one indivisible step**, and **supervise the checks until
> the PR is MERGED**.

`.claude/agents/executor.md` step 7 said the opposite:

> **Default posture is PR-only: do NOT enable auto-merge.** A human merges, because `main` deploys
> to production. Only enable auto-merge if the dispatch explicitly says `MERGE: auto`.

An executor obeying its own agent file violated CLAUDE.md on every finished PR, and vice versa. The
contradiction was put to the founder, who delegated the ruling to the team, verbatim (2026-08-15):

> "I don't know the difference but you can consider that you are completely autonomous on that"

Per ADR-20260812-143619 the whole roster (14 lenses) was consulted before the ruling; the
`Consulted:` block below records each answer. 11 of 14 converged on risk-tiering.

## Decision

**Default = auto-merge-on-green.** When the work is done, local gates are green, and the
independent review has passed, ready-for-review + enable-auto-merge remain **one indivisible step**
(CLAUDE.md's existing shape, ADR-20260721-044613's sequencing untouched), **supervised to MERGED**
— fix and push on failure, never end at "pushed, CI pending".

**The exception inverts the old flag.** `MERGE: auto` is retired. Instead a dispatch marks
**`HOLD: human`** for work in the named class below; those PRs stop at ready-for-review and a
human merges.

### The HOLD class (union of the lens carve-outs)

1. **Stored event shapes, aggregate fold/upcasting semantics, DB migrations** — a revert does not
   un-append history, and CI's fixture DB proves nothing about production CNPG at peak.
2. **Payments / customer-funds custody; GDPR erasure.**
3. **Legal surfaces**: allergen display, VAT/receipt computation, P2B restaurant terms/ranking —
   advance-notice duties can make the merge itself a violation.
4. **Non-additive GraphQL schema changes** (field removal/rename/narrowing/nullability) until a
   compiler-first breaking-SDL gate exists.
5. **Actor mailbox/lease/fencing runtime.**
6. **The merge/CI machinery itself.**

### Bound conditions on the default

- Pre-merge CI must run the behaviour suites with `DB_TESTS_REQUIRED=1` — a silently-skipped suite
  is not green.
- A diff that adds a gate/guard shows its mutation-red (the gate observed failing on the defect it
  exists for) in the PR body.
- Customer-facing `specs/screens/**`/translations diffs get a rendered-state check before the merge
  fires (the [#424 "Customer-anxiety quick wins: DeliveryPickedUp reaches order tracking, checkout shows a FAILED state (approved spec diff, option b)"](https://github.com/TheCaptainCompany/captain-food/issues/424)
  class: the validator proves refs resolve, not that the state is honest).
- The CI SIGSEGV flake
  [#388 "[watchdog] Flaky SIGSEGV in `infrastructure` lib-test binary reddens the `ci` build gate on `main`"](https://github.com/TheCaptainCompany/captain-food/issues/388)
  is named as **fix-first**: auto-merging on a flaky gate manufactures false confidence.
- Once anything deploys, a post-merge telemetry check of the touched observability contracts joins
  the supervision loop — merge-and-walk-away is the failure mode.

### Dissent, recorded

**farley** dissented on tiering: auto-merge-on-green unconditionally once #388 is fixed;
deploy≠release already does the tiering, and a standing hold is the long-lived-branch failure mode
in costume. Recorded, not adopted — the majority view is that a merge to `main` that deploys is
itself the irreversible fact for classes 1–3, before any release gate.

## Consequences

- The two documents now agree: CLAUDE.md gains one sentence naming `HOLD: human`; executor.md step
  7 and its "never merge by default" hard boundary are rewritten to the default above.
- The same commit sweeps executor.md's stale vocabulary (specs-freeze wording, "product-owner
  approval") per ADR-20260810-221840 and ADR-20260812-143619 (evans's line below).
- A dispatch is now responsible for classifying its work against the HOLD class; misclassification
  is a dispatch defect, and an executor that recognizes HOLD-class work in an unmarked dispatch
  stops at ready-for-review and says so.

## Consulted

- **architect** — risk-tier by the existing GREEN/AMBER lanes; hold event shapes, migrations,
  actor/mailbox runtime, payments — green CI proves compile-and-test, not "safe against live
  mailboxes and money".
- **reviewer** — its independent PASS + green gates suffice only where blast radius is bounded and
  reversible; the excluded tier needs a human because the reviewer judges the diff against its
  stated intent, not whether the intent was right for production.
- **beck** — default acceptable only with `DB_TESTS_REQUIRED=1` pre-merge and mutation-red evidence
  for new gates — otherwise supervision is watching a number, not evidence.
- **holub** — auto-merge default — a held-ready PR is inventory; invert the flag to `HOLD: human`
  for the named classes.
- **farley** — auto-merge-on-green unconditionally once #388 is fixed; deploy≠release already does
  the tiering; a standing hold is the long-lived-branch failure mode in costume. (Recorded as the
  dissent on tiering.)
- **dba** — standing carve-out for schema/migration paths regardless of default; a bad migration
  against append-only `domain_events` is recovered by the WAL restore drill, not git revert.
- **legal** — never auto-merge on green alone for allergen/VAT-receipt/erasure/funds/P2B-terms
  surfaces; a human hold is the artifact that substantive review occurred before an irreversible
  production fact.
- **business** — risk asymmetry — minutes of velocity against a peak-night outage on the order
  path; hold the order/payment/dispatch path, auto-merge the bounded-blast-radius lanes.
- **ux** — default fine; customer-facing screen diffs need a rendered-state check before merge (the
  validator proves refs resolve, not that the state is honest — the #424 class).
- **graphql** — additive api diffs auto-merge; non-additive ones hold until a compiler-first
  breaking-SDL gate exists.
- **observability** — whoever pulls the merge, contract-touching changes owe a post-merge telemetry
  verification once anything deploys — merge-and-walk-away is the failure mode.
- **evans** — sweep executor.md's stale "product-owner approval"/specs-freeze vocabulary in the
  same commit — a half-corrected operating doc is worse than a stale one.
- **vernon** — nothing in my lens.
- **young** — hold anything touching stored event shape or fold semantics — CI has no opinion on
  what already-appended rows mean; everything else (wire replies, `View_*`, snapshots) is
  disposable-and-rebuildable, auto-merge fine.
