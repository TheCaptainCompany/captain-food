# ADR-20260811-170559 — The validator owns the warning baseline; no document pins a warning count

## Status

Accepted

## Context

`make validate` must be **0 errors and no NEW warning**. The "no new warning" half was enforced by
prose: `CLAUDE.md` pinned the count and the per-kind histogram, next to an instruction telling every
reader to distrust it and *"ALWAYS re-measure the baseline on a pristine `main` worktree before
comparing"*.

Both halves of that arrangement failed, repeatedly and measurably:

- The pin went stale three times — 32, then 43, then 37 — each within days of being written.
- On 2026-08-11 alone, **four different agents** each paid a full extra validator run against a
  pristine `main` worktree to establish the real figure before they could claim "no new warning";
  three of them said some version of *"the pinned number looked wrong, so I re-derived it"*. A fifth
  then landed a corrected pin (`37, measured at d7087fb`) — reinstating the same trap with a fresher
  number.
- Three independent reviewer passes on
  [#304 "The Mailbox port surface hole"](https://github.com/TheCaptainCompany/captain-food/issues/304)
  had already had to stop and re-derive the same thing.

The defect is structural, not editorial: **a number in prose cannot be kept true, and prose telling
you not to trust the number printed beside it is a design smell.** CLAUDE.md's own rule already names
the fix — *"prefer executable over prose — a validator rule, test or hook beats a bullet point,
because prose can be ignored and a gate cannot"* — and ADR-20260808-235113 forbids the cheap
intermediate (updating the number again) where the final shape is buildable.

## Decision

**The validator owns its own baseline.** `tools/codegen-rs/warning-baseline.json` holds the per-rule
warning histogram (`total` + `by_rule`), and validator **§17** (`validate/warning_baseline.rs`)
asserts it on every run — `make validate`, the CI `codegen` job, and a codegen test
(`the_committed_warning_baseline_matches_the_real_specs`) that keeps the assertion alive under
`cargo test` alone.

The comparison is **exact in both directions**:

- **live > committed**, or a warning kind absent from the artifact — a regression. Fix it, or bank it
  deliberately.
- **live < committed** — an improvement that must be banked, because a baseline left high is exactly
  the stale number this ADR removes: the freed budget would be silently re-spendable.

`make warning-baseline` is the **only** writer (`generate --write-warning-baseline`). A change that
legitimately moves the warning surface refreshes the artifact **in the same commit**; the diff
(`+1 event-not-projected`) is the record, and the PR body says why an added warning is accepted. The
**Every field is asserted, prose included.** `total` is cross-checked against the histogram sum, and
the artifact's `doc` field — the one place a reader is told how to change the file — must be verbatim
the text the writer emits. That check was added after review caught the artifact shipping a `doc` that
pointed at validator **section 16** while the section had been renumbered to **17**: hand-patched
everywhere except in the file whose own text forbids hand-editing, and invisible to a gate that read
only `by_rule`/`total`. An unasserted self-description is just a comment — the exact defect class this
ADR exists to abolish, one level up. `make warning-baseline` is refused outright on a model with
errors, so a red spec can never mint a blessed baseline.

**No document pins a warning count any more.** `CLAUDE.md`, `docs/claude/codegen.md`,
`docs/claude/autonomous-run.md` and `.claude/agents/reviewer.md` point at the artifact and at the
gate; the "re-measure on a pristine `main` worktree" ritual is deleted, not restated.

**The sweep covers open proposals too, not only the agent-facing files.** A grep for the ritual found
it in nine proposals and in the live decision queue. Three classes, handled differently:

1. **A false premise arguing a still-open decision** — [DECISIONS.md](../proposals/DECISIONS.md) D3 of
   the business-metrics queue, and the matching pros/cons rows in `PROP-20260810-234225` and
   `PROP-20260811-000946`, all justified ERROR severity with "a warning changes no behaviour". That is
   now false and it was **load-bearing**, so it is corrected in place: the recommendation still stands,
   but on the ground that survives — a warning is cleared by refreshing a *count*, while the enumerated
   waiver list names each exemption and shrinks. The architect should know D3's argument is weaker than
   when it was written.
2. **Prospective acceptance criteria in unrealized proposals** — six lines instructing a future
   executor to re-measure against a pristine `main`. Corrected: proposals are living documents
   (ADR-20260801-020000), and leaving a live instruction to perform an abolished ritual is the same
   defect the agent-file sweep was for.
3. **Applied history** — the expected-validator-delta sections of `PROP-20260808-221424` and
   `PROP-20260808-233000` (both Approved *and applied*), and
   [ADR-20260810-234225](ADR-20260810-234225-business-metrics-for-every-feature-and-every-persona.md).
   Left as written: they record what was predicted for a change that already landed. History is not
   corrected, only superseded.

## Alternatives considered

- **Update the pin to 37** — rejected outright: it reinstates the trap with a fresher number and
  guarantees a sixth agent pays for it. This is the documented failure mode, three times over.
- **Delete the numbers, keep only the kind names + the re-measure instruction** — better, but it keeps
  the per-session pristine-`main` run (the actual cost) and still relies on a human noticing a new
  warning in a 37-line list.
- **Emit the profile as an ordinary generated artifact rewritten by `make generate`** — rejected: the
  ratchet would be self-satisfying (regenerate, and the "new" profile is the baseline), leaving only
  the diff to catch a regression nobody is required to look at.
- **Pin per (rule, location) rather than per rule** — rejected: locations churn on every rename, so the
  artifact becomes a merge-conflict generator, for a granularity the gate does not need.

## Consequences

### Positive

- The number cannot go stale: a stale baseline is a gate failure, not a misleading sentence.
- "No NEW warning" stops being a per-session hand-derivation (~1 validator run each, four in one day)
  and becomes an assertion the gate makes for free.
- The warning surface becomes reviewable: a PR that widens it shows `+1 <kind>` in its diff.
- The ratchet tightens automatically — fixing warnings is banked, never quietly re-spent.

### Negative

- A change that moves the warning surface now touches one extra file. This is deliberate: that file
  IS the record of what the change did to the warning surface. `make warning-baseline` makes it a
  one-command step, and the failure message says so, so it should not tempt anyone to route around
  the gate.
- Two gates assert the same artifact (the binary's `--check` and the codegen test). Duplication on
  purpose: `make rust` runs them separately and a ratchet only one of them enforces is a ratchet a
  partial run skips.

### Follow-up actions

- None for this mechanism. The artifact is self-refreshing by command, and every one of its fields —
  `doc` included — is asserted, so "self-describing" is a gate rather than a claim.
- For the architect, not for this ADR: **D3 of the business-metrics decision queue lost its original
  argument** (see above). The recommendation is unchanged and still defensible, but if that row is
  being decided, decide it on the waiver-list reasoning, not on "warnings are invisible".
