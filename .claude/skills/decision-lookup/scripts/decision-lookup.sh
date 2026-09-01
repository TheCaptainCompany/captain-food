#!/usr/bin/env bash
# decision-lookup — advisory BM25 candidate retrieval (row RETRIEVAL-QMD-ROWS, the chain head;
# decided_by ADR-20260901-025538, founder 2026-09-01; it carries forward the superseded
# RETRIEVAL-QMD-CI / RETRIEVAL-QMD and widens the corpus to docs/decisions/*.yaml). ADVISORY ONLY: candidates, never evidence or
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
#                 Markdown AND the decision-register row files (`docs/decisions/*.yaml`, row
#                 RETRIEVAL-QMD-ROWS) + `.sha` (the revision stamp, the same resolved SHA) + the
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
# The TOOL's own collection glob. qmd 2.8.3 defaults it to `**/*.md` (see qmd_pattern_widen).
QMD_PATTERN='**/*.{md,yaml}'
# The row-ingestion canary's nonce. It is a NONCE, not register content, on purpose: a guard
# coupled to a real row's key or wording would go red on ordinary register growth -- someone
# opening, rewording or superseding a decision row would fire a repository-wide gate, which is a
# worse failure than the one being guarded. A hit on this token is explicable ONLY by the canary
# FILE having been indexed, and that file sits in docs/decisions/ with a .yaml extension: the
# exact pathspec arm and extension arm the real rows travel.
CANARY_KEY='QMD-INGESTION-CANARY'
CANARY_TOKEN='qmdrowingestioncanaryf3a91c7d'

DISCLAIMER='ADVISORY ONLY — candidates, not evidence. READ every candidate directly at HEAD, and
resolve docs/decisions/<KEY>.yaml itself: the index is a disposable fold of ONE commit, and a
projection is never authority — including when the candidate IS a row file. Baseline and fallback:
rg + aliases (workflow.md alias table). No result is NOT evidence of "undecided".'

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
  echo "  then resolve the row: docs/decisions/<KEY>.yaml at HEAD and READ the controlling record"
  exit 0
}

