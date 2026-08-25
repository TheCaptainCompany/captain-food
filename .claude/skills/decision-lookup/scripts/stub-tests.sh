#!/usr/bin/env bash
# Hermetic stub tests for decision-lookup.sh (row RETRIEVAL-QMD).
# NEVER installs QMD, never calls the live package, never creates or modifies the real repo
# .qmd/ cache, and does not depend on it: every case runs against a temporary
# DECISION_LOOKUP_HOME with fake `qmd` executables; a fingerprint of the repo .qmd/ (if any)
# is asserted byte-identical before and after the run.
# Invocation (from the repo root):  bash .claude/skills/decision-lookup/scripts/stub-tests.sh
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
W="$HERE/decision-lookup.sh"
REPO_ROOT="$(cd "$HERE/../../../.." && pwd)"
S="$(mktemp -d)"
trap 'rm -rf "$S"' EXIT
pass=0; fail=0; skip=0
verdict() { if [ "$1" = ok ]; then pass=$((pass+1)); echo "PASS  $2"; else fail=$((fail+1)); echo "FAIL  $2"; fi }
# A SKIP is ONLY for a setup the host FILESYSTEM/KERNEL forbids — never for a precondition the
# harness could construct and didn't (those stay loud `verdict bad`, per T3/T15b). It does not
# count as a failure, and it is printed so a green run always names what it did not cover.
skipped() { skip=$((skip+1)); echo "SKIP  $1"; }

fingerprint() { # the real cache must be untouched by this suite, whether present or absent
  if [ -e "$REPO_ROOT/.qmd" ]; then find "$REPO_ROOT/.qmd" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum; else echo absent; fi
}
BEFORE="$(fingerprint)"

# A STUB `bun` on PATH for the whole suite. The wrapper preflights `command -v bun` before any
# lookup work, but a LOOKUP never invokes bun — only `--install` does, and all three install
# cases (T3/T3b/T3c) build their own controlled PATH, so nothing here can reach a real install.
# Without this stub every cache-building case fails its precondition on a host with no bun (a CI
# runner), which is a false red about the HOST, not the wrapper — the suite claims to be
# hermetic, so it must not silently depend on a bun being installed. Shadowing a real bun is
# deliberate: this suite never wants one.
mkdir -p "$S/bin"
printf '#!/bin/sh\nexit 0\n' > "$S/bin/bun"; chmod +x "$S/bin/bun"
PATH="$S/bin:$PATH"; export PATH
command -v bun >/dev/null 2>&1 || { echo "FATAL: stub bun not resolvable — the suite would report host-shaped failures"; exit 1; }

mkfake() { # $1 = QDIR, $2 = payload file for `search`
  mkdir -p "$1/tool/node_modules/.bin"
  # `update` mirrors real qmd 2.8.3: on success it leaves a VALID sqlite index database inside
  # the collection dir (cwd) at .qmd/index.sqlite — so the wrapper's index-presence AND
  # openability cache checks are exercised for real in these tests.
  cat > "$1/tool/node_modules/.bin/qmd" <<EOF
#!/usr/bin/env bash
case "\$1" in
  init|collection) exit 0 ;;
  update) rc=\${FAKE_UPDATE_EXIT:-0}
          [ "\$rc" -eq 0 ] && [ -z "\${FAKE_UPDATE_NO_INDEX:-}" ] && { mkdir -p .qmd; python3 -c 'import sqlite3; c = sqlite3.connect(".qmd/index.sqlite"); c.execute("CREATE TABLE IF NOT EXISTS t(x)"); c.commit(); c.close()'; }
          [ -n "\${FAKE_UPDATE_STAMP_BLOCK:-}" ] && mkdir -p .sha   # a DIRECTORY at the stamp path makes the stamp write fail deterministically
          exit "\$rc" ;;
  search) cat "$2"; exit \${FAKE_SEARCH_EXIT:-0} ;;
esac
EOF
  chmod +x "$1/tool/node_modules/.bin/qmd"
}

# T1 syntax
bash -n "$W" && verdict ok "T1 bash -n" || verdict bad "T1 bash -n"

# T2 fresh cache (no tool) -> fallback, exit 0
Q="$S/t2"; mkdir -p "$Q"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "who bears the refund cost" 2>&1)"; rc=$?
[ $rc -eq 0 ] && echo "$out" | grep -q "not installed" && echo "$out" | grep -q "rg --fixed-strings" \
  && verdict ok "T2 cache-miss fallback exit 0" || verdict bad "T2 cache-miss fallback (rc=$rc)"

# T3 --install without bun -> ACTIVATION FAILED, exit != 0. Controlled-PATH design (T3b's
# model — no reliance on bun being absent from any host directory): the PATH dir carries ONLY
# the externals the script needs before the bun check (dirname); PRECONDITIONS assert the
# symlink exists AND that bun is genuinely unresolvable in that PATH, else T3 FAILS outright.
Q="$S/t3"; mkdir -p "$Q" "$S/t3-bin"
if ! ln -s "$(command -v dirname)" "$S/t3-bin/dirname" 2>/dev/null || [ ! -x "$S/t3-bin/dirname" ] \
  || env PATH="$S/t3-bin" /bin/bash -c 'command -v bun' >/dev/null 2>&1; then
  verdict bad "T3 no-bun install (precondition: controlled PATH not constructible or bun resolvable)"
else
  out="$(env PATH="$S/t3-bin" DECISION_LOOKUP_HOME="$Q" /bin/bash "$W" --install 2>&1)"; rc=$?
  [ $rc -ne 0 ] && echo "$out" | grep -q "ACTIVATION FAILED: bun runtime not present" \
    && echo "$out" | grep -q "remove .qmd/ before any future approved retry" \
    && verdict ok "T3 no-bun install exit $rc" || verdict bad "T3 no-bun install (rc=$rc)"
fi

# T3b --install with bun resolvable but python3 ABSENT -> named python3 preflight failure,
# exit != 0. A controlled PATH dir carries only the externals the script needs before the
# preflight (bun, dirname) — python3 is deliberately unresolvable. bun is a STUB (exit-0, per
# T3c): the wrapper only needs resolvability before the preflight, and a stub means even a
# preflight-less mutant can never reach a real network install from inside this suite.
# PRECONDITION: the stub and symlink must exist — otherwise T3b FAILS outright, so it can
# never pass through the earlier "bun runtime not present" path by accident.
Q="$S/t3b"; mkdir -p "$Q" "$S/t3b-bin"
printf '#!/bin/sh\nexit 0\n' > "$S/t3b-bin/bun"; chmod +x "$S/t3b-bin/bun"
# `mkdir` MUST be on the controlled PATH: the wrapper creates $TOOL with it, so without it the
# "no install dir" assertion below passes because mkdir was unavailable, not because the preflight
# ran first — a vacuous green that survives the very mutant it exists to catch (moving `mkdir -p
# "$TOOL"` above the preflight). Same for t3c.
if [ ! -x "$S/t3b-bin/bun" ] \
  || ! ln -s "$(command -v dirname)" "$S/t3b-bin/dirname" 2>/dev/null || [ ! -x "$S/t3b-bin/dirname" ] \
  || ! ln -s "$(command -v mkdir)" "$S/t3b-bin/mkdir" 2>/dev/null || [ ! -x "$S/t3b-bin/mkdir" ]; then
  verdict bad "T3b no-python3 install (precondition: stub bun + symlinks unavailable)"
