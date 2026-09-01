---
name: decision-lookup
description: >
  Advisory BM25 candidate retrieval over the committed governing records (ADRs, proposals,
  session docs, STATUS.md, CLAUDE.md and the decision-register row files `docs/decisions/*.yaml`
  — status journals excluded) to speed up finding the record
  that answers a question. Use
  when searching for a controlling decision record, a prior ruling, or "have we decided X" — BEFORE
  formulating a register-check trail or a founder question. ADVISORY ONLY: candidates, never
  evidence or authority. Every use ends with direct reading of the candidate and exact
  docs/decisions/<KEY>.yaml resolution AT HEAD — including when the candidate IS a row file, because
  the index is a fold of one commit and never the working tree; rg + aliases stays the authoritative
  fallback and is the tool when QMD is unavailable, stale, or empty. Never a substitute for the register-check
  discipline or the AskUserQuestion gate.
---

# decision-lookup — advisory retrieval over the committed governing records

Decided by row `RETRIEVAL-QMD-ROWS` (`decided_by: ADR-20260901-025538`, founder 2026-09-01) — the CHAIN HEAD, which carries the controlling content of `RETRIEVAL-QMD-CI` (and, through it, `RETRIEVAL-QMD`) forward. Name the head, never a superseded row: a `reconsiders:` pointing at a superseded row is rejected by the validator. QMD is an
**advisory read path**; **decision YAML plus direct source reading is the authority path** — that
sentence is the whole architecture.

## How to use

```
.claude/skills/decision-lookup/scripts/decision-lookup.sh "<question in your own words>"
```

Output: at most **three** candidate records — repo-relative path + short excerpt — preceded by this
fixed disclaimer, printed on every invocation and never suppressed:

> ADVISORY ONLY — candidates, not evidence. READ every candidate directly at HEAD, and resolve
> `docs/decisions/<KEY>.yaml` itself: the index is a disposable fold of ONE commit, and a
> projection is never authority — including when the candidate IS a row file. Baseline and
> fallback: `rg + aliases` (workflow.md alias table). No result is NOT evidence of "undecided".

…followed by a **staleness header on every lookup** — `corpus: <sha> (working tree not indexed)` —
so the one SHA the index folds is stated for *every* candidate rather than reasoned about per
candidate.

**A decision-row candidate is rendered as a resolve-instruction, never as an excerpt**
(`RETRIEVAL-QMD-ROWS`): `docs/decisions/<KEY>.yaml` prints with *"resolve … at HEAD"* and no
snippet. No field subset of a row is safe to quote out of context — `PMW-1` is `status: "decided"`
with an evidence field reading as firm founder approval, while its own `note` records that the
premise is gone and the live challenge is the open row `PMW-4`. Excerpting `status` beside that row
would manufacture a more convincing false answer than printing nothing.

## The contract (binding on every consumer)

1. **Candidates are pointers, nothing more.** A candidate is never citable, never a trail entry,
   never "evidence". A record id may enter a `Register check:` trail only after you have READ that
   record directly.
2. **Row resolution is mandatory** before any decision assertion or founder question: resolve the
   exact `docs/decisions/<KEY>.yaml` at HEAD — **including when the retrieved candidate IS that
   row file**. The reason is not that the index is blind (it no longer is): **the index is a
   disposable fold of ONE head SHA, and a projection is never authority.** The retrieved copy is
   **stale**, and `status` is precisely the field that changes after a row is written. Stated this
   way the rule survives every future corpus widening; "it cannot see rows" survived none.
3. **No result decides nothing.** An empty result is not "undecided" and not "no controlling
   record": the index is corpus-masked and is a projection of ONE commit, never the working tree.
   A negative claim requires the `rg + aliases` search plus direct `docs/decisions/` resolution at
   HEAD. **Two bounds make this sharper than it used to be, not softer:** (a) rows are *written*
   in the same sessions that run register checks, and the working tree is never indexed — so **a
   lookup miss on a row is not a negative trail**; (b) 81 row files are indexed and **zero** of the
   100 `_legacy.yaml` keys are, because their prose home `DECISIONS.md` is an excluded corpus file,
   so a null result over the register is a null result over roughly half of it.
