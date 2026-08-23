---
name: decision-lookup
description: >
  Advisory BM25 candidate retrieval over the committed governing Markdown (ADRs, proposals,
  journals, session docs, CLAUDE.md) to speed up finding the record that answers a question. Use
  when searching for a controlling decision record, a prior ruling, or "have we decided X" — BEFORE
  formulating a register-check trail or a founder question. ADVISORY ONLY: candidates, never
  evidence or authority. Every use ends with direct reading of the candidate and exact
  docs/decisions/<KEY>.yaml resolution; rg + aliases stays the authoritative fallback and is the
  tool when QMD is unavailable, stale, or empty. Never a substitute for the register-check
  discipline or the AskUserQuestion gate.
---

# decision-lookup — advisory retrieval over committed governing Markdown

Decided by row `RETRIEVAL-QMD` (`decided_by: PROP-20260822-171212`, founder 2026-08-22). QMD is an
**advisory read path**; **decision YAML plus direct source reading is the authority path** — that
sentence is the whole architecture.

## How to use

```
.claude/skills/decision-lookup/scripts/decision-lookup.sh "<question in your own words>"
```

Output: at most **three** candidate records — repo-relative path + short excerpt — preceded by this
fixed disclaimer, printed on every invocation and never suppressed:

> ADVISORY ONLY — candidates, not evidence. READ the candidate directly, then resolve the exact
> row: `docs/decisions/<KEY>.yaml`. Baseline and fallback: `rg + aliases` (workflow.md alias
> table). No result is NOT evidence of "undecided".

## The contract (binding on every consumer)

1. **Candidates are pointers, nothing more.** A candidate is never citable, never a trail entry,
   never "evidence". A record id may enter a `Register check:` trail only after you have READ that
   record directly.
2. **Row resolution is mandatory** before any decision assertion or founder question: resolve the
   exact `docs/decisions/<KEY>.yaml` at HEAD. The index cannot see row files (Markdown-only mask,
   by design), so this step can never be skipped "because the tool already found it".
3. **No result decides nothing.** An empty result is not "undecided" and not "no controlling
   record": the index is Markdown-only, corpus-masked, and possibly stale. A negative claim
   requires the `rg + aliases` search plus direct `docs/decisions/` resolution at HEAD.
4. **Fallback is the system, not a degraded mode**: if QMD is unavailable, the index is stale, or
   the result is empty, use `rg --fixed-strings -i` with the question's words AND the alias table
   of `docs/claude/sessions/workflow.md`, then resolve the row.
5. **Advisory and non-blocking**: nothing requires this skill; no hook, gate, validator, or agent
   contract consumes its output; the AskUserQuestion register-check gate is unchanged.

## Corpus policy (include/exclude — the wrapper enforces it)

- **Included** (committed Markdown only, exported via `git archive HEAD`, never the working tree):
  `docs/adr/**`, `docs/proposals/**`, `docs/status/**`, `docs/claude/**`, `docs/STATUS.md`,
  `CLAUDE.md`.
- **Excluded**: `docs/proposals/DECISIONS.md` (carries a GENERATED region duplicating row data —
  the duplicate/authority-confusion shape this decision forbids), `docs/proposals/PROP-20260822-171212-*`
  and any QMD experiment artifacts (the recorded contamination source), `specs/generated/**` and
  all generated files, and everything that is not Markdown — **including `docs/decisions/*.yaml`:
  row indexing is explicitly out of scope pending separate evidence**.

## Verification cases (run manually after any wrapper or corpus-mask change)

The six recorded cases — the five smoke-test questions plus the known citation miss. Expected
behavior, not scores: the disclaimer prints; ≤3 candidates; and the mandatory resolution step
reaches the controlling record even where retrieval alone missed it.

