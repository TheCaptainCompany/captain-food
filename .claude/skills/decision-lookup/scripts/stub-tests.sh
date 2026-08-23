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
pass=0; fail=0
verdict() { if [ "$1" = ok ]; then pass=$((pass+1)); echo "PASS  $2"; else fail=$((fail+1)); echo "FAIL  $2"; fi }

fingerprint() { # the real cache must be untouched by this suite, whether present or absent
  if [ -e "$REPO_ROOT/.qmd" ]; then find "$REPO_ROOT/.qmd" -printf '%p %s %T@\n' 2>/dev/null | sort | md5sum; else echo absent; fi
}
BEFORE="$(fingerprint)"

mkfake() { # $1 = QDIR, $2 = payload file for `search`
  mkdir -p "$1/tool/node_modules/.bin"
  cat > "$1/tool/node_modules/.bin/qmd" <<EOF
#!/usr/bin/env bash
case "\$1" in
  init|collection) exit 0 ;;
  update) exit \${FAKE_UPDATE_EXIT:-0} ;;
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

# T3 --install without bun -> ACTIVATION FAILED, exit != 0
Q="$S/t3"; mkdir -p "$Q"
out="$(env PATH=/usr/bin:/bin DECISION_LOOKUP_HOME="$Q" bash "$W" --install 2>&1)"; rc=$?
[ $rc -ne 0 ] && echo "$out" | grep -q "ACTIVATION FAILED: bun runtime not present" \
  && echo "$out" | grep -q "remove .qmd/ before any future approved retry" \
  && verdict ok "T3 no-bun install exit $rc" || verdict bad "T3 no-bun install (rc=$rc)"

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
# never the empty-result wording; exit 0; no retry, no repair
Q="$S/t8"; mkfake "$Q" /dev/null
out="$(FAKE_SEARCH_EXIT=7 DECISION_LOOKUP_HOME="$Q" "$W" "x" 2>&1)"; rc=$?
[ $rc -eq 0 ] && echo "$out" | grep -q "qmd search failed (exit 7)" \
  && echo "$out" | grep -q "a tool failure, not an empty result" \
  && ! echo "$out" | grep -q "no result — the index is Markdown-only" \
  && verdict ok "T8 search failure -> distinct tool-failure fallback, exit 0" || verdict bad "T8 (rc=$rc)"

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

echo "----"
echo "RESULT: $pass passed, $fail failed"
AFTER="$(fingerprint)"
if [ "$BEFORE" = "$AFTER" ]; then echo "repo .qmd/ untouched by this suite — confirmed"; else echo "repo .qmd/ CHANGED during the suite — VIOLATION"; fail=$((fail+1)); fi
exit "$fail"