# Cache lifecycle: BEFORE EVERY LOOKUP the cache is validated — the stored corpus revision must
# equal `git rev-parse HEAD` AND the index database (corpus/.qmd/index.sqlite) must exist AND be
# OPENABLE (a bounded sqlite probe: connect + PRAGMA schema_version — never quick_check or
# integrity_check on the hit path): a matching stamp with a missing OR corrupt index is a BROKEN
# CACHE, not a hit — per the recorded delete-wholesale policy (proposal 6.3) it is wiped and
# rebuilt, never repaired. On mismatch or breakage, `.qmd/corpus` AND `.qmd/index` are discarded and rebuilt
# from `git archive` of the ONE resolved SHA that is also written to the stamp — HEAD is resolved
# exactly once, so the archive source and the stamp can never diverge (no re-resolve race). The
# WORKING TREE is never indexed. If the rebuild fails, the caches stay wiped and the caller gets
# the rg + aliases fallback — never stale QMD output. The stamp is written only after the index
# database is verified present and non-empty AND the row-ingestion canary has passed AND the index
# holds at least as many `docs/decisions/*.yaml` documents as this build exported: a successful
# update that writes no index at the expected location, one that indexes no `.yaml` at all, or one
# that indexes only SOME of them, is a rebuild failure, never a stamped cache. (The third arm is
# not defensive: the pinned qmd 2.8.3 does exactly that, non-deterministically — open row
# RETRIEVAL-QMD-INGEST-LOSS.) INCLUDED (row RETRIEVAL-QMD-ROWS, founder 2026-09-01):
# `docs/decisions/*.yaml`, the register's declaration site — ALL of them, superseded and withdrawn
# rows included, because `superseded_by:` is what makes a hit on a retired row resolvable to its
# chain head; truncating to live rows would destroy the DAG that makes the register answerable.
# Exclusions (decided): DECISIONS.md
# (generated region), the QMD proposal (recorded contamination), docs/status/** (the journals
# narrate this tool's own verification queries and answers — indexing them lets a lookup match
# the account of itself, the recorded self-contamination/false-authority shape; rg + aliases
# still searches status records directly), `docs/decisions/_legacy.yaml` and
# `docs/decisions/_exempt.yaml` (CONTROL FILES, not rows: they carry no `status`/`owner`/`capacity`
# to disambiguate a hit, and `_legacy.yaml` is one document naming 100 prose-only keys, so it ranks
# for any register query while answering none — and a hit on it points at a key with NO row file,
# the one case the mandatory-resolution contract cannot discharge), `docs/decisions/README.md`
# (never exported: the pathspec is `docs/decisions/*.yaml`, so the register's schema/semantics doc
# is excluded BY CONSTRUCTION rather than by a second mask — it is a term sponge for register
# vocabulary that answers no decision question), and all other non-Markdown.
index_openable() { # bounded openability probe (delete-wholesale on failure — no repair path).
  # IMMUTABLE (implies read-only) on purpose — the probe must be a ZERO-WRITE observer: a
  # default rw connect silently runs SQLite WAL recovery on a pending -wal (a WRITE into the
  # derived index on the hit path, i.e. the repair proposal 6.3 forbids) and can block on the
  # 5s busy wait; even plain mode=ro still CREATES the -shm side file when a -wal is present
  # (verified empirically, sqlite 3.45). immutable=1 + timeout=0 touch nothing: the question is
  # whether the MAIN database file is openable — a pending -wal is deliberately ignored (WAL
  # handling belongs to the tool's own rw open). ACCEPTED ASSUMPTION, not an enforced property:
  # lookups are treated as SEQUENTIAL, so immutable's no-locking semantics are safe. Concurrent
  # sessions share one checkout's .qmd/ and nothing serializes it, so a probe racing another
  # session's rebuild can take a torn read, return the exit-1 verdict, and wipe the corpus under
  # that running update — which then reports a rebuild failure for a healthy rebuild. Bounded
  # and self-healing: both sessions land on the loud exit-0 rg fallback, the next lookup
  # rebuilds, and the tool is advisory-only. urllib.parse.quote (safe="/" default; on POSIX
  # pathname2url IS quote) keeps an arbitrary DECISION_LOOKUP_HOME path URI-safe — imported
  # INSIDE the guarded try because it must never widen the import surface that can be read as
  # corruption (urllib.request would transitively pull socket, another compile-time optional).
  # Exit contract: 1 = the DELIBERATE NOT-openable verdict (wipe + rebuild); 0 = openable;
  # 2 = probe UNAVAILABLE (the sqlite3 module is a COMPILE-TIME optional of python3, unlike
  # json); ANY OTHER exit (import chain failure, signal death, 126/127) is a probe failure,
  # never a corruption verdict — the same conflation the lookup-path python3 preflight closes,
  # one layer down. The probe is BEST-EFFORT: on anything but 1 the caller accepts the stamped
  # non-empty index at the pre-probe trust level — the rebuild arm serves exactly that trust
  # level unprobed, so refusing on the hit path would disable the advisory tool on such hosts
  # between HEAD changes for zero gained safety (qmd bundles its own SQLite; host-python
  # module absence says nothing about the index). Deep corruption on such hosts stays bounded
  # by the search-failure wipe below.
  # URI construction lives in the UNAVAILABLE arm, not the verdict arm: quote() raises
  # UnicodeEncodeError on a cache path carrying non-UTF-8 bytes (argv arrives surrogate-
  # escaped; verified empirically), and a path-shaped failure must never read as the
  # not-openable verdict. The exit-1 arm holds ONLY connect + PRAGMA — the sole operations
  # that can genuinely testify about the database file — and catches ONLY sqlite3.Error: the
  # database's own verdicts (garbage bytes -> DatabaseError, unreadable -> OperationalError)
  # are subclasses of it, while a failure of the CALL itself (e.g. a python3 whose connect()
  # lacks the uri=/timeout= kwargs -> TypeError) says nothing about the file and routes to the
  # unavailable arm. The classification uses getattr + an isinstance/issubclass GUARD rather
  # than `except sqlite3.Error:` on purpose: a module whose Error attribute is ABSENT would
  # raise AttributeError while EVALUATING the except clause (unhandled -> exit 1, a wipe decided
  # by a missing attribute), and one whose Error is PRESENT BUT NOT A CLASS would raise
  # TypeError inside isinstance() for the same result. Both are probe failures, not verdicts —
  # the guard routes them to exit 2, so only a genuine sqlite3 exception can reach the wipe.
  # Stdout is silenced with stderr: the caller dispatches on the exit status, and
  # interpreter-level stdout noise (a printing sitecustomize.py) must never reshape it.
  python3 -c 'import sys
try:
    import sqlite3
    from urllib.parse import quote
    uri = "file:" + quote(sys.argv[1]) + "?immutable=1"
except Exception:
    sys.exit(2)
try:
    c = sqlite3.connect(uri, uri=True, timeout=0)
    c.execute("PRAGMA schema_version"); c.close()
except Exception as e:
    E = getattr(sqlite3, "Error", ())
    sys.exit(1 if isinstance(E, type) and issubclass(E, BaseException) and isinstance(e, E) else 2)' "$1" >/dev/null 2>&1
}

