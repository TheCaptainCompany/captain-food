# ADR-20260901-010206 — The link gate: relative links block, external URLs do not

- **Status**: Accepted (founder directive, 2026-09-01)
- **Directive, verbatim**: *"excellent point put in place this url checker that must be executed
  locally and enforced in the CI too"* — **both halves are named by him**, local and CI.
- **Issue**: [#837 "No link checker exists, so ~25 broken relative links accumulated silently: add one runnable locally and enforced in CI"](https://github.com/TheCaptainCompany/captain-food/issues/837)
- **Supersedes / amends**: nothing. There was no link checking anywhere before this.

## Context

`docs/**` **is** the operating model. CLAUDE.md is explicitly an index whose authority is the topic
file it links to; every ADR cites its neighbours; the register-check discipline turns on a reader
being able to *follow* a citation. **A broken link is a citation that silently resolves to nothing** —
GitHub renders it dead with no error, so nothing in this repository would ever have told us.

Nothing did. Re-derived on a stated method, the tree carried **8,045 relative links across 451
tracked markdown files, of which 130 were broken**.

## Decision

A checker (`tools/link-check.py`) runs from `make link-check` locally and from two pinned steps in
CI's always-run `gate-scripts` job. Five choices, recorded because each was a real fork:

1. **Relative links block; external URL liveness is OUT.** A blocking gate whose verdict depends on
   a third party's uptime and rate limiter reds on honest work — the instrument
   `tools/codegen-rs/src/tests.rs` has retracted five times over, under one rule: *a red that fires
   on innocent work trains readers to discount reds*. An unreachable URL is also usually not a defect
   in the commit being gated. Link rot is real, but it is a periodic report's job, not a merge
   blocker's. Reversing this needs a new decision, and `T9` of the selftest reds if someone wires
   network checking in without taking it.

2. **Fragments (`#section`) are IN.** The slug algorithm is deterministic and published
   (github-slugger), there is no network and no flake in it, and a citation that lands in the right
   file at the wrong heading is the same silent nothing. This is what found the emitter defect below.

3. **Fix-all, gated at ZERO. No baseline file.** A baseline is a second thing to keep honest and this
   repo has been bitten by exactly that. All 130 were fixed in the landing change.

4. **It lives in `tools/`, not `.claude/hooks/`.** Not a preference: the `gate-scripts` job's own pin
   forbids a non-gate step from mentioning `.claude` anywhere in its definition
   (`tools/codegen-rs/src/tests.rs`, the needle scan over every non-gate step). A checker in
   `.claude/hooks/` could not be invoked from that job at all.

5. **The selftest runs in CI as its own step, before the scan.** A scanner that matches nothing
   passes — it reports zero broken links over zero links and exits 0. The checker's three vacuity
   guards are what make its green mean something, and a guard nobody has watched fail is an
   unverified claim.

## Compiler-first: this lands at level 3, and level 3 is the ceiling

[ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) makes a check
the *fallback*: ask first whether the type system can make the mistake unspellable. It cannot reach
here, and the argument is the one already recorded for `specs/**` YAML in
[PROP-20260802-130500](../proposals/PROP-20260802-130500-isolation-by-construction.md) §1 — the
enforcement hierarchy ranks ways to stop *Rust code* naming something it should not, and it has no
rung for a target written in prose. No newtype, sealed trait or capability witness can make
`[x](gone.md)` unspellable, because the compiler never sees the markdown. So "start at level 4"
resolves to **level 3 is correct here, not lazy**, exactly as it did for the `reads:` wall
([ADR-20260812-214500](ADR-20260812-214500-a-read-target-is-declared-not-inferred-the-reads-ownership-wall.md)).
It is also the case ADR-20260803-234035 names in its own carve-out: non-Rust artifacts.

**Where a stronger instrument was available it was taken.** `specs/generated/documentation.generated.md`
can no longer contain a dead in-page link *at all*: the emitter de-links any anchor it does not
define, so the class is unrepresentable in the artifact rather than merely detected in it.

## Consequences

- Every push runs the gate, including the docs-only lane that reaches `main` as a push with **no PR**
  and skips every Rust job — which is the lane most likely to break a citation.
- **A generated-documentation defect was found by it, not by review**: a test's `when:` is not always
  a command (59 tests are driven by an inbound integration event —
  [ADR-0004](0004-commands-derived-from-use-cases.md)), and a saga is documented as an `actor`, not an
  `entity`. Both were hardcoded kinds in `emit/docs.rs`, producing 102 anchors no `<a id>` defined.
- **Known residue, not hidden**: `CartLine` and the referential tables (`PricingPolicy`,
  `UberEstimationPolicy`, `UberSplitPolicy`) have **no section at all** in the generated
  documentation. That is a content gap, not a link bug, and closing it means deciding what sections
  that document should grow — a separate question. Today those labels render as plain text.
- A contributor who renames an ADR now learns immediately that three files cited it.
