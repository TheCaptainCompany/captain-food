# ADR-20260815-134655 — The team merges its own work; no PR waits on founder review

- **Status**: Accepted (founder directive, 2026-08-15)
- **Date**: 2026-08-15
- **Amends**: [ADR-20260815-115220](ADR-20260815-115220-auto-merge-on-green-by-default-hold-human-for-the-named-class.md)
  — the `HOLD: human` reading only; the HOLD class, the bound conditions and the default posture
  stand unchanged.
- **Relates**: [ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
  (team ownership — the coordinator handles GitHub mechanics, never the diff) ·
  the independent-review directive (founder, 2026-08-01, CLAUDE.md non-negotiables).

## Context — the founder was asked to merge, and refused the role

PR [#576 "Opening-hours guard: three-valued verdict, undeclared hours accept (#180 / RSO-1)"](https://github.com/TheCaptainCompany/captain-food/pull/576)
(closing [#180 "Opening hours are stored, displayed, and never enforced — a customer can order at 04:00"](https://github.com/TheCaptainCompany/captain-food/issues/180))
was held at ready-for-review under ADR-20260815-115220's `HOLD: human`, read as "the founder
merges", because it touches a stored event shape and the money path. Asked to merge it, the founder
answered, verbatim (2026-08-15):

> "Never wait my review you are responsible of your work. Why are you asking me to review?"

— and in the same message redirected the conversation to product design.

## Decision — the "human" in `HOLD: human` is the TEAM, never the founder

The smallest true change to ADR-20260815-115220:

- **The hold target is the team's own independent reviewer pass** — eyes that did not write the
  diff — never a founder wait. **No PR is ever held awaiting founder review.**
- **What stays**: the named high-stakes class keeps its stricter discipline. Independent full-diff
  review is mandatory before ready-for-review everywhere; the HOLD class stops at ready-for-review
  until that review has PASSED (multi-lens review for payments/migrations/erasure stays, per the
  #270 model); the bound conditions (DB_TESTS_REQUIRED, mutation-red evidence, rendered-state
  checks, post-merge telemetry) stand.
- **What changes**: only the merge click. Once the independent review passes and gates are green,
  **the team merges its own work, including the named class** — mechanically, the dispatch's
  poster (the coordinator) performs the merge, since GitHub mechanics are the coordinator's lane
  (ADR-20260810-011500). The founder can always intervene; the team never waits for him.

## First application, same day

PR [#576 "Opening-hours guard: three-valued verdict, undeclared hours accept (#180 / RSO-1)"](https://github.com/TheCaptainCompany/captain-food/pull/576)
— held at ready-for-review under the old reading, merged on the founder's directive after
independent review PASS + green CI.

## Consulted

Per [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md),
this directive lands on an option space the whole 14-lens roster consulted **hours earlier the same
day** — the roster consult recorded verbatim in
[ADR-20260815-115220](ADR-20260815-115220-auto-merge-on-green-by-default-hold-human-for-the-named-class.md)'s
`Consulted` block. The directive resolves the one residual variable that consult left open — WHO
performs the held merge — and no lens's position other than **reviewer's** hinged on it: every
carve-out argued for *a hold pending stricter review*, not for a specific merger's identity. The
one lens that did hinge on it, **reviewer** ("the excluded tier needs a human because the reviewer
judges the diff against its stated intent, not whether the intent was right for production"), is
now explicitly satisfied by the independent reviewer pass itself, per the founder's own ruling —
the human confirmation for the excluded tier IS the team's independent review.