# MASK #3 — THE TOOL'S OWN, and the one that is easy to miss. The wrapper has two masks of its
# own (the `git archive` pathspec and the `find` extension sweep below), but qmd keeps a THIRD:
# `qmd init` + `qmd collection add` write `pattern: "**/*.md"` into $CORPUS/.qmd/index.yml, and
# that glob decides what `qmd update` actually ingests. Widening only the wrapper's two masks
# exports the rows, keeps them on disk, builds an index and writes the stamp — and indexes ZERO
# rows, silently, forever. There is NO CLI route: `qmd collection add . '<glob>'` accepts the
# argument and IGNORES it (verified against the pinned 2.8.3 — `collection show` still reported
# `**/*.md`), so the pattern is set in the config file qmd itself just created, inside the
# gitignored disposable cache. Corpus composition only: `models:` and every other key are left
# untouched, which is why this is a targeted scalar rewrite and not a config rewrite.
# EXACTLY ONE `pattern:` scalar must match. Zero (a future qmd changing the config shape) or more
# than one (a second collection) FAILS the rebuild into the named fallback rather than guessing —
# the whole hazard here is a silent no-op, so an unrecognised config is never written to.
# The config is read and rewritten with `errors="surrogateescape"` because it is NOT guaranteed
# to be valid UTF-8: qmd writes the collection's absolute PATH into it verbatim, so on a cache
# path carrying a non-UTF-8 byte a plain utf-8 read raises and the rebuild would fail for an
# encoding reason that says nothing about the config. surrogateescape round-trips those bytes
# unchanged, so the rewrite touches the one scalar and nothing else. (Found by T15g, the
# non-UTF-8 cache-path case, which regressed the moment this helper was added.)
# INGESTION-COMPLETENESS PROBE — reads the indexed side out of the INDEX, never off the disk.
# The distinction it enforces is the one this whole change is about: corpus PRESENCE is not
# INGESTION, so a check whose "indexed" number comes from `find`, `ls`, or the tool's own
# `qmd collection list` compares a number to itself and can never go red. (`collection list`
# reported "Files: 440" against a database holding 420 — it is the SCAN count, not the persisted
# one.) The only witness that a document is retrievable is a row in `documents`.
#
# THE VERDICT IS THE EXIT CODE, and the comparison happens INSIDE python — deliberately, and for
# the reason T15i already pinned one layer up: a helper that printed the count would be read
# through a command substitution, and an interpreter that writes to stdout on start (a
# sitecustomize.py in PYTHONPATH) would prepend noise to the number. A wrapper that then parsed
# "sitecustomize noise\n84" would take the malformed-count arm and fail every rebuild on such a
# host. Nothing is printed; nothing is parsed.
#   0 = complete (indexed >= exported)
#   1 = the DELIBERATE INCOMPLETE verdict — the database answered and the answer is short, OR the
#       pinned query itself failed (no `documents` table, renamed columns: qmd changed the schema
#       this check is pinned to). Both fail CLOSED, because both mean the wrapper cannot vouch
#       for the index it is about to stamp, and an unrecognised shape is never assumed benign
#       (same call as the collection-config widen, T10d).
#   ANY OTHER exit = COULD NOT LOOK, never the verdict: the sqlite3 module is a compile-time
#       optional of python3; `quote()` raises UnicodeEncodeError on a cache path carrying
#       non-UTF-8 bytes (T15g); a python3 whose connect() lacks uri=/timeout= raises TypeError;
#       126/127/signal deaths land here too. The caller proceeds — BEST-EFFORT, exactly as
#       index_openable is and for the same reason: refusing would disable an advisory tool on
#       such a host for zero gained safety, while the canary (which needs no host sqlite3) still
#       runs. The hole is named rather than hidden: on a host that cannot look, a partial index
#       is stamped and the row canary alone guards the arm.
# Read-only + immutable + timeout=0: a zero-write observer on a derived cache, like the probe.
index_rows_complete() { # $1 = index.sqlite path, $2 = the exported row count to meet
  python3 -c '
import sys
try:
    import sqlite3
    from urllib.parse import quote
    con = sqlite3.connect("file:" + quote(sys.argv[1]) + "?immutable=1", uri=True, timeout=0)
except Exception:
    sys.exit(2)
try:
    # Bound parameter, not an inlined literal: this whole helper lives inside a
    # single-quoted `python3 -c` argument, so a SQL string literal would need a quote
    # character the shell cannot carry through.
    n = con.execute(
        "select count(*) from documents where active = 1 and path like ?",
        ("docs/decisions/%.yaml",)).fetchone()[0]
except Exception:
    sys.exit(1)
finally:
    try:
        con.close()
    except Exception:
        pass
sys.exit(0 if int(n) >= int(sys.argv[2]) else 1)' "$1" "$2"
}