4. **Fallback is the system, not a degraded mode**: if QMD is unavailable, the index is stale, or
   the result is empty, use `rg --fixed-strings -i` with the question's words AND the alias table
   of `docs/claude/sessions/workflow.md`, then resolve the row.
5. **Advisory and non-blocking**: nothing requires this skill; no hook, gate, validator, or agent
   contract consumes its output; the AskUserQuestion register-check gate is unchanged.

## Corpus policy (include/exclude — the wrapper enforces it)

- **Included** (committed sources only, exported via `git archive` of the one resolved HEAD SHA,
  never the working tree): `docs/adr/**`, `docs/proposals/**`, `docs/claude/**`, `docs/STATUS.md`,
  `CLAUDE.md`, and — since `RETRIEVAL-QMD-ROWS` (founder 2026-09-01) — the decision-register row
  files `docs/decisions/*.yaml`. **All** rows, including `superseded` and `withdrawn` ones:
  `superseded_by:` is what makes a hit on a retired row resolvable to its chain head, so the
  reduction rule is *follow the edge*, and truncating the corpus to live rows would destroy the
  DAG that makes the register answerable.
- **Excluded**: `docs/status/**` (**the status journals narrate this tool's own activation and
  verification — queries and answers verbatim — so indexing them lets a lookup match the account
  of itself, the recorded self-contamination/false-authority shape; `rg + aliases` still searches
  status records directly whenever they are actually the target**), `docs/proposals/DECISIONS.md`
  (carries a GENERATED region duplicating row data — the duplicate/authority-confusion shape this
  decision forbids), `docs/proposals/PROP-20260822-171212-*` and any QMD experiment artifacts
  (the recorded contamination source), `specs/generated/**` and all generated files,
  `docs/decisions/_legacy.yaml` and `docs/decisions/_exempt.yaml` (**control files, not rows**:
  no `status`/`owner`/`capacity` to disambiguate a hit, and `_legacy.yaml` is one document naming
  100 prose-only keys, so it ranks for any register query while answering none — and a hit on it
  points at a key with **no row file to resolve**, the one case the mandatory-resolution contract
  cannot discharge), `docs/decisions/README.md` (excluded **by construction**: the pathspec is
  `docs/decisions/*.yaml`), and everything else that is not Markdown. (`specs/**` and
  `docs/legal/**` were never included, and `docs/legal/**` stays out deliberately.)

**The mask exists in THREE places, and all three must agree** — this is the change's primary silent
failure mode, so it is stated on the authority surface rather than only in the code: the `git
archive` pathspec, the `find … ! -name '*.md'` sweep, **and qmd's own collection glob** (`pattern:`
in the corpus-local `.qmd/index.yml`, which the tool defaults to `**/*.md` and which
`qmd collection add . '<glob>'` silently refuses to set). Widen two of the three and the corpus
exports the rows, keeps them, indexes an empty `.yaml` arm, stamps, and answers **forever without
ever returning a row**. A **rebuild-time ingestion canary** now closes that: a nonce-bearing
`docs/decisions/*.yaml` file is planted before `qmd update` and searched for after it; zero hits
wipes the caches, **never stamps the corpus**, and takes a named fallback. The sentinel is a nonce
rather than a real row on purpose — a guard coupled to register *content* would go red whenever
someone opens or supersedes an unrelated decision row.

**`docs/decisions/**` is NOT in `claude_citation_corpus`, and must not be added to it.** Rows cite
superseded rows *by construction* (`reconsiders:`, `superseded_by:`), so wiring them in would make
`decision-superseded-authority` — an **error** — fire across the register itself. The retrieval
corpus and the citation corpus are unrelated and stay unrelated.

