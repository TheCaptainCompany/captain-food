# ADR-20260901-025538 — YAML decision rows enter the QMD retrieval corpus

**Status**: Accepted · **Date**: 2026-09-01 ·
**Decider**: the **FOUNDER / Tech CEO**, verbatim below ·
**Closes**: [`docs/decisions/RETRIEVAL-QMD-ROWS.yaml`](../decisions/RETRIEVAL-QMD-ROWS.yaml)
(supersedes [`docs/decisions/RETRIEVAL-QMD-CI.yaml`](../decisions/RETRIEVAL-QMD-CI.yaml)) ·
**Issue**: [#840 "Index YAML decision rows in the decision-lookup (QMD) retrieval corpus"](https://github.com/TheCaptainCompany/captain-food/issues/840) ·
**Session**: https://claude.ai/code/session_01WSHcekapAAm87XGQsTST5U

## Status

Accepted.

## The directive, verbatim

Founder, 2026-09-01, via `/decision`:

> integrate yaml decision row indexing in retrieval qmd ci

## Context

The `decision-lookup` skill retrieves BM25 candidates over committed governing Markdown. The
decision register's **declaration site** — `docs/decisions/*.yaml`, the files that are
authoritative for a row's CURRENT status, and the exact artifact every `Register check:` trail is
required to resolve — was **not** in that corpus. The chain head `RETRIEVAL-QMD-CI` listed "YAML
decision-row indexing" in its not-authorized set, each item of which "needs a new decision row".
This is that row.

## What was established before anything was designed

The chain head attributed the blindness to "the default Markdown mask", without recording whether
that mask was the wrapper's or the **tool's**, and nothing in `SKILL.md` or `PROP-20260822-171212`
recorded whether qmd ingests `.yaml` at all. Both were established empirically on an activated
checkout (base `7d47355`, pinned `@tobilu/qmd@2.8.3`, scriptless install, lockfile integrity
verified) before any code was written.

**Measured, with antecedents** (ADR-20260817-105845 — every number below was produced this
session; nothing is carried from the dispatch card unverified):

| Question | Answer | How |
|---|---|---|
| Does qmd 2.8.3 ingest `.yaml`? | **Yes** | 3-file probe: `Indexed: 2 new` once the collection glob admitted them |
| Where is the mask? | **Three places, not two** | `git archive` pathspec; `find … ! -name '*.md' -delete`; **and qmd's own `pattern:`** |
| Is there a CLI route to the glob? | **No** | `qmd collection add . '**/*.{md,yaml}'` accepts the argument and *ignores* it — `collection show` still reported `**/*.md` |
| Do row keys retrieve? | **Yes, rank 1** | `CREDIT-DRAIN-ORDER` → its own row, sole result, 0.92; `QUOTE-TOKEN` → 1 of 7, 0.88; `KEY-NAMESPACE` → 1 of 7, 0.89 |
| Index rebuild duration | **0.46 s** over 437 documents | previously **UNMEASURED**; cold wipe-to-answer lookup 2.8 s |
| Corpus size | 355 Markdown + 81 rows + 1 canary | 83 `.yaml` under `docs/decisions/` minus 2 control files |
| Row sizes | min 326 B, **median 961 B**, mean 2168 B, max 19674 B | `find -printf '%s'` over the 81 rows |
| Keys with zero topical words | **36 of 81** | 27 `PROP-…--D<n>` plus `CONFLICTS-20260819`, `LOSS-1`, `PMW-1`, `PMW-4`, `REG-1`…`REG-4`, `REV-1` |

**The third mask is the finding that changed the work.** `qmd init` + `qmd collection add` write
`pattern: "**/*.md"` into the corpus-local `.qmd/index.yml`, and *that* glob decides what `qmd
update` ingests. Widening only the two wrapper-side masks would have exported the rows, kept them
on disk, built an index, written the stamp — and indexed **zero rows, silently, forever**. It is
the same silent-no-op hazard the dispatch identified in the `find` sweep, one layer deeper, and it
is why the canary below is not optional.

## Decision

Index `docs/decisions/*.yaml` in the retrieval corpus. The full fence — authorized surface,
not-authorized list, failure protocol, exclusions, accepted behaviour and known limits — lives in
[`RETRIEVAL-QMD-ROWS`](../decisions/RETRIEVAL-QMD-ROWS.yaml), which is now the chain head. The
clauses that are decisions rather than mechanics:

1. **`_legacy.yaml` and `_exempt.yaml` are excluded.** They are control files, not rows: no
   `status`/`owner`/`capacity` to disambiguate a hit, and `_legacy.yaml` is one document naming
   100 prose-only keys, so it ranks for any register query while answering none. The tiebreak
   against the opposing argument (it names all 100 legacy keys, and is therefore high-value): a
   hit on it points at a key with **no row file to resolve**, which is the one case the mandatory
   resolution contract cannot discharge.
2. **`docs/decisions/README.md` is excluded** — by construction, because the pathspec is
   `docs/decisions/*.yaml`. Named anyway: an unnamed rider is the pattern this chain has drifted
   on three times.
3. **Superseded and withdrawn rows are all indexed.** `superseded_by:` is what makes a hit on a
   retired row resolvable to its head; truncating to live rows would destroy the DAG that makes
   the register answerable. The reduction rule is stated on the surface.
4. **`docs/decisions/**` is NOT added to `claude_citation_corpus`**, and must not be. Rows cite
   superseded rows *by construction* (`reconsiders:`, `superseded_by:`), so wiring them in would
   make `decision-superseded-authority` — an **error** — fire across the register itself. "Rows
   are in the corpus now" is otherwise a live invitation to the wrong corpus.
5. **The path is the payload; the excerpt is decorative.** A row candidate renders as
   `resolve docs/decisions/<KEY>.yaml at HEAD`, never as a quotable excerpt.
6. **Corpus composition becomes a fifth protected dimension of the failure protocol**, and
   **silent partial ingestion a fourth trigger.**

### Why rows are not excerpted, against five lenses

`young`, `beck`, `legal`, `graphql` and `business` asked for `key` + `status` rendered from the
file at HEAD. `ux` produced a live counter-example and it decides the question:
[`PMW-1`](../decisions/PMW-1.yaml) is `status: "decided"` with an evidence field that reads as firm
founder approval, while its own `note` records that the premise is gone, the founder struck it
2026-08-31, and the live challenge is the open row `PMW-4`. Rendering `status: decided` beside that
row would manufacture a **more convincing false answer than rendering nothing**. No field subset is
safe: resolving PMW-1 needs `status` **and** `reconsiders`/`superseded_by` **and** the note,
together. Instead every lookup prints `corpus: <sha> (working tree not indexed)`, which makes
staleness honest for *every* candidate and removes the need to reason about which fields are safe.

### Contract clause 2 loses its premise and keeps its rule

Mandatory row resolution was justified by "the index cannot see row files". That premise dies
here. Restated capability-independently: **the index is a disposable fold of ONE head SHA, and a
projection is never authority.** Resolution is mandatory because the retrieved copy is **stale**,
not because it is blind — and `status` is precisely the field that changes after a row is written.
That form survives every future corpus widening; "it cannot see rows" survives none.

## Corrections to the predecessor, made rather than copied

Three of these are the **same drift the predecessor documents**, so carrying them forward would
have been occurrence four. **The locus is stated by its pin test name, never by a job name**: a job
name drifts every time the locus moves, a test name is the stable handle.

| Predecessor says | Actually |
|---|---|
| the step is in the always-run `changes` job | it is the **`gate-scripts:`** job (`GATE-STEP-LOCUS`, 2026-08-27) |
| pinned by `the_stub_suite_runs_in_the_always_run_changes_job` | no such test; it is **`the_stub_suite_runs_in_the_always_run_gate_job`** |
| rider (f)'s `timeout-minutes` cap is on `changes` | the cap mitigating *this* integration's step is on **`gate-scripts`** |
| clause (d): `decision-superseded-authority` **WARNS** | it is an **error** — `issues.push(err(…))`, closed as ERR by `CITATION-RULE-LEVEL` |

The predecessor's spike caveat — "the default Markdown mask is blind to `docs/decisions/*.yaml` so
QMD cannot discover or resolve authoritative rows" — becomes **false on landing** and is
**retracted** in the successor's own words rather than carried. The two caveats that survive are
carried: both QMD and `rg` missed the docs-only-CI citation case, and the spike corpus was
contaminated by the proposal's own transcripts, so no performance claim transfers.

**What was deliberately left behind**, and why: the predecessor's clause history, review-number
archaeology, self-retractions and per-clause rider narration are **narration, not controlling
content**. They stay in `RETRIEVAL-QMD-CI`, readable at its own path. Authorized / not-authorized /
failure protocol / activation state were carried **in full**. The executable pin
`the_ride_along_count_matches_the_clauses_named` stays pointed at the **historical** row, because
the riders it counts are that row's.

## Accepted behaviour, recorded and not mitigated

- **BM25 length normalization favours short documents.** The median row is 961 B against a corpus
  of 355 Markdown documents, and the boost is **inversely correlated with content**: the ~900 B
  stubs win slots while the two largest rows — `GATE-STEP-LOCUS` (19674 B) and `RETRIEVAL-QMD-CI`
  (16720 B) — lose them. With `K=3` that is **slot occupancy**: three thin rows can evict a
  deciding ADR, and dedup is path-only, so all three slots can be one decision family.
- **No ranking claim is made anywhere.** Rows become *discoverable*; that is the only property
  asserted.
- **36 of 81 keys carry zero topical words**, so they get no key signal from BM25. This is **not**
  to be "fixed" by renaming keys — a key rename breaks every chain edge and every citation.
- **New false-negative class**: rows are written in the same sessions that run register checks, and
  the working tree is never indexed. **A lookup miss on a row is not a negative trail**; `rg` over
  the working tree stays mandatory.
- **Coverage bound**: 81 rows indexed, **zero** of the 100 `_legacy.yaml` keys, because their prose
  home `DECISIONS.md` is an excluded corpus file. A null result over the register is a null result
  over roughly half of it.

## The canary, and the honest bound on the stub suite

Corpus **presence** is not **ingestion**. The wrapper now plants a nonce-bearing
`docs/decisions/*.yaml` file, and after `qmd update` searches for that nonce; zero hits ⇒ caches
wiped, **corpus never stamped**, its own named fallback. Non-vacuity is asserted rather than
assumed: if the nonce occurs in any indexed Markdown, the build fails.

The sentinel is a **nonce, not register content**, deliberately. A guard coupled to a real row's
key or wording would go red on ordinary register growth — someone opening or superseding a decision
row would fire a repository-wide merge block, worse than the failure being guarded, and on the
docs-straight-to-`main` lane it inverts into "landed with `codegen` red".

**Honest bound**: the stub suite fakes `qmd`, so it can prove the wrapper's control flow around the
canary (present ⇒ proceed; absent ⇒ fail closed, unstamped) and **structurally cannot** prove that
a real qmd ingested a real row. That half is the empirical verification above, plus the canary
running in real use. Both halves were exercised this session: the real path returned
`CREDIT-DRAIN-ORDER.yaml` as candidate 1, and a mutant with the glob re-narrowed to `**/*.md`
produced the named canary fallback with the caches wiped and no stamp.

## Legal posture — unchanged, restated because discoverability changed

A hit on a counsel-owned row **licenses only the external action, never the answer**. Three rows
carry `owner: counsel`: `MONEY-LINE-LEGAL`, `PUBLISH-PRECONDITIONS`, `REVOKED-COLLEAGUE-NOTICE`.
A `decided` status on any row is a recorded decision, **never legal clearance**. Making a row
easier to find is neither discharge of a counsel-gated question nor counsel re-entry.
`docs/legal/**` stays **out** of the corpus, deliberately. The "may not leave the repo" constraint
continues to hold because the cache is local and gitignored and no network path exists on the
lookup path — a **condition, not a property**, which every fenced item this corpus might later gain
would re-open.

## Consequences

- `.claudeignore` and `.gitignore` were **checked and need no change** — `.qmd/` is already ignored
  by both and the corpus lives only inside it. Stated because "the ignore lists were checked" is
  otherwise indistinguishable from "nobody looked".
- **The CI diff is zero lines.** The one authorized step already runs the stub suite; the suite
  gained cases, not a step. No new job, step or permission.
- `specs/**` is untouched, so **`docs/SPEC-LOG.md` gets no sentence**.
- Residue split off at close time per `docs/decisions/README.md`: the row schema has no field for a
  fence, tracked as the team-owned open row
  [`REGISTER-ROW-FENCE`](../decisions/REGISTER-ROW-FENCE.yaml).

## Consulted

Per CLAUDE.md, records created from a founder directive carry a `Consulted:` block — one line per
lens, because a lens never asked is indistinguishable from a lens with nothing to say. All thirteen
answered at the briefing.

- **architect** — flagged the unverified premise that gated everything (does qmd ingest `.yaml` at
  all?), and verified that the chain head's evidence now contains three false statements because
  `GATE-STEP-LOCUS` moved the locus; supplied the "name the locus by its pin test name" rule.
- **beck** — owned the test bar: positive vacuity guard rather than another absence, corpus
  presence ≠ ingestion, plant every new case red once; and held that no ranking claim may enter the
  record.
- **business-specialist** — asked for `key` + `status` rendering (overruled by `ux`'s
  counter-example); no other concern.
- **dba** — flagged the tokenization question, argued *for* including `_legacy.yaml` on
  coverage grounds (overruled on the no-row-file-to-resolve tiebreak), and supplied the BM25
  length-normalization analysis recorded as accepted behaviour.
- **evans** — named the successor off the `RETRIEVAL-QMD` stem rather than extending
  `RETRIEVAL-QMD-CI`, specifically so `git grep RETRIEVAL-QMD-CI` stays an answerable sweep; and
  found the executable pins on the old row that sit outside the citation corpus.
- **farley** — counted the key-sweep sites and, decisively, forbade count- and content-coupled test
  assertions: a case coupled to the register population reds on every new decision row anyone adds.
- **graphql-architect** — required the not-authorized list to restate "MCP or any server, hosted
  services, credentials" verbatim and to affirm the local-CLI-over-gitignored-cache posture.
- **holub** — measured the evidence field at 2413 words (re-measured here: 2412) against the
  predecessor's 249 (re-measured: 248); argued clause history is narration; found the sixth
  live-authority instruction surface in `.claude/skills/coordinator-register-check/SKILL.md`.
- **legal-specialist** — supplied the counsel-owned-row posture, the discoverability-is-not-
  discharge clause, and the "condition, not a property" framing of the no-network constraint.
- **observability-agent** — the two failure-protocol gaps now closed: corpus composition as a fifth
  protected dimension, silent partial ingestion as a fourth trigger.
- **ux-designer** — the `PMW-1` counter-example that overturned five lenses on rendering, and the
  `corpus: <sha>` staleness header.
- **vernon** — index superseded and withdrawn rows, because the DAG is what makes the register
  resolvable; and the additive-row-is-validator-legal-but-wrong finding.
- **young** — restated contract clause 2 capability-independently (a projection is never
  authority), which is what lets the rule outlive its premise.