qmd_pattern_widen() { # $1 = index.yml path, $2 = the wanted glob
  python3 -c '
import re, sys
path, want = sys.argv[1], sys.argv[2]
try:
    src = open(path, encoding="utf-8", errors="surrogateescape").read()
except Exception:
    sys.exit(1)
out, n = re.subn(
    r"(?m)^(?P<indent>[ \t]+)pattern:[ \t]*(?P<q>[\"\x27])(?P=q)?.*?(?P=q)[ \t]*$",
    lambda m: m.group("indent") + "pattern: \"" + want + "\"",
    src)
if n != 1:
    sys.exit(1)
try:
    open(path, "w", encoding="utf-8", errors="surrogateescape").write(out)
except Exception:
    sys.exit(1)' "$1" "$2" 2>/dev/null
}

build_corpus() { # 0 = cache ready; 1 = rebuild failed (wiped); 2 = canary failed; 3 = ingestion incomplete
  local head; head="$(git -C "$REPO" rev-parse HEAD)"
  if [ -f "$CORPUS/.sha" ] && [ "$(cat "$CORPUS/.sha")" = "$head" ] \
    && [ -s "$CORPUS/.qmd/index.sqlite" ]; then
    # Dispatch on the EXIT STATUS directly, never on a command substitution: matching a pattern
    # against captured stdout + the echoed code would let interpreter-level stdout noise turn a
    # genuine exit-1 verdict into the accept arm, silently disabling the only verdict that wipes.
    index_openable "$CORPUS/.qmd/index.sqlite"
    case $? in
      1) : ;;          # the deliberate NOT-openable verdict: broken cache — wipe and rebuild
      *) return 0 ;;   # 0 = openable; 2 = probe unavailable; anything else = probe failure —
                       # never read as corruption: accept the stamped hit at the pre-probe
                       # trust level (see the probe's exit contract above)
    esac
  fi
  rm -rf "$CORPUS" "$QHOME" && mkdir -p "$CORPUS" "$QHOME"
  # MASK #1 — the export. `docs/decisions/*.yaml` is scoped to the ROW FILES: the register's
  # README.md is never exported, so it needs no second mask.
  git -C "$REPO" archive "$head" -- docs/adr docs/proposals docs/claude docs/STATUS.md CLAUDE.md \
      'docs/decisions/*.yaml' \
    | tar -x -C "$CORPUS" || { rm -rf "$CORPUS" "$QHOME"; return 1; }
  rm -f "$CORPUS/docs/proposals/DECISIONS.md" "$CORPUS"/docs/proposals/PROP-20260822-171212-*
  rm -f "$CORPUS/docs/decisions/_legacy.yaml" "$CORPUS/docs/decisions/_exempt.yaml"
  # MASK #2 — keep Markdown everywhere, plus `.yaml` UNDER docs/decisions/ ONLY. Scoped BY PATH,
  # never by relaxing the extension filter globally: zero non-`.md` files are tracked under
  # docs/adr, docs/proposals or docs/claude today, so a blanket `*.yaml` relaxation would be
  # silently correct now and silently wrong the day one lands there.
  find "$CORPUS" -type f ! -name '*.md' \
    ! \( -path "$CORPUS/docs/decisions/*" -name '*.yaml' \) -delete
  # The canary file is planted BEFORE `update` so it is ingested with everything else, and its
  # non-vacuity is ASSERTED rather than assumed: if the nonce occurs in any Markdown that is also
  # indexed, a hit no longer testifies about the `.yaml` arm and the guard proves nothing.
  if grep -rlF --include='*.md' -e "$CANARY_TOKEN" "$CORPUS" >/dev/null 2>&1; then
    rm -rf "$CORPUS" "$QHOME"; return 1
  fi
  printf 'key: "%s"\ncanary: "%s"\n' "$CANARY_KEY" "$CANARY_TOKEN" \
    > "$CORPUS/docs/decisions/$CANARY_KEY.yaml" || { rm -rf "$CORPUS" "$QHOME"; return 1; }
  ( cd "$CORPUS" && env HOME="$QHOME" "$QMD" init >/dev/null 2>&1 \
    && env HOME="$QHOME" "$QMD" collection add . >/dev/null 2>&1 ) \
    || { rm -rf "$CORPUS" "$QHOME"; return 1; }
  qmd_pattern_widen "$CORPUS/.qmd/index.yml" "$QMD_PATTERN" \
    || { rm -rf "$CORPUS" "$QHOME"; return 1; }
  ( cd "$CORPUS" && env HOME="$QHOME" "$QMD" update >/dev/null 2>&1 ) \
    || { rm -rf "$CORPUS" "$QHOME"; return 1; }
  # A "successful" update that left no index at the checked location is a rebuild failure:
  # never stamp it (a stamped index-less corpus would rebuild forever, silently).
  [ -s "$CORPUS/.qmd/index.sqlite" ] || { rm -rf "$CORPUS" "$QHOME"; return 1; }
  # ROW-INGESTION CANARY. Corpus PRESENCE is not INGESTION: all three masks can be correct on
  # disk while a future qmd, a changed config shape or a re-narrowed glob drops every `.yaml`,
  # and the index would still build, still stamp and still answer — never returning a row again,
  # silently. So the `.yaml` arm is proven END TO END at rebuild time, and a failure is fail-CLOSED:
  # caches wiped, corpus NEVER stamped, its own named fallback. This runs before the stamp write
  # for exactly that reason.
  printf '%s' "$(cd "$CORPUS" && env HOME="$QHOME" "$QMD" search "$CANARY_TOKEN" --json 2>/dev/null)" \
    | grep -qF "$CANARY_KEY" || { rm -rf "$CORPUS" "$QHOME"; return 2; }
  # INGESTION-COMPLETENESS CHECK — the canary's companion, and NOT a duplicate of it. The canary
  # proves the `.yaml` ARM is alive end to end (pathspec + extension sweep + collection glob +
  # search path, one nonce). It cannot see a PARTIAL arm: one ingested row satisfies it while
  # every other row is missing. That is not a hypothetical — PR #841 review round 1 measured
  # exactly it against the pinned qmd 2.8.3 (row RETRIEVAL-QMD-INGEST-LOSS): 15 of 15 clean
  # rebuilds landed short and NON-DETERMINISTICALLY short (64, 67, 70 and 72 of 84 expected
  # `.yaml`), `qmd update` printed "All collections updated" every time, re-running it never
  # recovered a single dropped row, and the stamp was written on every one of them. So the guard
  # is a COMPARISON, and both sides are derived at build time from this corpus — never a literal
  # and never a count copied from a record, because a literal goes stale on the next row anyone
  # opens and would turn ordinary register growth into a repository-wide failure.
  #   exported = the `.yaml` files this build actually placed under $CORPUS/docs/decisions/
  #              (the canary file included: it is planted before `update` and must ingest too)
  #   indexed  = rows in `documents` for that same path prefix
  # `>=` rather than `=` deliberately: fewer indexed than exported is the defect, more is not a
  # loss of rows and must not fail a lookup. Fail-CLOSED like the canary — caches wiped, corpus
  # NEVER stamped, its own named fallback — and for the same reason: an index missing an unknown
  # subset of rows answers register checks with false negatives, which is worse than answering
  # nothing, because the fallback tells the operator to use `rg` and a partial index does not.
  exported_rows="$(find "$CORPUS/docs/decisions" -type f -name '*.yaml' 2>/dev/null | wc -l)"
  index_rows_complete "$CORPUS/.qmd/index.sqlite" "$exported_rows"
  # Dispatch on $? DIRECTLY — never on a captured "$(...; echo $?)", which is what lets
  # interpreter stdout noise reach a pattern match (T15i). Only 1 is the verdict.
  [ $? -eq 1 ] && { rm -rf "$CORPUS" "$QHOME"; return 3; }
  # A failed stamp write wipes the derived caches like every other failure arm, so the caller's
  # "caches wiped" fallback wording stays exactly true and no partial state survives.
  printf '%s' "$head" > "$CORPUS/.sha" || { rm -rf "$CORPUS" "$QHOME"; return 1; }
}