else
  out="$(env PATH="$S/t3b-bin" DECISION_LOOKUP_HOME="$Q" /bin/bash "$W" --install 2>&1)"; rc=$?
  [ $rc -ne 0 ] \
    && ! echo "$out" | grep -q "bun runtime not present" \
    && echo "$out" | grep -q "ACTIVATION FAILED: python3 not usable" \
    && echo "$out" | grep -q "structural lockfile-binding and trustedDependencies verifications and the strict results parser" \
    && echo "$out" | grep -q "remove .qmd/ before any future approved retry" \
    && [ ! -d "$Q/tool" ] \
    && verdict ok "T3b no-python3 install: named preflight, no install dir, exit $rc" || verdict bad "T3b no-python3 install (rc=$rc)"
fi

# T3c --install with bun resolvable but python3 BROKEN (resolves, cannot start) -> the named
# "python3 not usable" preflight failure BEFORE any install dir is created, exit != 0. The
# preflight executes (command -v proves resolvability, not runnability): this path routes the
# lockfile-binding TAMPERING verdict through python3, so a broken interpreter must fail here,
# named as a host defect — never inside the binding check alleging a non-assessed artifact.
# bun is a FAKE (exit-0 stub), so even a preflight-less mutant can never touch the network.
Q="$S/t3c"; mkdir -p "$Q" "$S/t3c-bin"
printf '#!/bin/sh\nexit 9\n' > "$S/t3c-bin/python3"; chmod +x "$S/t3c-bin/python3"
printf '#!/bin/sh\nexit 0\n' > "$S/t3c-bin/bun"; chmod +x "$S/t3c-bin/bun"
if [ ! -x "$S/t3c-bin/python3" ] || [ ! -x "$S/t3c-bin/bun" ] \
  || ! ln -s "$(command -v dirname)" "$S/t3c-bin/dirname" 2>/dev/null || [ ! -x "$S/t3c-bin/dirname" ] \
  || ! ln -s "$(command -v mkdir)" "$S/t3c-bin/mkdir" 2>/dev/null || [ ! -x "$S/t3c-bin/mkdir" ]; then
  verdict bad "T3c broken-python3 install (precondition: controlled PATH not constructible)"
else
  out="$(env PATH="$S/t3c-bin" DECISION_LOOKUP_HOME="$Q" /bin/bash "$W" --install 2>&1)"; rc=$?
  [ $rc -ne 0 ] \
    && echo "$out" | grep -q "ACTIVATION FAILED: python3 not usable" \
    && ! echo "$out" | grep -q "not the assessed one" \
    && [ ! -d "$Q/tool" ] \
    && verdict ok "T3c broken python3 install: named host-defect preflight, no install dir, exit $rc" \
    || verdict bad "T3c broken-python3 install (rc=$rc)"
fi

# T4 rebuild failure wipes caches and falls back
Q="$S/t4"; mkfake "$Q" /dev/null
out="$(FAKE_UPDATE_EXIT=1 DECISION_LOOKUP_HOME="$Q" "$W" "free-delivery threshold" 2>&1)"; rc=$?
[ $rc -eq 0 ] && echo "$out" | grep -q "rebuild failed" && [ ! -d "$Q/corpus" ] && [ ! -d "$Q/index" ] \
  && verdict ok "T4 rebuild-failure wipe + fallback exit 0" || verdict bad "T4 rebuild-failure (rc=$rc)"

# T5a valid object.results shape: schema line + source order + evidence file
Q="$S/t5a"
cat > "$S/p5a.json" <<'EOF'
{"results":[
 {"file":"docs/adr/A.md","snippet":"alpha"},
 {"file":"docs/proposals/B.md","snippet":"beta"},
 {"file":"docs/status/C.md","snippet":"gamma"}
]}
EOF
mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "tip contribution default" 2>&1)"; rc=$?
[ $rc -eq 0 ] && echo "$out" | grep -q "^qmd-json-schema: object.results-array$" \
  && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
  && echo "$out" | grep -q "candidate 1: docs/adr/A.md" \
  && echo "$out" | grep -q "candidate 3: docs/status/C.md" \
  && verdict ok "T5a object.results-array schema line + order" || verdict bad "T5a (rc=$rc)"
grep -q "^qmd-json-schema: object.results-array$" "$Q/activation-evidence.txt" 2>/dev/null \
  && grep -q "^package: @tobilu/qmd@2.8.3$" "$Q/activation-evidence.txt" \
  && grep -q "^corpus-head-sha: " "$Q/activation-evidence.txt" \
  && grep -q "^corpus-stamp: $Q/corpus/.sha$" "$Q/activation-evidence.txt" \
  && grep -q "^sqlite-index: " "$Q/activation-evidence.txt" \
  && grep -q "^lockfile-integrity: " "$Q/activation-evidence.txt" \
  && grep -q "^scriptless-install: " "$Q/activation-evidence.txt" \
  && verdict ok "T5a evidence file recorded once with schema line" || verdict bad "T5a evidence file"
# second run must NOT re-print the evidence block
out2="$(DECISION_LOOKUP_HOME="$Q" "$W" "tip contribution default" 2>&1)"
echo "$out2" | grep -q "activation evidence (recorded" \
  && verdict bad "T5a evidence printed once only" || verdict ok "T5a evidence printed once only"

# T5b nested misleading path never becomes a candidate
Q="$S/t5b"
cat > "$S/p5b.json" <<'EOF'
{"results":[
 {"file":"docs/adr/A.md","snippet":"real","meta":{"path":"docs/EVIL.md","source":{"file":"docs/EVIL2.md"}}},
 {"file":"docs/adr/B.md","snippet":"real2"}
]}
EOF
mkfake "$Q" "$S/p5b.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
[ $rc -eq 0 ] && ! echo "$out" | grep -q "EVIL" && [ "$(echo "$out" | grep -c '^candidate ')" -eq 2 ] \
  && verdict ok "T5b nested EVIL path excluded" || verdict bad "T5b (rc=$rc)"