**Accepted behaviour, measured and not mitigated** (`RETRIEVAL-QMD-ROWS`): BM25 length
normalization favours short documents, and the median row is 961 B (81 rows: min 326 B, mean
2168 B, max 19674 B) against 355 Markdown documents. The boost is *inversely correlated with
content* — the ~900 B stubs win slots, the two largest rows lose them — and with `K=3` that is slot
occupancy: three thin rows can evict a deciding ADR, and dedup is path-only, so all three slots can
be one decision family. **No ranking claim is made anywhere**; rows are *discoverable*, and that is
the only property asserted. Known limit, not to be "fixed": **36 of the 81 keys carry zero topical
words**, so they get no key signal — renaming keys is not the remedy, because a rename breaks every
chain edge and every citation.

## Verification cases (run manually after any wrapper or corpus-mask change)

The six recorded cases — the five smoke-test questions plus the known citation miss. Expected
behavior, not scores: the disclaimer prints; ≤3 candidates; and the mandatory resolution step
reaches the controlling record even where retrieval alone missed it.

**These cases no longer distinguish "retrieval found it" from "resolution found it"** for five of
the six, because the row files they name are now IN the corpus (`RETRIEVAL-QMD-ROWS`). The column
is therefore what the RESOLUTION step must reach, unchanged — and the re-run column records what
retrieval actually did on 2026-09-01, at corpus SHA `3afd0fe`.

| Question | The controlling record the RESOLUTION step must reach | Re-run 2026-09-01 |
|---|---|---|
| who bears the refund cost | `docs/decisions/REFUND-BEARER.yaml` | 3 ADR candidates, **no row** in the top 3 (`ADR-20260819-103112` first) — resolution still does the work |
| is a tip/contribution pre-filled by default | `docs/decisions/CONTRIB-DEFAULT.yaml` | **empty result**, fallback printed, exit 0, cache intact. **Query-shape effect, not a corpus effect**: the same question without the `/` (*"is a tip contribution pre-filled by default"*) returns 3 candidates. A `/` in a query is a second known empty-result shape beside the leading hyphen |
| what is the free-delivery threshold | `docs/decisions/DELIV-THRESHOLD.yaml` | 3 candidates, none of them the row; `PROP-20260819-110442` (which quotes the row) ranks 2 |
| what does the docs-only CI citation rule require | `docs/adr/ADR-20260821-103403-decision-ask-unregistered-and-the-citation-ratchet.md` — **the recorded miss case** | **The most informative case, and it gained information.** The originally-named ADR is still not in the top 3, so the *miss on that record stands*; but candidate 2 is now `docs/decisions/CITATION-RULE-LEVEL.yaml` and candidate 3 the superseded `docs/decisions/RETRIEVAL-QMD-CI.yaml`, so the ROW that answers the question is retrieved and the answer is reachable in one resolution hop. Candidate 3 is also a live demonstration of the DAG rule: a superseded row retrieves, and resolving it means following `superseded_by:` to the head |
| what is the order of the rider/delivery work | the delivery proposals; resolution lands on the current row/record | **empty result** — the same `/` shape as the tip case |
| (fallback case) any query with the tool cache deleted | the exact fallback text prints; **exit 0**; no install occurs; `rg + aliases` answers | unchanged |

Two things the re-run establishes that a passing suite cannot: an **empty result keeps the cache**
(it is not a tool failure, so nothing is wiped), and **a row ranking below a prose record is normal**
— no ranking claim is made anywhere, and the value of row indexing is *discoverability plus a
one-hop resolution*, not displacing the ADRs.

## Exit semantics and activation

