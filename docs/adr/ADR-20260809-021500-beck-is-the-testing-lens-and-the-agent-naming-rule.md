# ADR-20260809-021500 — `beck` becomes the testing lens; the agent-naming rule is written down

**Status**: Accepted · **Date**: 2026-08-09 · **Decider**: the customer (product owner), in
session, after noticing that Kent Beck anchored a review-only agent and that the roster's naming was
inconsistent. Amends [ADR-20260808-154005](ADR-20260808-154005-advisory-agents-channel-named-experts.md)
(named-expert channeling) and composes with
[ADR-20260809-013142](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md) (mob
programming).

## Context — the customer's observation

> "I thought we give them names why I don't see Kent beck fit the testing?"

Both halves were right:

1. **Beck anchored the wrong moment.** `reviewer` speaks only AFTER code exists. Beck's published
   subject — TDD, Test Desiderata, small safe steps, *make the change easy then make the easy
   change* — is about what happens BEFORE and DURING the work. Using him as a review flavour used
   the testing master as an auditor, and **no agent owned testing at all**.
2. **The naming split was never decided.** Agents added before ADR-20260808-154005 carry role names;
   the two the customer added after it carry person names (`holub`, `farley`).

Under mob programming the first point stopped being cosmetic: the briefing now needs someone whose
job is *"how will we know this works, and what test fails if it doesn't?"* — asked while the answer
can still change the work.

### The evidence that earned the lens (all 2026-08-08/09, same run)

- A manifest guard in [#335](https://github.com/TheCaptainCompany/captain-food/issues/335) had
  **never been seen red** until a violation was planted at the very end.
- The [#424](https://github.com/TheCaptainCompany/captain-food/issues/424) executor nearly shipped
  on DB tests that would have silently SKIPPED; real evidence existed only because it stood up a
  Postgres itself.
- [#354](https://github.com/TheCaptainCompany/captain-food/issues/354) — an oversell hole that ships
  green — is a money-path coverage gap **no lens owned**.

## Decision

1. **`beck` is a new standing agent: the testing lens** (`.claude/agents/beck.md`). It participates
   from the MOB BRIEFING onward, and its first move is always to name the failing test. It holds:
   a gate never seen RED is an unverified claim (mutation-test it, record the message); Test
   Desiderata are named trade-offs, not commandments; silent skips are worse than failures
   (`DB_TESTS_REQUIRED=1`, #230 — the variables are the evidence, not the number); structural and
   behavioural changes never share a commit; when a slice is hard to test, that is a DESIGN finding.
   It advises and designs tests, may sketch assertions in its report, and never commits, never edits
   `specs/**`, never claims an issue.
2. **`reviewer` keeps its job and loses the Beck anchor** — it independently verifies a FINISHED
   diff. On a mobbed dispatch both lenses speak by design: `beck` shapes the work while it can still
   change, `reviewer` judges what was actually produced. That is not duplication.
3. **The naming rule, recorded as-is** (amending ADR-20260808-154005): **one anchor → the person's
   name** (`holub`, `farley`, now `beck`); **several anchors → a role name** (`architect` =
   Young + Vernon + Evans; `ux-designer` = Norman + Patton; `business-specialist` = Meyer + Scholz).
   A role name is what a school of thought is called when no single person can name it without
   silencing the others. Renaming a multi-anchor lens after one of its anchors is therefore NOT a
   tidy-up — it is a demotion of the others, and this ADR exists so the next session does not
   "fix" the inconsistency arbitrarily.

## Consequences

- Every mob briefing now includes `beck`; its briefing answer is part of the record like any other
  lens's, and "nothing in my lens" stays a complete one-line answer.
- `beck` inherits the ownerless test holes, starting with #354.
- The internal-attribution boundary of ADR-20260808-154005 §3 applies unchanged: these names live in
  `.claude/agents/**` and internal reports only — nothing public-facing may claim these people
  advise or endorse this project.
