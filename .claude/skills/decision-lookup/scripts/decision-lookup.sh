#!/usr/bin/env bash
# decision-lookup — advisory BM25 candidate retrieval (row RETRIEVAL-QMD, decided_by
# PROP-20260822-171212, founder 2026-08-22; the decision adopts the DESIGN — the first controlled
# `--install` run is a required activation test). ADVISORY ONLY: candidates, never evidence or
# authority. Decision YAML + direct source reading is the authority path; rg + aliases is the
# baseline and the fallback. This wrapper NEVER installs anything on its own during a task:
# installation happens only via the explicit documented command `decision-lookup.sh --install`,
# pinned and scriptless.
#
# EXIT SEMANTICS (founder-directed, 2026-08-22): the LOOKUP path always exits 0 — unavailability,
# an empty result, a stale index, a rebuild failure, or an output-contract failure print the
# advisory fallback and exit 0. The `--install` ACTIVATION TEST exits NON-ZERO on any failure:
# bun absent, install failure, pin/integrity verification failure, or lifecycle-script enforcement
# not establishable — and prints the reversal-decision instruction.
#
# Cache layout — project-local `.qmd/` (gitignored AND claudeignored; derived, disposable, never
# authoritative), overridable for hermetic tests via DECISION_LOOKUP_HOME:
#   .qmd/tool/    the pinned package: package.json (trustedDependencies: []), bunfig.toml
#                 (ignoreScripts), bun.lock, node_modules/ — created only by `--install`.
#   .qmd/corpus/  the `git archive` export (of the one resolved HEAD SHA) of committed governing
#                 Markdown + `.sha` (the revision stamp, the same resolved SHA) + the
#                 project-local index dir `qmd init` creates inside it.
#   .qmd/index/   QMD's HOME-side state: with HOME pointed here, qmd writes its config under
#                 .qmd/index/.config/qmd/. The index DATABASE is project-local (observed at
#                 activation, 2026-08-23): qmd 2.8.3 writes .qmd/corpus/.qmd/index.sqlite
#                 (+ -wal/-shm) inside the collection dir. Activation evidence after the first
#                 successful lookup = `.qmd/corpus/.sha` exists and .qmd/corpus/.qmd/ holds
#                 index.sqlite.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
PIN="@tobilu/qmd@2.8.3"        # exact pin; a missing version FAILS the install, never floats
INTEGRITY="sha512-zjfVwrObPB618B6x8SdhlGv/tX9OxRHsbQnr5DUtBvqPK6HGQ27lM+9/BAY5okpjrHVnW56hLyDkqoTcsrVLzA=="
QDIR="${DECISION_LOOKUP_HOME:-$REPO/.qmd}"
TOOL="$QDIR/tool"
CORPUS="$QDIR/corpus"
QHOME="$QDIR/index"
QMD="$TOOL/node_modules/.bin/qmd"
K=3

DISCLAIMER='ADVISORY ONLY — candidates, not evidence. READ the candidate directly, then resolve the
exact row: docs/decisions/<KEY>.yaml. Baseline and fallback: rg + aliases (workflow.md alias
table). No result is NOT evidence of "undecided".'

fallback() { # $1 = reason, $2 = query — lookup-path degradation is loud but exit 0
  echo "$DISCLAIMER"
  echo
  echo "qmd unavailable ($1) — use the baseline (this is the system, not a degraded mode):"
  # %q renders the query as DATA for copy/paste into BASH specifically (this wrapper's and the
  # repo's documented shell) — quotes, $(), backticks, newlines and a leading hyphen (already
  # fenced by --) cannot execute there. Bash-quoting only: %q may emit $'...' and backslash
  # forms that other shells do not guarantee to parse identically.
  printf '  rg --fixed-strings -i -l -- %q docs/ CLAUDE.md\n' "$2"
  echo "  aliases: docs/claude/sessions/workflow.md (contribution/tip, delivery/rider, founder/product owner/customer, register/decision queue)"
  echo "  then resolve the row: docs/decisions/<KEY>.yaml and READ the controlling record"
  exit 0
}