activation_fail() { # $1 = what failed — the activation test is loud AND non-zero
  echo "ACTIVATION FAILED: $1"
  echo "activation failed; remove .qmd/ before any future approved retry."
  # NAME THE CHAIN HEAD. This is the string the operator reads on the rollback path the FAILURE
  # PROTOCOL governs, and it used to say RETRIEVAL-QMD -- now superseded, so a session doing exactly
  # what this line said wrote `reconsiders: RETRIEVAL-QMD` against that superseded row, and hit the
  # validator's "challenge the HEAD of its supersession chain" -- a gate error on the one path
  # where the operator is already dealing with something broken. Review of PR #679. It is now
  # RETRIEVAL-QMD-ROWS, moved 2026-09-01 in the same commit that superseded RETRIEVAL-QMD-CI --
  # this line is the SECOND supersession it has had to survive, which is why it is called out.
  echo "Per row RETRIEVAL-QMD-ROWS (the chain head): record this failure and open a new/reversal decision before any change to package, version, permissions, dependency shape, or CORPUS COMPOSITION (what is exported, what is masked, and the collection glob -- a fifth protected dimension since 2026-09-01). The baseline (rg + aliases + direct row resolution) is the system."
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

# Structural version<->integrity BINDING in bun.lock (the separate verification-design decision
# the #671 deferred note required — separately authorized under the standing completion
# directive, recorded in journal-2026-W34; closing the recorded asymmetry with the structural
# package.json check): the
# "@tobilu/qmd" packages entry must itself name exactly $PIN AND carry the recorded integrity
# digest as its final element. Two independent presence greps could be satisfied by different
# entries; this cannot. bun.lock is JSONC (bun emits trailing commas), so trailing commas are
# stripped before json parsing — a whitespace/format change never produces a false verdict
# (the 588cbd8 lesson); any parse failure fails LOUD at --install, never silently.
qmd_lock_binding_ok() { # $1 = bun.lock, $2 = exact pin, $3 = recorded integrity
  # The comma-strip regex runs over raw bytes, including string INTERIORS — provably harmless
  # for the verdict: the two compared elements (pin alphabet, sha512 base64) cannot contain
  # "," + whitespace + "}"/"]", so no rewrite can manufacture a matching first/last element;
  # the effect elsewhere is either a harmless string alteration that cannot touch the compared
  # elements, or a parse difference that lands in the loud activation_fail arm. Never a false
  # pass either way. The file is read as UTF-8 EXPLICITLY (bun.lock is UTF-8 by construction):
  # a locale-dependent open() would make the verdict host-dependent — on a LANG=C host a single
  # non-ASCII byte anywhere in the lockfile would raise and be reported under the binding
  # failure, a host defect surfacing among artifact causes.
  python3 -c 'import json, re, sys
try:
    raw = open(sys.argv[1], encoding="utf-8").read()
    data = json.loads(re.sub(r",(\s*[}\]])", r"\1", raw))
    entry = data["packages"]["@tobilu/qmd"]
except Exception:
    sys.exit(1)
ok = isinstance(entry, list) and len(entry) >= 2 and entry[0] == sys.argv[2] and entry[-1] == sys.argv[3]
sys.exit(0 if ok else 1)' "$1" "$2" "$3" 2>/dev/null
}

if [ "${1:-}" = "--install" ]; then
  # THE ACTIVATION TEST and the ONLY path that touches the network. Pinned, scriptless, inside the
  # gitignored cache. Run it deliberately — agents never run it implicitly as part of another
  # task, and its first execution requires the founder's separate approval.
  command -v bun >/dev/null 2>&1 || activation_fail "bun runtime not present"
  # Preflight python3 BY EXECUTION, mirroring the lookup path (`command -v` proves
  # resolvability, not runnability): this path routes the lockfile-binding TAMPERING verdict
  # through python3, so a broken-but-resolvable interpreter must fail HERE, named and before
  # any network touch — never inside the binding check, where its failure would allege a
  # non-assessed artifact for a host defect.
  python3 -c 'import json, re' >/dev/null 2>&1 \
    || activation_fail "python3 not usable — required for the structural lockfile-binding and trustedDependencies verifications and the strict results parser"
  mkdir -p "$TOOL" "$QHOME"
  printf '{"name":"captain-qmd","private":true,"trustedDependencies":[]}\n' > "$TOOL/package.json"
  printf '[install]\nignoreScripts = true\n' > "$TOOL/bunfig.toml"
  ( cd "$TOOL" && env HOME="$QHOME" bun add --exact "$PIN" --ignore-scripts ) \
    || activation_fail "bun add --exact $PIN --ignore-scripts returned non-zero (a partially created .qmd/tool/ may exist)"
  # Pin + integrity verification against the recorded digest (supply-chain assessment 2026-08-22):
  qmd_lock_binding_ok "$TOOL/bun.lock" "$PIN" "$INTEGRITY" \
    || activation_fail "bun.lock does not structurally bind $PIN to the recorded integrity digest (parse failure, missing package, wrong version, a digest attached to a different entry — or the lockfile entry shape differs from the assumed [pin, ..., integrity] tuple, a format-assumption miss in this check, not tampering) — either the artifact is not the assessed one or the shape assumption needs re-verification"
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
# python3 must be preflighted BEFORE the cache is consulted: without it the openability probe
# fails with a code that is indistinguishable from "corrupt" — every lookup would then wipe and
# fully rebuild a healthy cache and still fail later at the parser, misnamed as a contract
# failure. The preflight EXECUTES rather than resolves (`command -v` proves resolvability, not
# runnability — a resolvable python3 that cannot start, e.g. a venv shim whose interpreter was
# removed or a broken PYTHONHOME, would re-enter the same loop), and `import json` vouches for
# exactly what the results parser needs. Unusability degrades to the named fallback with the
# caches untouched.
python3 -c 'import json' >/dev/null 2>&1 \
  || fallback "python3 not usable — required for the openability probe and the strict results parser" "$Q"
[ -x "$QMD" ] || fallback "not installed — to install deliberately: .claude/skills/decision-lookup/scripts/decision-lookup.sh --install" "$Q"
build_corpus
case $? in
  0) : ;;
  2) fallback "row-ingestion canary FAILED — the corpus' docs/decisions/*.yaml arm did not enter the index, so every register lookup would silently miss every row (caches wiped, corpus never stamped)" "$Q" ;;
  3) fallback "row-ingestion INCOMPLETE — the index holds fewer docs/decisions/*.yaml documents than this build exported, so an unknown subset of rows is silently missing and a lookup miss over the register would be a FALSE NEGATIVE (caches wiped, corpus never stamped; open row RETRIEVAL-QMD-INGEST-LOSS)" "$Q" ;;
  *) fallback "corpus/index rebuild failed (caches wiped — no stale output is ever served)" "$Q" ;;
