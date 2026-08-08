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
  it; dispatch briefs inherit it.
- Immediate application: the D-QW1 option-(b) rewrite (in flight) resolves its required-vs-nullable
  calls toward the final clean contract (required wherever every emitter can supply the field).