# Cache lifecycle: BEFORE EVERY LOOKUP the cache is validated — the stored corpus revision must
# equal `git rev-parse HEAD` AND the index database (corpus/.qmd/index.sqlite) must exist: a
# matching stamp with a missing index is a BROKEN CACHE (it would answer "no result" forever),
# not a hit. On mismatch or breakage, `.qmd/corpus` AND `.qmd/index` are discarded and rebuilt
# from `git archive` of the ONE resolved SHA that is also written to the stamp — HEAD is resolved
# exactly once, so the archive source and the stamp can never diverge (no re-resolve race). The
# WORKING TREE is never indexed. If the rebuild fails, the caches stay wiped and the caller gets
# the rg + aliases fallback — never stale QMD output. The stamp is written only after the index
# database is verified present and non-empty: a successful update that writes no index at the
# expected location is a rebuild failure, never a stamped cache. Exclusions (decided): DECISIONS.md
# (generated region), the QMD proposal (recorded contamination), docs/status/** (the journals
# narrate this tool's own verification queries and answers — indexing them lets a lookup match
# the account of itself, the recorded self-contamination/false-authority shape; rg + aliases
# still searches status records directly), and all non-Markdown (row-YAML indexing is out of
# scope by decision).
build_corpus() {
  local head; head="$(git -C "$REPO" rev-parse HEAD)"
  [ -f "$CORPUS/.sha" ] && [ "$(cat "$CORPUS/.sha")" = "$head" ] \
    && [ -s "$CORPUS/.qmd/index.sqlite" ] && return 0
  rm -rf "$CORPUS" "$QHOME" && mkdir -p "$CORPUS" "$QHOME"
  git -C "$REPO" archive "$head" -- docs/adr docs/proposals docs/claude docs/STATUS.md CLAUDE.md \
    | tar -x -C "$CORPUS" || { rm -rf "$CORPUS" "$QHOME"; return 1; }
  rm -f "$CORPUS/docs/proposals/DECISIONS.md" "$CORPUS"/docs/proposals/PROP-20260822-171212-*
  find "$CORPUS" -type f ! -name '*.md' -delete
  ( cd "$CORPUS" && env HOME="$QHOME" "$QMD" init >/dev/null 2>&1 \
    && env HOME="$QHOME" "$QMD" collection add . >/dev/null 2>&1 \
    && env HOME="$QHOME" "$QMD" update >/dev/null 2>&1 ) || { rm -rf "$CORPUS" "$QHOME"; return 1; }
  # A "successful" update that left no index at the checked location is a rebuild failure:
  # never stamp it (a stamped index-less corpus would rebuild forever, silently).
  [ -s "$CORPUS/.qmd/index.sqlite" ] || { rm -rf "$CORPUS" "$QHOME"; return 1; }
  # A failed stamp write wipes the derived caches like every other failure arm, so the caller's
  # "caches wiped" fallback wording stays exactly true and no partial state survives.
  printf '%s' "$head" > "$CORPUS/.sha" || { rm -rf "$CORPUS" "$QHOME"; return 1; }
}

activation_fail() { # $1 = what failed — the activation test is loud AND non-zero
  echo "ACTIVATION FAILED: $1"
  echo "activation failed; remove .qmd/ before any future approved retry."
  echo "Per row RETRIEVAL-QMD: record this failure and open a new/reversal decision before any change to package, version, permissions, or dependency shape. The baseline (rg + aliases + direct row resolution) is the system."
  exit 1
}

# Structural check (reversal 2026-08-23, replacing a whitespace-sensitive grep that failed against
# Bun's reformatted JSON while the enforcement itself held): the key "trustedDependencies" must
# exist in package.json and be EXACTLY an empty list — never an allowlist entry.
trusted_deps_empty() {
  python3 -c 'import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception:
    sys.exit(1)
sys.exit(0 if d.get("trustedDependencies") == [] else 1)' "$1" 2>/dev/null
}