esac

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
# Per the delete-wholesale policy (proposal 6.3), a search failure also wipes the derived caches
# BEFORE falling back — deep index corruption THAT QMD REPORTS AS A NON-ZERO EXIT must not
# degrade every lookup until HEAD changes; the next lookup rebuilds from the pinned archive.
# SCOPE (recorded honestly): corruption that surfaces as a SUCCESSFUL exit — garbage output
# (the contract fallback below) or empty output (the no-result arm) — keeps the cache and does
# degrade per-HEAD; the contract arm is DELIBERATELY not a wipe, because it is also the
# schema-pin-mismatch path, and wiping there would rebuild-loop a healthy cache under a
# genuinely changed output contract. HONEST COST
# (recorded): the exit code cannot distinguish a damaged index from qmd rejecting the QUERY
# itself, so a query-triggered failure pays the same wipe and the NEXT lookup pays a full
# rebuild. That cost-shift is accepted over ever serving a possibly-poisoned cache; if the
# cache "keeps rebuilding", look for a query shape qmd rejects — the KNOWN reproducer is a
# LEADING-HYPHEN query ("$Q" is positional and unfenced below, so qmd may parse it as an
# option and exit non-zero; whether qmd 2.8.3 honors a "--" fence is unverifiable offline —
# the pinned package lives inside the claudeignored cache — and fencing unverified would risk
# breaking every query, so the class is documented instead: rephrase the query without the
# leading hyphen).
[ "$SEARCH_RC" -ne 0 ] && { rm -rf "$CORPUS" "$QHOME"; fallback "qmd search failed (exit $SEARCH_RC) — a tool failure, not an empty result; derived caches wiped (delete-wholesale, never repair); no retry" "$Q"; }
[ -z "$OUT" ] && fallback "no result — the index is a corpus-masked projection of ONE commit and never the working tree; absence decides nothing" "$Q"

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
    # THE PATH IS THE PAYLOAD FOR A ROW; THE EXCERPT IS DECORATIVE. No field subset of a
    # decision row is safe to quote: PMW-1 is `status: "decided"` with an evidence field that
    # reads as firm founder approval, while its own `note` records that the premise is gone and
    # the live challenge is the open row PMW-4. Excerpting `status` beside it would manufacture a
    # MORE convincing false answer than printing nothing, so a row candidate renders as a
    # RESOLVE-INSTRUCTION and never as a quotable snippet.
    if p.startswith("docs/decisions/") and p.endswith(".yaml"):
        rows.append((p, None))
        if len(rows) == K:
            break
        continue
    ex = next((r[k] for k in ("snippet", "excerpt", "text", "title") if isinstance(r.get(k), str)), "")
    rows.append((p, " ".join(ex.split())[:200]))
    if len(rows) == K:
        break