- **Lookup path**: always exit 0 — unavailability, empty result, stale index, rebuild failure, a
  search-tool failure, or an output-contract failure print the advisory fallback. **A non-zero
  `qmd search` exit is a named tool failure, distinct from an empty successful result** — it never
  reads as "no candidates", **and it wipes the derived corpus/index caches before falling back**
  (delete-wholesale, never repair — proposal §6.3): deep index corruption **that qmd reports
  as a non-zero exit** cannot degrade every lookup until HEAD changes; the next lookup
  rebuilds from the pinned archive. Scope, recorded honestly: corruption surfacing as a
  successful exit — garbage output (the contract fallback) or empty output (the no-result
  arm) — keeps the cache and does degrade per-HEAD; the contract arm deliberately never
  wipes, because it is also the schema-pin-mismatch path, and wiping there would rebuild-loop
  a healthy cache under a genuinely changed output contract. Honest cost,
  recorded: the exit code cannot distinguish a damaged index from qmd rejecting the query
  itself, so a query-triggered failure pays the same wipe and the **next** lookup pays a full
  rebuild — accepted over ever serving a possibly-poisoned cache (a cache that "keeps
  rebuilding" points to a query shape qmd rejects; the **known reproducer is a leading-hyphen
  query** — the query is positional and unfenced at the qmd call, so it may parse as an
  option; rephrase without the leading hyphen. Whether qmd 2.8.3 honors a `--` fence is
  unverifiable offline — the pinned package lives inside the claudeignored cache — and
  fencing unverified would risk breaking every query). The cache-hit
  check also runs a **bounded openability probe** (**immutable read-only** sqlite connect —
  zero writes, zero locks, zero busy timeout — + `PRAGMA schema_version`; never
  `quick_check`/`integrity_check` per lookup): a corrupt-but-present index is a broken cache
  and takes the ordinary wipe-and-rebuild path. The probe is a **zero-write observer** — a
  default read-write connect would silently run SQLite WAL recovery on the hit path (a write
  into the derived index: the repair §6.3 forbids), and even a plain read-only connect creates
  the `-shm` side file; `immutable=1` touches nothing. The probe asks only whether the main
  database file is openable — a pending `-wal` is deliberately ignored (WAL handling belongs to
  the tool's own read-write open); an unopenable main file fails the probe and is wiped and
  rebuilt. **Only the probe's deliberate not-openable verdict (exit 1) may wipe** — probe
  unavailability or failure is never read as corruption: a python3 built without the
  compile-optional sqlite3 module exits distinctly (2), and **any other** exit (import-chain
  failure, signal death, 126/127) is likewise a probe failure, not a verdict — the same
  conflation guard as the preflight, one layer down (the probe's import surface is kept
  minimal on purpose: `urllib.parse.quote` inside the guarded try, never `urllib.request`,
  whose transitive `socket` import is another compile-time optional; URI construction also
  lives in that guarded arm — a non-UTF-8 byte in the cache path makes `quote()` raise, and a
  path-shaped failure must never read as the verdict — so the verdict arm holds only
  connect + `PRAGMA`, the sole operations that can testify about the database file, and catches
  only `sqlite3.Error`: the database's own verdicts are subclasses of it, while a failure of the
  *call* — an interpreter whose `connect()` lacks the `uri=`/`timeout=` kwargs raising
  `TypeError` — says nothing about the file and takes the unavailable arm). The verdict is read
  from the probe's **exit status**, never from captured output, and the probe's stdout is
  silenced with its stderr: interpreter-level noise (a printing `sitecustomize.py`) must not be
  able to reshape the one verdict that wipes. **One accepted assumption**, stated rather than
  enforced: lookups are treated as sequential, which is what makes `immutable`'s no-locking
  semantics safe. Concurrent sessions share one checkout's `.qmd/` and nothing serializes it, so
  a probe racing another session's rebuild can take a torn read and wipe the corpus under it —
  bounded and self-healing (both sessions land on the loud exit-0 fallback, the next lookup
  rebuilds, and the tool is advisory-only). The probe is
  **best-effort**: on anything but exit 1 the stamped non-empty hit is **accepted at the
  pre-probe trust level** — the rebuild arm serves exactly that trust level unprobed, so
  refusing the hit would disable the advisory tool on such hosts between HEAD changes for zero
  gained safety (qmd bundles its own SQLite; host-python module absence says nothing about the
  index). Deep corruption on such hosts stays bounded by the search-failure wipe. **python3 is preflighted on the lookup
  path before the cache is consulted, by execution** (`python3 -c 'import json'` — `command -v`
  proves resolvability, not runnability: a resolvable interpreter that cannot start would fail
  the probe exactly like corruption): without the preflight, every lookup would wipe and
  rebuild a healthy cache — an absent or unusable python3 instead degrades to the named
  fallback with the caches untouched. The fallback's `rg` command renders the
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
- **Python 3 is a required local runtime** for the structural lockfile-binding and
  `trustedDependencies` install verifications and the strict JSON results parser. `--install`
  **preflights it by execution** (`command -v` proves resolvability, not runnability — and this
  path routes the lockfile-binding tampering verdict through python3, so a broken interpreter
  must fail as a named host defect before any network touch, never inside the binding check):
  an absent or unusable python3 causes a **named non-zero activation failure** (with the
  standard reversal instruction) — never a fallback installation, download, or repair.
- **`--install` (the activation test)**: exits **non-zero** on any failure — bun absent, install
  failure, the **structural `bun.lock` binding** failing (the `@tobilu/qmd` packages entry must
  itself name the exact pin AND carry the recorded integrity digest — parse failure, missing
  package, wrong version, a digest attached to a different entry, or a lockfile entry shape
  differing from the assumed `[pin, …, integrity]` tuple all fail loudly, and the failure
  message names the shape-assumption cause so it is never misread as tampering; bun's JSONC
  trailing commas are stripped before parsing, so formatting churn never produces a false
  verdict), or lifecycle-script
  enforcement (`trustedDependencies: []` + `ignoreScripts = true`) not establishable on disk — and
  prints: *activation failed; remove `.qmd/` before any future approved retry*, plus the
  reversal-decision instruction (row `RETRIEVAL-QMD-ROWS`, the chain head). A failed install may leave a partial
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

The committed suite is the authority for this wrapper, and it is **executable in CI**: one step of
`.github/workflows/ci.yml`'s always-run `gate-scripts` job runs it on every PR and on every push
to `main` (GATE-STEP-LOCUS option (a), 2026-08-27 -- it lived in `changes` before), pinned by the
`the_stub_suite_runs_in_the_always_run_gate_job` codegen test — carried forward unchanged by the
chain head `RETRIEVAL-QMD-ROWS`, and first authorized 2026-08-24 by the superseded `RETRIEVAL-QMD-CI`.
**Name the locus by that pin test, never by a job name** — the job name has already drifted once and
the record that named it went stale in three places. Re-run it locally after any wrapper change:

```
DECISION_LOOKUP_ALLOW_DIRTY=1 bash .claude/skills/decision-lookup/scripts/stub-tests.sh
```

**The variable is not optional in that loop.** Before running a single case, the suite compares all
FOUR gate scripts (`stub-tests.sh`, `decision-lookup.sh`, and the two `.claude/hooks/register-check*`
scripts) against their committed blobs — at `$GITHUB_SHA` in CI, at `HEAD` locally — and refuses to
report if any of them drifted: the overwrite class a review planted green twice. So the moment you
edit the wrapper or this suite, a bare invocation exits 1 with
`FATAL: ... differs from the committed blob at <ref>` and **zero cases run**. `DECISION_LOOKUP_ALLOW_DIRTY=1` opts out of that comparison for the edit-and-re-run loop and
nothing else. CI invokes the script with no opt-out, and the codegen pin forbids the variable as a
CI `env:` key at every scope, so the CI path cannot be talked out of verifying.

The same applies on a host that keeps `git` or `tr` outside `/usr/bin:/bin:/usr/local/bin` — the
block pins that PATH deliberately, so that neither can be sent to a shim, and exits 1 if either is
absent. Nix and some containers will need the opt-out for that reason alone. (`cmp` was required
here until the comparison became object-id against object-id; the sentence outlived the dependency
by two commits, which would have sent a maintainer on a `git`-but-no-`cmp` host to opt OUT of the
supply-chain gate on a host where it runs fine — the exact false refusal removing `cmp` was
justified by, re-entering through the doc. Review #15.)

**`make stub-tests`** is the interactive entrypoint and passes the opt-out for you. It exists
because the trap above was fixed for `make hooks-test` and `workflow.md` and left open for the very
suite this skill is about: a maintainer editing the wrapper ran the bare command, got
`FATAL: … differs from the committed blob` with zero cases run, and had to have read this section
to know why. Use the target while editing; CI runs the bare command, default-on, on purpose.

**59 cases** — the 19 existing behavioral cases retained (with limited harness adaptations for
repository-relative execution, cache-invariance verification, and a controlled-PATH rework of the
bun-absent install case), plus 1 search-failure case (now also asserting the cache wipe),
5 quoting cases, 4 python3-preflight cases (absent and broken installs non-zero before any
network touch and with no install dir; absent and broken lookups fall back cache-untouched),
1 corpus-mask case, 1 stamp/archive-SHA case,
2 broken-cache cases, 2 post-update index-assertion cases, 1 stamp-write-failure case,
9 corrupt-index/probe cases (garbage index rebuilt, and rebuilt too under interpreter stdout
noise — the verdict is dispatched on the exit status, never on captured output; a planted `-wal`
survives a hit byte-identical — the probe never writes; a poisoned sqlite3 module, an unknown
probe exit, a call-site `TypeError` that is not a `sqlite3.Error`, a module with no `Error`
attribute or an `Error` that is not a class, and a non-UTF-8 cache path all still serve the
stamped hit cache-untouched — only the
deliberate exit-1 verdict wipes), and 9 lockfile-binding cases (the extracted real
`qmd_lock_binding_ok` against fixtures: valid binding; right version with the digest on
another package; tampered digest; wrong version with the recorded digest last — pinning the
version arm red; the digest present but NOT as the final element, a non-list
entry, and a one-element entry — pinning every guard of the `[pin, …, integrity]` shape
assumption red; valid JSONC trailing commas; a non-ASCII lockfile read on an ASCII-locale host —
pinning the verdict's locale independence) — all against a temporary `DECISION_LOOKUP_HOME` with fake `bun`/`qmd`
executables; the real repo `.qmd/` is never created, never modified (a before/after fingerprint
asserts it) and never depended on, and no package is installed. **One case is host-gated**: the
non-UTF-8 cache path is unconstructible on filesystems that enforce valid UTF-8 names (macOS/
APFS), where it prints a named `SKIP` and does not count as a failure — a Linux-only case, and
the ONLY skip the suite allows; every other precondition stays a loud failure. Coverage:

1. **Syntax**: `bash -n .claude/skills/decision-lookup/scripts/decision-lookup.sh`.
2. **Lookup cache miss falls back**: fresh cache home (no tool) + a query → fallback text, exit 0.
3. **Install without Bun exits non-zero**: `PATH` without `bun`, `--install` → "ACTIVATION
   FAILED", exit ≠ 0, with the remove-`.qmd/`-before-retry message. **Install with Bun but
   without python3, and install with a python3 that resolves but cannot start** → the named
   "python3 not usable" preflight failure, exit ≠ 0, same reversal message, no install dir
   created and no network touched (both install-preflight cases use a stub bun so even a
   preflight-less mutant cannot reach the network from inside the suite).
   **Lookup without python3, and lookup with a python3 that resolves but cannot start** (seeded
   healthy cache, controlled PATH) → the named preflight fallback, exit 0, and the cache
   fingerprint (paths, sizes, mtimes) byte-identical — never a wipe or rebuild.
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
   The probe itself never writes: a garbage `index.sqlite-wal` planted beside a healthy stamped
   index survives a cache-hit lookup byte-identical (a read-write connect would have run WAL
   recovery and deleted it). Probe unavailability or failure is neither corruption nor a
   refusal: with the sqlite3 module poisoned (PYTHONPATH shim raising ImportError → exit 2, or
   hard-exiting 7 → an unknown code), a shim whose `connect()` raises `TypeError` (a call-site
   failure, not a `sqlite3.Error`), or a non-UTF-8 byte in the cache path (URI construction
   raises in the unavailable arm), the stamped hit is accepted — candidates print, exit 0,
   cache fingerprint byte-identical — as is a module with no `Error` attribute at all, which a
   bare `except sqlite3.Error:` would turn into a wipe decided by a missing attribute. Only the
   probe's deliberate exit-1 verdict wipes — and it still wipes under interpreter stdout noise:
   a corrupt index with a printing `sitecustomize.py` on `PYTHONPATH` is still rebuilt (planted
   red: the **combined** mutant — command-substitution dispatch AND unsilenced probe stdout —
   leaves the garbage index in service; each half alone is still safe, so the two are one
   defense, not two).
11. **Bash-safe fallback rendering**: for a double quote, `$()`, backticks, a newline and a
   leading hyphen, the rendered `rg` command is executed under **Bash — the emitted command's
   documented target shell** — against a recording `rg` stub: the exact query must arrive
   verbatim as ONE argv element and no side-effect sentinel may appear; the `$()`/backtick cases
   additionally assert in the RENDERED TEXT that the payload is escaped as data (no unescaped
   `$(` or backtick). The guarantee is Bash-quoting only — no claim for other shells.
12. **Structural lockfile binding** (the shipped `qmd_lock_binding_ok`, extracted verbatim): a
   valid `[pin, …, integrity]` entry and a JSONC lockfile with trailing commas pass; the digest
   on ANOTHER package (the discriminator the two old greps could not provide — that fixture
   passes the old grep logic), a tampered digest, a wrong version with the recorded digest last
   (the version arm), the digest present but not final, a non-list entry, and a one-element
   entry all fail. A non-ASCII lockfile read on a genuine ASCII-locale host still passes —
   the verdict is locale-independent (planted red: dropping `encoding="utf-8"` fails there). The
   ASCII-locale precondition asserts the property directly — a locale-dependent `open()` of that
   fixture must actually raise — never a codeset-string compare, which aliasing would defeat.

## What this skill must never grow without a NEW decision row

GraphRAG, vector/semantic search, embeddings, model downloads, reranking, query expansion, hybrid
modes, **MCP or any server**, **hosted services**, **credentials**, incremental index maintenance,
hooks, **any other CI or workflow change that references or serves this
integration** (this clause governs the QMD surface, not unrelated CI work), validator or
agent-contract changes, `settings.json` changes, decision-register **semantic** changes, generated-spec
changes, production code, any mandatory-workflow rule, and any widening of package, version,
permissions or dependency shape.

**The retrieval surface is a LOCAL CLI over a gitignored, disposable cache.** No server, no daemon,
no network path on the lookup path — and nothing in the 2026-09-01 row-indexing decision creates
one. Row indexing widened the **corpus**, and only the corpus.

**The one CI change that IS authorized** (`RETRIEVAL-QMD-ROWS`, the chain head, `ADR-20260901-025538`;
first authorized 2026-08-24 by the superseded `RETRIEVAL-QMD-CI`/`ADR-20260824-205911`): the single
`bash .claude/skills/decision-lookup/scripts/stub-tests.sh` step in the job pinned by
`the_stub_suite_runs_in_the_always_run_gate_job`, plus that codegen test. It tests the **wrapper** — it
runs no QMD, installs nothing, and never touches a live `.qmd/` cache. Anything else in CI still
needs a new row. **The row-indexing decision's CI diff is zero lines**: the suite gained cases, not
a step.