| Question | The controlling record the RESOLUTION step must reach |
|---|---|
| who bears the refund cost | `docs/decisions/REFUND-BEARER.yaml` (candidates led to its deciding ADR in the spike) |
| is a tip/contribution pre-filled by default | `docs/decisions/CONTRIB-DEFAULT.yaml` |
| what is the free-delivery threshold | `docs/decisions/DELIV-THRESHOLD.yaml` |
| what does the docs-only CI citation rule require | `docs/adr/ADR-20260821-103403-decision-ask-unregistered-and-the-citation-ratchet.md` — **the recorded miss case: both QMD and rg missed it in the spike; it verifies the fallback + direct-read discipline, not the retriever** |
| what is the order of the rider/delivery work | the delivery proposals surfaced ranked in the spike; resolution lands on the current row/record |
| (fallback case) any query with the tool cache deleted | the exact fallback text prints; **exit 0**; no install occurs; `rg + aliases` answers |

## Exit semantics and activation

- **Lookup path**: always exit 0 — unavailability, empty result, stale index, rebuild failure, or an
  output-contract failure print the advisory fallback. The wrapper consumes qmd's `--json` output
  through a python3 standard-library parser against a **pinned, strict top-level schema** (a
  ranked-result array, or `{results: [...]}`; per-result path/excerpt read from DIRECT keys only —
  **nothing nested is ever scanned**, so a metadata path can never become a candidate; source order
  preserved; first-occurrence dedup; first three unique paths). The pin is **provisional** — the
  sandbox spike ran `search` without `--json`, so the real shape is confirmed at the activation
  test; any other structure is "QMD output contract unavailable; use rg + aliases", never
  guesswork.
- **`--install` (the activation test)**: exits **non-zero** on any failure — bun absent, install
  failure, pin/integrity verification against the recorded digest failing, or lifecycle-script
  enforcement (`trustedDependencies: []` + `ignoreScripts = true`) not establishable on disk — and
  prints: *activation failed; remove `.qmd/` before any future approved retry*, plus the
  reversal-decision instruction (row `RETRIEVAL-QMD`). A failed install may leave a partial
  `.qmd/tool/`; it never claims "nothing changed".
- **Cache layout / activation evidence**: `.qmd/tool/` (pinned package, lockfile, manifests),
  `.qmd/corpus/` (`git archive HEAD` export + `.sha` revision stamp + the project-local index
  database qmd 2.8.3 writes inside the collection dir, observed at activation 2026-08-23:
  `.qmd/corpus/.qmd/index.sqlite` plus its `-wal`/`-shm` files), `.qmd/index/` (qmd's HOME-side
  state — config under `.qmd/index/.config/qmd/`). **Activation evidence** is printed and recorded at
  `.qmd/activation-evidence.txt` on the **first successful lookup**: the package pin
  `@tobilu/qmd@2.8.3`, lockfile-integrity verification, scriptless-install enforcement, the corpus
  HEAD SHA, the `.qmd/corpus/.sha` stamp path, the observed SQLite index location, and the
  **observed JSON schema** as exactly `qmd-json-schema: top-level-array` or
  `qmd-json-schema: object.results-array`. Any other top-level shape keeps the fallback behavior
  and makes activation **failed/inconclusive pending a new decision — never a reason to modify the
  parser or broaden the accepted shapes**.

## Non-executing test plan (stubs only — never creates `.qmd/`, never calls the real package)

Run with a temporary cache override and fake `bun`/`qmd` executables on `PATH`; the real repo
`.qmd/` is never created and no package is installed:

1. `bash -n .claude/skills/decision-lookup/scripts/decision-lookup.sh` — syntax.
2. **Lookup cache miss falls back**: fresh `DECISION_LOOKUP_HOME` (no tool) + a query → fallback
   text, exit 0.
3. **Install without Bun exits non-zero**: `PATH` without `bun`, `--install` → "ACTIVATION
   FAILED", exit ≠ 0.
4. **Rebuild failure wipes cache and falls back**: fake `qmd` whose `update` exits 1 → fallback,
   exit 0, and the corpus/index dirs are gone.
5. **Parser failure falls back**: fake `qmd search --json` emitting invalid JSON → "output
   contract unavailable" fallback, exit 0.
6. **At most three candidates**: fake `qmd search --json` emitting five results → exactly three
   `candidate N:` lines.

## What this skill must never grow without a NEW decision row

Vector/semantic search, embeddings, model downloads, reranking, query expansion, MCP or any server
process, hosted services, credentials, hooks or CI or validator or agent-contract changes, YAML
decision-row indexing, or any mandatory-workflow rule.
