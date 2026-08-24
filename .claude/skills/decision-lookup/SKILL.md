---
name: decision-lookup
description: >
  Advisory BM25 candidate retrieval over the committed governing Markdown (ADRs, proposals,
  session docs, STATUS.md, CLAUDE.md — status journals excluded) to speed up finding the record
  that answers a question. Use
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

- **Included** (committed Markdown only, exported via `git archive` of the one resolved HEAD SHA,
  never the working tree): `docs/adr/**`, `docs/proposals/**`, `docs/claude/**`, `docs/STATUS.md`,
  `CLAUDE.md`.
- **Excluded**: `docs/status/**` (**the status journals narrate this tool's own activation and
  verification — queries and answers verbatim — so indexing them lets a lookup match the account
  of itself, the recorded self-contamination/false-authority shape; `rg + aliases` still searches
  status records directly whenever they are actually the target**), `docs/proposals/DECISIONS.md`
  (carries a GENERATED region duplicating row data — the duplicate/authority-confusion shape this
  decision forbids), `docs/proposals/PROP-20260822-171212-*` and any QMD experiment artifacts
  (the recorded contamination source), `specs/generated/**` and all generated files, and
  everything that is not Markdown — **including `docs/decisions/*.yaml`: row indexing is
  explicitly out of scope pending separate evidence**. (`specs/**` was never included.)

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

- **Lookup path**: always exit 0 — unavailability, empty result, stale index, rebuild failure, a
  search-tool failure, or an output-contract failure print the advisory fallback. **A non-zero
  `qmd search` exit is a named tool failure, distinct from an empty successful result** — it never
  reads as "no candidates", **and it wipes the derived corpus/index caches before falling back**
  (delete-wholesale, never repair — proposal §6.3): deep index corruption cannot degrade every
  lookup until HEAD changes; the next lookup rebuilds from the pinned archive. The cache-hit
  check also runs a **bounded openability probe** (sqlite connect + `PRAGMA schema_version` —
  never `quick_check`/`integrity_check` per lookup): a corrupt-but-present index is a broken
  cache and takes the ordinary wipe-and-rebuild path. The fallback's `rg` command renders the
  query as data via Bash `printf %q` — **Bash-safe only**: safe to copy/paste into Bash whatever
  the query contains (`%q` may emit `$'...'`/backslash forms), with no claim made for other
  shells. The wrapper consumes qmd's `--json` output
  through a python3 standard-library parser against a **pinned, strict top-level schema** (a
  ranked-result array, or `{results: [...]}`; per-result path/excerpt read from DIRECT keys only —
  **nothing nested is ever scanned**, so a metadata path can never become a candidate; source order
  preserved; first-occurrence dedup; first three unique paths). The pin is **provisional** — the
  sandbox spike ran `search` without `--json`, so the real shape is confirmed at the activation
  test; any other structure is "QMD output contract unavailable; use rg + aliases", never
  guesswork.
- **Python 3 is a required local runtime** for both the structural `trustedDependencies` install
  verification and the strict JSON results parser. Its absence at `--install` causes a **named
  non-zero activation failure** (with the standard reversal instruction) — never a fallback
  installation, download, or repair.
- **`--install` (the activation test)**: exits **non-zero** on any failure — bun absent, install
  failure, pin/integrity verification against the recorded digest failing, or lifecycle-script
  enforcement (`trustedDependencies: []` + `ignoreScripts = true`) not establishable on disk — and
  prints: *activation failed; remove `.qmd/` before any future approved retry*, plus the
  reversal-decision instruction (row `RETRIEVAL-QMD`). A failed install may leave a partial
  `.qmd/tool/`; it never claims "nothing changed".
- **Post-update index assertion**: the corpus stamp is written only after the index database is
  verified present and non-empty; a successful `qmd update` that leaves no index at the expected
  location is treated as a rebuild failure — caches wiped, the named fallback fires, the corpus
  is never stamped (an index-less stamped corpus would otherwise rebuild forever, silently).
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

## Hermetic test suite (stubs only — never installs, never touches the real `.qmd/`)

The committed suite is the executable authority; re-run it after any wrapper change:

```
bash .claude/skills/decision-lookup/scripts/stub-tests.sh
```

