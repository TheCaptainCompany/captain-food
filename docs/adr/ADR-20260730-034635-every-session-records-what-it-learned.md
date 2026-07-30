# ADR-20260730-034635 — Every session records what it learned

- **Status**: Accepted (product-owner directive, 2026-07-30)
- **Date**: 2026-07-30
- **Extends**: the existing CLAUDE.md rule *"Every recurring agent/loop failure becomes a new rule, test, or ADR"*
- **Companion**: [docs/claude/sessions.md](../claude/sessions.md) (where operational lessons land)

## Context

A session is ephemeral: the container is reclaimed, the transcript is not read again, and everything
learned in it is lost unless something was written to the repo. The operating model already captures
three kinds of knowledge — **decisions** (ADRs), **rationale and option space** (proposals), and
**state** (`STATUS.md`) — but nothing owned the fourth: what the session *discovered* about how to
work here.

That gap had a measurable cost. The 2026-07-30 session rediscovered that PDFs cannot be read in this
container (four separate dead ends), that `df` misreports a spent disk allowance as a broken machine,
that GitHub MCP search returns full issue bodies by default, and — most expensively — that proposing
credential names before establishing which product and auth mechanism an integration uses produces
wrong names (ADR-20260730-032306: two wrong key sets, four mis-named repository secrets). None of that
was novel. It was simply nowhere.

The existing rule only fired on *recurring* failures, which means the first repeat was already paid
for, and only on *failures*, which excluded the many findings that are neither failure nor decision.

## Decision

**Every session records what it learned that would help the next one, in the same change as the work.**

Where it goes, by kind:

| What was learned | Where it lands |
|---|---|
| Environment limit, tool behaviour, gate cost, workflow trap | [`docs/claude/sessions.md`](../claude/sessions.md) or the relevant `docs/claude/` topic file |
| A decision that was made | an ADR |
| An option space that was weighed | a proposal in `docs/proposals/` + its tracking issue |
| Current state of the system | `docs/STATUS.md` |
| A rule the system can check itself | **a validator rule, test, or hook — not prose** |

Three constraints keep this from becoming ceremony:

1. **Prefer executable over prose.** If a lesson can be a validator check, a behaviour test, or a
   hook, write that instead — prose can be ignored, a gate cannot. `makefile_recipe_lines_are_ascii`
   is the model: a one-off breakage became a codegen test, so it cannot silently return.
2. **Only what is not derivable from the code, and only what would cost the next session time.**
   Each entry carries the concrete cost that earned it. This is not a session diary, and a change log
   is not a lesson.
3. **Writing nothing is a valid outcome.** A session that learned nothing transferable adds nothing.
   Padding the file is worse than leaving it short, because it lowers the odds the real rules are read.

Sharpen or extend an existing rule rather than appending a near-duplicate; two overlapping rules mean
neither is trusted.

## Consequences

- `docs/claude/sessions.md` becomes a living file with an owner: whoever finishes a session.
- Wrapping up a session gains one step — decide what, if anything, was learned, and put it where the
  table says. It lands in the same commit as the work, so it cannot be deferred and lost.
- Lessons compound instead of being re-paid. The first repeat of a known trap is no longer the price
  of admission.
- Risk accepted: the file grows. Mitigated by constraints 2 and 3, and by preferring executable rules
  — which shrink the prose rather than adding to it. If it ever reads as a diary, it has failed, and
  the fix is deletion, not more headings.
- CLAUDE.md's non-negotiable rules now state the obligation directly, so it is in scope for every
  session without needing to have read this ADR.
