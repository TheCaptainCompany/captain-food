# ADR-20260816-020752 — The loop's context budget: a dispatch card, snapshot semantics, and phase commits

**Status**: Accepted (the six technique changes below; the *detection policy* they do not settle is a
FOUNDER-OWNED register row) · **Date**: 2026-08-16 ·
**Decider**: the team (technique is the team's — ADR-20260810-011500), from a founder question ·
**Register**: [DECISIONS](../proposals/DECISIONS.md) §44 (**MOB-COST-1**) ·
**Session**: https://claude.ai/code/session_01SDJjYQsfwaa4DVyNfFepbA

## The question (founder / Tech CEO, 2026-08-16)

> "Do you have recommendations to optimise tokens consumption?"

## The measured baseline, honestly

**~2.5M tokens for one merged work item** on 2026-08-15 — briefing, implementation, checkpoint,
independent review, gate rounds, merge supervision. That figure is an after-the-fact reconstruction,
not a measurement: **no per-item instrument exists today**. Decision 6 below is what makes the next
such number a reading rather than an estimate, and every quantity in this record should be read as
an order of magnitude, not a metric.

## Decisions

### 1. Reading a subagent's `.output` transcript is BANNED when the completion notification carries the answer

The agent's answer arrives in the completion notification. Re-reading the run's `.output` transcript
adds nothing and costs the whole transcript — **~300k tokens per chunk of pure loss**. The file is a
**fallback for a DEAD agent only**: an agent that died before answering, where the transcript is the
only surviving artifact. Ceremonially opening it "to check" is banned outright.
(**holub** #1; **architect**: *"ceremonial, ban outright"*.)

### 2. The dispatch card — one file per chunk, and the lenses read the card, not the repo

The coordinator authors **ONE file per chunk**: the chunk, the paths in play, the phases, the gates,
the out-of-scope fences. Every lens in the mob briefing reads **that file**, not the repository.
**12×50k becomes 12×~5k.** Lens replies **append one line to the card's Findings block**, and that
block **IS the mob evidence the PR body cites** — so the findings are written once, in the place they
are consumed, instead of being re-summarised into a PR body afterwards. (**architect**.)

### 3. Snapshot semantics for the card — it is a cached fold, never a second source of truth

The doctrinal ruling, because a card is exactly the shape our own read side has (**young**):

- The card is a **cached fold over the tree — disposable, never authoritative.**
- **Stamp it with the commit SHA it was taken at.** A checkpoint invocation loads **card@SHA +
  `git diff <SHA>..HEAD`** — never a re-fold of the whole tree.
- **Version mismatch = DISCARD and re-derive, never patch.** Patching a stale snapshot is how a cache
  acquires its own history.
- **Every lens keeps the right to fall through to the tree.** If a lens *cannot* — if the card is the
  only thing it is allowed to see — the cache has become a **second write model**, which is the
  failure this project refuses everywhere else.
- **Falsification test**: delete the card, re-run one briefing against the tree. **No verdict may
  change.** If one does, the card was lying, not summarising.

### 4. Phase commits make death cheap

**Commit at every declared phase boundary.** An agent that dies mid-run then costs **one phase**, not
the whole run. Earned today: a reviewer agent died on a credit limit and its entire run — ~400k
tokens of work — was lost, with nothing committed to resume from. This composes with the existing
`wip:` rescue-commit rule in [sessions.md](../claude/sessions.md). (**architect** + **observability**.)

### 5. Mutation-red is paid once, not twice

Proving a gate can fail is mandatory (the "seen red" rule); paying for it twice is not (**beck**):

- **Red-first**: write the assertion *before* the rule it checks. The red is then a **TDD byproduct**,
  free — instead of planting a violation after green, which pays for the mutation *and* the restore.
- **Mutate DATA, not Rust source**: a deliberately bad spec fragment through `make validate` proves
  the same rule with **no recompile**.
- **Batch** independent mutations that fail *distinguishable* tests into one run.
- **Revert with `git checkout -- <path>`** (idempotent; never from a copy you took yourself).
- **Never re-run the full suite to "confirm green after revert."** An empty `git diff` plus the prior
  green **is** the evidence.

### 6. Gate economics — a full local `make rust` before every push is not always load-bearing

(**farley**.) A full local `make rust` before every push **duplicates CI**. It is load-bearing in
exactly one case: **direct-to-`main` spec/doc pushes, where no CI follows**. On a PR branch, the
pre-flight that pays for itself is **seconds long**:

- a fetch + rebase check (does this branch still merge?),
- `make validate`,
- a markdown-table lint.

That pre-flight **would have caught BOTH of today's red CI rounds** — a merge conflict and a
malformed register row — **without a workspace build**.

**Path filters keyed on one question: can this change generated output?** `docs/**`, ADRs and
`STATUS.md` skip the matrix; **`specs/**` is NEVER filtered**. Keep the **required-check names
stable** with a skip-job that reports success, or branch protection deadlocks on a check that never
reports.

And: **a flaky gate is a token pump.** [#388 "[watchdog] Flaky SIGSEGV in `infrastructure` lib-test
binary reddens the `ci` build gate on
`main`"](https://github.com/TheCaptainCompany/captain-food/issues/388) buys a full matrix re-run
every time it fires.

### 7. Cost becomes observable — one column, and a dead-man's-switch

(**observability**.) Add a **`tokens` field to the existing `.claude/loop-budget/<ISO-week>/*.json`
ledger** — same files, same weekly window, same guard, **one append per agent completion**, plus an
**`agent` field** so cost attributes to a lens rather than to "the session".

**The alarm is a DEAD-MAN'S-SWITCH, not a threshold.** A run burning tokens with **no ledger write**
is indistinguishable from a run that never started, and a threshold-only warning goes silent exactly
when the writer dies. This is the
[ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)
defect class — *a monitoring path that can only fire when a signal arrives* — applied to ourselves.

## What this ADR does NOT decide

**How the mob's fan-out is priced.** Narrowing the roster at the checkpoint amends a founder directive
([ADR-20260809-013142](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md): *"the roster
is invited by default and a lens excuses itself"*), so it is a **decision reversal, not a technique
change** — it is [DECISIONS](../proposals/DECISIONS.md) **§44 / MOB-COST-1**, 🟡 FOUNDER-OWNED.
Note the ordering: **decision 2 above cuts the per-lens cost ~10× whichever way §44 goes**, so §44 is
a question about **detection policy**, not about the bill.

## Consequences

- Nothing here weakens a gate: decisions 5 and 6 change *when and how* a gate is run, never *what it
  proves*. `specs/**` remains unfiltered; the "seen red" evidence rule is unchanged.
- Decision 3 is a constraint on the card, not an optimisation of it — a card that cannot be deleted
  and re-derived without changing a verdict is a defect, and the falsification test is how a session
  finds that out cheaply.
- Decisions 1, 4 and 5 are operational and land in
  [docs/claude/sessions.md](../claude/sessions.md) in this same change; 2, 3 and 6 are technique the
  coordinator applies from the next dispatch; 7 is a build item with no issue yet.

## Consulted (ADR-20260812-143619 — one line per lens)

- **architect**: the dispatch card, and *"ceremonial, ban outright"* on `.output` transcripts —
  12×50k becomes 12×~5k, and phase commits make a dead executor cost one phase.
- **holub**: the `.output` ban (#1, ~300k/chunk of pure loss), and the narrowed-checkpoint option in
  §44 with its own verification condition attached.
- **young**: snapshot semantics — the card is a cached fold, SHA-stamped, discarded on mismatch, with
  the fall-through right and the delete-and-re-run falsification test.
- **beck**: red-first over plant-after-green; mutate data, not source; batch; `git checkout --`;
  never re-run the suite to confirm a revert.
- **farley**: gate economics — the seconds-long pre-flight that would have caught both of today's red
  CI rounds, path filters keyed on generated output, stable required-check names, flake-as-token-pump.
- **observability**: the `tokens`/`agent` fields on the existing ledger, and the dead-man's-switch
  framing that keeps the alarm honest when the writer dies.
- **business**: price review by **reversibility**, not by chunk — §44 option (c).
- **Remaining roster lenses (ux, legal, dba, vernon, evans, security)**: not asked on this message —
  stated rather than elided. Nothing here touches a customer-visible surface, a legal artifact, the
  schema, an aggregate boundary or the ubiquitous language; if §44 is answered in a way that narrows
  the checkpoint, ux and legal are the two whose exclusion today's evidence argues hardest against
  (both of today's checkpoint STOPs were theirs).
