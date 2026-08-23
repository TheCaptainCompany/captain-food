# PROP-20260822-171212 — QMD Phase 0: a scriptless, BM25-only, advisory retrieval experiment

- **Status**: **Decided** (founder, 2026-08-22) — a **minimal, advisory, BM25-only QMD integration** is authorized: the `decision-lookup` skill plus one wrapper, and nothing else. See the Integration decision record below for the sandbox-spike evidence, the narrow boundary, the non-goals, and what still requires separate approval. The row `RETRIEVAL-QMD` is `decided` with this proposal as `decided_by`.
- **Date**: 2026-08-22
- **Decision row**: `RETRIEVAL-QMD` (decided; this proposal is its option-space authority and deciding record — the row is a compact index and link)
- **Tracking issue**: PENDING — created immediately before this document's landing commit, its real number inserted here in that same change (ADR-20260724-143000; a guessed number would be the dead-reference anti-pattern PROP-20260818-013222's own header documents)
- **Realized by**: (filled at completion)
- **Base**: `main` @ `7c6f0bf` — the supply-chain evidence in §2 was gathered 2026-08-22 with this as the repository head
- **Authority statement**: this proposal proposes. `docs/decisions/<KEY>.yaml` rows remain the sole decision authority; QMD output is **neither controlling nor evidence — a candidate pointer with no citation standing of any class** (evans lens, adopted).
- **Landing mechanics** (architect lens): key grammar + required fields per `docs/decisions/README.md`; `make generate` in the same commit (the DECISIONS.md index is a GENERATED region — `check-drift` is red otherwise).

## Integration decision record (founder, 2026-08-22)