if [ "${1:-}" = "--install" ]; then
  # THE ACTIVATION TEST and the ONLY path that touches the network. Pinned, scriptless, inside the
  # gitignored cache. Run it deliberately — agents never run it implicitly as part of another
  # task, and its first execution requires the founder's separate approval.
  command -v bun >/dev/null 2>&1 || activation_fail "bun runtime not present"
  command -v python3 >/dev/null 2>&1 \
    || activation_fail "python3 not present — required for the structural trustedDependencies verification and the strict results parser"
  mkdir -p "$TOOL" "$QHOME"
  printf '{"name":"captain-qmd","private":true,"trustedDependencies":[]}\n' > "$TOOL/package.json"
  printf '[install]\nignoreScripts = true\n' > "$TOOL/bunfig.toml"
  ( cd "$TOOL" && env HOME="$QHOME" bun add --exact "$PIN" --ignore-scripts ) \
    || activation_fail "bun add --exact $PIN --ignore-scripts returned non-zero (a partially created .qmd/tool/ may exist)"
  # Pin + integrity verification against the recorded digest (supply-chain assessment 2026-08-22):
  grep -q '"@tobilu/qmd@2.8.3"' "$TOOL/bun.lock" 2>/dev/null \
    || activation_fail "pinned version @tobilu/qmd@2.8.3 not found in bun.lock"
  grep -qF "$INTEGRITY" "$TOOL/bun.lock" 2>/dev/null \
    || activation_fail "recorded integrity digest not found in bun.lock — the artifact is not the assessed one"
  # Lifecycle-script enforcement must be establishable from the on-disk configuration:
  trusted_deps_empty "$TOOL/package.json" \
    || activation_fail "trustedDependencies is not exactly an empty list in .qmd/tool/package.json"
  grep -q 'ignoreScripts = true' "$TOOL/bunfig.toml" 2>/dev/null \
    || activation_fail "ignoreScripts = true not present in .qmd/tool/bunfig.toml"
  [ -x "$QMD" ] || activation_fail "qmd binary missing at .qmd/tool/node_modules/.bin/qmd after install"
  echo "ACTIVATION INSTALL OK: $PIN (scriptless, pin+integrity verified) at $TOOL"
  echo "activation completes at the first successful lookup: expect .qmd/corpus/.sha (revision stamp) and .qmd/corpus/.qmd/index.sqlite (the project-local index database) — report both as activation evidence."
  exit 0
fi

Q="${1:-}"
[ -z "$Q" ] && { echo "usage: decision-lookup.sh \"<question>\"   (or --install)"; exit 0; }
command -v bun >/dev/null 2>&1 || fallback "bun runtime not present" "$Q"
[ -x "$QMD" ] || fallback "not installed — to install deliberately: .claude/skills/decision-lookup/scripts/decision-lookup.sh --install" "$Q"
build_corpus || fallback "corpus/index rebuild failed (caches wiped — no stale output is ever served)" "$Q"

# Machine-readable output only: qmd's --json mode, parsed with the python3 standard library
# against a PINNED, STRICT top-level schema. PROVENANCE NOTE (honest): the completed sandbox spike
# exercised `qmd search` WITHOUT --json, so no JSON shape exists in the recorded evidence; the
# expectation below is a minimal provisional pin — top level is either a JSON array of result
# objects or an object whose top-level `results` key holds that array; each result object carries
# its path in a DIRECT key among file/path/filename and optionally an excerpt in a DIRECT key
# among snippet/excerpt/text/title. NOTHING NESTED IS EVER SCANNED — a metadata path buried
# deeper can never become a candidate. Source order is preserved exactly; paths deduplicate on
# first occurrence; the first three unique ranked paths win. ANY other structure or a result
# without its path field is an output-contract failure -> fallback ("QMD output contract
# unavailable") — never guesswork. The activation test confirms the real shape; a mismatch
# degrades safely to the fallback and is recorded, per the activation/rollback condition.
OUT="$(cd "$CORPUS" && env HOME="$QHOME" "$QMD" search "$Q" --json 2>/dev/null)"
SEARCH_RC=$?
# A tool failure is NOT an empty result: a non-zero qmd exit takes its own named fallback and
# must never read as "no candidates". An empty SUCCESSFUL output is the empty-result path.
[ "$SEARCH_RC" -ne 0 ] && fallback "qmd search failed (exit $SEARCH_RC) — a tool failure, not an empty result; no retry, no repair" "$Q"
[ -z "$OUT" ] && fallback "no result — the index is Markdown-only and corpus-masked; absence decides nothing" "$Q"

