# ADR-20260801-020000 — Proposals are LIVING documents; git is the history

**Status**: Accepted (product-owner decision, in-session 2026-08-01) — SUPERSEDES the
"historical record, never rewritten" posture of ADR-20260724-135945 and retires the
inline-supersession-callout convention (introduced hours earlier, retired by this decision).
**Context**: the append-only discipline produced a proposal whose superseded sections, appended
refinements and callouts made the owner's own thinking unreadable ("I need a clean latest
version state of my thinking").

## Decision

1. **A proposal file always holds the CURRENT state of the design** — the clean latest version
   of the owner's thinking. When a decision refines an approved proposal, the file is REWRITTEN
   to the new state in the same change that records the decision (ADR).
2. **History lives in git, not in the document**: every prior state is `git log -p` on the file.
   The header carries one line pointing there. No superseded blocks, no post-approval appendix
   sections, no inline callouts.
3. **The rationale trail stays**: options-considered tables (with the chosen option marked) remain
   part of the current document — they are rationale, not history — and the ADR series remains
   the immutable record of WHAT was decided WHEN. Rewriting the proposal never rewrites an ADR.
4. Everything else stands: proposals live in docs/proposals/ on main, one tracking issue each,
   the proposal-hygiene validator rules, proportionality, "GitHub is never the record".

## Consequences

- PROP-20260731-195500 is the first proposal rewritten to this model (same change).
- CLAUDE.md's proposal bullet and docs/proposals/README.md drop the never-rewrite +
  inline-callout language in favor of the living-document rule.
