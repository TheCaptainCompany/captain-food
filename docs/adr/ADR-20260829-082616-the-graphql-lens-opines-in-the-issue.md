# ADR-20260829-082616 — When it concerns GraphQL, the graphql lens's opinion is posted in the issue

**Status**: Accepted · **Date**: 2026-08-29 ·
**Decider**: the **FOUNDER / Tech CEO**, verbatim below ·
**Prompted by**: [#749 "the storefront MENU has never rendered from a real paint"](https://github.com/TheCaptainCompany/captain-food/issues/749) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted — first applied on #749 itself (the graphql-architect opinion is a comment on the issue,
and the #749 dispatch named it the design authority for the arg shape).

## The directive, verbatim

> *"For the 749, adding a facultative argument slug seems to be the good approach. I need the
> opinion of graphql-architect in the issue next time when it concerns graphql"*

## The decision

When a chunk or issue **concerns the GraphQL surface** (schema shape, argument/type design,
role-path composition, versioning posture, resolver contracts), the **graphql-architect lens's
opinion is posted INTO the GitHub issue** — as a comment, before or at dispatch — not only voiced
inside the session. The issue is where the founder reads the design conversation, so the record
must live where he reads.

Boundaries, so this composes with what already stands:

- **GitHub is still never the record** (CLAUDE.md): the issue comment is the founder-facing
  RELAY; anything the opinion decides still lands in the repo (spec, ADR, proposal) in the same
  change. This ADR does not move any authority to GitHub — it adds a visibility obligation.
- **The mob briefing is unchanged** (ADR-20260809-013142 / ADR-20260816-134352): every invited
  lens still answers at the briefing; this rule additionally publishes ONE lens's answer to the
  issue when the subject is GraphQL.
- **Who does it**: the coordinator posts the relay (GitHub mechanics are the coordinator's lane,
  ADR-20260810-011500); the lens authors the content.

## Alternatives considered

- Keep lens opinions session-only and summarize in the PR body — rejected by the directive: the
  founder asked for the opinion *in the issue*, where the design question lives before any PR
  exists.

## Consequences

Positive: GraphQL design rationale is visible at the point of decision and quotable by later
dispatches (the #749 dispatch cites the in-issue opinion as design authority). Negative: one more
posting step per GraphQL-touching issue. Follow-up: none.

## Consulted

Same full-roster consult as the #749 briefing (2026-08-29):

- **architect** — adopt as a standing relay rule; the issue comment is the visibility surface,
  the repo stays the record.
- **beck** — nothing in my lens.
- **business** — nothing in my lens.
- **dba** — nothing in my lens (note the symmetry: storage-shape opinions already reach issues
  through HOLD-class review; no equivalent rule requested).
- **evans** — name it precisely: a visibility obligation, not a records move — "the issue is
  where he reads, the repo is where it lands".
- **farley** — fine; keep it human-process (no bot), since the CI auto-review is retired
  (ADR-20260828-091500).
- **graphql** — accept the duty; the opinion states the doctrine (additive evolution, one-of
  posture, role-path composition) so the issue reader gets the WHY, not a verdict.
- **holub** — nothing in my lens.
- **legal** — nothing in my lens.
- **observability** — nothing in my lens.
- **ux** — nothing in my lens.
- **vernon** — nothing in my lens.
- **young** — nothing in my lens.
