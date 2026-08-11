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
artifact carries its own `doc` field saying this, and `total` is cross-checked against the histogram
sum so a hand-edit that fudges one of the two is rejected.

**No document pins a warning count any more.** `CLAUDE.md`, `docs/claude/codegen.md`,
`docs/claude/autonomous-run.md` and `.claude/agents/reviewer.md` point at the artifact and at the
gate; the "re-measure on a pristine `main` worktree" ritual is deleted, not restated.

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

- None. The artifact is self-refreshing by command and self-describing in its `doc` field; nothing
  needs to be remembered.