if not rows:
    sys.exit(4)
for i, (p, ex) in enumerate(rows, 1):
    print(f"candidate {i}: {p}")
    if ex is None:
        print(f"  decision row — resolve {p} at HEAD. Not excerpted on purpose: status,")
        print("  reconsiders/superseded_by and note only mean anything read together.")
    elif ex:
        print(f"  {ex}")
')" || case $? in
  3) fallback "QMD output contract unavailable (top-level shape is neither pinned form); if this is the activation lookup, activation is FAILED/INCONCLUSIVE pending a new decision — do not modify the parser; use rg + aliases" "$Q" ;;
  4) fallback "no result — the index is a corpus-masked projection of ONE commit and never the working tree; absence decides nothing" "$Q" ;;
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
    if qmd_lock_binding_ok "$TOOL/bun.lock" "$PIN" "$INTEGRITY"; then echo "lockfile-integrity: verified (structurally bound to $PIN; $INTEGRITY)"; else echo "lockfile-integrity: NOT VERIFIED in this cache"; fi
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
# STALENESS IS STATED ON EVERY LOOKUP, not reasoned about per candidate. The index is a fold of
# the one SHA on the stamp; the working tree is never indexed. That is now load-bearing in a new
# way: rows are WRITTEN in the same sessions that run register checks, so a lookup miss on a row
# is NOT a negative trail and `rg` over the working tree stays mandatory.
echo "corpus: $(cat "$CORPUS/.sha" 2>/dev/null || echo unknown) (working tree not indexed)"
echo "$SCHEMA_LINE"
printf '%s\n' "$CANDIDATES"
echo
echo "next: READ the candidate(s) above at HEAD, and resolve docs/decisions/<KEY>.yaml directly — including when a candidate IS a row file — before any decision assertion or founder question."
exit 0
