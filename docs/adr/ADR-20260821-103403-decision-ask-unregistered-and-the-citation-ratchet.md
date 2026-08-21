# ADR-20260821-103403 — Decision-ask-unregistered and the citation ratchet: every decision question names its open row, every citation resolves

## Status

Accepted

> **Amended 2026-08-21 (the founder-ordered verification slice, same day).** The "accepted
> residual" claim below — *"a docs-only push that skips local gates can land a dangling citation
> that only CI reports"* — **overstated the backstop: no asynchronous CI red existed.** Verified:
> `ci.yml`'s docs-only detector skipped `lint`/`specs`/`build-test`/`db-test` entirely and the
> `codegen` aggregator accepts `skipped`, so a docs-only push (proposals, ADRs, CLAUDE.md — the
> exact surface §22/§23 govern) reached `main` with a **green** required check and **zero**
> validation. Closed by the `docs-validate` job (runs on exactly the docs-only complement,
> executing the `specs` job's canonical commands verbatim — never a parallel implementation) plus
> a **named** aggregator assertion (`docs_only=='true' ⇒ docs-validate=='success'`), both pinned
> by a red-first shape test in `tools/codegen-rs`. Also found and fixed by the same slice:
> `decision-index-stale` was fatal in generate mode, deadlocking `make generate` against the very
> staleness generation repairs — the rule now fires in `--check` only (the gate must never lock
> its own key in the room). The generated index tail now states the legacy boundary explicitly
> (`Legacy rows remaining: N` · `Migrated rows: N` · the four migration triggers); "migrated in
> the current change" is deliberately NOT emitted — it is not derivable from committed state, and
> the diff of the tail lines is the per-change migration record. `_legacy.yaml` semantics
> unchanged; KEY-NAMESPACE untouched. Narrow briefing roster (farley, beck, generator) under
> ADR-20260816-134352, class REVERSIBLE INTERNAL.

## Closes

Nothing new — this executes the enforcement half that `REG-1` (decided, ADR-20260821-010543)
still owed and PROP-20260819-110442 slices 1 and 3 designed. `KEY-NAMESPACE` remains open,
untouched (the founder's own bound: no D1–D7 normalization in this slice).

## Enforced by

n/a — no behavioral guarantee in `rules.yaml` (operating-model surface). The executable
enforcement: the register-check hook's ENVELOPE lane (`.claude/hooks/register-check.sh` + its
selftest — one red-or-green case per lane verdict plus the live-corpus and wiring anchors, run by
the stop gate every turn; block cases assert the logged REASON, not only the exit code, since the
2026-08-21 independent review of
[#669 "Decision-register enforcement"](https://github.com/TheCaptainCompany/captain-food/pull/669)),
validator **§22** additions (`reconsiders`
shape + closure coupling, `decision-index-stale`, `decision-form-template-row`) and **§23**
(`record-citation-unresolved`, `citation-exemption-shape`/`-unused`, `record-stamp-collision`) on
`make validate`'s single gate, all with planted-defect tests proven red first.

## Context

Founder directive, 2026-08-21 (third of the day, 15 numbered requirements — quoted in full in the
session record; the load-bearing lines): *"A founder-directed decision question must reference
exactly one declared decision key … that key must resolve to a row whose status is `open` … a
legacy key … must be rejected for new founder-directed decision questions, unless it is being
migrated in the same change. Do not preserve legacy as a permanent bypass … A re-opening of a
previously decided matter is never a re-ask of the old row … Add a validator rule that resolves
every full ADR and PROP citation … Do not attempt a repository-wide historical cleanup … Record
the exact enforcement boundary honestly."*

Foundation: REG-2/REG-4 (ADR-20260821-095957) gave rows machine identity; the ask gate could
check row status but the legacy lane passed silently, unknown keys were indistinguishable from
acronyms, and nothing required a question to name its row at all. Separately, a REAL resolver bug
was found during this slice's measurement: **104 of the middle-era ADR files carry no `ADR-`
prefix** (`20260720-233000-….md`), and §22's `record_resolves` matched on `contains(id)`, so a
full-form citation of any of them was falsely unresolvable — pre-fix, a naive ratchet reported
90+ dangling ids; post-fix the true count is **4**.

## Decision

1. **The envelope** (requirements 1–5): a founder-directed decision question carries exactly one
   `Decision row: <KEY>` line. Declared + `open` → passes (an open `counsel`-owned row still takes
   only the external-action question); `decided`/`superseded`/`deferred`/`withdrawn` → refused
   with the controlling record, the status-specific explanation, and the correct next action;
   **unknown** → refused, listing today's open rows (bounded) and the create-row path; **legacy**
   → refused with migrate-first — *a founder-facing question IS a migration trigger*, and because
   the hook re-reads the files live, migrating in the same change unblocks the same question
   immediately. A valid envelope IS the register check (the declared row carries the provenance);
   no separate trail line is required. Two envelope lines, or a garbled key token, refuse loudly,
   echoing the rejected line.
2. **The negative trail is reclassified** (dated meaning-shift, declared at the canonical site in
   workflow.md): since 2026-08-21 `Register check: no controlling record …` asserts *"this is not
   a decision question"* — legitimate for clarifications of an in-flight directive, external-clock
   relays (never delayed by row ceremony) and mechanical choices. The published tiebreaker: **would
   the answer bind future work? Then it is a decision question and needs a row.** A genuinely new
   decision question declares its `open` row FIRST (key, `opened`, `owner`, `question`, `register`
   anchor, `evidence`) and then references it. Pre-2026-08-21 negative trails in the record were
   written under the old grammar and make no classification claim.
3. **Reopening is `reconsiders:`** (requirement 5): a challenge to a closed decision is a NEW open
   row carrying `reconsiders: <OLD-KEY>` with the changed premise in its evidence. Validator
   shape: the target is declared (a legacy target is migrated first), never self, never an
   open/deferred row (those are simply asked), and never a superseded mid-chain row — the message
   names the chain head. **Closure coupling** (one controlling record per key): when the challenge
   is `decided`, the old row must be `superseded` with `superseded_by:` naming the challenge, in
   the same commit — otherwise two rows each believe they are controlling. The field is retained
   after closure (history is additive). On legal-exposed rows the changed-premise evidence is
   VERIFY-FIRST material, and a reconsideration never downgrades a counsel-confirmed posture
   without counsel re-entry (the counsel-owned-row rule covers the re-entry ask).
4. **Structured surfaces** (requirement 6): dispatch cards (`docs/dispatch/*.md`) — any
   `Decision row:` line must name a declared, non-legacy key (§22 side; deliberately
   **resolution-only**: status is enforced at ask time by the hook, because a committed card is a
   fact at its timestamp and retro-reddening history when its row later closes would be the
   projection-rebuild sin in gate form — pinned by a green test). The decision-form template's
   FORM schema now requires `row:` per option-question, rendered as a key-first eyebrow on the
   card and carried into the pasted answer block (`decision-form-template-row` keeps the template
   on-contract). The machine-readable decision queue IS `docs/decisions/*.yaml`, governed by §22.
5. **The citation ratchet** (requirements 7–8): every full-form `ADR-YYYYMMDD-HHMMSS`,
   `PROP-YYYYMMDD-HHMMSS` and legacy `ADR-00NN` citation across **all of docs/** + CLAUDE.md**
   must resolve to a record file (kind-aware: an ADR stamp resolves against `ADR-`-prefixed and
   prefixless `docs/adr/` filenames; a PROP id only against `docs/proposals/`; a mistyped kind
   never resolves cross-kind). Fenced code blocks are quoted output and are not scanned; inline
   code spans are. Measured at adoption: **5,130 citations, 4 distinct dangling ids** — no
   date-scoping needed and **zero historical rewriting performed**: the four ride
   `docs/decisions/_exempt.yaml`, each with `id`, `reason` and `retires_when` (the held trio
   ADR-20260817-232744/5/6 until deposited or retired; ADR-20260724-172808, cited only where
   records DESCRIBE that dangling citation as a defect). An exemption that exempts nothing is an
   error naming its id — the file is a self-pruning queue, never a permanent bypass. A companion
   rule (`record-stamp-collision`) keeps stamps unique per kind, since stamp-based resolution is
   sound only while the id scheme's concurrency guarantee holds on disk.
6. **Requirement 9, in the rule text itself**: resolution proves EXISTENCE, never authority — an
   ADR controls by its own status; a PROP is an option space; a legal brief is preparation, never
   clearance; a held record is citable as existing, not controlling; and citations resolve to
   source records only (the resolver knows nothing but `docs/adr/` and `docs/proposals/`
   filenames, so a generated projection can never satisfy it). The stale-projection half is
   `decision-index-stale`: at validate time the committed `GENERATED:decisions` region must equal
   the fold over the source rows — same emit function and same `legacy_count` source as
   generation, so this gate and `check-drift` can never disagree; the validator reads the ROW
   FILES as truth and never parses the index (requirement 11).

**The exact enforcement boundary (requirement 13), honestly**: mechanical enforcement covers the
`AskUserQuestion` transport (envelope + trail + passive references), `docs/decisions/*.yaml`
(§22), the committed index region, `Decision row:` lines in committed dispatch cards
(resolution-only), the form TEMPLATE's schema, and citations across docs/** + CLAUDE.md. NOT
mechanically enforced: a prose question that omits the envelope (self-classification — the
tiebreaker is published, review catches the dodge); published decision-form COPIES (uncommitted
artifacts); free-text PR/issue comments and reports (bound by the agent citation blocks and
review, and by §23 the moment their text lands in a governed doc); and semantic duplication — a
fresh key declared for an already-answered question (the PROP's own §9 bound: shape, never
semantics). A docs-only push that skips local gates can land a dangling citation that only CI
reports — the local rule (run `make validate` before pushing a citation-governed docs change) is
stated at the canonical site; the asynchronous-CI backstop is the accepted residual, recorded
here rather than papered over.

**Legacy burn-down (requirement 14)**: a legacy row MUST be migrated — in the same change — when
it is (a) named in a founder-facing decision question (the hook forces this), (b) amended, (c)
reopened/challenged (`reconsiders` targets must be declared), or (d) explicitly included in a
dispatch. Merely citing a legacy key as context stays legal. Mid-run origination: an executor
never files the row — it reports the proposed key in its hand-back and the coordinator/relayer
declares the row (the executor writes only its dispatched diff). This slice touched **no** legacy
row; the count stands at **103**, measurable three ways (the `_legacy.yaml` list, the index
counts line, and `key-legacy-ask` log events, each of which is a forced migration).

## Alternatives considered

- **Blocking passive mentions of legacy/counsel rows** — rejected: citing a row as context is not
  asking it; ask-vs-cite is distinguished by the envelope (the R5 flip is recorded in the
  selftest with its rationale).
- **Validating row STATUS on committed dispatch cards** — rejected as retroactive history-reddening
  (architect, unanimously endorsed); resolution-only there, status at ask time.
- **Date-scoping the ratchet to newly-authored records** — unnecessary once the resolver bug fell:
  the whole corpus is clean with 4 exemptions, which is strictly stronger than a date fence and
  performs no cleanup.
- **Hook shelling into codegen-rs for one shared row parser** — rejected for now: the hook must
  stay dependency-free and fast at ask time; the dual-parser drift risk (bash `field()` vs Rust)
  is accepted and tripwired by the selftest's live anchors. Recorded as the known seam.

## Consequences

### Positive
- A founder decision question now cannot target a decided, superseded, withdrawn, deferred,
  unknown, or unmigrated-legacy row, on the tool path, with the correct next action in every
  refusal; and every citation in the documentation is live or declared-held.
### Negative
- One more line of ceremony on decision questions, and row declaration before genuinely new
  questions — priced against the founder round-trip it prevents; the external-clock lane is
  explicitly exempt from row ceremony.
- The prose-dodge hole (omitting the envelope) remains, by construction; it is named, published
  and reviewed rather than falsely claimed closed.

### Follow-up actions
- The firing log still has no rotation story (bounded deferral, second recording); its `|| true`
  write means a full disk silently stops the burn-down counters — accepted until rotation lands.
- Remaining historical debt outside this ratchet's error set: the 27 time-only shorthand sites
  (`ADR-150500`-style, PROP UQ-4) remain un-governed — they do not match the full-form grammar;
  UQ-4's recommendation (error + fix the 27) stays open with slice 5.

## Consulted

Whole roster briefed in parallel on the concrete slice before any code (ADR-20260812-143619);
what each lens changed:

- **architect**: the reconsiders closure coupling (a decided challenge without the two-file
  supersession is an error) and chain-head targeting (landed); dual-parser drift named — his
  shell-out proposal weighed and recorded as the accepted seam above; the deferred refusal now
  names its reopen path (landed); card status-non-check endorsed and pinned.
- **beck**: the three inverted selftest cases proven against the OLD hook with recorded exits
  (E1 2→0, E3 0→2, E4 0→2, R5 2→0 — run in-session, complete fixtures); the two-line envelope
  case planted (E5); the resolver fix red-first (RecordCorpus test failed to compile/resolve
  pre-fix); the live unknown-key anchor (L3) added.
- **business-specialist**: the external-clock carve-out explicitly bypasses row ceremony
  (landed in the canonical site and this ADR); migrate-first is same-change or it inverts into a
  deferral tax (landed — the hook re-reads live); exemption refill discipline via
  `retires_when` (landed).
- **dba**: reconsiders forbids mid-chain targets, resolved via the existing DAG walk (landed);
  the stamp-uniqueness gate (landed as `record-stamp-collision`); legacy `ADR-00NN` resolution
  confirmed against the 47 sequential files (tested); exemption entries carry their retirement
  event, duplicates error (landed).
- **evans**: the envelope declared once with its exact meaning ("this question targets/creates
  register row <KEY>"); the reclassified negative carries a DATED meaning-shift banner at the
  declaration site so historic trails are not re-read (landed); the bind-future-work tiebreaker
  published (landed).
- **executor**: the mid-run origination gap settled — the executor reports the proposed key, the
  coordinator declares the row (landed in this ADR + README); the ratchet binds executor-authored
  records at zero cost (validate already in the gate loop); a card instructing a legacy migration
  as a side errand is a card defect.
- **farley**: validate-stale and check-drift share the same emit + inputs so they cannot disagree
  (landed); the docs-only carve-out gap named and settled honestly — local-validate rule at the
  canonical site, asynchronous-CI backstop recorded as the accepted residual (landed).
- **generator**: kind-aware resolver — a PROP id never resolves against an ADR stamp (landed +
  tested); block fences skipped, inline spans scanned (landed + tested); region comparison
  normalized identically to the injector framing, same legacy_count source (landed); sorted
  walks; the template `row:` scanner kept separate from the fence-skipping extractor (landed).
- **graphql-architect**: two-line duplicates block explicitly, never first-wins; the token
  terminator is hard (a trailing key-alphabet character garbles loudly, echoing the token);
  reconsiders retained after closure rather than stripped (landed — his additive-evolution
  argument overturned the draft's open-only field); unused-exemption errors name the id.
- **holub**: the exclusion line held — no fuzzy key suggestions, no form-instance validation, no
  journal/STATUS citation backfill, no REG-5; the worked genuinely-new-question example landed in
  the README so declare-first stays one cheap act; this commit spawns nothing beyond the already-
  recorded open rows, and the next dispatch is #556.
- **legal-specialist**: the authority-grade sentence in the ratchet's own rule text (resolution ≠
  authority; PROP = option space; brief ≠ clearance; held = existing, not controlling) (landed);
  citations resolve to source records never projections (landed by construction, stated);
  reconsiders on legal-exposed rows carries VERIFY-FIRST premise discipline (landed).
- **observability-agent**: garbled/multiple refusals log the offending lines verbatim in their
  own column (landed — the 100-char payload truncation no longer amputates them); the session id
  joined to every log line (landed); the `|| true` log-write dead-man gap recorded rather than
  hidden.
- **reviewer**: every planted red re-runnable and matched to its exact rule id (the deliverable
  table); the resolver fix's red attributably demonstrated (compile-red plus the pre-fix
  90+-dangling measurement vs 4 post-fix); the card non-check pinned green; the warning-baseline
  diff is empty (no new warnings).
- **ux-designer**: the row key renders as a key-first eyebrow above the form question and rides
  the pasted answers (landed); the unknown-key refusal lists open rows BOUNDED (12 + count) with
  the create-row instruction inline (landed); the decided-key refusal returns the recorded
  decision as a citation (landed).
- **vernon**: the pairing enforced as one unit at the gate — a decided challenge whose target
  lacks the matching `superseded_by` is rejected (landed); chain-head targeting (landed); the
  open row as durable process state means an unanswered Ask hangs nothing.
- **young**: the fold's declared total order and canonical rendering make index-sync a replay
  guarantee, not a flaky byte-compare (landed — deterministic emit, same inputs both gates); the
  closed post-closure mutation set stated (`superseded_by` on coupling is the ONE legal edit to a
  decided row — README; mechanical git-history enforcement out of scope, said so); his
  cite-a-superseded-record-as-controlling ratchet idea recorded as not implementable for file
  citations (files carry no machine status) — the row-key half already blocks.
