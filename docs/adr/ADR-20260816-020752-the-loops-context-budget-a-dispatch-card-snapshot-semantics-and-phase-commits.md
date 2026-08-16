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

**Amended 2026-08-16 by [ADR-20260816-134352](ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)**
(founder ruling on §44): every card additionally states its **`Reversibility class:`** and the
**briefing roster** derived from it, and carries a **`Checkpoint verification:`** line in the
Findings block at the checkpoint. Cards written before that ruling are historical and are not
retrofitted.

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

## Amendment, same day — the three landed mechanisms (founder: *"Apply these recommendations"*)

Decisions 1–7 above are technique. These three are the **artifacts** that carry them, landed
together in one docs/config change; they extend, and do not modify, anything above.

### 8. `make test-quiet` / `make rust-quiet` — filtering that may drop PROGRESS, never VERDICTS

(**farley**.) Makefile TARGETS, not a hook: **a hook is invisible to CI and cannot be diffed**. Each
wrapper runs the real gate, keeps the full output in `target/quiet-gate.log`, prints the **verdict
lines first (grep) and the tail second**, and echoes the gate's exit status before re-raising it.

**The rule, stated in the recipe comment: filtering may drop PROGRESS, never VERDICTS** — a verdict
is anything that could turn green into red: the DB-skip receipt
([#230](https://github.com/TheCaptainCompany/captain-food/issues/230), *"a skip that reports ok is
not evidence"*), the first panic, every `test result:` summary, the validator's error lines, the
warning-baseline diff. Grep-**first** is load-bearing: a tail-only filter loses an early panic, which
is the case that matters. Proven red before landing — a 122-line run whose panic is on line 1 keeps
that panic while the 50-line tail cannot reach it, and `exit=101` propagates.

Two constraints the next editor must not "simplify" away: the gate is **not piped** (its status is
captured directly, which is stronger than `set -o pipefail` **and** portable — make runs recipes
under `/bin/sh`, which is dash on Debian/Ubuntu, and dash answers `set -o pipefail` with *"Illegal
option"*, failing the recipe before the gate runs); and `QUIET_KEEP` must stay **pure ASCII**,
because it is expanded INTO a recipe line, so a byte > 127 there breaks Cygwin make at runtime even
though `makefile_recipe_lines_are_ascii` reads the recipe text as ASCII.

### 9. `.claudeignore` + `permissions.deny` — deny what no verdict is ever derived from

(**farley**.) Build output, object stores and vendored trees are re-derivable from source, so reading
them can only cost tokens — never change a gate result. The list lives in `.claudeignore` **and** is
mirrored in `.claude/settings.json` `permissions.deny`, so a proactive read is stopped even by a
client that never loads the ignore file.

**Three paths are deliberately NOT denied**, and each carries that note in both files:
`specs/generated/**` (the codegen drift gate's **evidence** — denying it makes `check-drift`
unauditable by the very agent that must fix it), `Cargo.lock` (a lock diff **is** a supply-chain
review), `tools/codegen-rs/warning-baseline.json` (the ratchet).

### 10. CLAUDE.md compressed to an index — and young's residency test

The resident file keeps **every rule** and loses the incident narratives (the stale-warning-count
history, the seven-review-rounds story, the two-wrong-key-sets story, the four-agents-a-day cost),
which already live in the ADRs that earned them; verbatim founder quotes survive as the operative
clause plus a pointer to the ADR holding the full text, never as a hand-written paraphrase
(**evans**: a gloss competes with the original). DSL/`$ref` mechanics, screens/translations detail,
C4, Honeycomb query discipline and HubRise mapping are now a pointer index into
[docs/claude/](../claude/). **No `.claude/rules/` directory was created** (**evans**: two homes with
near-identical names are confused within a week — extend `docs/claude/` instead). Makefile-ASCII and
the warning ratchet are **already gates**, so their prose is one imperative line each pointing at the
gate (**architect**: a rule whose trigger path differs from the path being edited can never load in
time, so it must be a gate, not lazily-loaded prose).

**The residency test, verbatim (young)**: *"if forgetting the rule produces state a rebuild cannot
undo, it must be always-resident."* A lazily-loaded rule is a **read model** — legitimate exactly
when it is **rebuildable**, because the decision it governs happens after the load, so a late fetch
changes nothing. The exception is the class where **touching the path IS the mistake**: appending a
stored event shape, a migration, a decision reversal, GDPR erasure — write-side, irreversible,
history cannot be replayed away. That test is stated at the top of CLAUDE.md, together with the rule
that the compressed file is a **snapshot plus its topic file, never the snapshot alone**, and that
removing a rule from it is a decision reversal needing a register row rather than an edit.

Measured with the crude `wc -w` × 1.35 proxy (a proxy, not a token count): **~7,570 → ~4,610**. The
~2,500 target was **not** reached, and deliberately so: the remaining text is ~24 rules plus the
domain lens, and closing the gap would mean dropping rules, which is a decision reversal, not
compression.

### 11. Idle MCP servers are disabled — and the disablement is recorded where the next session looks

(**observability**.) The honeycomb MCP server is **disabled** via `disabledMcpjsonServers` in
`.claude/settings.json`. Its reasoning, quoted: it is **"not a blinded instrument, it is a broken
one"** — unauthenticated, its tools cannot run, and no `apps/` runtime emits spans yet, so nothing is
lost that re-auth would not have to restore anyway. **Re-auth is the event that re-enables it**
(delete the array entry).

The condition attached, and it is the load-bearing half: **record the disablement where the next
session sees it, otherwise "no Honeycomb server" reads as "no telemetry concern" — silence must not
be ambiguous** (the same defect class as
[ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md)'s
monitoring clause). Hence: the server DEFINITION stays in `.mcp.json` rather than being deleted — so
the **`eu1` EU-host pin, a GDPR constraint, is not lost with the server config** — CLAUDE.md's
telemetry paragraph carries one line saying the server is deliberately disabled pending re-auth, and
[docs/claude/observability.md](../claude/observability.md) keeps the EU-host warning intact. A Gmail
server appears nowhere in this repo's `.mcp.json` or docs, so there was nothing to disable; `github`,
`claude-code-remote` and `supabase` are untouched.

## What this ADR does NOT decide (**decided 2026-08-16 — see below**)

**How the mob's fan-out is priced.** Narrowing the roster at the checkpoint amends a founder directive
([ADR-20260809-013142](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md): *"the roster
is invited by default and a lens excuses itself"*), so it is a **decision reversal, not a technique
change** — it is [DECISIONS](../proposals/DECISIONS.md) **§44 / MOB-COST-1**, 🟡 FOUNDER-OWNED.
Note the ordering: **decision 2 above cuts the per-lens cost ~10× whichever way §44 goes**, so §44 is
a question about **detection policy**, not about the bill.

**Resolved the same day** by the founder, verbatim: *"Go for the Recommendation: (b)+(c), with
holub's verification condition."* — recorded in
[ADR-20260816-134352](ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md),
which amends [ADR-20260809-013142](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md).
The briefing half is untouched; the checkpoint goes to lenses that declared a concern, and the
chunk's reversibility class sizes the briefing roster. The measured basis for the ~10× claim above:
lenses reading the repo on #167 ran **50–85k each**; card-based lenses on #588 ran **26–44k each**.

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
- **architect** (amendment): the resident list — everything that fires before the first file is
  read; and path-mismatched rules become gates, never lazily-loaded prose.
- **evans** (amendment): the domain lens and conventions ARE the ubiquitous language and stay
  resident; no `.claude/rules/`; a paraphrase of a founder quote competes with the original.
- **holub** (amendment): the cut test — does this paragraph change what an agent does on a turn
  where it is not already obviously in scope? The incident narratives are the fat.
- **farley** (amendment): `make test-quiet` as a diffable Makefile target rather than a hook, and
  the deny/keep split — deny what no verdict is ever derived from.
- **observability** (amendment): disable honeycomb ("not a blinded instrument, it is a broken one"),
  but record the disablement, because silence about telemetry is ambiguous.
- **young** (amendment): a lazily-loaded rule is a read model, legitimate when rebuildable —
  *"if forgetting the rule produces state a rebuild cannot undo, it must be always-resident."*
- **Remaining roster lenses (ux, legal, dba, vernon, evans, security)**: not asked on this message —
  stated rather than elided. Nothing here touches a customer-visible surface, a legal artifact, the
  schema, an aggregate boundary or the ubiquitous language; if §44 is answered in a way that narrows
  the checkpoint, ux and legal are the two whose exclusion today's evidence argues hardest against
  (both of today's checkpoint STOPs were theirs).
