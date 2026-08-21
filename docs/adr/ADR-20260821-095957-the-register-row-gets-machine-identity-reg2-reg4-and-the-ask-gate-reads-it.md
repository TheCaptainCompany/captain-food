# ADR-20260821-095957 — The register row gets machine identity: REG-2/REG-4 land, the index is generated, and the ask gate reads the rows

## Status

Accepted

## Closes

`REG-2` · `REG-4` · `REG-3` · `REG-SEQ` — and records `REG-1`'s built form (each row now lives in
`docs/decisions/<KEY>.yaml`; this header names its keys per the ADR-VOLUME discipline this same
proposal ruled: *"an ADR that closes a register row names the key in its header"*).

## Enforced by

n/a — no behavioral guarantee in `rules.yaml` (operating-model surface, not domain behaviour).
The executable enforcement is validator **§22** (`tools/codegen-rs/src/validate/decisions.rs`,
on `make validate`'s single gate, with planted-defect tests proving every rejection rule red),
the **generated index region** in `docs/proposals/DECISIONS.md` (drift-gated by `check-drift`;
missing markers are a generation ERROR, never a skip), and the **register-check hook**'s row gate
(`.claude/hooks/register-check.sh` + its selftest, run by the stop gate every turn).

## Context

Founder directive, 2026-08-21, verbatim: *"Implement REG-2 and REG-4 as the next bounded slice:
one machine-readable decision file per globally unique key, a closed status vocabulary, resolvable
decided_by / superseded_by, generated decision index, and a validator that rejects any
founder-directed decision question referencing a non-open row. Do not backfill the entire
historical corpus in this slice; migrate only active and newly touched rows, with explicit legacy
handling."*

The design was already recorded and `Proposed` in
[PROP-20260819-110442](../proposals/PROP-20260819-110442-the-decision-register-is-the-unit-of-decision.md)
(rows in [DECISIONS §48](../proposals/DECISIONS.md)); this directive decides its D2(a), D4(a) and
D3(a) recommendations, and overrides REG-SEQ's parking (the proposal itself had held, with
`holub`, that it must not displace
[#556 "Local acceptance harness"](https://github.com/TheCaptainCompany/captain-food/issues/556) —
that position is recorded, not re-argued; the founder sequenced it). It builds on
ADR-20260821-010543 (the same morning's trail requirement), whose ask gate could until now check a
trail's shape but had no row statuses to check it against.

## Decision

1. **One file per key** (`REG-2` = D2(a)): `docs/decisions/<KEY>.yaml`, key == filename stem,
   v1 grammar `^[A-Z][A-Z0-9-]{2,63}$` with `--` **reserved** for the future namespaced-key
   encoding of the `D1`–`D7` family (D5 stays with slice 5; `/` is illegal in both the grammar and
   a filename, so the encoding is deliberately unresolved — widening the grammar is a versioned
   change to a Published Language, not a patch). File-per-row is the mailbox discipline applied to
   a document: the true invariant is intra-file, a git commit is the transaction, and two sessions
   closing two different rows never contend (ADR-20260812-011057's lesson).
2. **The closed vocabulary** (`REG-4` = D4(a), vocabulary half): `open · decided · deferred ·
   superseded · withdrawn`, with the status↔field couplings **biconditional**: `decided`/
   `superseded` require a `decided` date and a `decided_by` that RESOLVES to a file under
   `docs/adr/` or `docs/proposals/`; `superseded` additionally requires `superseded_by` naming
   another DECLARED key (scalar — fan-out is unrepresentable; self-reference and cycles are
   errors; a successor is migrated before it is pointed at); `deferred` requires `until` (the wake
   condition — without one, deferred is `open` wearing a euphemism); `withdrawn` requires `note`;
   `open`/`deferred` may carry none of the closing fields. Unknown fields are errors. Required on
   every row: `question` (one line, phrased as the answerable question), `owner`
   (`founder|team|counsel|external` — who owes the next move on the ANSWER), `opened`
   (YYYY-MM-DD, per-row so the index's aggregates stay decomposable), `register` (the pointer to
   the authoritative prose) and `evidence` (a verbatim quote, so the extraction stays reviewable
   against its source). Optional `capacity` records in what capacity a closure was taken —
   **`decided` is a recorded decision, never legal clearance**, and the schema says so where the
   agents read it. The second half of the original REG-4 row (the 22 ambiguous keys) is **not**
   decided here: split at close time into the new open row `KEY-NAMESPACE` — the partial-closure
   pattern this register already used informally (REFUND-BEARER's residue → CAPTAINNET-ZERO), now
   named in `docs/decisions/README.md`.
3. **The generated index** (`REG-3` = D3(a); the directive's *"generated decision index"* read as
   the PROP's recommended index-only form): a `GENERATED:decisions` marker region at the top of
   `DECISIONS.md`, emitted deterministically (BTreeMap order, open-oldest-first; **the stored
   `opened` date, never a computed age** — an age makes the drift gate red every day), with cell
   pipes escaped and the emitted body itself checked against §13b's GFM table rules BEFORE it is
   spliced. Missing markers are a **generation error**, not a database.md-style skip: a silently
   stale register index is a wrong founder surface. Merge conflicts inside the region are resolved
   by regenerating, never hand-merging — the fold is disposable.
4. **The ask gate reads the rows** (the directive's validator, the REG-1(a) mechanical form): the
   register-check hook now reads `docs/decisions/*.yaml` **at the point of need — never the
   generated index** (a stale projection must not gate a live decision) and refuses a question
   referencing a non-open row with a status-specific citation: `decided`/`superseded` cite
   `decided_by` (and the successor) plus the reversal rule — *a decided row is not a question;
   open a NEW row citing this one*; `deferred` cites `until`; `withdrawn` cites its note. An open
   `counsel`-owned row takes only questions about the **external action** (no lens output or
   founder answer is legal advice or clearance, ADR-20260812-143619) — the documented escape is
   naming the external action in the question. Key matching is exact with the full key alphabet as
   the boundary (never `\b`, which treats `-` as a boundary and would match `REG-2` inside
   `REG-2-A`). The hook stays exit-2-only and fail-closed, including on a broken
   `REGISTER_CHECK_DECISIONS` fixture override, and its log now carries a **closed reason
   taxonomy** (`trail-missing`, `trail-hollow`, `key-decided`, `key-superseded`, `key-deferred`,
   `key-withdrawn`, `key-counsel-owned`, `key-legacy`) plus the keys hit and a fixture tag — so
   "agents skip the trail" and "agents cite stale decisions" stay decomposable defects.
5. **Explicit legacy handling, no backfill**: 19 rows are migrated (the 2026-08-19
   reconciliation's live set, the §48 family, the sitting's just-closed money rows — so the gate
   has real decided rows to catch — and the new `KEY-NAMESPACE`), each carrying its `register`
   anchor and verbatim `evidence`. The remaining **103** prose-only keys are enumerated in
   `docs/decisions/_legacy.yaml` — **legacy is a declaration, not a default**: a key in neither
   set is not a register reference at all, a key may never be in both sets (validator error), and
   a legacy key leaves the allowlist in the SAME change that creates its file. Migration is
   next-touch, decided at dispatch time (citing a legacy row is not a touch). For a migrated key
   the FILE is authoritative for CURRENT status; the prose row is its history. Temporal questions
   are answered by declared fields, never by parsing git history.

**Honest limits.** A legacy-lane question about a non-open *prose* row still passes (the price of
no-backfill — each migration shrinks it); a typo'd key is indistinguishable from an acronym and
passes (closed by PROP slice 3's `decision-ask-unregistered` after full migration); semantic
duplication — declaring a fresh key for an already-answered question — remains reviewer work, not
machine work (the PROP's own §9 bound). The index's completeness caveat and legacy count are
printed on the page itself.

## Alternatives considered

- **One `decisions.yaml`** (REG-2(b)) — every concurrent session conflicts on it; rejected in the
  PROP on the recorded ADR-20260812-011057 failure and not reopened.
- **Status glyphs + a table parser** (REG-2(c)/REG-4(b)) — brittle cell-position parsing, keys
  stay bare strings with no walker; rejected in the PROP.
- **Blocking legacy references** ("migrate before asking") — argued by two lenses at the
  briefing; rejected because it converts the founder's no-backfill bound into an ad-hoc migration
  tax collected mid-dispatch (executor/holub), and the closed allowlist already makes "legacy" a
  checkable claim. The disagreement is recorded, not averaged.
- **Rewriting each migrated prose row to a pointer in the same commit** (`young`'s stricter
  two-authorities rule) — deliberately not done for 19 giant table cells; the authority split is
  instead declared once on the generated index ("the file is authoritative for CURRENT status;
  the prose below is history") and in the README. Recorded as a divergence, revisitable if a
  migrated row's prose glyph misleads in practice.

## Consequences

### Positive
- "Is this still open?" is now machine-answerable for every migrated row, the founder's index
  orders itself with the stalled row first, and re-asking a decided row fails mechanically with
  the citation that answers it.
- Closing a row is one file edit + regenerate in one commit; two sessions closing two rows never
  merge-conflict.

### Negative
- Any `docs/decisions/**` edit is now a **generating** edit: `make generate` in the same commit,
  including on the docs-straight-to-main path — `check-drift` is red otherwise (stated in the
  README and the index region; the cost of a register page that cannot lie).
- 103 keys stay prose-only until touched; the index says so on every render.

### Follow-up actions
- `KEY-NAMESPACE` (new, open): the D1–D7 namespacing + filename encoding (slice 5, with the
  remaining backfill and `decision-ask-unregistered`).
- Dispatch cards that close a row must name the key; the closing commit edits the file and
  regenerates (README "Editing discipline").
- The firing log has no rotation story yet — bounded deferral, noted here.

## Consulted

Whole roster briefed in parallel on the concrete slice before any code (ADR-20260812-143619);
what each lens changed:

- **architect**: authority during partial migration must be stated (landed: README + the index's
  authority line); distinct refusal texts for `deferred`/`withdrawn` (landed); the touch-it-
  migrate-it owner named (landed: dispatch-time, README) — and the architect's own reconciliation
  duty now includes authoring row files.
- **beck**: planted-defect fixtures per rejection rule, seen red (landed in `tests.rs` §22); the
  legacy lane makes ALLOW cases indistinguishable from "nothing loaded" — the BLOCK case is the
  load-proof and a broken override fails loudly (landed: selftest R1/R8); mechanism and data land
  as separate commits (landed).
- **business-specialist**: partial closure must be representable — split-at-close into a residue
  key, never a false "already decided" citation (landed: README pattern; REFUND-BEARER and REG-4
  migrated that way); the file mandatorily points at the authoritative prose (landed: `register`
  required); owner semantics defined as who-owes-the-answer (landed).
- **dba**: supersession validated as a DAG — no self-reference, no cycles, scalar successor,
  fan-in legal (landed); couplings biconditional (landed); filename↔key match checked (landed);
  the legacy lane needs a visible count so it cannot become the steady state (landed: the index
  counts line, chosen over a warning-ratchet entry to avoid a baseline refresh per migration).
- **evans**: the key grammar is a Published Language — `--` reserved now, grammar recorded as v1
  (landed); legacy as a declaration, not absence (landed: `_legacy.yaml`); `deferred` needs a
  wake condition (landed: `until`); the closing ADR names its keys in a header field (landed:
  this file's `## Closes`).
- **executor**: the closing step works only if the dispatch card names the key (landed: README);
  "touch" defined at dispatch time, citing a legacy row is not a touch (landed); no-file must
  mean legacy, never non-open (landed: allowlist pass lane); decision edits on the straight-to-
  main path need `make generate` stated explicitly (landed: README).
- **farley**: decision edits are generating edits — say it where the docs-only carve-out lives
  (landed: README + index note); regenerate-never-hand-merge for the region (landed); a leaked
  fixture override must be visible and a broken one fail closed (landed: fixture tag + R8);
  legacy as a closed committed allowlist (landed — his blocking concern).
- **generator**: escape pipes and validate the emitted body against §13b BEFORE splicing (landed
  in main.rs and the emitter test); BTreeMap determinism, no clocks anywhere in the body
  (landed); missing markers are an error, not a skip (landed); no marker substring in the body
  (landed); reuse the proposals.rs root/purity seam (landed).
- **graphql-architect**: `\b` is the wrong boundary for hyphenated keys — full-alphabet
  adjacency, case-exact (landed in the hook); the strict unknown-field rule is safe only because
  parser and files co-version in one repo (recorded here); required-iff couplings encoded as
  validation (landed; a Rust tagged enum does not survive the YAML boundary, the biconditional
  rules are the honest equivalent).
- **holub**: the slice is the shortest form of the founder's order — hold the exclusion line (no
  D1–D7 namespacing, no 148-row backfill, no citation ratchet, no `docs/adr/README.md`
  generation — all explicitly NOT built); an unmigrated row must never block unrelated work
  (landed: legacy passes); one pass, then back to #556 (this ADR spawns no follow-on work beyond
  the recorded KEY-NAMESPACE row).
- **legal-specialist**: a founder question on an open counsel-owned row is mis-routing — gate it,
  with the external-action escape (landed: `key-counsel-owned`); capacity recorded on closure and
  the decided≠clearance disclaimer carried by the schema itself (landed: `capacity` +
  README/rule text).
- **observability-agent**: per-row `opened` required by the VALIDATOR, not just used by the
  emitter, so aggregates stay decomposable (landed); the log's closed reason taxonomy with keys
  on ALLOW lines too (landed); log rotation deferred, said so (Follow-ups).
- **reviewer**: each migrated row carries a verbatim quote + stable anchor in the artifact itself
  (landed: `evidence` + `register` required fields); review verifies quote-appears / status-
  entailed / references-resolve (the commit separation makes the data diff reviewable alone);
  every fixture proven to actually turn the validator red (landed).
- **ux-designer**: the counts line keeps "oldest open row: KEY since DATE" (landed); owner
  distinguishes founder-actionable from waiting-on-external (landed: `counsel|external`);
  questions phrased as answerable questions, option shape preserved where enumerated (landed in
  the migrated rows); the completeness caveat printed on the page (landed).
- **vernon**: the row file is the right aggregate — invariants intra-file, commit as transaction
  (landed); supersession is the one two-file write, atomic in one commit with gate-time
  resolvability (landed: validator per commit); the index fold declared disposable (landed); the
  hook reads the FILES at the point of need, never the projection (landed).
- **young**: the index is a fold over HEAD state — temporal questions answered by declared
  fields, never `git log` parsing (landed: README); the two-authorities window needs the
  authority split stated (landed as the declared split; his stricter prose-reduction rule is the
  recorded divergence above); the rebuild test — regenerate must be byte-identical (landed: the
  determinism assertion + drift gate); the validator reads row files, not the index (landed).