# T5c invalid JSON -> contract fallback exit 0
Q="$S/t5c"; printf 'not json at all' > "$S/p5c.json"; mkfake "$Q" "$S/p5c.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
[ $rc -eq 0 ] && echo "$out" | grep -q "output contract unavailable" \
  && verdict ok "T5c invalid JSON -> contract fallback" || verdict bad "T5c (rc=$rc)"

# T5d top-level string -> contract fallback, with activation-inconclusive wording
Q="$S/t5d"; printf '"a string"' > "$S/p5d.json"; mkfake "$Q" "$S/p5d.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
[ $rc -eq 0 ] && echo "$out" | grep -q "neither pinned form" \
  && echo "$out" | grep -q "FAILED/INCONCLUSIVE pending a new decision" \
  && echo "$out" | grep -q "do not modify the parser" \
  && [ ! -f "$Q/activation-evidence.txt" ] \
  && verdict ok "T5d unpinned shape -> fallback + activation-inconclusive, no evidence file" || verdict bad "T5d (rc=$rc)"

# T5e dict without results / result missing path -> contract fallback
Q="$S/t5e"; printf '{"hits":[{"file":"docs/A.md"}]}' > "$S/p5e.json"; mkfake "$Q" "$S/p5e.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
r1=$([ $rc -eq 0 ] && echo "$out" | grep -q "output contract unavailable" && echo ok || echo bad)
Q="$S/t5e2"; printf '{"results":[{"score":9,"meta":{"file":"docs/A.md"}}]}' > "$S/p5e2.json"; mkfake "$Q" "$S/p5e2.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
r2=$([ $rc -eq 0 ] && echo "$out" | grep -q "output contract unavailable" && echo ok || echo bad)
[ "$r1" = ok ] && [ "$r2" = ok ] && verdict ok "T5e no-results-key / path-only-nested -> contract fallback" || verdict bad "T5e"

# T6 top-level array, five results with a duplicate -> exactly 3, schema line top-level-array
Q="$S/t6"
cat > "$S/p6.json" <<'EOF'
[
 {"path":"docs/adr/one.md","excerpt":"1"},
 {"path":"docs/adr/two.md","excerpt":"2"},
 {"path":"docs/adr/one.md","excerpt":"dup"},
 {"path":"docs/adr/three.md","excerpt":"3"},
 {"path":"docs/adr/four.md","excerpt":"4"}
]
EOF
mkfake "$Q" "$S/p6.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
[ $rc -eq 0 ] && echo "$out" | grep -q "^qmd-json-schema: top-level-array$" \
  && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
  && echo "$out" | grep -q "candidate 1: docs/adr/one.md" \
  && echo "$out" | grep -q "candidate 2: docs/adr/two.md" \
  && echo "$out" | grep -q "candidate 3: docs/adr/three.md" \
  && verdict ok "T6 top-level-array: dedup, cap 3, order, schema line" || verdict bad "T6 (rc=$rc)"

# T7 structural trustedDependencies check — tests the REAL shipped function (extracted verbatim)
eval "$(sed -n '/^trusted_deps_empty()/,/^}/p' "$W")"
t7() { printf '%s' "$2" > "$S/t7.json"; trusted_deps_empty "$S/t7.json"; rc=$?; [ $rc -eq "$3" ] && verdict ok "T7 $1" || verdict bad "T7 $1 (rc=$rc, want $3)"; }
t7 "bun pretty-printed empty list -> pass"      '{"name":"captain-qmd","private":true,"trustedDependencies": [],"dependencies":{"@tobilu/qmd":"2.8.3"}}' 0
t7 "compact empty list -> pass"                 '{"trustedDependencies":[]}' 0
t7 "multiline bun format -> pass"               '{
  "name": "captain-qmd",
  "private": true,
  "trustedDependencies": [],
  "dependencies": { "@tobilu/qmd": "2.8.3" }
}' 0
t7 "key missing -> FAIL"                        '{"name":"captain-qmd"}' 1
t7 "allowlist entry -> FAIL"                    '{"trustedDependencies":["node-llama-cpp"]}' 1
t7 "non-list value -> FAIL"                     '{"trustedDependencies":true}' 1
t7 "invalid JSON -> FAIL"                       'not json' 1

# T8 planted-red: qmd search exits non-zero -> the DISTINCT named tool-failure fallback,
# never the empty-result wording; exit 0; no retry, no repair — and per the delete-wholesale
# policy the derived caches are WIPED before the fallback, so deep corruption cannot cause a
# permanent tool-failure state until HEAD changes.
Q="$S/t8"; mkfake "$Q" /dev/null
out="$(FAKE_SEARCH_EXIT=7 DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
[ $rc -eq 0 ] && echo "$out" | grep -q "qmd search failed (exit 7)" \
  && echo "$out" | grep -q "a tool failure, not an empty result" \
  && echo "$out" | grep -q "derived caches wiped" \
  && ! echo "$out" | grep -q "no result — the index is Markdown-only" \
  && [ ! -d "$Q/corpus" ] && [ ! -d "$Q/index" ] \
  && verdict ok "T8 search failure -> tool-failure fallback, caches wiped, exit 0" || verdict bad "T8 (rc=$rc)"

# T9 Bash-safe fallback rendering: the emitted rg command's documented target shell is BASH
# (the wrapper quotes with Bash printf %q — no claim is made for other shells). Each rendered
# command is executed under `bash -c` (never eval in this harness's own shell) against a
# recording `rg` stub that captures argv NUL-separated: the exact query must arrive verbatim as
# ONE argv element (position 5) and no side-effect sentinel may appear. For the command-subst
# and backtick payloads, the RENDERED TEXT is additionally asserted to carry the payload
# escaped as data for Bash — no unescaped `$(` and no unescaped backtick.
rgbin="$S/rgstub"; mkdir -p "$rgbin"
cat > "$rgbin/rg" <<'EOF'
#!/usr/bin/env bash
printf '%s\0' "$@" > "$RGARGS"
exit 0
EOF
chmod +x "$rgbin/rg"
t9() { # $1 = label, $2 = query, $3 = rendered-text check: none | nodollarparen | nobacktick
  local Q9="$S/t9home" out line got textok=ok
  rm -rf "$Q9"; mkdir -p "$Q9"; rm -f "$S/pwned"
  out="$(DECISION_LOOKUP_HOME="$Q9" "$W" "$2" 2>&1)"   # not-installed fallback path
  line="$(printf '%s\n' "$out" | grep -m1 '^  rg ')"
  case "$3" in
    nodollarparen) printf '%s\n' "$line" | grep -Eq '(^|[^\\])\$\(' && textok=bad ;;  # unescaped $( = executable syntax
    nobacktick)    printf '%s\n' "$line" | grep -Eq '(^|[^\\])`'    && textok=bad ;;  # unescaped ` = executable syntax
  esac
  ( cd "$S" && RGARGS="$S/t9args" PATH="$rgbin:$PATH" bash -c "$line" )
  got="$(python3 -c 'import sys; a=open(sys.argv[1],"rb").read().split(b"\0"); sys.stdout.write(a[4].decode())' "$S/t9args")"
  [ "$textok" = ok ] && [ ! -e "$S/pwned" ] && [ "$got" = "$2" ] \
    && verdict ok "T9 $1" || verdict bad "T9 $1"
}
t9 "double quote stays data (bash)"          'say "hello" there'       none
t9 "command subst quoted as data (bash)"     "\$(touch $S/pwned)"      nodollarparen
t9 "backticks quoted as data (bash)"         "\`touch $S/pwned\`"      nobacktick
t9 "newline stays one argument (bash)"       $'line one\nline two'     none
t9 "leading hyphen stays data (bash)"        '-hyphen-start'           none

