# ADR-20260828-063500 — Always stay quiet: artifacts and records are the report, chat is for questions

**Status**: Accepted · **Date**: 2026-08-28 ·
**Decider**: the **FOUNDER / Tech CEO**, verbatim below ·
**Prompted by**: the round-4 call-sheet answers (2026-08-28 morning) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted.

## The directive, verbatim

Appended to the round-4 call-sheet answers:

> *"Always stay quiet"*

## What it binds

The team's chat channel to the founder goes quiet by default, at all hours — the overnight
"founder is asleep" posture becomes the permanent one:

- **Work happens silently.** Progress, merges, routine triage and re-armed check-ins produce no
  chat messages. The repo (commits, journal, SPEC-LOG, issues/PRs) and the call-sheet artifact are
  the report.
- **Chat is for questions.** A chat line is written only when something needs the founder's answer
  to proceed (a decision-queue item, an external-clock fact under its carve-out) — and then it is
  one short line pointing at the page that carries the options.
- **The call-sheet page remains the founder's primary interface** (ux lens): decision queue,
  shipping report and open questions live there, updated in place at its stable URL.
- **Scope** (architect lens): an operational communication stance, not a code gate and not a
  change to any recorded doctrine — autonomous starts (ADR-20260810-011500), the decision queue,
  the founder-message relay rule (ADR-20260812-143619) and the carve-outs all stand unchanged.
  It sharpens ADR-20260810-114242's "transparency, never a permission request": the transparency
  artifact is the page, not a chat monologue.

## Consulted

- **architect** — record it; radio discipline made explicit, composes with autonomous-start and
  recording doctrine; not new doctrine, not a code gate.
- **beck** — nothing on the directive (contributed the #703 failing-test spec).
- **business-specialist** — communication cadence moves no unit economics; flagged the general
  risk of hidden option spaces, unmeasured here (the decision queue still surfaces them).
- **dba** — nothing in my lens.
- **evans** — asked for scope specificity so the directive does not float free; answered above
  ("What it binds").
- **farley** — nothing on the directive (contributed the #703 extraction/gate read).
- **graphql-architect** — nothing in my lens.
- **holub** — nothing on the directive (contributed the #703 scope fence).
- **legal-specialist** — nothing on the directive itself (contributed the #699 SMS-continuity
  obligation map, recorded on that issue).
- **observability-agent** — nothing on the directive (contributed the #703 merged_at
  failure-mode rule).
- **reviewer** — nothing on the directive (contributed the #703 clean-pass checklist).
- **ux-designer** — coherent; internal coordination surfaces stay rich, the call-sheet page stays
  the primary interface; asked which surfaces "quiet" touches — answered above: the founder chat
  channel; product notification surfaces are untouched by this ADR.
- **vernon** — nothing on the directive (contributed the #703 purity-boundary ruling).
- **young** — nothing in my lens.
