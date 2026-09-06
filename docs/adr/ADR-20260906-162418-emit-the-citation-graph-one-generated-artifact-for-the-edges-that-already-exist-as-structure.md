# ADR-20260906-162418 — Emit the citation graph: one generated artifact for the edges that already exist as structure

<!-- Filename: docs/adr/ADR-20260906-162418-emit-the-citation-graph-one-generated-artifact-for-the-edges-that-already-exist-as-structure.md -->

## Status

Accepted — a **founder decision**, 2026-09-06, recorded under `/decision`. His words, verbatim, answering the
coordinator's `/direct-question` reply on "graph engineering":

> **"emit citation graph built, spec and code gen chunk the team can take"**

Reversal check on the terms *citation graph*, *knowledge graph*, *graph artifact*, *retrieval*, *decision-lookup*,
*Lane D*, *generated artifacts* across `docs/decisions/`, `docs/proposals/DECISIONS.md`, `docs/adr/`,
`docs/claude/`: **nothing is reversed**; four records are extended and fence the chunk (architect, below):
[RETRIEVAL-QMD-CI](../decisions/RETRIEVAL-QMD-CI.yaml) (decided 2026-08-24) — the graph never enters the QMD index
and the chunk adds no CI step; [CITATION-RULE-LEVEL](../decisions/CITATION-RULE-LEVEL.yaml) (decided) — the emitter
reuses the existing citation walker, never a second prose parser; [DISPATCH-CARD-CITATION](../decisions/DISPATCH-CARD-CITATION.yaml)
(decided) — the graph settles what is resolvable; [ADR-20260811-014129](ADR-20260811-014129-a-business-metric-is-a-projection-and-every-reference-is-a-ref.md)
— prose citations stay prose and the graph is not a business metric. Precedent:
[ADR-20260807-183024](ADR-20260807-183024-specs-per-scope-layout-and-the-cross-scope-ref-dag.md)'s
`crate-graph.generated.json`. Not a graph database and not GraphRAG — the founder's question was answered on
that ground and the decision adopts the emitted artifact.

## Enforced by

`make check-drift` (the artifact is generated; a stale copy fails the gate) plus the corpus tests named on the
tracking issue; the emitter lives in `tools/codegen-rs/src/emit/`. Until it lands, this record and the issue bind.

## Decision

1. **One generated artifact**, `docs/generated/citation-graph.generated.json` — the noun and suffix by the repo's
   convention (architect, evans), the DIRECTORY by the release path (farley): the docs-only CI detector's allowlist
   is `docs/*`, so an artifact under `specs/generated/` would turn every ADR, journal and proposal commit into a
   full Rust CI run (build-test + db-test, minutes instead of one), while `docs-validate`'s whole-tree regeneration
   drift already covers `docs/generated/` at zero coverage cost. The codegen already writes into `docs/` (the
   DECISIONS.md generated region), so this is not a new lane. **A docs edit that moves an edge runs `make generate`
   and commits the artifact in the same commit** — the rule that already governs `docs/decisions/*.yaml`.
2. **Edges are DERIVED, never hand-declared** (evans, architect): the emitter reuses the loader's walkers
   (`validate::citations::extract_citations`, `load_governed_doc_files`, `load_adr_status_corpus`, the `$ref`
   walker); the moment a human maintains an edge the graph is a second source of truth. Two graphs in one file,
   each edge carrying a `kind`: **citations** (record→record `cites`; test→claim `pins`; config key→decision row
   `binds`; ADR→ADR `amends` only where DECLARED, never inferred from prose — `supersedes` stays its own verb) and
   **structure** (`refs`: every `$ref` the walker resolves, screen→operation included; `reads` is an operation's
   declared read target and is emitted, never re-meant). A `$ref` is checkable; a citation is only resolvable —
   the edge record says which oracle applies (evans).
3. **Slice 1** (architect): record→record with the citing `file:line` as an ADVISORY line beside an ANCHOR
   (heading, symbol, rule id — beck: a line edge is falsified by any edit above it), record→test symbol including
   `Pinned by:`, test→claim through `tests.yaml` `rules:` `$ref`s, config key→`decisionRow`, screen→operation,
   each ADR's declared `Status:` as a node attribute. Deferred with reasons: semantic amends, code symbols, C4,
   the wider `$ref` DAG, any query language or index.