**Sequence of the decision, recorded accurately**: the founder authorized an isolated sandbox smoke spike (nine commands, `/tmp/qmd-audit`, scriptless pinned `@tobilu/qmd@2.8.3`, deleted after; the repository verified clean before and after), briefly directed a deferral behind [#556](https://github.com/TheCaptainCompany/captain-food/issues/556), then **reversed it the same day before the deferral landed**: the spike showed enough value to justify the smallest repository integration now, the objective being to improve agent reflection and recall of committed repository knowledge before further product work. holub's recorded sequencing dissent (§16) remains visible; the founder decided with it in view.

**Smoke-test result, recorded accurately (evidence, not benchmark)**: scriptless pinned BM25 installed with all lifecycle scripts blocked ("Blocked 6 postinstalls") in 18.1 s; indexed 337 Markdown files in 2.2 s; five test queries at ~0.3 s each returned ranked, scored, path-anchored candidates that surfaced the deciding ADR first on three of five questions and beat flat `rg` lists on the vaguest query. Its default Markdown mask **excludes `docs/decisions/*.yaml`**, so it cannot discover or resolve authoritative rows; **both QMD and `rg` missed the controlling record on the docs-only-CI citation case**; and the corpus was contaminated by this proposal's own example transcripts — no performance claim transfers without a clean, contamination-excluded benchmark.

**What is authorized (all of it, nothing more)**: `.claude/skills/decision-lookup/SKILL.md` + `.claude/skills/decision-lookup/scripts/decision-lookup.sh` — local, scriptless, exact-pinned QMD BM25 over the approved committed-Markdown corpus; at most three candidates with path and short excerpt; a fixed advisory disclaimer on every output; mandatory direct reading of candidates and exact `docs/decisions/<KEY>.yaml` resolution before any decision assertion or founder question; `rg + aliases` fallback whenever QMD is unavailable, stale, or empty; **no result is never evidence of "undecided"**.

**Non-goals (each requires a new decision)**: GraphRAG (the [#643](https://github.com/TheCaptainCompany/captain-food/issues/643) deferral stands untouched), vector search, embeddings, model downloads, reranking, query expansion, MCP or any server, hosted services, credentials, hooks, CI changes, validator changes, agent-contract changes, YAML decision-row indexing, and any mandatory-workflow rule — the skill is advisory and non-blocking. **QMD is not memory and not decision authority**: it retrieves candidates from committed Markdown; a result or absence of a result never decides anything; `rg + aliases`, direct source reading, exact row resolution, and the AskUserQuestion gate are unchanged and remain the authority path.

**Activation and rollback condition (founder, 2026-08-22 — part of the decision)**: this decision adopts the **integration design, not a proven working repository installation**. The first controlled `decision-lookup.sh --install` is a **required activation test**, run separately after this design lands, under the documented scriptless protocol, with activation evidence reported. **If install, scriptless BM25 indexing, or the required fallback behavior fails, the integration is not silently repaired or widened**: the failure is recorded and a new/reversal decision opens before any change to package, version, permissions, or dependency shape. The successful sandbox spike is evidence; it is not proof that the repository wrapper works. The cache is project-local `.qmd/` (`tool/`, `corpus/`, `index/`), gitignored and claudeignored — derived, disposable, never authoritative; before every lookup the wrapper compares the cached corpus revision with `git rev-parse HEAD` and on mismatch discards and rebuilds corpus and index from `git archive HEAD` (the working tree is never indexed; a failed rebuild yields the `rg + aliases` fallback, never stale output).

**Separate approval still required**: executing the activation test (the first `--install` run) in any environment; any corpus-mask widening; any Phase-1-type capability.

## 1. Question and context

Agents repeatedly spend time re-finding controlling decision records across `docs/adr/`, `docs/proposals/`, and `docs/decisions/`. The working system today — and the system that remains if this proposal is rejected — is **`rg` + the alias protocol + direct structured-register resolution**: search terms and their recorded aliases via ripgrep, then resolve the row file and **read the controlling record directly before acting**. That baseline is authoritative, deterministic, offline, dependency-free, and stays mandatory regardless of this decision.

The question: may a **local, disposable, read-only, advisory BM25 index** (QMD) be trialled as a candidate-retrieval accelerator in front of that baseline — under the constraint that it must **earn its place through a pre-registered benchmark** and dies on any failure?

**Wording fixed by the founder (2026-08-22, binding here):** `rg + aliases + direct row resolution` is the **authoritative fallback and benchmark control**. QMD is an **optional advisory retrieval accelerator**.

**The premise is marked `UNVERIFIED input`** (holub + business lenses, adopted; ADR-20260817-105845): no counted evidence yet exists that rg+aliases misses have cost real sessions anything. Phase 0 therefore measures the **before-number first** — register-check searches per session and their cost at a named SHA (the same re-derivation method PROP-20260818-013222 used) — and the metric that decides whether the experiment *mattered* is not hit-rate but **register-check and citation misses in real sessions** (a controlling record existed, the search trail came back negative, the question was re-asked). A +15pp benchmark win that does not move that number is a tool that works on a problem that does not exist.

**Sequencing clause** (holub lens, adopted; framing corrected by the founder 2026-08-22): Phase 0 is **not an exception to [#643](https://github.com/TheCaptainCompany/captain-food/issues/643)** — QMD BM25 retrieval is neither GraphRAG nor graph engineering, and the [PROP-20260818-013222](PROP-20260818-013222-graph-engineering-for-the-team-workflow.md) deferral (*"we will not apply it yet we will finish what we have started first"*) stands untouched, controlling any future graph proposal on its own. Phase 0 is a separate, bounded advisory experiment that nonetheless **does not displace [#556 "Local acceptance harness"](https://github.com/TheCaptainCompany/captain-food/issues/556)** and consumes no scheduled mob checkpoint or coordinator dispatch slot before one order flows end to end. holub's dissent and the resemblance he named are recorded in §16 and were visible to the founder at his 2026-08-22 in-principle acceptance; the decision itself is recorded only on the row.

## 2. Current evidence and its limits

A read-only supply-chain assessment of `@tobilu/qmd@2.8.3` (2026-08-22, conversation record; to be attached to the tracking issue on approval) established from primary sources:

- **Pinned artifact**: `https://registry.npmjs.org/@tobilu/qmd/-/qmd-2.8.3.tgz`, integrity `sha512-zjfVwrObPB618B6x8SdhlGv/tX9OxRHsbQnr5DUtBvqPK6HGQ27lM+9/BAY5okpjrHVnW56hLyDkqoTcsrVLzA==`, shasum `7e1515b1daf349a1dc88cff53da925d34f89c948`, 53 files, 914,522 bytes; MIT; maintainer `tobilu <tobi@lutke.com>`; released 2026-08-16; repo 29.1k stars, active.
- **Script-bearing packages in the direct graph**: `node-llama-cpp 3.20.0` (`postinstall`; its tree carries `cmake-js`/`simple-git`/`ipull` — fetch/compile/execute capability if ever allowed to run) and the four tree-sitter grammars (`install: node-gyp-build`). `better-sqlite3 13.x` and `sqlite-vec 0.1.9` ship native artifacts **inside integrity-hashed tarballs with no lifecycle scripts**; the remaining direct deps are scriptless pure JS/wasm.
- **BM25-path independence, established by reading the shipped code** (`dist/llm.js`, `dist/db.js`, `dist/cli/qmd.js`): `node-llama-cpp` is imported **only** via dynamic `import()` inside model functions with zero top-level side effects; under Bun the SQLite driver is the built-in `bun:sqlite` (not `better-sqlite3`); `sqlite-vec` loads lazily and its failure is handled with "BM25 and other operations are unaffected"; tree-sitter loads only via dynamic AST paths.
- **Bun's script policy** (official docs): lifecycle scripts do not run by default; `trustedDependencies` **replaces** the default allowlist; `--ignore-scripts` disables everything.

**Limits of this evidence — stated per the founder's correction:** source-code reading is **not** proof of runtime behavior. Registry metadata proves artifact identity, not behavior. The three runtime acceptance gates in §11 exist precisely because these claims must be proven by execution inside the audit sandbox before any benchmark number is believed. The full transitive dependency graph is enumerable only from the install-time lockfile (an audit artifact, §6.5). No performance or cost number for this repository exists yet, and none may be stated before it is measured here (ADR-20260817-105845). The problem-size premise itself is unverified (§1) until the before-number exists.

## 3. Tool-selection principle and candidate record

**Principle (founder-directed, 2026-08-22, verbatim):**

> Captain.Food prefers maintained Rust implementations for local development and agent tooling when they satisfy the required functional, operational, reproducibility, supply-chain, and maintenance contracts at equal or lower total cost. Rust is a preference, not a waiver: language choice never overrides evidence of maintenance, documented behavior, reproducible installation, bounded machine-readable output, or least-privilege supply-chain posture.

**Decision hierarchy (in order):**
1. Extend an existing repository-owned Rust tool only when retrieval remains clearly outside validator/decision-authority semantics and the added maintenance is justified.
2. Prefer a maintained, pinned Rust CLI with documented JSON output, local/disposable storage, offline operation, release artifacts or verifiable source builds, and an acceptable supply-chain path.
3. Use a non-Rust local tool only when no Rust candidate satisfies those same contracts and the non-Rust tool demonstrably meets the Phase-0 benchmark and supply-chain requirements better.
4. Never choose a tool solely because it is written in Rust, JavaScript, Bun, Node, or any other language.

Scope guard: this principle governs tool **selection**. It is not a mandate to replace existing working tools, and Bun's own implementation language is irrelevant to the trust posture of packages it installs.

**Candidate record (accurate, evidence-based):**
- **`qntx/qmd` (Rust crates `qmd`/`qmd-cli` 0.3.2) — REJECTED for Phase 0** on maintenance, documentation, release-artifact, and supply-chain evidence, **not because it is Rust**: single pseudonymous publisher; the complete release history inside one 48-hour window (2026-02-02/03) followed by ~6.5 months of silence; zero GitHub release artifacts; an opaque third-party install domain; undocumented index format and machine-readable output. Re-evaluate only on a demonstrated maintained release stream.
- **`@tobilu/qmd` — PROVISIONAL non-Rust candidate**, pinned exactly at `2.8.3`. **Not approved for installation**; carrying it into Phase 0 is what row `RETRIEVAL-QMD` decides.
- **`rg + aliases + direct structured-register resolution` — the approved baseline and fallback**, and the benchmark control.
- If no acceptable installation path is established, **Phase 0 is rejected** rather than the Rust preference relaxed or supply-chain ambiguity accepted.

## 4. Phase-0 boundary — SANDBOX-ONLY (founder approval rider, 2026-08-22)

**Phase 0 authorizes only an isolated audit-sandbox experiment outside the repository.** Everything Phase 0 creates — the wrapper/harness, the package installation, the lockfile, the `.qmd/` configuration and index, the measurement log, the benchmark fixtures, and the rollback — lives **only inside the isolated audit directory** (`/tmp/qmd-audit/`). **The repository remains clean before and after the experiment**, asserted by `git status --porcelain` at every evidence checkpoint.

**Phase 0 does NOT authorize adding to Captain.Food**: a wrapper script; a QMD skill; `.qmd/` configuration or index; `.gitignore`/`.claudeignore` entries; Makefile targets; any `.claude/**` file; agent instructions; hooks, CI, validator, decision-register, or generated-document changes. **Exit deliverable**: after the three runtime gates and the benchmark pass, a short evidence report plus an **exact proposed repository diff** — and a **separate founder decision** is required before any QMD skill, wrapper, repository-local index configuration, or workflow integration is added to Captain.Food.

**In scope (all of it sandbox-local, disposable, advisory):** one isolated scriptless install; `qmd init` inside the sandbox; BM25-only indexing of the corpus copy (§6); `qmd search --json` behind one sandbox wrapper script implementing the contract in §8; the pre-registered benchmark (§10); the three runtime acceptance gates (§11); the non-severable measurement design (§9); the before-number baseline measurement (§1, §10).

**Standing invariants** (farley lens, adopted; strengthened by the rider): the experiment lives **outside the repository entirely** — nothing of it is tracked, so it cannot ride CI and no gate verdict input changes. **The moment anything tracked references the wrapper, qmd, or the sandbox — any tracked file, any `.claude/**` config, any Makefile target — Phase 0 is over by definition**; the audit script carries an executable `grep -r` detector for exactly this plus the repo-cleanliness assertion, run at every evidence checkpoint, so load-bearing-ness has a positive detector rather than depending on someone noticing an incident.

**Non-goals — every one of these requires a NEW decision row, none is a fallback:** model downloads of any kind; vector/semantic search; reranking; query expansion; hybrid modes; the QMD MCP server or any server process; GraphRAG (§13); hosted or external services; credentials; hooks; CI changes; `.claude/agents/**` or agent-contract edits; `settings.json` changes; decision-register semantic changes; generated-spec changes; production code; incremental index maintenance (young lens: "freshness pressure is how a cache acquires authority" — rebuild-only, §6); any mandatory-workflow rule (the skill is **advisory** in Phase 0 — it creates no obligation to use it).

## 5. Supply-chain protocol

**5.1 The invariant (verbatim, binding):**

> No lifecycle script is authorized. A Phase-0 command whose BM25 path requires one fails the experiment; it does not widen `trustedDependencies`.

Failure of any acceptance gate likewise must not trigger a permission grant, a package substitution, a vector-mode fallback, or broader scope without a new decision.

**5.2 Two independent enforcement layers, both used and both tested:** `trustedDependencies: []` in the sandbox `package.json` (declarative: replaces Bun's default allowlist with the empty set, so no package's scripts are trusted even if a future Bun version changes flag behavior) **and** `--ignore-scripts` on every install invocation (imperative: disables script execution for that command regardless of the manifest). They fail independently — a forgotten flag is caught by the manifest, a mangled manifest by the flag — and gate T2 (§11) verifies the combination by asserting zero script execution evidence after install. A third declarative layer, `bunfig.toml [install] ignoreScripts = true`, sits in the sandbox config.

**5.3 Minimized allowlist environment** — not called "fully scrubbed"; it is an explicit allowlist over an environment whose inherited configuration is enumerated first:
- **Allowed environment variables (exactly)**: `PATH` (current value, recorded), `HOME=/tmp/qmd-audit/home` (fresh), `XDG_CACHE_HOME=/tmp/qmd-audit/xdg-cache`, `XDG_CONFIG_HOME=/tmp/qmd-audit/xdg-config`, `XDG_DATA_HOME=/tmp/qmd-audit/xdg-data`, and the effective proxy set below. Everything else is dropped (`env -i`).
- **Effective proxy variables**: `HTTPS_PROXY`/`HTTP_PROXY`/`NO_PROXY` as configured by this environment (recorded verbatim in the evidence file), plus the CA bundle variables the proxy requires (`SSL_CERT_FILE`/`NODE_EXTRA_CA_CERTS` pointing at the environment's CA bundle, path recorded). All egress rides the observed proxy; TLS verification is never disabled.
- **Certificate/config-file locations recorded**: the CA bundle path; presence/absence and content-hash of any `~/.npmrc`, `/etc/npmrc`, and `bunfig.toml` reachable from the *original* environment (enumerated before the run), and the guarantee that the sandbox `HOME`/`XDG_*` contain none except the sandbox `bunfig.toml`.
- **Bun/package-manager configuration sources**: Bun reads `bunfig.toml` from the project directory and `$HOME`; both locations in the sandbox are fresh and empty except the sandbox `bunfig.toml` declaring `[install] ignoreScripts = true`. npm is not used.
- **Installation evidence recorded** (the evidence set, kept per §6.5): the exact command line; the enumerated pre-run configuration; the proxy egress log for the install window; `bun.lock`; the native-artifact inventory (§5.4); stdout/stderr of the install; the post-install absence-of-script-execution checks (T2).

**5.4 Native artifacts do not disappear because scripts are disabled** (founder correction, adopted): downloaded tarballs can and do contain native `.node`/`.so`/`.dylib` files — `better-sqlite3` and the `sqlite-vec-*`/`@node-llama-cpp/*` platform packages ship them. The audit therefore, **before first execution**: inventories every native binary under `node_modules` (`find … -name '*.node' -o -name '*.so' -o -name '*.dylib'`), binds each file to its owning package, version, and lockfile integrity entry, and records which native binary the BM25 path under Bun actually loads — expected answer: **none from npm packages** (`bun:sqlite` is Bun-internal); gate T3 (§11) proves it at runtime. A binary that cannot be bound to a lockfile entry fails the audit.

**5.5 Evidence layers, kept separate** (per the founder's correction — one layer never substitutes for another):
1. **Registry/tarball integrity**: lockfile sha512 entries match the digests recorded in §2.
2. **No lifecycle-script execution**: manifest + flag + bunfig (§5.2), verified by T2.
3. **Permitted install egress**: proxy log for the install window shows `registry.npmjs.org` only.
4. **No operation-time egress**: proxy log for the index/search window shows zero outbound requests.
5. **Runtime proof**: `qmd init`, BM25 indexing, and `qmd search --json` succeed **without loading disallowed optional modules** (T1/T3) — reading the source predicted this; only execution proves it.

**5.6 Version discipline**: `bun add --exact @tobilu/qmd@2.8.3` (the literal versioned spec — a missing/yanked version **fails the command**; it never resolves to a newer release); subsequent installs use the frozen-lockfile mode so drift fails loudly. The exact frozen-lockfile and omit-optional flag behavior is acceptance-tested (T2), not assumed.

## 6. Corpus, storage, and artifact policy (sandbox-only)

- **6.1 Corpus (read-only input)**: `docs/**` + `CLAUDE.md` + `specs/**` markdown — the governed decision/record surfaces — **exported into the sandbox at the pinned commit SHA** (`git archive <SHA> -- docs CLAUDE.md specs | tar -x -C /tmp/qmd-audit/corpus`), so indexing never touches the working tree and the corpus is byte-identical across reruns (a floating corpus makes every number unrepeatable). **Re-index cadence** (architect + young lenses): manual full rebuild only, each rebuild re-pins the SHA; **never incremental maintenance**.
- **6.2 Nothing enters Git or Claude context** (rider): the `.qmd/` configuration and index live **inside the sandbox** (`/tmp/qmd-audit/work/.qmd/`), not in the repository — so no `.gitignore`/`.claudeignore` entries exist or are needed in Phase 0. Ignore entries appear only in the **proposed repository diff** delivered at Phase-0 exit, for the separate integration decision. The repo-cleanliness assertion (`git status --porcelain` empty) is a planted-red test case (§11).
- **6.3 Storage discipline** (dba lens, adopted): the index size bound is stated **with its antecedents** — corpus bytes at the pinned SHA × an FTS5 expansion factor of roughly 0.5–1.5× source text + SQLite page/WAL overhead; the builder enforces a hard cap derived from that arithmetic and runs a **pre-build free-space check** (the ephemeral-container disk allowance fails hard at exhaustion — `docs/claude/sessions/environment.md` §2; this repo already paid for skipping this once, SIRENE at 655 MB/77% before #231). **Repair is forbidden by policy**: any open/corruption/version-mismatch error on the index ⇒ delete wholesale and rebuild from the pinned corpus — this is derived data with a replay-restore story by construction; no one writes a repair path. `journal_mode=OFF` or `MEMORY` during the one-shot build is legitimate because durability is explicitly not a goal.
- **6.4** The index lives at the sandbox `qmd init` location, never in `~/.cache` (the sandbox `XDG_CACHE_HOME` catches any leak; a non-empty sandbox cache after a run is an audit finding).
- **6.5** Nothing under `.qmd/` or the audit directory is ever citable: citations resolve to `docs/adr/`/`docs/proposals/` files only (validator §23 enforces this — the resolver knows only those directories, by construction). **The audit-directory lockfile is an EVIDENCE ARTIFACT, not a repository dependency lockfile.** It is **not** committed to Captain.Food unless a later approved decision explicitly chooses that. Retention: the evidence set (lockfile, inventories, egress logs, command transcripts, the **dated** rollback-proof run) is retained in the audit directory for the life of the experiment plus 30 days after the Phase-0 decision, then deleted by the rollback procedure (§12); if Phase 0 is approved to continue, the evidence set is archived as an attachment on the tracking issue before the sandbox is deleted.

## 7. Architecture: a skill, not an agent — and a projection, not an authority

Per the founder's standing correction: the abstraction is the **`decision-lookup` retrieval skill plus one local wrapper script** — no dedicated agent, no graph, no vector store, no MCP server. A dedicated agent may be reconsidered only on **measured** evidence that repeated multi-step decision research cannot be handled through the skill contract; that reconsideration is a new decision row.

**QMD is declared a projection of the register** (evans + young lenses, adopted), inheriting the decided rule verbatim by reference: projection/record disagreement is a **staleness report, not a conflict** (`docs/claude/sessions/workflow.md`). The architecture of authority is unchanged:
- **QMD**: advisory candidate retrieval only — a disposable projection; **no hook, validator, gate, or ask-envelope ever consumes index output** (the same rule that has `register-check.sh` reading row FILES, never the generated index).
- **`docs/decisions/<KEY>.yaml`**: authoritative decision status and relationship resolution. The staleness mitigation works **because** the register resolves forward at direct-read time (`superseded_by` chains) — a property of the register's design, not of the index; it breaks if anyone ever consults a snapshot of row content inside the index instead of the file at HEAD, which is therefore forbidden.
- **Direct reading of the controlling record**: required before acting — always. **A record id may enter a `Register check:` trail only after a direct read of that record; the ADVISORY banner never survives into a trail** (evans lens — the trail's `nearest:` slot is exactly where a candidate could become a citation without a read; this sentence closes that path).
- **Negatives never come from the index** (young lens, adopted): a "no controlling record" assertion may only be sourced from a direct search of `docs/decisions/` + the record corpus **at HEAD** — never from a QMD miss, whose corpus is pinned at an older SHA by design. **A negative trail sourced from a stale index is a false-authority incident** under the kill criteria (§12): a row opened after the pinned SHA is invisible to the index, and a retrieval miss is indistinguishable from "no record exists".
- **The existing `AskUserQuestion` register-check gate**: deterministic enforcement (untouched by this proposal).
- **`rg` + alias protocol**: mandatory fallback and baseline. **QMD consumes the same alias table by name** (evans lens): the alias table is the declared published language for search; if QMD searched its own vocabulary, the negative trail's `terms:` clause would be ambiguous about which language was searched and the control would stop being a control.

## 8. Wrapper input/output contract

- **Input**: one query string; optional `--k N` (default 5, max 20; out-of-range or non-numeric values **clamp and proceed** — graphql lens — consistent with "never refuse, never retry-with-privileges").
- **One envelope, every path** (graphql lens, adopted — a shape-flip on the unavailability path would make a healthy fallback parse as a crash): stdout is always the same bounded JSON:
  ```json
  {"advisory": true,
   "banner": "ADVISORY ONLY — candidates, not evidence. Resolve and READ the controlling record (docs/decisions/, docs/adr/) before acting. Baseline: rg + aliases.",
   "candidates": [{"kind": "candidate", "path": "docs/decisions/REFUND-BEARER.yaml", "record": "REFUND-BEARER", "score": 0.91}],
   "unavailable_reason": null,
   "fallback": ["rg -n -i \"<actual query>\" docs/ specs/ CLAUDE.md",
                 "check docs/claude/sessions/workflow.md aliases for <actual query>",
                 "resolve the row under docs/decisions/, then READ the controlling record"]}
  ```
  This **refines, and does not contradict**, the founder's "must print the exact fallback" requirement: the exact fallback commands are printed on every invocation, inside the envelope, **with the user's actual query substituted** — no placeholders, no cwd-dependent paths (ux lens).
- **Contract rules** (graphql + ux lenses, adopted): the JSON shape evolves **additively only** — keys are never removed or renamed, consumers ignore unknown keys; `score` is **ordinal within a single response only**, higher = better, never comparable across queries or versions — no threshold on it may become load-bearing; `path` is **repo-root-relative POSIX**; every candidate row carries `"kind": "candidate"` and the resolvable `record` id + path, so a single extracted line still self-declares as advisory and the mandated direct read is the cheapest next action (one copy-paste).
- **Banner discipline** (ux lens): the banner is present on **every** invocation, is **not suppressible by any flag in Phase 0**, and is the first element of the envelope.
- **Unavailability**: same envelope, `candidates: []`, `unavailable_reason` set to one of `"not-installed" | "index-absent" | "index-stale" | "index-error"` (naming *why*, because "fix it or ignore it" are different next actions), the `fallback` commands carrying the actual query. Exit 0. The wrapper **must not** install anything, download anything, call any other external tool, or retry with different privileges — silent auto-repair is the ADR-20260810-231300 silent-fallback class and is a planted-red test.
- **Zero-candidates empty state** (ux + young lenses, adopted — the most expensive wrong belief this tool can create is a trusted "nothing exists"): `candidates: []` with a `banner` extension stating verbatim that **an empty advisory result is NOT evidence of absence** and that a "no controlling record" conclusion requires the fallback search at HEAD; the fallback commands are always present in the envelope.
- The wrapper is a **sandbox artifact** (`/tmp/qmd-audit/bin/decision-lookup`, rider) — never a repository file in Phase 0; the repository version, if ever, arrives via the proposed diff and the separate integration decision. It never writes outside the sandbox (index + opt-in measurement log, §9) and never reads or transmits credentials.

## 9. Measurement design (opt-in write, sanitized, local — a NON-severable Phase-0 condition)

**Non-severability (founder ruling, 2026-08-22)**: this measurement design is a condition of Phase 0, necessary for the benchmark to be auditable. There is no "run without durable evidence" option. If the founder objects to the specific retention rule below, the ask is for a **replacement retention rule before Phase 0 starts** — not a waiver.

- **Off by default.** Enabled only by an explicit environment variable for a session. **Coverage is counted** (observability lens): the evidence set records logged-sessions vs total-sessions so the opt-in bias is bounded and visible in the gate decision; the case-authoring procedure is declared blind-before-exposure or the "blind" label is not used.
- **Location**: a sandbox-local file (`/tmp/qmd-audit/measurement.log`, rider) — never inside the repository in Phase 0.
- **Row schema** (observability lens, adopted — the wide event, decomposable back to the case that hurt): `timestamp, run_id (pairing key joining a qmd row to its rg control row for the same query), case-label (from the fixed benchmark/alias vocabulary — never free text), tool (qmd|rg), outcome (hit|miss|tool-error|index-stale — technical failure never conflated with a true miss), hit-rank (with the rank horizon k declared so "miss" has a well-defined denominator), corpus-sha (per row — a mid-window reindex otherwise makes rows unattributable), index-built-at-sha vs HEAD-at-query (staleness incidents are a Phase-1 input), wall-ms`. Latency is reported as raw rows or percentiles, **never an average**.
- **The decision outlives the rotation** (observability lens): the Phase-1/continue decision is made from a **frozen export** of the rows (or within the retention window) — a gate verdict whose antecedent rows have rotated away would be a derived number without antecedents (ADR-20260817-105845).
- **Sanitization is the data-minimisation artifact**: raw queries are never written; the design goal is a log containing **no personal data at all**.
- **Privacy posture** (legal lens, 2026-08-22): professional processing — the Art. 2(2)(c) GDPR household exemption does not apply, but a local, gitignored, opt-in, sanitized log is minimal-risk; purpose = retrieval-quality measurement only; lawful basis Art. 6(1)(f) legitimate interest (the founder is effectively the only identifiable subject); no Art. 35 DPIA threshold met. **Fence**: the moment the log would leave the machine (committed, uploaded, pasted into an issue, fed to any hosted service) or capture raw queries, this posture is re-assessed as a new decision — that is a stop, not a judgment call.
- **Retention and cleanup**: 30 days rolling, enforced by the same command that rebuilds the index (delete-then-recreate `.qmd/`); the rollback procedure (§12) deletes it entirely (subject to the frozen-export rule above). No lens output here is legal advice; the one low-priority counsel question (Art. 30(5) register-of-processing derogation applicability) is queued, non-blocking.

## 10. Benchmark design (pre-registered; the control is not a straw man)

- **The before-number comes first** (holub + business lenses, adopted): before any QMD number exists, measure the baseline problem size — register-check searches per session and their cost, re-derived at a named SHA (the PROP-20260818-013222 method). Without the before-number, "it helps" is unfalsifiable and the experiment survives by default — sunk attention defending itself.
- **Hurdle, pre-registered before the first QMD run**: QMD earns Phase-1 consideration only if it achieves **top-3 hit rate ≥ control + 15 percentage points on ≥ 20 blind cases**, with zero false-authority incidents. A threshold chosen after seeing results is a rationalisation, not a gate.
- **The metric that decides it mattered**: movement in **real-session register-check/citation misses** (§1), not the hurdle alone. The hurdle qualifies the tool; the session metric qualifies the problem.
- **Scorer proven red first** (beck): the scoring harness is run with a known-bad retriever (shuffled file list) and must score it below the control before any real number is believed; a scorer that cannot distinguish random from the control invalidates the benchmark.
- **Control specified reproducibly**: exact commands (`rg -n -i --glob` invocations over the pinned corpus), the alias input set **frozen at a named commit that predates the fixture sessions** (never hand-tuned per fixture after seeing failures), corpus pinned at a named SHA, and the full command transcript kept in the evidence set. **The control is the LIVE baseline** (architect lens): rg *with the alias table*, exactly as sessions use it — the blind set includes **cross-era-vocabulary cases** (contribution/tip, delivery/rider, founder/customer) run symmetrically, or the hurdle measures against a weaker control than the one sessions actually run. Both systems consume the **same alias expansion** (§7).
- **Fixtures without leakage**: ≥ 20 retrieval cases taken **verbatim from historical session transcripts as originally typed** — never rephrased by someone who knows the answer file; expected answers = the file(s) that historically resolved the case, fixed at one granularity (file paths) and scored identically for both systems; ties and both-fail counted explicitly. The blind set includes **superseded-top-hit cases** (architect lens): queries whose best lexical match is superseded prose, scoring whether each system surfaces the CURRENT controlling record — the exact shape by which an advisory tool would mint false authority.
- **Blind evaluation**: the evaluator sees two unlabeled ranked lists (A/B) per case, never which tool produced which.
- **Determinism**: BM25 scores depend on tokenizer + corpus snapshot — the pinned SHA makes reruns reproducible; a rerun that diverges is itself a finding.
- **No informal comparison is ever citable**: "QMD beats rg" may only be uttered with this harness's output as its antecedent.

## 11. Acceptance gates and planted-red tests

**Runtime acceptance gates (execution, in the sandbox — source-reading predictions are NOT accepted as passes):**
- **T1 — scriptless BM25 works**: `bun add --exact @tobilu/qmd@2.8.3 --ignore-scripts` with `trustedDependencies: []`, then `qmd init` + index the pinned corpus + `qmd search --json` returns well-formed results. Any step demanding a lifecycle script → the invariant (§5.1) fires: experiment fails.
- **T2 — no script executed, reproducible install**: post-install checks find zero script-execution evidence (no build artifacts, no gyp output, no modified package contents vs tarball); lockfile integrity matches §2 digests; frozen-lockfile reinstall reproduces byte-identical `node_modules` inventory; the omit-optional flag behavior is recorded.
- **T3 — egress and module-loading proof**: proxy logs show `registry.npmjs.org` only during install and **zero** egress during init/index/search; runtime module tracing confirms neither `node-llama-cpp` nor any `@node-llama-cpp/*`/model path loads during the BM25 path, and records which SQLite driver actually loaded (expected `bun:sqlite`).

**Additional acceptance requirements:**
- **Delete-the-index neutrality test** (young lens, adopted): delete `.qmd/` and re-run the workflow — **only recall may change, never an answer**. If any conclusion differs with the index absent, the index was load-bearing: false-authority incident.
- **Rollback proof is dated and re-run** (farley lens, adopted): the planted-red rollback run is dated in the evidence set, and the review that decides "continue" **re-runs it first** — a continue decision on a stale rollback proof is the same as no proof.
- **Load-bearing detector**: the audit script's `grep -r` assertion (§4) — no tracked file, no `.claude/**` config, no Makefile target references qmd/`.qmd/` — runs at every evidence checkpoint.

**Planted-red tests (each seen red before trusted, per repo doctrine):**
- Unavailability: point the wrapper at a missing binary → the standard envelope with `candidates: []`, the correct `unavailable_reason`, and the query-substituted fallback commands; exit 0; **no** install/network attempt (red by deleting the fallback branch).
- Envelope stability: every path — success, empty, unavailable — emits the same JSON shape (red by re-introducing the plain-text unavailability message).
- Repo cleanliness (rider): after every experiment step — install, init, index, search, benchmark, measurement — `git status --porcelain` in the repository is empty and no repo path has a new/modified file (red by planting a file at the repo root from the harness).
- No raw-query leakage: run a query containing a unique sentinel token with measurement ON → sentinel absent from every file the wrapper can write (red by flipping a verbose flag).
- Kill-path works: execute §12 → assert `.qmd/`, the sandbox, and the wrapper are gone and the freed bytes verified writable (red by leaving one file behind).
- Banner: the advisory banner asserted verbatim in wrapper output on every path (red by deleting the banner line).

## 12. Kill criteria and rollback/deletion procedure

**Kill criteria — any single one ends the experiment:**
- failed benchmark hurdle (§10);
- unavailable/yanked package at the pinned version;
- T1/T2/T3 or delete-the-index gate failure;
- **excessive index latency, pre-registered** (farley lens — a kill criterion without a number is a discussion): cold full-index build > **120 s** or benchmark query wall-time p95 > **1000 ms** on the pinned corpus, both fixed here a priori, before any measurement exists;
- **any false-authority incident** — explicitly including (a) QMD output cited as evidence or authority anywhere, (b) a wrong-record citation traced to the tool reaching any decision surface (business lens: the confident wrong hit is the expensive failure, weighted above misses), and (c) **a "no controlling record" negative sourced from a stale index** (young lens);
- any lifecycle-script requirement on the BM25 path;
- **any unexplained egress, with the detector named** (farley lens): the environment proxy's access-log diff over the run window — not the phrase "no egress";
- **adoption floor** (business lens): fewer than **5 wrapper invocations across 10 logged sessions** after the benchmark completes → the tool is a standing tax with no benefit; rollback executes without debate;
- the load-bearing detector firing (§11).

**Rollback/deletion (executable, not prose; byte-counted per dba lens; simplified by the sandbox-only rider — there is nothing in the repository to remove):**
```bash
# after frozen-export of measurement rows if a gate decision is pending (§9),
# and after issue-archival of the evidence set if §6.5 applies:
rm -rf /tmp/qmd-audit/            # wrapper, install, lockfile, index, logs, fixtures, evidence — everything
df -h /tmp                        # freed space verified immediately writable (environment.md §2)
git -C /home/user/captain-food status --porcelain   # asserted EMPTY — the repo never held any of it
```
plus one journal line recording the kill and which criterion fired. The procedure is itself planted-red tested and its proof dated (§11). After rollback, the system is exactly `rg + aliases + direct row resolution` — which it never stopped being.

## 13. Phase 1/2 approval boundaries, and the GraphRAG exclusion

- **Phase 1 (vector/embeddings — NOT approved by anything in Phase 0)**: may only be *proposed* if Phase 0 passes all gates AND the pre-registered hurdle AND the session-miss metric moved (§10), and requires its own decision row (model downloads, disk, and a fresh supply-chain assessment are new facts). Same for reranking/query expansion. **Phase 1/2 inherit the resume-condition's spirit of [#643](https://github.com/TheCaptainCompany/captain-food/issues/643)** (business lens): finish started work first — they are not proposed while the walk (#556) remains undelivered, so this experiment never becomes the deferred graph plan re-entering through a side door.
- **Phase 2 (MCP server / any always-on process)**: own decision row; brings ADR-20260810-231300 (push/poll, liveness) into scope; nothing in Phases 0–1 creates a server. (vernon lens noted: a resident process is also where mailbox-contention smells become spellable — re-run the Ask/Tell checklist then.)
- **GraphRAG: a fixed non-goal and scope boundary — not a checkbox.** The controlling record is the founder's existing deferral: [PROP-20260818-013222 "Graph engineering for the team workflow"](PROP-20260818-013222-graph-engineering-for-the-team-workflow.md) / [#643](https://github.com/TheCaptainCompany/captain-food/issues/643), deferred verbatim — *"we will not apply it yet we will finish what we have started first."* That deferral remains controlling regardless of what is decided on `RETRIEVAL-QMD` today. Any future GraphRAG proposal requires **new evidence and a new decision row** with `reconsiders:` semantics: at minimum, Phase 1 passed its own pre-registered benchmark and ran ≥ 4 weeks without a false-authority incident, AND a measured, documented failure class exists that graph-structured retrieval demonstrably addresses and flat retrieval demonstrably cannot (named cases, counted, with the failed flat-retrieval transcripts as antecedents). Never a silent scope-widening.

## 14. Options (final vision first, per ADR-20260808-235113)

**The final vision, stated first** (architect lens — so Phase 0 reads as a gated first step, not a forbidden intermediate): a `decision-lookup` skill whose advisory retrieval is good enough that agents reach the controlling record in one step, with the register untouched as the sole authority — possibly BM25-only forever (if the benchmark says lexical is enough), possibly Phase-1 hybrid later (only if measured evidence demands it). Phase 0 is that final shape's cheapest honest test, run under gate-then-stabilize: the finished advisory skill exists behind an experiment gate, and flipping anything further is a separate recorded decision. This composition — evidence displaces proxy judgment (ADR-20260808-144738), gated not staged — is deliberate, not an intermediate step where the final step could already be built: the final step *cannot* be built yet, because its justifying evidence does not exist.

**Option A — run Phase 0 as specified above (recommended).** Pros: bounded, scriptless, reversible, evidence-producing; the baseline is never displaced; every widening needs a new decision; produces the before-number this problem space has never had. Cons: nonzero attention cost drawn from the same weekly loop budget that got graph engineering deferred (denominated in agent-minutes, not euros — business lens); a non-Rust dependency enters one sandbox; the benchmark may simply confirm the baseline (a useful, cheap outcome).

**Option B — reject Phase 0; the baseline remains the system.** Pros: zero new surface, zero supply-chain exposure; the alias protocol keeps improving with use; fully honors the "finish what we have started first" posture with no exception to record. Cons: the hypothesis (faster candidate discovery) stays untested; the before-number likely goes unmeasured; retrieval friction persists unquantified.

**Option C — defer until a maintained Rust candidate satisfies the same contracts.** Pros: aligns with the language preference at zero present cost. Cons: indefinite wait on third-party maintenance; the preference explicitly does not license waiting when a compliant candidate exists and the experiment is this bounded.

## 15. Lookup flow (sequence) and terminal transcripts

```mermaid
sequenceDiagram
    participant A as Agent (any session)
    participant W as decision-lookup wrapper (skill)
    participant Q as QMD (.qmd/ BM25 index — projection)
    participant R as docs/decisions/<KEY>.yaml + controlling record (authority, at HEAD)
    A->>W: query ("refund bearer decision")
    alt QMD available, candidates found
        W->>Q: qmd search --json (BM25, local, no egress)
        Q-->>W: ranked candidate paths
        W-->>A: envelope: banner + candidates(kind,record,path,score) + fallback cmds
    else QMD available, ZERO candidates
        W-->>A: envelope: banner + candidates:[] + "empty is NOT evidence of absence" + fallback cmds
        Note over A: a negative REQUIRES the fallback search at HEAD — never the index
    else QMD unavailable
        W-->>A: envelope: banner + candidates:[] + unavailable_reason + query-substituted fallback cmds (exit 0, no install, no retry)
    end
    A->>R: resolve row, READ controlling record at HEAD (MANDATORY — unchanged)
    R-->>A: authoritative status + record (supersession followed forward)
    Note over A,R: A record id enters a trail only after this read. The register-check gate is untouched.
```

Transcript mockups (Phase-0 target behavior; one envelope on every path):

```
$ decision-lookup "who bears refund cost"
{"advisory":true,
 "banner":"ADVISORY ONLY — candidates, not evidence. Resolve and READ the controlling record (docs/decisions/, docs/adr/) before acting. Baseline: rg + aliases.",
 "candidates":[
   {"kind":"candidate","record":"REFUND-BEARER","path":"docs/decisions/REFUND-BEARER.yaml","score":0.91},
   {"kind":"candidate","record":"ADR-20260819-103112","path":"docs/adr/ADR-20260819-103112-the-six-queue-answers.md","score":0.63}],
 "unavailable_reason":null,
 "fallback":["rg -n -i \"who bears refund cost\" docs/ specs/ CLAUDE.md",
              "check docs/claude/sessions/workflow.md aliases for: refund, bearer",
              "resolve the row under docs/decisions/, then READ the controlling record"]}

$ decision-lookup "anything"        # with .qmd/ deleted
{"advisory":true,
 "banner":"ADVISORY ONLY — candidates, not evidence. ... Baseline: rg + aliases.",
 "candidates":[],
 "unavailable_reason":"index-absent",
 "fallback":["rg -n -i \"anything\" docs/ specs/ CLAUDE.md",
              "check docs/claude/sessions/workflow.md aliases for: anything",
              "resolve the row under docs/decisions/, then READ the controlling record"]}
```

## 16. Consulted (whole roster, per ADR-20260812-143619 — consultation COMPLETE, 2026-08-22)

One line per lens; every response received before this proposal is presented for approval. Disagreements carry their disposition.

- **architect** — PROP + open row classification correct; form must cite the near-miss records (the #643 deferral rationale and holub's dissent) so approval is a knowing exception; superseded-top-hit benchmark cases; live-alias-table control with cross-era vocabulary; final-vision-first composition stated. **Adopted** (§§1, 10, 14, form).
- **beck** — scorer-seen-red; pre-registered hurdle; leakage vectors (verbatim historical queries, frozen alias commit); single-granularity scoring, blind A/B; banner as testable contract. **Adopted** (§§8, 10, 11).
- **business-specialist** — economics in agent-minutes under the weekly budget; before-number or the kill switch is unfalsifiable; confident-wrong-hit weighted above misses; adoption floor; Phase 1/2 inherit the #643 resume spirit. **Adopted** (§§1, 10, 12, 13, 14).
- **dba** — size bound with antecedents; pre-build free-space check + hard cap; delete-and-rebuild-never-repair; byte-counted rollback; ignores in the same change. **Adopted** (§§6, 12).
- **evans** — "controlling record" referenced not redefined; "neither controlling nor evidence — no citation standing of any class"; record ids enter trails only after direct read, banner never survives into a trail; QMD declared a projection inheriting the staleness-report rule; QMD bound to the shared alias table. **Adopted** (§§ header, 7, 10).
- **farley** — release path untouched by construction, plus the tracked-reference grep detector as the load-bearing tripwire; dated rollback proof re-run before "continue"; latency and egress kill criteria given numbers/named detectors. **Adopted** (§§4, 11, 12).
- **graphql-architect** — one JSON envelope on every path (recorded as a REFINEMENT of the founder's "print the exact fallback" — the commands are printed inside the envelope, query-substituted); additive-only evolution; ordinal scores; repo-relative POSIX paths; `--k` clamp. **Adopted** (§8).
- **holub** — DISSENT, recorded and not smoothed over: this resembles the deferred #643 appetite re-entering in smaller clothes; the "retrieval is a bottleneck" premise is UNVERIFIED; the metric that matters is real-session register-check/citation misses; consent conditioned on a sequencing clause (does not displace #556, no checkpoint/dispatch slot until one order flows). **Disposition: adopted in full** — premise marked UNVERIFIED (§1), sequencing clause added (§1, §4), session-miss metric made the "mattered" metric (§10), the resemblance named in §1 and cited in the founder form; the go/no-go itself remains the founder's, exercised through row `RETRIEVAL-QMD` with the dissent visible.
- **legal-specialist** — measurement-log GDPR posture (Art. 6(1)(f), minimisation-by-sanitization, no DPIA, leave-the-machine fence); license inventory duty; `--ignore-scripts` as a pin condition; no counsel needed pre-Phase-0. **Adopted** (§§5, 9).
- **observability-agent** — `run_id` pairing key + per-row corpus SHA; `outcome` field separating tool-error/staleness from a true miss; frozen export vs retention; opt-in coverage counted; declared rank horizon; percentiles never averages. **Adopted** (§9).
- **ux-designer** — banner survives every consumption path (unsuppressible, per-line `kind` marker); record id + path per candidate as the read affordance; query-substituted executable fallback + `unavailable_reason`; reject path as a real state in the form; unhappy paths (unavailable, zero-candidates) first-class in the diagram, empty state never asserts absence. **Adopted** (§§8, 15, form).
- **vernon** — nothing in lens; noted the wrapper invocation is a legitimate Ask (bounded, correlated, timeout-able, advisory failure mode) and the checklist re-runs if Phase 2 ever makes it a resident process. **Banked** (§13).
- **young** — projection discipline endorsed; delete-the-index neutrality test; stale-index NEGATIVES named a false-authority incident (negatives only from HEAD); no hook/validator/envelope ever consumes index output; refuse incremental freshening. **Adopted** (§§4, 7, 11, 12).
