# ADR-20260807-235930 — `main` ruleset: required checks + required Claude review, admin bypass preserves straight-to-main

- **Status**: Accepted (product owner configured and saved the ruleset, 2026-08-08 ~01:55 CEST)
- **Context**: PR merging was gated only by CI and agent discipline: auto-merge fired on green
  checks, the three-lens review passes were self-administered by the executing agent, and nothing
  *mechanically* required a review to have happened. The product owner asked whether reviewer
  comments were taken into account — the honest answer was "yes while subscribed, but nothing
  enforces it" — and chose to make the gate mechanical.

## Decision

A repository ruleset **"Checks"** (Active) targets the **default branch**:

- **Required status checks**: `codegen`, `build-test`, `db-test`, **`claude-review`**
  (`.github/workflows/claude-code-review.yml`; skips DRAFT PRs by design — a skipped required
  check is satisfied, so claim-time draft PRs are unaffected and the review runs exactly at
  `ready_for_review`, when auto-merge arms).
- **Require a pull request before merging**, **required approvals: 0**, dismiss stale approvals.
- **Restrict deletions** and **block force pushes**.
- **Bypass: Repository admin — always allow.**
- "Require branches to be up to date before merging": **deliberately OFF**.

## Options considered

- **Required human approvals (≥1)** — rejected for now: the pipeline is sequential
  coordinator-dispatched executors with the product owner asleep half the cycle; a required
  human approval would stall auto-merge for hours with no quality gain over the required
  `claude-review` check plus the coordinator acting on its comments. Revisit when a second
  human joins.
- **Require branches up to date** — rejected: the operating model pushes docs/spec commits
  straight to `main` (CLAUDE.md), and each such push would invalidate every armed PR and force
  a full CI re-run (~30+ min). Sequential dispatching already branches each step from fresh
  `main`. Revisit alongside a merge queue if PR concurrency grows.
- **No admin bypass** — rejected: it would outlaw the documented straight-to-main lane for
  docs/spec/ADR commits. The bypass keeps that lane for repository admins while every PR takes
  the full gate.

## Consequences

- Nothing merges without codegen, build+test, DB suites, and a Claude review having RUN —
  mechanically, whatever any agent does. Note the `claude-review` check gates "review
  happened", not "review approved": findings land as PR comments, which the coordinator
  session receives via its PR subscription and must act on before merge (fix, reply, or
  escalate) — comment handling stays a process obligation, now backed by a mechanical
  guarantee that the review ran.
- `claude-review` depends on the `CLAUDE_CODE_OAUTH_TOKEN` repository secret; expiry blocks
  PRs with a visible red check (the correct failure direction — fix the secret, not the gate).
- Straight-to-main docs/spec pushes continue under the admin bypass; the branch-deletion
  restriction that already bit one session (sessions.md) is now part of the same recorded
  ruleset.