# T10 corpus mask: docs/status/** is never exported into the corpus, so a status-only journal
# document can never be indexed and therefore never surface as a candidate (qmd only indexes the
# collection dir). Uses the REAL `git archive` of the repo via a normal fake lookup; also
# re-proves the standing exclusions and that the governed sources remain present.
Q="$S/t10"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "activation verification" 2>&1)"; rc=$?
[ $rc -eq 0 ] && [ ! -e "$Q/corpus/docs/status" ] \
  && [ ! -e "$Q/corpus/docs/proposals/DECISIONS.md" ] \
  && ! ls "$Q"/corpus/docs/proposals/PROP-20260822-171212-* >/dev/null 2>&1 \
  && [ -d "$Q/corpus/docs/adr" ] && [ -f "$Q/corpus/CLAUDE.md" ] && [ -f "$Q/corpus/docs/STATUS.md" ] \
  && verdict ok "T10 corpus excludes docs/status/**, keeps governed sources" || verdict bad "T10 corpus mask (rc=$rc)"

# T11 stamp/archive same-SHA: the stamp equals `git rev-parse HEAD` (the one resolved SHA), and
# the wrapper source archives "$head" — never a re-resolved symbolic HEAD.
[ "$(cat "$Q/corpus/.sha")" = "$(git -C "$(dirname "$W")/../../../.." rev-parse HEAD)" ] \
  && grep -q 'git -C "\$REPO" archive "\$head" --' "$W" \
  && ! grep -q 'archive HEAD --' "$W" \
  && verdict ok "T11 stamp == resolved SHA; archive uses \$head, not HEAD" || verdict bad "T11 stamp/archive SHA"

# T12 broken cache: corpus/.sha matches HEAD but the index database is missing -> the wrapper
# must REBUILD (never treat it as a successful empty result). 12a: rebuild succeeds -> candidates
# print, no empty-result wording. 12b: rebuild fails -> the named rebuild-failed fallback, caches
# wiped, still never the empty-result wording.
rm -f "$Q/corpus/.qmd/index.sqlite"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
[ $rc -eq 0 ] && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
  && [ -s "$Q/corpus/.qmd/index.sqlite" ] \
  && ! echo "$out" | grep -q "no result — the index is Markdown-only" \
  && verdict ok "T12a missing index + matching stamp -> rebuilt, candidates, no empty-result" || verdict bad "T12a (rc=$rc)"
rm -f "$Q/corpus/.qmd/index.sqlite"
out="$(FAKE_UPDATE_EXIT=1 DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
[ $rc -eq 0 ] && echo "$out" | grep -q "rebuild failed" \
  && [ ! -d "$Q/corpus" ] && [ ! -d "$Q/index" ] \
  && ! echo "$out" | grep -q "no result — the index is Markdown-only" \
  && verdict ok "T12b missing index + rebuild failure -> named fallback, caches wiped" || verdict bad "T12b (rc=$rc)"

# T13 post-update index assertion: the stamp is written only when the index verifiably landed.
# T13a (positive): a successful update that writes the index -> stamped AND indexed, candidates.
Q="$S/t13a"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
[ $rc -eq 0 ] && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
  && [ -f "$Q/corpus/.sha" ] && [ -s "$Q/corpus/.qmd/index.sqlite" ] \
  && verdict ok "T13a successful update -> stamp and index coexist, candidates" || verdict bad "T13a (rc=$rc)"

# T13b (planted red): update exits 0 but writes NO index -> rebuild failure, caches wiped,
# NOTHING stamped, the named fallback — never candidates, never the empty-result wording.
Q="$S/t13b"; mkfake "$Q" "$S/p5a.json"
out="$(FAKE_UPDATE_NO_INDEX=1 DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
[ $rc -eq 0 ] && echo "$out" | grep -q "rebuild failed" \
  && ! echo "$out" | grep -q '^candidate ' \
  && ! echo "$out" | grep -q "no result — the index is Markdown-only" \
  && [ ! -d "$Q/corpus" ] && [ ! -d "$Q/index" ] \
  && [ -z "$(find "$Q" -name '.sha' 2>/dev/null)" ] \
  && verdict ok "T13b index-less successful update -> wiped, unstamped, named fallback" || verdict bad "T13b (rc=$rc)"

# T15 corrupt-but-present index: a matching stamp with GARBAGE bytes at index.sqlite must fail
# the openability probe and take the ordinary wipe-and-rebuild path — candidates print (the
# rebuild recreates a valid index), never a permanent tool-failure and never the empty-result
# wording. Delete-wholesale, no repair (proposal 6.3).
Q="$S/t15"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?   # first lookup builds a healthy cache
if [ $rc -ne 0 ] || [ ! -f "$Q/corpus/.sha" ]; then
  verdict bad "T15 corrupt-index rebuild (precondition: healthy build failed)"
else
  printf 'garbage-not-a-database' > "$Q/corpus/.qmd/index.sqlite"
  out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
  [ $rc -eq 0 ] && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
    && ! echo "$out" | grep -q "qmd search failed" \
    && ! echo "$out" | grep -q "no result — the index is Markdown-only" \
    && index_ok="$(python3 -c 'import sqlite3,sys
try:
    sqlite3.connect(sys.argv[1]).execute("PRAGMA schema_version"); print("ok")
except Exception:
    print("bad")' "$Q/corpus/.qmd/index.sqlite")" && [ "$index_ok" = ok ] \
    && verdict ok "T15 corrupt index + matching stamp -> rebuilt, candidates, healthy index" || verdict bad "T15 (rc=$rc)"
fi