**34 cases** — the 19 existing behavioral cases retained (with limited harness adaptations for
repository-relative execution, cache-invariance verification, and a controlled-PATH rework of the
bun-absent install case), plus 1 search-failure case (now also asserting the cache wipe),
5 quoting cases, 1 python3-preflight case, 1 corpus-mask case, 1 stamp/archive-SHA case,
2 broken-cache cases, 2 post-update index-assertion cases, 1 stamp-write-failure case, and
1 corrupt-index case — all against a temporary `DECISION_LOOKUP_HOME` with fake `bun`/`qmd`
executables; the real repo `.qmd/` is never created, never modified (a before/after fingerprint
asserts it) and never depended on, and no package is installed. Coverage:

1. **Syntax**: `bash -n .claude/skills/decision-lookup/scripts/decision-lookup.sh`.
2. **Lookup cache miss falls back**: fresh cache home (no tool) + a query → fallback text, exit 0.
3. **Install without Bun exits non-zero**: `PATH` without `bun`, `--install` → "ACTIVATION
   FAILED", exit ≠ 0, with the remove-`.qmd/`-before-retry message. **Install with Bun but
   without python3** → the named python3-preflight failure, exit ≠ 0, same reversal message.
4. **Rebuild failure wipes cache and falls back**: fake `qmd` whose `update` exits 1 → fallback,
   exit 0, and the corpus/index dirs are gone.
5. **Strict parser** (both pinned shapes and every rejection path): `top-level-array` and
   `object.results-array` accepted with the `qmd-json-schema:` line printed, source order
   preserved, first-occurrence dedup, at most three `candidate N:` lines; a nested
   `meta.path`/`meta.source.file` never becomes a candidate; invalid JSON, an unpinned top-level
   shape, a missing `results` key, or a result without a direct path key → "output contract
   unavailable" fallback (the unpinned-shape case with the activation-inconclusive wording), exit
   0, and **no** activation-evidence file; on a valid first success the evidence file is written
   exactly once with all seven lines and never re-printed.
6. **Structural `trustedDependencies` check** (the shipped function, extracted verbatim):
   Bun-reformatted, compact, and multiline empty lists accepted; a missing key, an allowlist
   entry, a non-list value, and invalid JSON all rejected.
7. **Search failure ≠ empty result** (planted red): fake `qmd search` exiting non-zero → the
   distinct named tool-failure fallback, exit 0, never the empty-result wording.
8. **Corpus mask**: after a lookup, the exported corpus contains no `docs/status/**`, no
   `DECISIONS.md`, no QMD-proposal file — and still contains the governed sources — so a
   status-only journal document can never be indexed or surface as a candidate.
9. **Stamp/archive same-SHA**: `corpus/.sha` equals the one resolved `git rev-parse HEAD`, and
   the wrapper archives `"$head"`, never a re-resolved symbolic `HEAD`.
10. **Broken cache**: a matching `corpus/.sha` with a missing `corpus/.qmd/index.sqlite` is
   REBUILT (candidates print; never the empty-result wording); if that rebuild fails, the named
   rebuild-failed fallback fires with caches wiped — still never the empty-result wording. A
   successful `qmd update` that writes no index is never stamped (positive + planted-red pair:
   stamp and index must coexist; an index-less "success" is wiped, unstamped, and falls back).
   A failed `corpus/.sha` write also wipes the derived caches before falling back, so the
   "caches wiped" wording is true on every failure arm. A corrupt-but-present index (garbage
   bytes, matching stamp) fails the openability probe and is rebuilt — candidates print, never a
   permanent tool-failure; a failed search wipes the derived caches before its named fallback.
11. **Bash-safe fallback rendering**: for a double quote, `$()`, backticks, a newline and a
   leading hyphen, the rendered `rg` command is executed under **Bash — the emitted command's
   documented target shell** — against a recording `rg` stub: the exact query must arrive
   verbatim as ONE argv element and no side-effect sentinel may appear; the `$()`/backtick cases
   additionally assert in the RENDERED TEXT that the payload is escaped as data (no unescaped
   `$(` or backtick). The guarantee is Bash-quoting only — no claim for other shells.

## What this skill must never grow without a NEW decision row

Vector/semantic search, embeddings, model downloads, reranking, query expansion, MCP or any server
process, hosted services, credentials, hooks or CI or validator or agent-contract changes, YAML
decision-row indexing, or any mandatory-workflow rule.
