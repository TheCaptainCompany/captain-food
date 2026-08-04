# ADR-20260803-234035 — Compiler first: a check is the fallback, not the tool of choice

- **Status**: Accepted
- **Date**: 2026-08-03
- **Origin**: product-owner directive, 2026-08-03 — *"I prefer to rely on compiler this should be the
  default approach."*
- **Refines**: [PROP-20260802-130500](../proposals/PROP-20260802-130500-isolation-by-construction.md)
  §1 (the enforcement hierarchy), which ranked the levels but never said where to START
- **Occasioned by**: [#329](https://github.com/TheCaptainCompany/captain-food/issues/329) — seven
  review rounds and ~191 lines spent hardening a level-3 scanner over a level-4 boundary

## Decision

**Before writing any check, ask whether the type system can make the mistake unspellable.** A rule
that fails compilation is the default; an executable gate is what you fall back to when it cannot.

The enforcement hierarchy already existed (prose → review → gate → compiler → credential) and
already said each level catches what the one below misses. It did not say *start at the top*, so
"climb one level" got read as an achievement rather than a floor. It is a floor.

In practice, the level-4 tools available with no extra machinery are:

| want to forbid | type-system answer |
|---|---|
| calling an operation from outside a boundary | a capability witness with a `pub(crate)` constructor (#304) |
| implementing/receiving something not declared | a sealed marker trait (#288) |
| constructing a value outside its owner | private fields + constructors (`MailboxEntry`, #290) |
| passing the wrong thing where a primitive would compile | a newtype (`Money`, the id scalars) |
| a state that should not exist | make it unrepresentable — split the enum, not the validation |

## Why

The compiler is the only gate that is free after it is installed, cannot be skipped, cannot go
stale, and does not need a reviewer to notice it did not run. Every other level costs attention
forever. That is the whole content of the hierarchy, and #329 is the empirical case: a syntactic
scanner over a boundary the compiler *already enforced* took seven adversarial review rounds and
~191 net lines, and every gap in it was found by a reviewer rather than by the scanner. The
underlying lock — a private constructor — took one commit and has never been bypassed.

A check is still worth building where types genuinely cannot reach: cross-crate manifest capability
(the D3 `sqlx` allowlist), spec↔generation drift, non-Rust artifacts (`makefile_recipe_lines_are_ascii`).
The rule is not "never write a gate", it is "a gate is what you write when the compiler cannot".

## Consequences

- **When a check IS the answer, prefer one that sees what the compiler sees.** A scan of source
  TEXT loses to reformatting; a scan of the AST loses to macros and to type information it does not
  have. If a real custom lint becomes worth it, that is `dylint` — and its cost is concrete: it
  links compiler internals, so it pins a **specific nightly**, against `rust-toolchain.toml`'s
  deliberate `channel = "stable"`. That trade is [#331](https://github.com/TheCaptainCompany/captain-food/issues/331),
  and it is a product-owner decision because it changes the toolchain posture, not just a test.
- **Deleting a gate the compiler subsumes is a legitimate outcome**, not a regression. If a check
  exists only because nobody asked the type-system question, the answer is to ask it now.
- Sessions.md §8b previously taught "a guard over Rust structure must parse the AST". True, but it
  was the wrong DEFAULT — it answers "how do I write this guard" before "should this be a guard".
  It is now subordinated to this directive.
- This does not weaken PROP-20260802-130500 phases 2–3. Per-actor crates are a level-4 mechanism
  (the manifest becomes the permission); they are exactly what this directive prefers.
