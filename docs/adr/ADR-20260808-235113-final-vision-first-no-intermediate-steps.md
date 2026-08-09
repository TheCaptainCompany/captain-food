# ADR-20260808-235113 — Final vision first: no intermediate step where the final step can be built

**Status**: Accepted · **Date**: 2026-08-08 (night) · **Decider**: the customer (product owner),
in session, as a standing principle — recorded verbatim so every lens and dispatch weighs options
by it.

## The directive (verbatim)

> "Principle: do not choose the easy path, choose the final clean vision no intermediate step
> always put in place the final step"

Given minutes after choosing D-QW1 option (b) (ADR-20260808-234907) — the self-contained event
payload over the expedient worker-side lookup — which is the directive's lived example: when an
option space contains a cheap intermediate and the final clean shape, **build the final shape
directly**, even when the intermediate is smaller today.

## Interpretation (bounded honestly, so it is applied and not over-applied)

- **What it changes**: option analysis and recommendations. A "pragmatic for now, redo later"
  option is no longer the default recommendation; the final-vision option is presented first and
  chosen unless something *external* (law, a provider, physics of the cutover) forces staging.
  It composes with the compiler-first doctrine (ADR-20260803-234035) — the type-level answer IS
  the final clean vision — and with the structural-isolation program
  (ADR-20260808-212741 §6: the maintainer is the AI; structure is the safety system).
- **What it does NOT overturn**: gate-then-stabilize (CLAUDE.md non-negotiable). Gating is about
  WHEN a finished thing takes over, not about building throwaway shims — a gated final
  implementation satisfies both rules; a temporary shim satisfies neither. Likewise the
  proportionality rule for records stands: this is a design principle, not a mandate to
  gold-plate documents.
- **When staging is genuinely forced**, the intermediate step must be chosen WITH the final step
  already designed and recorded, so the intermediate can never quietly become the end state.

## Consequences

- CLAUDE.md carries the principle as a one-bullet rule referencing this ADR.
- Standing lens agents (architect, dba, graphql-architect, farley, holub) weigh option spaces by
  it; dispatch briefs inherit it. **Made executable 2026-08-09** (audit finding A1): the
  proposal-authoring contract (docs/proposals/README.md) requires the final-vision option FIRST
  and a recorded final for any staged recommendation; the architecture-review checklist carries
  the matching probe.
- Immediate application: the D-QW1 option-(b) rewrite (in flight) resolves its required-vs-nullable
  calls toward the final clean contract (required wherever every emitter can supply the field).

## Boundary sharpenings (2026-08-09, from the corpus audit — customer veto open)

The full-corpus audit (same night) found the decisions largely compliant and the recommendation
engine the gap; it also surfaced two boundaries the principle needs to avoid misuse:

1. **Scope staging is not shape staging.** Thin vertical slices OF the final shape (slices 3–8 of
   the rider epic) are how the final vision ships — compliant. What the principle bans is SHAPE
   staging: building a different shape that must be redone.
2. **Evidence-deferred decisions are not intermediates.** Where the final value is not yet
   knowable (rider-offer TTL, avelo37 threshold), the compliant move is instrument-then-decide
   (ADR-20260808-144738), never guessing a "final" number to satisfy the principle's letter.