# T15b lookup with python3 ABSENT -> the named preflight fallback, exit 0, and a HEALTHY cache
# left byte-untouched. Without the lookup-path preflight, the openability probe would exit 127
# (indistinguishable from "corrupt") and every lookup would wipe + fully rebuild a healthy cache.
# Controlled-PATH model per T3/T3b: the PATH dir carries only bun + dirname; PRECONDITIONS
# assert the seeded cache exists, the symlinks resolve, and python3 is genuinely unresolvable —
# else the case FAILS outright.
Q="$S/t15b"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "seed" 2>&1)"   # seed a healthy stamped cache first
mkdir -p "$S/t15b-bin"
BUN_REAL="$(command -v bun || true)"
if [ -z "$BUN_REAL" ] || [ ! -s "$Q/corpus/.qmd/index.sqlite" ] || [ ! -f "$Q/corpus/.sha" ] \
  || ! ln -s "$BUN_REAL" "$S/t15b-bin/bun" 2>/dev/null || [ ! -x "$S/t15b-bin/bun" ] \
  || ! ln -s "$(command -v dirname)" "$S/t15b-bin/dirname" 2>/dev/null || [ ! -x "$S/t15b-bin/dirname" ] \
  || env PATH="$S/t15b-bin" /bin/bash -c 'command -v python3' >/dev/null 2>&1; then
  verdict bad "T15b no-python3 lookup (precondition: seeded cache or controlled PATH unavailable)"
else
  fp_b="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  out="$(env PATH="$S/t15b-bin" DECISION_LOOKUP_HOME="$Q" /bin/bash "$W" "seed" 2>&1)"; rc=$?
  fp_a="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  [ $rc -eq 0 ] && echo "$out" | grep -q "python3 not usable" \
    && echo "$out" | grep -q "openability probe and the strict results parser" \
    && ! echo "$out" | grep -q '^candidate ' \
    && [ "$fp_b" = "$fp_a" ] \
    && verdict ok "T15b no-python3 lookup: named fallback, healthy cache untouched, exit $rc" \
    || verdict bad "T15b no-python3 lookup (rc=$rc)"
fi

# T15c the probe is a ZERO-WRITE observer: a garbage index.sqlite-wal planted beside a healthy
# stamped index must survive a cache-hit lookup with the index sidecar set byte-identical — no
# file changed, none deleted, NONE CREATED. Verified empirically (sqlite 3.45): a default rw
# connect runs WAL recovery during the probe and DELETES the planted -wal (the silent repair
# proposal 6.3 forbids); even plain mode=ro CREATES the -shm side file. Both mutants change the
# fingerprint; only immutable=1 leaves it. Candidates must still print (the fake search never
# reads the index).
Q="$S/t15c"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?   # first lookup builds a healthy cache
if [ $rc -ne 0 ] || [ ! -f "$Q/corpus/.sha" ] || [ ! -s "$Q/corpus/.qmd/index.sqlite" ]; then
  verdict bad "T15c probe read-only (precondition: healthy build failed)"
else
  printf 'garbage-not-a-wal-header' > "$Q/corpus/.qmd/index.sqlite-wal"
  fp_b="$(find "$Q/corpus/.qmd" -name 'index.sqlite*' -printf '%p %s\n' -exec md5sum {} \; 2>/dev/null | sort)"
  out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
  fp_a="$(find "$Q/corpus/.qmd" -name 'index.sqlite*' -printf '%p %s\n' -exec md5sum {} \; 2>/dev/null | sort)"
  [ $rc -eq 0 ] && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
    && [ "$fp_b" = "$fp_a" ] \
    && verdict ok "T15c probe is read-only: planted -wal survives a hit byte-identical" \
    || verdict bad "T15c probe read-only (rc=$rc)"
fi

# T15d probe UNAVAILABLE is not corruption: python3 present but the sqlite3 MODULE missing (a
# compile-time optional, unlike json) must ACCEPT the stamped non-empty hit at the pre-probe
# trust level — candidates print and the cache stays byte-untouched. Neither the silent
# wipe-and-rebuild that reading exit 2 as "corrupt" would cause, nor a refusal fallback: the
# rebuild arm serves exactly this trust level unprobed, so refusing the hit would disable the
# advisory tool on such hosts between HEAD changes for zero gained safety. Simulated
# hermetically by poisoning PYTHONPATH with an import-raising sqlite3 module (the parser needs
# only json, so candidates still parse).
Q="$S/t15d"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?   # first lookup builds a healthy cache
mkdir -p "$S/t15d-py"
printf 'raise ImportError("sqlite3 blocked for T15d")\n' > "$S/t15d-py/sqlite3.py"
if [ $rc -ne 0 ] || [ ! -f "$Q/corpus/.sha" ] \
  || PYTHONPATH="$S/t15d-py" python3 -c 'import sqlite3' 2>/dev/null; then
  verdict bad "T15d probe-unavailable (precondition: seeded cache or sqlite3 poisoning failed)"
