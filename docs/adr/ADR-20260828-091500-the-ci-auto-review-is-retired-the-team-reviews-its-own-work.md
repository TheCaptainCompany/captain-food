# ADR-20260828-091500 — The CI auto-review is retired: the team reviews its own work

**Status**: Accepted · **Date**: 2026-08-28 ·
**Decider**: the **FOUNDER / Tech CEO**, verbatim below ·
**Prompted by**: the AI cost of the `claude-code-review.yml` pass on every PR presentation ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted.

## The directive, verbatim

> *"Remove code review from the ci. It cost ai usage for each commit and unnecessary because we
> are doing the code review ourselves"*

## The decision

`.github/workflows/claude-code-review.yml` — the workflow that ran an AI review pass on every PR
presentation (`opened`/`ready_for_review`/`reopened`, per ADR-20260826-084500) — is **deleted**.

This is an **implementation shift, the review pattern unchanged** (evans): every rule about
reviews stays in force, with the team's own pass as the sole standing mechanism —

- **The independent review before ready-for-review stands** (founder directive 2026-08-01,
  CLAUDE.md): a PR is marked ready only after the TEAM's reviewer-agent pass over the FULL branch
  diff, in-session, by eyes that did not write it — run BEFORE ready, not filed later (reviewer's
  holding conditions). Payments, migrations and erasure keep the multi-lens fan-out.
- **ADR-20260826-084500 stands**: one pass per presentation, findings triaged never chased,
  three rounds is a ceiling. Only the pass's runner changes (CI bot → team reviewer agent).
- **REV-1 already held**: `claude-review` was a non-required check, so no merge machinery, ruleset
  or required-check surface moves; auto-merge-on-green (ADR-20260815-115220) is untouched.
- **`claude.yml` stays**: the `@claude`-mention workflow is on-demand — it costs nothing unless
  someone asks — and remains the founder's way to summon a look on any PR or issue.
- **Executable gates are untouched**: `codegen`, `ci`, `make validate` remain the blocking,
  mechanical third look (farley, vernon, young, graphql, dba lenses).

## Watch item (farley)

Review latency is now a team-process property with no observability contract; if ready→merge
becomes a bottleneck, that contract is the thing to add — not a return of the per-presentation
bot.

## Consulted

- **architect** — sound; confirm the reviewer-agent hand-off is live before deleting the fallback
  (it is: the in-session pass ran for #684 and runs for every PR from this change on).
- **beck** — nothing in my lens; testing gates unaffected.
- **business-specialist** — cost clear, risk acceptable; the mandatory team pass is the binding
  control; proceed.
- **dba** — nothing in my lens; storage shapes keep the multi-lens fan-out and `make validate`.
- **evans** — say "implementation shift, review pattern unchanged" and name the reviewer agent in
  the standing rule, so a later reader does not mistake the workflow's absence for the review's.
- **farley** — clear; gates untouched, feedback path tighter; watch review latency (above).
- **graphql-architect** — nothing blocking; the validator gates the hard errors, the team pass
  keeps the judgment calls.
- **holub** — waste confirmed: an automated pass duplicating the actual blocking gate; retire it,
  keep `@claude` on-demand.
- **legal-specialist** — no compliance role in the CI automation; the team-review precondition and
  audit trail are what matter, both kept.
- **observability-agent** — nothing; no telemetry surface touched.
- **reviewer** — nothing lost IF: the in-session pass is mandatory before ready (not async later),
  it reads the FULL branch diff at presentation, gates stay mechanical, and no branch merges
  without both. All four hold in the recorded flow.
- **ux-designer** — nothing in my lens; the team pass keeps the surface-regression gate.
- **vernon** — nothing in my lens; cost optimization on a reversible surface.
- **young** — nothing in my lens; no event/fold/read-model surface touched.