CANDIDATES="$(printf '%s' "$OUT" | python3 -c '
import json, sys
K = 3
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(3)
if isinstance(data, list):
    results, schema = data, "top-level-array"
elif isinstance(data, dict):
    results, schema = data.get("results"), "object.results-array"
else:
    sys.exit(3)
if not isinstance(results, list):
    sys.exit(3)
print(f"qmd-json-schema: {schema}")
rows, seen = [], set()
for r in results:
    if not isinstance(r, dict):
        sys.exit(3)
    path = next((r[k] for k in ("file", "path", "filename") if isinstance(r.get(k), str)), None)
    if path is None:
        sys.exit(3)  # a ranked result without its documented path field = contract mismatch
    p = path
    if "://" in p:
        p = p.split("://", 1)[1]
        p = p.split("/", 1)[1] if "/" in p else p
    while p.startswith("./"):
        p = p[2:]
    if p in seen:
        continue
    seen.add(p)
    ex = next((r[k] for k in ("snippet", "excerpt", "text", "title") if isinstance(r.get(k), str)), "")
    rows.append((p, " ".join(ex.split())[:200]))
    if len(rows) == K:
        break
if not rows:
    sys.exit(4)
for i, (p, ex) in enumerate(rows, 1):
    print(f"candidate {i}: {p}")
    if ex:
        print(f"  {ex}")
')" || case $? in
  3) fallback "QMD output contract unavailable (top-level shape is neither pinned form); if this is the activation lookup, activation is FAILED/INCONCLUSIVE pending a new decision — do not modify the parser; use rg + aliases" "$Q" ;;
  4) fallback "no result — the index is Markdown-only and corpus-masked; absence decides nothing" "$Q" ;;
  *) fallback "QMD output contract unavailable; use rg + aliases" "$Q" ;;
esac
[ -z "$CANDIDATES" ] && fallback "QMD output contract unavailable; use rg + aliases" "$Q"

SCHEMA_LINE="$(printf '%s\n' "$CANDIDATES" | head -1)"   # "qmd-json-schema: <observed>"
CANDIDATES="$(printf '%s\n' "$CANDIDATES" | tail -n +2)"

# First successful real lookup after --install: print and RECORD the activation evidence,
# including the observed JSON schema, exactly once (the file is the durable record inside the
# disposable cache; copy it into the activation report before any cache wipe).
EVIDENCE="$QDIR/activation-evidence.txt"
if [ ! -f "$EVIDENCE" ]; then
  {
    echo "activation evidence — first successful lookup ($(date -u +%Y-%m-%dT%H:%M:%SZ))"
    echo "package: $PIN"
    if grep -qF "$INTEGRITY" "$TOOL/bun.lock" 2>/dev/null; then echo "lockfile-integrity: verified ($INTEGRITY)"; else echo "lockfile-integrity: NOT VERIFIED in this cache"; fi
    if trusted_deps_empty "$TOOL/package.json" && grep -q 'ignoreScripts = true' "$TOOL/bunfig.toml" 2>/dev/null; then echo "scriptless-install: enforced (trustedDependencies [] + ignoreScripts)"; else echo "scriptless-install: enforcement NOT confirmed in this cache"; fi
    echo "corpus-head-sha: $(cat "$CORPUS/.sha" 2>/dev/null || echo missing)"
    echo "corpus-stamp: $CORPUS/.sha"
    echo "sqlite-index: $(find "$CORPUS/.qmd" -maxdepth 1 -name '*.sqlite' 2>/dev/null | head -1 || true)"
    echo "$SCHEMA_LINE"
  } > "$EVIDENCE"
  echo "── activation evidence (recorded at $EVIDENCE) ──"
  cat "$EVIDENCE"
  echo "──"
fi

echo "$DISCLAIMER"
echo
echo "$SCHEMA_LINE"
printf '%s\n' "$CANDIDATES"
echo
echo "next: READ the candidate(s) above, then resolve docs/decisions/<KEY>.yaml before any decision assertion or founder question."
exit 0