else
  fp_b="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  out="$(PYTHONPATH="$S/t15d-py" DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
  fp_a="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  [ $rc -eq 0 ] && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
    && ! echo "$out" | grep -q "rebuild failed" \
    && ! echo "$out" | grep -q "qmd unavailable" \
    && [ "$fp_b" = "$fp_a" ] \
    && verdict ok "T15d sqlite3-module-absent: stamped hit accepted, cache untouched, exit $rc" \
    || verdict bad "T15d probe-unavailable (rc=$rc)"
fi

# T15e a python3 that RESOLVES but cannot start (broken venv shim, missing libpython) must hit
# the same named preflight fallback with the cache untouched — `command -v` would pass it and
# the probe's startup-failure exit would then read as a broken cache (wipe + rebuild every
# lookup, misnamed as a contract failure at the parser). The preflight executes, so it catches
# this. Controlled PATH per T15b, with a fake python3 that exits 9 without running anything.
Q="$S/t15e"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?   # seed a healthy stamped cache
mkdir -p "$S/t15e-bin"
printf '#!/bin/sh\nexit 9\n' > "$S/t15e-bin/python3"; chmod +x "$S/t15e-bin/python3"
BUN_REAL="$(command -v bun || true)"
if [ -z "$BUN_REAL" ] || [ ! -f "$Q/corpus/.sha" ] \
  || ! ln -s "$BUN_REAL" "$S/t15e-bin/bun" 2>/dev/null || [ ! -x "$S/t15e-bin/bun" ] \
  || ! ln -s "$(command -v dirname)" "$S/t15e-bin/dirname" 2>/dev/null || [ ! -x "$S/t15e-bin/dirname" ]; then
  verdict bad "T15e broken-python3 lookup (precondition: seeded cache or controlled PATH unavailable)"
else
  fp_b="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  out="$(env PATH="$S/t15e-bin" DECISION_LOOKUP_HOME="$Q" /bin/bash "$W" "x" 2>&1)"; rc=$?
  fp_a="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  [ $rc -eq 0 ] && echo "$out" | grep -q "python3 not usable" \
    && ! echo "$out" | grep -q '^candidate ' \
    && ! echo "$out" | grep -q "rebuild failed" \
    && [ "$fp_b" = "$fp_a" ] \
    && verdict ok "T15e broken python3: named preflight fallback, cache untouched, exit $rc" \
    || verdict bad "T15e broken-python3 lookup (rc=$rc)"
fi

# T15f an UNKNOWN probe exit is never a corruption verdict: only exit 1 — the probe's
# deliberate NOT-openable verdict — may wipe; any other failure code (import-chain death,
# signals, 126/127) must accept the stamped hit at the pre-probe trust level, cache untouched.
# Simulated by a poisoned sqlite3 module that hard-exits 7 on import (no exception raised, so
# the probe's own exit-2 arm cannot catch it — the code reaches the caller as-is).
Q="$S/t15f"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?   # seed a healthy stamped cache
mkdir -p "$S/t15f-py"
printf 'import os\nos._exit(7)\n' > "$S/t15f-py/sqlite3.py"
if [ $rc -ne 0 ] || [ ! -f "$Q/corpus/.sha" ] \
  || PYTHONPATH="$S/t15f-py" python3 -c 'import sqlite3' 2>/dev/null; then
  verdict bad "T15f unknown-probe-exit (precondition: seeded cache or exit-7 poisoning failed)"
else
  fp_b="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  out="$(PYTHONPATH="$S/t15f-py" DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
  fp_a="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  [ $rc -eq 0 ] && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
    && ! echo "$out" | grep -q "rebuild failed" \
    && [ "$fp_b" = "$fp_a" ] \
    && verdict ok "T15f unknown probe exit (7): stamped hit accepted, cache untouched, exit $rc" \
    || verdict bad "T15f unknown-probe-exit (rc=$rc)"
fi

# T15g URI construction failure is not a verdict: a cache path carrying a non-UTF-8 byte makes
# quote() raise UnicodeEncodeError (argv arrives surrogate-escaped). That failure must land in
# the probe's UNAVAILABLE arm (accept the stamped hit, bytes untouched), never the exit-1
# verdict arm — with quote() inside the verdict try, every lookup under such a path silently
# wiped and fully rebuilt a healthy cache (candidates still printed, so only the fingerprint
# discriminates).
# CAPABILITY GATE (Linux-only case): macOS/APFS enforces valid UTF-8 in filenames and REJECTS
# the \375 name outright, so the setup is impossible by filesystem policy — not a precondition
# this harness declined to build. That is a SKIP, not a FAIL: a hard red on every Mac would
# train readers to discount reds. The mkdir is attempted first and decides.
Q="$S/t15g$(printf '\375')"
# An ASCII CONTROL decides WHY the mkdir failed. Skipping on any mkdir failure would silently
# swallow ENOSPC/EROFS/EACCES/ENOTDIR — coverage vanishing while the suite still exits 0. Control
# succeeds AND the \375 name fails => a genuine filesystem-encoding refusal (skip). Control also
# fails => the harness/host is broken, which is a loud failure like every other precondition.
if ! mkdir -p "${Q%$(printf '\375')}-ascii-control" 2>/dev/null; then
  verdict bad "T15g non-utf8 path (precondition: ASCII control mkdir failed — host/harness broken, not an encoding refusal)"
elif ! mkdir -p "$Q" 2>/dev/null || [ ! -d "$Q" ]; then
  skipped "T15g non-utf8 cache path — filesystem rejects non-UTF-8 names (Linux-only case)"
else
  mkfake "$Q" "$S/p5a.json"
  out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?   # seed a healthy stamped cache
  if [ $rc -ne 0 ] || [ ! -f "$Q/corpus/.sha" ]; then
    verdict bad "T15g non-utf8 path (precondition: seeded build under \\375 path failed)"
  else
    fp_b="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
    out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
    fp_a="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
    [ $rc -eq 0 ] && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
      && ! echo "$out" | grep -q "rebuild failed" \
      && [ "$fp_b" = "$fp_a" ] \
      && verdict ok "T15g non-utf8 cache path: stamped hit accepted, cache untouched, exit $rc" \
      || verdict bad "T15g non-utf8 path (rc=$rc)"
  fi
fi

# T15h a non-sqlite3 exception inside the probe's verdict arm is NOT the not-openable verdict:
# a connect() call failure (e.g. an interpreter whose sqlite3.connect lacks the uri=/timeout=
# kwargs -> TypeError) says nothing about the database file, so it must take the UNAVAILABLE
# arm and accept the stamped hit. Simulated by a PYTHONPATH sqlite3 shim that imports cleanly,
# defines Error, and raises TypeError from connect() — indistinguishable from the real
# kwargs-unsupported host. With a bare `except Exception: sys.exit(1)` this wipes and rebuilds.
Q="$S/t15h"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?   # seed a healthy stamped cache
mkdir -p "$S/t15h-py"
printf 'class Error(Exception):\n    pass\n\n\ndef connect(*a, **k):\n    raise TypeError("connect() got an unexpected keyword argument")\n' > "$S/t15h-py/sqlite3.py"
if [ $rc -ne 0 ] || [ ! -f "$Q/corpus/.sha" ] \
  || ! PYTHONPATH="$S/t15h-py" python3 -c 'import sqlite3, sys
try:
    sqlite3.connect("x", uri=True, timeout=0)
except TypeError:
    sys.exit(0)
sys.exit(1)' 2>/dev/null; then
  verdict bad "T15h non-sqlite3 probe exception (precondition: seeded cache or TypeError shim failed)"
else
  fp_b="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  out="$(PYTHONPATH="$S/t15h-py" DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
  fp_a="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  [ $rc -eq 0 ] && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
    && ! echo "$out" | grep -q "rebuild failed" \
    && [ "$fp_b" = "$fp_a" ] \
    && verdict ok "T15h call-site TypeError: not a verdict, stamped hit accepted, cache untouched" \
    || verdict bad "T15h non-sqlite3 probe exception (rc=$rc)"
fi

# T15j a sqlite3 module WITHOUT an `Error` attribute must not decide a wipe: with a bare
# `except sqlite3.Error:` the except clause itself raises AttributeError while being evaluated,
# which is unhandled and exits 1 — a corruption verdict decided by a missing attribute rather
# than by the database. The shipped `isinstance(e, getattr(sqlite3, "Error", ()))` form routes
# it to the unavailable arm instead, so the stamped hit is accepted, cache untouched.
Q="$S/t15j"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?   # seed a healthy stamped cache
mkdir -p "$S/t15j-py"
printf 'def connect(*a, **k):\n    raise RuntimeError("no Error attribute on this module")\n' > "$S/t15j-py/sqlite3.py"
if [ $rc -ne 0 ] || [ ! -f "$Q/corpus/.sha" ] \
  || PYTHONPATH="$S/t15j-py" python3 -c 'import sqlite3, sys; sys.exit(0 if hasattr(sqlite3, "Error") else 1)' 2>/dev/null; then
  verdict bad "T15j sqlite3 without Error (precondition: seeded cache or attribute-less shim failed)"
else
  fp_b="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  out="$(PYTHONPATH="$S/t15j-py" DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
  fp_a="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  [ $rc -eq 0 ] && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
    && ! echo "$out" | grep -q "rebuild failed" \
    && [ "$fp_b" = "$fp_a" ] \
    && verdict ok "T15j sqlite3 module without Error: not a verdict, cache untouched" \
    || verdict bad "T15j sqlite3 without Error (rc=$rc)"
fi

# T15k an Error attribute that is PRESENT BUT NOT A CLASS must not decide a wipe either: with a
# bare `isinstance(e, getattr(sqlite3, "Error", ()))`, a shim exporting `Error = "not a class"`
# makes isinstance() raise TypeError inside the except clause -> unhandled -> exit 1, the wipe
# verdict decided by a malformed attribute. The shipped isinstance/issubclass guard routes it to
# the unavailable arm. One step out from T15j (absent attribute); same defect class.
Q="$S/t15k"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?   # seed a healthy stamped cache
mkdir -p "$S/t15k-py"
printf 'Error = "not a class"\n\n\ndef connect(*a, **k):\n    raise RuntimeError("boom")\n' > "$S/t15k-py/sqlite3.py"
if [ $rc -ne 0 ] || [ ! -f "$Q/corpus/.sha" ] \
  || ! PYTHONPATH="$S/t15k-py" python3 -c 'import sqlite3, sys; sys.exit(0 if not isinstance(getattr(sqlite3, "Error", ()), type) else 1)' 2>/dev/null; then
  verdict bad "T15k malformed Error attribute (precondition: seeded cache or non-class Error shim failed)"
else
  fp_b="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  out="$(PYTHONPATH="$S/t15k-py" DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
  fp_a="$(find "$Q/corpus" "$Q/index" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum)"
  [ $rc -eq 0 ] && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
    && ! echo "$out" | grep -q "rebuild failed" \
    && [ "$fp_b" = "$fp_a" ] \
    && verdict ok "T15k Error present but not a class: not a verdict, cache untouched" \
    || verdict bad "T15k malformed Error attribute (rc=$rc)"
fi

# T15i the verdict survives interpreter stdout NOISE: a genuinely corrupt index under a
# sitecustomize.py that prints on every interpreter start must still be REBUILT. Dispatching on
# a command substitution ("$(index_openable ...; echo $?)") would match the pattern against the
# noise plus the code and fall to the accept arm, silently disabling the only verdict that
# wipes — a corrupt index would then be served until HEAD changes.
Q="$S/t15i"; mkfake "$Q" "$S/p5a.json"
out="$(DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?   # seed a healthy stamped cache
mkdir -p "$S/t15i-py"
printf 'print("sitecustomize noise")\n' > "$S/t15i-py/sitecustomize.py"
if [ $rc -ne 0 ] || [ ! -f "$Q/corpus/.sha" ] \
  || [ "$(PYTHONPATH="$S/t15i-py" python3 -c 'pass' 2>/dev/null)" != "sitecustomize noise" ]; then
  verdict bad "T15i stdout-noise verdict (precondition: seeded cache or sitecustomize noise failed)"
else
  printf 'garbage-not-a-database' > "$Q/corpus/.qmd/index.sqlite"
  out="$(PYTHONPATH="$S/t15i-py" DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
  index_ok="$(python3 -c 'import sqlite3,sys
try:
    sqlite3.connect(sys.argv[1]).execute("PRAGMA schema_version"); print("ok")
except Exception:
    print("bad")' "$Q/corpus/.qmd/index.sqlite")"
  [ $rc -eq 0 ] && [ "$(echo "$out" | grep -c '^candidate ')" -eq 3 ] \
    && [ "$index_ok" = ok ] \
    && verdict ok "T15i corrupt index under stdout noise: verdict holds, rebuilt" \
    || verdict bad "T15i stdout-noise verdict (rc=$rc, index=$index_ok)"
fi

# T14 stamp-write failure: a successful indexed build whose corpus/.sha write fails must wipe
# the derived caches (so the named "caches wiped" fallback wording is TRUE), serve nothing,
# and exit 0. The fake update blocks the stamp by pre-creating a DIRECTORY at the stamp path.
Q="$S/t14"; mkfake "$Q" "$S/p5a.json"
out="$(FAKE_UPDATE_STAMP_BLOCK=1 DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
[ $rc -eq 0 ] && echo "$out" | grep -q "rebuild failed" \
  && echo "$out" | grep -q "caches wiped" \
  && ! echo "$out" | grep -q '^candidate ' \
  && [ ! -d "$Q/corpus" ] && [ ! -d "$Q/index" ] \
  && verdict ok "T14 stamp-write failure -> caches truly wiped, named fallback, exit 0" || verdict bad "T14 (rc=$rc)"

# T16 structural bun.lock version<->integrity BINDING — tests the REAL shipped function
# (extracted verbatim). Fixtures only; no lockfile is generated or modified.
eval "$(sed -n '/^qmd_lock_binding_ok()/,/^}/p' "$W")"
PIN16="$(sed -n 's/^PIN="\(.*\)" *#.*$/\1/p; s/^PIN="\(.*\)"$/\1/p' "$W" | head -1)"
INTEG16="$(sed -n 's/^INTEGRITY="\(.*\)"$/\1/p' "$W" | head -1)"
if [ -z "$PIN16" ] || [ -z "$INTEG16" ]; then
  verdict bad "T16 (precondition: PIN/INTEGRITY not extractable from wrapper)"
else
  t16() { printf '%s' "$2" > "$S/t16.lock"; qmd_lock_binding_ok "$S/t16.lock" "$PIN16" "$INTEG16"; rc=$?; [ $rc -eq "$3" ] && verdict ok "T16 $1" || verdict bad "T16 $1 (rc=$rc, want $3)"; }
  t16 "valid binding -> pass" \
    "{\"packages\": {\"@tobilu/qmd\": [\"$PIN16\", \"\", {}, \"$INTEG16\"]}}" 0
  t16 "right version, integrity on ANOTHER package -> FAIL" \
    "{\"packages\": {\"@tobilu/qmd\": [\"$PIN16\", \"\", {}, \"sha512-WRONGWRONGWRONG\"], \"other\": [\"other@1.0.0\", \"\", {}, \"$INTEG16\"]}}" 1
  t16 "wrong integrity on the qmd entry -> FAIL" \
    "{\"packages\": {\"@tobilu/qmd\": [\"$PIN16\", \"\", {}, \"sha512-TAMPERED\"]}}" 1
  t16 "digest present but NOT last element -> FAIL" \
    "{\"packages\": {\"@tobilu/qmd\": [\"$PIN16\", \"$INTEG16\", {}]}}" 1
  t16 "entry not a list (object shape) -> FAIL" \
    "{\"packages\": {\"@tobilu/qmd\": {\"version\": \"$PIN16\", \"integrity\": \"$INTEG16\"}}}" 1
  t16 "one-element entry -> FAIL" \
    "{\"packages\": {\"@tobilu/qmd\": [\"$PIN16\"]}}" 1
  t16 "wrong version, recorded digest last -> FAIL" \
    "{\"packages\": {\"@tobilu/qmd\": [\"@tobilu/qmd@2.9.9\", \"\", {}, \"$INTEG16\"]}}" 1
  t16 "valid JSONC trailing commas -> pass" \
    "{
  \"lockfileVersion\": 1,
  \"workspaces\": {
    \"\": { \"dependencies\": { \"@tobilu/qmd\": \"2.8.3\", }, },
  },
  \"packages\": {
    \"@tobilu/qmd\": [\"$PIN16\", \"\", {}, \"$INTEG16\"],
  },
}" 0
  # The verdict must be HOST-LOCALE INDEPENDENT: a valid binding in a lockfile carrying a
  # non-ASCII byte elsewhere (a unicode author/description in another entry's metadata) must
  # still PASS on an ASCII-locale host. Without an explicit encoding="utf-8", open() decodes
  # with the host locale and .read() raises there, reporting a host defect among the artifact
  # causes. PYTHONCOERCECLOCALE=0 + PYTHONUTF8=0 are required to reach a genuine ASCII locale:
  # PEP 538/540 otherwise coerce LC_ALL=C back to UTF-8 and the case would pass vacuously —
  # asserted as a PRECONDITION, so a python that ignores them fails the case loudly.
  printf '%s' "{\"packages\": {\"@tobilu/qmd\": [\"$PIN16\", \"\", {}, \"$INTEG16\"], \"other\": [\"other@1.0.0\", \"$(printf '\303\234nicode Auth\303\266r')\", {}, \"sha512-OTHER\"]}}" > "$S/t16-utf8.lock"
  ascii_locale='LC_ALL=C LANG=C PYTHONCOERCECLOCALE=0 PYTHONUTF8=0'
  # Assert the PROPERTY, not a codeset string: a locale-dependent open() of this file must
  # actually RAISE under the chosen env. Comparing `getpreferredencoding()` to "utf-8" is
  # case-sensitive and alias-blind ("UTF-8" on glibc, musl/Alpine's C locale), so the guard could
  # declare an ASCII locale reached while the case ran under UTF-8 — passing vacuously with the
  # encoding= fix removed. This probe cannot be aliased away.
  if env $ascii_locale python3 -c 'import sys; open(sys.argv[1]).read()' "$S/t16-utf8.lock" 2>/dev/null; then
    enc="$(env $ascii_locale python3 -c 'import locale; print(locale.getpreferredencoding(False))' 2>/dev/null)"
    verdict bad "T16 non-ASCII lockfile on ASCII locale (precondition: locale-dependent open did NOT raise; enc='$enc' — no genuine ASCII locale on this host)"
  else
    enc="$(env $ascii_locale python3 -c 'import locale; print(locale.getpreferredencoding(False))' 2>/dev/null)"
    ( eval "export $ascii_locale"; qmd_lock_binding_ok "$S/t16-utf8.lock" "$PIN16" "$INTEG16" ); rc=$?
    [ $rc -eq 0 ] \
      && verdict ok "T16 non-ASCII lockfile on ASCII locale ($enc) -> pass (locale-independent)" \
      || verdict bad "T16 non-ASCII lockfile on ASCII locale (rc=$rc, want 0)"
  fi
fi

echo "----"
if [ "$skip" -gt 0 ]; then echo "RESULT: $pass passed, $fail failed, $skip skipped (host capability)"; else echo "RESULT: $pass passed, $fail failed"; fi
# A verdict that can say "pass" without having asked the question is the defect class this suite
# exists to catch, so it must not have it itself. `skipped()` is deliberately NOT a failure (see
# T15g: a hard red on every Mac would train readers to discount reds), but that alone would let a
# host on which preconditions become unconstructible print a green over a fraction of the cases.
#
# So the invariant is COMPLETENESS, not passes: every declared case must reach a verdict of some
# kind. pass + fail + skip == EXPECTED_CASES means no case silently vanished; it stays true when
# T15g legitimately skips, and it goes false the moment cases stop running at all.
# Adding or removing a case must move this number in the same diff.
EXPECTED_CASES=54
accounted=$((pass + fail + skip))
if [ "$accounted" -ne "$EXPECTED_CASES" ]; then
  echo "INCOMPLETE: $accounted of $EXPECTED_CASES declared cases reached a verdict ($pass passed, $fail failed, $skip skipped)."
  echo "  Cases stopped running rather than failing — the harness broke, or EXPECTED_CASES is stale."
  echo "  If a case was added or removed, the diff that did it must move EXPECTED_CASES too."
  fail=$((fail + 1))
elif [ "$skip" -gt 0 ]; then
  # Loud, but NOT a failure — the suite's own rule, kept.
  echo "NOTE: $skip case(s) SKIPPED on host capability (not a failure). Every declared case is accounted for."
  echo "  A skip means the HOST changed (filesystem, locale, python3, PATH), not that the wrapper is wrong."
  echo "  Adapt the case to the host; never delete it and never weaken its assertion to recover green."
fi
# Why this is keyed on `accounted` and not on `fail`: a FAILURE IS A VERDICT, so real failures keep
# the count balanced and INCOMPLETE stays silent — the failures printed above are then the whole
# diagnosis, with no "cases missing" line pointing the reader at the host. (If the count is ALSO
# stale, both fire and INCOMPLETE does add one to `fail`; the exit status is then a count of
# problems, not of failed cases, which is the honest reading of that state.)
AFTER="$(fingerprint)"
if [ "$BEFORE" = "$AFTER" ]; then echo "repo .qmd/ untouched by this suite — confirmed"; else echo "repo .qmd/ CHANGED during the suite — VIOLATION"; fail=$((fail+1)); fi
exit "$fail"