4. **Consumers stay advisory** (farley, RETRIEVAL-QMD-CI): an edge is a pointer; the record is re-read at the
   moment it licenses an action. The first consumers are the decision-lookup skill's resolution step and Lane D's
   `Pinned by:` resolution — the Lane D edit is owned by [#923](https://github.com/TheCaptainCompany/captain-food/issues/923)
   item 2, not this chunk, and once the fail-closed hook reads the file Lane D is a gate: its verdict distinguishes
   *artifact missing or stale* from *no edge found* (dead-man; farley, beck). Any new CI step or hook wiring for
   the graph needs its own register row first (RETRIEVAL-QMD-CI's not-authorized list).
5. **Dangling edges** (beck): `citation-graph-dangling-target` is a WARNING on the ratchet, one issue per edge;
   flipping it to an error is a separate recorded decision (precedent: `decision-superseded-authority`);
   `citation-graph-stale-line` stays a warning by construction. The emitter is deterministic and idempotent, its
   artifact is tracked and in the emitter registry (`check-drift` cannot see an untracked file — the pin is a
   test), and its cost stays in the seconds regime (`--check` was 1.38 s warm at `8f3392ee`; reuse the one walk
   the §23 ratchet already performs — farley).
6. **Class, tier, container, order** (architect): reversible — beck, farley, evans at the briefing; executor on
   the lower tier; backlog `High` under [docs/BACKLOG.md](../BACKLOG.md) foundations-first, ranked below #923 item
   2 (the consumer); **Lane B after #914** — its write set collides with #914 and #923 on `tools/codegen-rs/src/tests.rs`
   and the hook, so a third container fails ADR-20260906-152024's independence test. Tracking issue: [#925 "Emit the citation graph"](https://github.com/TheCaptainCompany/captain-food/issues/925).

## Consequences

- Retrieval for every lens and for the register check becomes one lookup instead of a re-read, on the retrieval
  row already decided; nothing new is operated.
- `CLAUDE.md`'s carve-out *"a docs-only edit that regenerates nothing may skip `make rust`"* narrows once docs are
  an edge source (farley) — a `CLAUDE.md` edit needs in-conversation approval and is an item on the issue, not
  this change.
- The STOP list on the card carries: indexing the artifact into QMD; adding a CI step; a hand-declared edge; an
  inferred `amends`; a hard-coded node count.

## Consulted (ADR-20260812-143619 — one line per lens)

Consulted for the completeness of the record, never to relitigate; **no lens output is legal advice or
clearance**. Roster: the reversible-refactor class of ADR-20260816-134352 (a generated artifact and tooling) —
four lenses asked; young, vernon, dba, graphql-architect, observability-agent, ux-designer, holub,
legal-specialist and business-specialist were not asked, the class carrying no stored shape, money, API, legal or
customer surface.

- evans — Consent; one naming defect and one conflation: `citation` already means a record naming a record (workflow.md Register check; register-check.sh; the two decided rows) — keep it for record→record edges; the artifact is `citations.generated.json`; if DSL-structure edges ride in the same file it is two graphs with a `kind` discriminator, never one flat list. One verb each, all already in the tree: `cites` (record→record), `pins` (test→claim, active voice), `amends` (ADR→ADR; never absorbing `supersedes`), `binds` (config key→decision row, verbatim in configuration.yaml), `refs` (any $ref). `reads` is TAKEN (an operation's declared read target, ADR-20260812-214500). Screen→operation is a $ref, emitted under `refs`. The non-conflation the emitter enforces: a $ref is checkable (ref-dangling), a citation is only resolvable — Lane D must tell from the edge record which oracle applies. Edges are DERIVED, never hand-declared in a new DSL kind — the moment a human maintains an edge, the graph is a second source of truth.
- beck — Nothing blocks; red-first shapes: (1) tools/codegen-rs/src/tests.rs::every_citation_edge_in_the_corpus_resolves_or_is_reported — the emitter over the REAL record dirs (417 records, three ADR filename eras + docs/decisions/*.yaml) — mutant: drop an era — expected red: N records produce zero nodes naming the era; (2) ::the_citation_graph_is_a_tracked_generated_artifact — THE DRIFT HOLE: check-drift is generate + git diff --quiet, which does NOT see untracked files — pin: the path is tracked, not gitignored, in the emitter registry; (3) ::the_citation_graph_emitter_is_deterministic_and_idempotent — emit twice with permuted input order — expected red: byte diff; (4) register-check-selftest.sh LDG1: a Pinned by symbol present in the graph with the graph file MOVED/EMPTY — expected red: Lane D refuses naming the missing graph (fail-open is the defect); LDG2 unknown pin symbol → refusal naming it; LDG3 green = a symbol only in the graph, not grep-visible on the card (proves graph-backed resolution). Tautology to refuse: a test asserting an edge the same emitter run produced — every assertion closes through an independent oracle. Dangling edge = WARNING on the ratchet, kind `citation-graph-dangling-target`, one issue per edge; flip to ERROR is a separate decision (precedent decision-superseded-authority); split `citation-graph-stale-line` (file exists, line moved) — a <record>:<line> edge is falsified by any edit above it → emit line edges as an ANCHOR (heading/symbol/rule id) with the line advisory. Emitter + artifact + drift pin = one commit; the Lane D consumer = a second.
- farley — The artifact is cheap and the drift gate covers it; what I catch is the LANE: docs-only pushes do NOT bypass CI — `docs-validate` (ci.yml:590-650) runs the validator and a whole-tree regen-drift on push to main, post-hoc (no PR, no protection): a missed regen lands and reds main. Rule as it already exists: a docs edit that moves an edge runs `make generate` and commits the artifact in the same commit (as docs/decisions → DECISIONS.md today); narrow CLAUDE.md's "a docs-only edit that regenerates nothing may skip make rust" — with docs as an edge source that set is nearly empty. EMIT UNDER docs/generated/, NOT specs/generated/ — the docs-only detector's allowlist is docs/*|README.md|CLAUDE.md|LICENSE (ci.yml:143-148); under specs/ every ADR commit drags build-test (~4m17s) + db-test (~3m54s) where ~1 minute did. Cost: --check 1.38 s warm at 8f3392ee over 290 ADRs / 93 rows / 69 proposals; the §23 citation ratchet already walks that corpus (5,130 citations) — reuse that one walk; pin a seconds budget in the emitter's test. Lane D is a GATE once the fail-closed PreToolUse hook reads the file — its verdict must distinguish artifact-missing/stale from no-edge-found (dead-man). Consumption stays ADVISORY (RETRIEVAL-QMD-CI): an edge is a pointer, never a citation — the record is re-read when it licenses an action. Authorization gap: RETRIEVAL-QMD-CI authorizes exactly one CI step and explicitly not hooks or other CI wiring — a new CI step or hook wiring for the graph needs its own row before dispatch; the emitter + drift coverage needs none. Bring pain forward: add `make generate` to the stop gate on docs-touching sessions.
- architect — Extends everything, contradicts nothing, four fences: the graph must never enter the QMD index and needs no CI step (RETRIEVAL-QMD-CI's corpus policy and its not-authorized list — a CI step here would be a reversal); the emitter REUSES validate::citations::extract_citations and load_governed_doc_files — a second prose parser rebuilds the heuristic CITATION-RULE-LEVEL half (2) is still open about; prose citations stay prose (ADR-20260811-014129 clause 4) and the graph is not a business metric; precedent crate-graph.generated.json. Slice 1 edges: record→record with citing file:line, record→test symbol (incl. Pinned by), test→claim via tests.yaml rules $refs, config key→decisionRow, screen→operation, ADR declared Status as a node attribute; DEFERRED: semantic amends (not a declared field — inferring it is the heuristic trap), code symbols, C4, the wider $ref DAG, any query language. Emitter tools/codegen-rs/src/emit/citation_graph.rs; gate check-drift; class reversible (beck, farley, evans); tier sonnet; backlog High under BACKLOG.md:47, below #923 item 2's gate half (the consumer); container: Lane B after #914, not a third chunk (write-set collision on tests.rs and the hook); the hook edit stays #923 item 2's. Most likely to go wrong: an executor indexing the artifact into QMD or adding a CI step — both reverse RETRIEVAL-QMD-CI — on the STOP list.
- **Split, resolved**: architect placed the artifact under `specs/generated/`; farley under `docs/generated/` on
  the CI-lane cost. Both reversible; the release-path cost is concrete and measured, so `docs/generated/` — the
  name and suffix stay the convention.
