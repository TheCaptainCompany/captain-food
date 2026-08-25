#!/usr/bin/env bash
# Guard tests for .claude/hooks/register-check.sh, plus the wiring- and drift-checks that keep the
# register-check discipline alive (run from stop-gate.sh on every turn -- pure shell, ~200ms).
#
# WHY THIS EXISTS. "A gate never seen to fire is an unverified claim" (#292, beck): each case below
# shows the hook red or green against the REAL script before any session trusts it. The hook is
# exactly the silent-when-broken shape ADR-20260810-231300 warns about -- a matcher typo or a
# removed settings entry disarms it with no signal -- so cases W/W1-W3 assert the wiring
# SEMANTICALLY (event + exact matcher + command, with planted disarming mutants), case D
# asserts every standing agent still carries its citation block, and the R cases assert the
# REG-2/REG-4 row gate (ADR-20260821-095957) actually loads and reads decision rows: the BLOCK
# case is the load-proof, because with the legacy lane a hook that parsed nothing would pass
# every ALLOW case indistinguishably (beck, 2026-08-21 briefing).
#
# Hermetic: payload cases run against FIXTURE rows in a throwaway dir via REGISTER_CHECK_DECISIONS
# (never the live corpus, whose statuses change) with the log at /dev/null; the L cases then prove
# the LIVE corpus wiring on two anchors that cannot legitimately change (REG-2 is decided forever
# -- a reversal opens a NEW row, never reopens the file).
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HOOK="$ROOT/.claude/hooks/register-check.sh"
[ -f "$HOOK" ] || { echo "register-check selftest: cannot find $HOOK" >&2; exit 2; }

# ── This selftest testifies about its own inputs ─────────────────────────────────────────────
# GATE-SELF-VERIFICATION-V2 -- same block as the decision-lookup stub suite, and pinned in both by
# `assert_gate_script_self_verifies` (tools/codegen-rs/src/tests.rs), which runs in the `codegen`
# job: a DIFFERENT job with its own checkout, outside the blast radius of anything the `changes`
# job does. The eighth review of PR #679 planted `find . -name 'register-check-selftest.sh' -exec
# cp /tmp/exit0.sh {} +` in an innocent-looking step and the whole gate went green with the hook
# replaced by `exit 0`. A substring scan of step definitions cannot bound arbitrary shell; asking
# "is the script about to run the one in the commit?" can.
#
# DEFAULT-ON with a named opt-out. stop-gate.sh -- the interactive path, where editing a hook and
# re-running is the normal loop -- passes REGISTER_CHECK_ALLOW_DIRTY=1 explicitly. CI invokes this
# script directly and gets the comparison. Both opt-out names are forbidden as workflow/job/step
# `env:` keys by the codegen pin, so the CI path cannot be talked out of verifying.
#
# It DETECTS the named overwrite routes. It is not a defence against arbitrary code running first.
if [ "${REGISTER_CHECK_ALLOW_DIRTY:-}" = "1" ]; then
  echo "self-verification: OPTED OUT (REGISTER_CHECK_ALLOW_DIRTY=1) -- gate scripts NOT compared to HEAD."
else
  unset -f git cmp command 2>/dev/null || true
  _vpath="/usr/bin:/bin:/usr/local/bin"
  _git="$(PATH="$_vpath" command -v git || true)"
  _cmp="$(PATH="$_vpath" command -v cmp || true)"
  if [ -z "$_git" ] || [ -z "$_cmp" ]; then
    echo "FATAL: git or cmp not found on $_vpath -- refusing to report on scripts that cannot be verified." >&2
    exit 1
  fi
  echo "self-verification: comparing gate scripts against their committed blobs at HEAD."
  for rel in .claude/hooks/register-check.sh .claude/hooks/register-check-selftest.sh; do
    if ! "$_git" -C "$ROOT" cat-file -e "HEAD:$rel" 2>/dev/null; then
      echo "FATAL: $rel is not tracked at HEAD -- refusing to report on scripts CI cannot verify." >&2
      exit 1
    fi
    if ! "$_git" -C "$ROOT" cat-file blob "HEAD:$rel" 2>/dev/null | "$_cmp" -s - "$ROOT/$rel"; then
      echo "FATAL: $rel differs from the committed blob at HEAD." >&2
      echo "  Something modified a gate script between checkout and this run -- the disarm shape" >&2
      echo "  this check exists to DETECT. A green here would be a lie." >&2
      echo "  Editing it locally? Re-run with REGISTER_CHECK_ALLOW_DIRTY=1." >&2
      exit 1
    fi
  done
  echo "self-verification: OK -- both gate scripts are byte-identical to HEAD."
fi

FIX="$(mktemp -d "${TMPDIR:-/tmp}/register-check-selftest.XXXXXX")"
trap 'rm -rf "$FIX"' EXIT
cat > "$FIX/OPEN-ROW.yaml" <<'EOF'
key: "OPEN-ROW"
status: "open"
owner: "founder"
EOF
cat > "$FIX/OPEN-TWO.yaml" <<'EOF'
key: "OPEN-TWO"
status: "open"
owner: "founder"
EOF
cat > "$FIX/GONE-ROW.yaml" <<'EOF'
key: "GONE-ROW"
status: "decided"
owner: "founder"
decided: "2026-08-19"
decided_by: "ADR-20260819-103112"
EOF
cat > "$FIX/DEFER-ROW.yaml" <<'EOF'
key: "DEFER-ROW"
status: "deferred"
owner: "team"
until: "after one order flows end to end (#556)"
EOF
cat > "$FIX/LAW-ROW.yaml" <<'EOF'
key: "LAW-ROW"
status: "open"
owner: "counsel"
EOF
cat > "$FIX/_legacy.yaml" <<'EOF'
legacy:
  - OLD-ROW
EOF

fail=0
expect() { # expect <case> <want-exit> <decisions-dir> <payload> [want-reason]
  # want-reason (optional) is compared EXACTLY against the hook log's reason field: a case that
  # goes red for the WRONG rule is a claim without evidence — E5 sat green for months carried by
  # key-unknown while the envelope-multiple lane it names was never exercised (PR #669 review, F2).
  local case="$1" want="$2" dir="$3" payload="$4" want_reason="${5:-}" got reason log="$FIX/case.log"
  : > "$log"
  printf '%s' "$payload" | REGISTER_CHECK_LOG="$log" REGISTER_CHECK_DECISIONS="$dir" bash "$HOOK" >/dev/null 2>&1
  got=$?
  if [ "$got" -ne "$want" ]; then
    echo "register-check selftest: case $case FAILED (want exit $want, got $got)" >&2
    fail=1
  fi
  if [ -n "$want_reason" ]; then
    reason="$(tail -1 "$log" 2>/dev/null | cut -f3)"
    if [ "$reason" != "$want_reason" ]; then
      echo "register-check selftest: case $case FAILED (want reason '$want_reason', got '${reason:-none}')" >&2
      fail=1
    fi
  fi
}

TRAIL='Register check: no controlling record -- terms: fixture; nearest: none'

# ── The trail check (ADR-20260821-010543) ───────────────────────────────────────────────────────
# 1 BLOCK: no trail at all -- the incident shape (ADR-20260818-210000 defect 2).
expect 1-no-trail 2 "$FIX" '{"questions":[{"question":"Which funding model applies to tips?"}]}' trail-missing
# 2 BLOCK: bare marker token without a record id or the explicit negative -- the cargo-cult trail.
expect 2-hollow-trail 2 "$FIX" '{"questions":[{"question":"Which funding model? Register check: done"}]}' trail-hollow
# 3 ALLOW: trail citing a controlling record id.
expect 3-record-id 0 "$FIX" '{"questions":[{"question":"Confirm scope. Register check: ADR-20260819-103112 (2026-08-19, decided) -- covers refunds, silent on thresholds"}]}'
# 4 ALLOW: legacy ADR id form.
expect 4-legacy-id 0 "$FIX" '{"questions":[{"question":"... Register check: ADR-0032 (completeness) covers this"}]}'
# 5 ALLOW: explicit negative with terms -- a genuinely new question is the system working.
expect 5-no-record 0 "$FIX" '{"questions":[{"question":"New option space. Register check: no controlling record -- terms: payout, settlement, virement; nearest: none"}]}'
# 6 ALLOW: DECISIONS register section citation.
expect 6-register-row 0 "$FIX" '{"questions":[{"question":"... Register check: DECISIONS.md section 48 (open)"}]}'
# 7 BLOCK: empty stdin -- fail closed, never fail open (ADR-20260810-231300).
expect 7-empty-input 2 "$FIX" '' empty-input

# ── The row gate (REG-2/REG-4, ADR-20260821-095957) ─────────────────────────────────────────────
# R1 BLOCK: a well-trailed question referencing a DECIDED row -- the founder's own rule, and the
#    load-proof that the fixture rows were actually parsed.
expect R1-decided-key 2 "$FIX" "{\"questions\":[{\"question\":\"Should we revisit GONE-ROW? $TRAIL\"}]}" key-decided
# R2 ALLOW: a question referencing an OPEN row is exactly what the queue is for.
expect R2-open-key 0 "$FIX" "{\"questions\":[{\"question\":\"OPEN-ROW options A/B? $TRAIL\"}]}"
# R3 ALLOW: a legacy-allowlisted key passes (no backfill; migrate at next touch), logged.
expect R3-legacy-key 0 "$FIX" "{\"questions\":[{\"question\":\"About OLD-ROW: which change carries it? $TRAIL\"}]}" key-legacy
# R4 BLOCK: a DEFERRED row is un-askable until its wake condition; the refusal cites `until`.
expect R4-deferred-key 2 "$FIX" "{\"questions\":[{\"question\":\"Can we do DEFER-ROW now? $TRAIL\"}]}" key-deferred
# R5 ALLOW (FLIPPED 2026-08-21, ADR-20260821-103403): a PASSIVE mention of an open counsel-owned
#    row is context, not the ask -- the counsel routing now binds the ENVELOPE lane (E7/E8).
expect R5-counsel-passive 0 "$FIX" "{\"questions\":[{\"question\":\"Context: LAW-ROW is still open. $TRAIL\"}]}"
# R7 ALLOW: key-shaped tokens declared nowhere are not register references in PROSE (the envelope
#    lane rejects them as E4; free-text enforcement stays un-mechanical, recorded in the ADR).
expect R7-unknown-key 0 "$FIX" "{\"questions\":[{\"question\":\"NOT-A-ROW and GONE-ROWBOAT are not references. $TRAIL\"}]}"
# R8 BLOCK: a broken REGISTER_CHECK_DECISIONS override fails closed, never silently skips.
expect R8-override-broken 2 "$FIX/absent" "{\"questions\":[{\"question\":\"Anything. $TRAIL\"}]}" override-broken

# ── The envelope lane (decision-ask-unregistered, ADR-20260821-103403) ──────────────────────────
# E1 ALLOW: a decision question = one `Decision row:` naming a declared OPEN row; the envelope IS
#    the register check, so no trail line is required. (Old hook: BLOCK trail-missing -- flipped.)
expect E1-envelope-open 0 "$FIX" '{"questions":[{"question":"Decision row: OPEN-ROW -- option A or B?"}]}' ok
# E2 BLOCK: the envelope on a DECIDED row -- the reversal path is a NEW row with reconsiders.
expect E2-envelope-decided 2 "$FIX" '{"questions":[{"question":"Decision row: GONE-ROW -- revisit?"}]}' key-decided
# E3 BLOCK: the envelope on a LEGACY key -- legacy is not a bypass; migrate in the same change,
#    then the SAME question passes live. (Old hook with a trail: ALLOW key-legacy -- flipped.)
expect E3-envelope-legacy 2 "$FIX" "{\"questions\":[{\"question\":\"Decision row: OLD-ROW -- decide it? $TRAIL\"}]}" key-legacy-ask
# E4 BLOCK: the envelope on an UNKNOWN key -- typo or undeclared; the refusal lists open rows and
#    the create-row path. (Old hook with a trail: ALLOW as a non-reference -- flipped.)
expect E4-envelope-unknown 2 "$FIX" "{\"questions\":[{\"question\":\"Decision row: NO-SUCH-ROW -- decide it? $TRAIL\"}]}" key-unknown
# E5 BLOCK: two envelope lines -- a decision question references EXACTLY ONE declared row. Both
#    keys are OPEN so no other lane can carry the red: the reason MUST be envelope-multiple (the
#    old fixture used OPEN+DECIDED on one line and sat green for the wrong rule -- PR #669, F2).
expect E5-envelope-multiple 2 "$FIX" '{"questions":[{"question":"Decision row: OPEN-ROW\nDecision row: OPEN-TWO -- pick both?"}]}' envelope-multiple
# E5b BLOCK: two tokens on ONE line -- same rule, deliberately, whatever the line layout; the
#    token count (not the extracted-line count) is what the hook gates on.
expect E5b-envelope-multiple-same-line 2 "$FIX" '{"questions":[{"question":"Decision row: OPEN-ROW and also Decision row: OPEN-TWO"}]}' envelope-multiple
# E6 BLOCK: a garbled envelope (no valid key token) fails loudly, echoing the rejected line.
expect E6-envelope-garbled 2 "$FIX" '{"questions":[{"question":"Decision row: bad-key please?"}]}' envelope-garbled
# E7 BLOCK: the envelope on an open counsel-owned row without the external-action framing.
expect E7-envelope-counsel 2 "$FIX" '{"questions":[{"question":"Decision row: LAW-ROW -- what is the answer?"}]}' key-counsel-owned
# E8 ALLOW: the documented escape -- the question asks for the external action itself.
expect E8-counsel-action 0 "$FIX" '{"questions":[{"question":"Decision row: LAW-ROW -- external action: engage counsel this week?"}]}'

# ── The LIVE corpus wiring (no env override) ────────────────────────────────────────────────────
# L1: the live dir parses and gates -- REG-2 is decided forever (a reversal opens a NEW row).
printf '%s' "{\"questions\":[{\"question\":\"Reopen REG-2? $TRAIL\"}]}" | REGISTER_CHECK_LOG=/dev/null bash "$HOOK" >/dev/null 2>&1
if [ $? -ne 2 ]; then
  echo "register-check selftest: case L1 FAILED -- the LIVE docs/decisions corpus did not gate a question referencing decided row REG-2" >&2
  fail=1
fi
# L3: the live envelope lane rejects an unknown key (proves the live wiring of the new lane).
printf '%s' '{"questions":[{"question":"Decision row: ZZZZ-NOT-DECLARED -- decide?"}]}' | REGISTER_CHECK_LOG=/dev/null bash "$HOOK" >/dev/null 2>&1
if [ $? -ne 2 ]; then
  echo "register-check selftest: case L3 FAILED -- the LIVE corpus did not reject an unknown envelope key" >&2
  fail=1
fi
# L2: the live legacy allowlist exists and is non-empty (legacy is a declaration, not a default).
if ! sed -n 's/^  - \([A-Z][A-Z0-9-]*\)$/\1/p' "$ROOT/docs/decisions/_legacy.yaml" 2>/dev/null | grep -q .; then
  echo "register-check selftest: case L2 FAILED -- docs/decisions/_legacy.yaml missing or lists no legacy keys" >&2
  fail=1
fi

# W WIRING (SEMANTIC since the 2026-08-21 hardening slice): the arming declaration is checked
# structurally, not by substring -- the old greps stayed green with the matcher fuzzed
# (AskUserQuestionX) or the whole entry moved to PostToolUse, both of which disarm the gate.
# check_wiring proves .claude/settings.json carries a hooks.PreToolUse entry whose matcher is
# EXACTLY AskUserQuestion and whose command runs the real script. python3 (stdlib json only, no
# added toolchain) is required; its absence FAILS the case -- fail closed, never a silent skip.
check_wiring() { # check_wiring <settings.json> -> 0 armed / nonzero not
  command -v python3 >/dev/null 2>&1 || return 3
  python3 - "$1" <<'PYEOF'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception:
    sys.exit(1)
for entry in d.get("hooks", {}).get("PreToolUse", []):
    if entry.get("matcher") != "AskUserQuestion":
        continue
    for h in entry.get("hooks", []):
        if h.get("type") == "command" and h.get("command", "").endswith("/.claude/hooks/register-check.sh"):
            sys.exit(0)
sys.exit(1)
PYEOF
}
# The three disarming shapes, planted as mutant fixtures derived from the REAL committed file, so
# each red case proves the checker sees through exactly one disarming move.
if ! python3 - "$ROOT/.claude/settings.json" "$FIX" <<'PYEOF'
import copy, json, sys
src, out = sys.argv[1], sys.argv[2]
d = json.load(open(src))
entry = d["hooks"]["PreToolUse"][0]
assert "register-check.sh" in entry["hooks"][0]["command"], "settings.json PreToolUse[0] is no longer the register-check entry -- update the mutant builder"
m1 = copy.deepcopy(d); m1["hooks"]["PreToolUse"][0]["matcher"] = "AskUserQuestionX"
m2 = copy.deepcopy(d); m2["hooks"]["PostToolUse"] = m2["hooks"].pop("PreToolUse")
m3 = copy.deepcopy(d); m3["hooks"]["PreToolUse"][0]["hooks"][0]["command"] = "bash \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/some-other-hook.sh"
for name, m in [("settings-mutant-matcher.json", m1), ("settings-mutant-event.json", m2), ("settings-mutant-command.json", m3)]:
    json.dump(m, open(f"{out}/{name}", "w"), indent=1)
PYEOF
then
  echo "register-check selftest: case W FAILED -- python3 missing or the settings mutant builder broke (fail closed)" >&2
  fail=1
elif ! check_wiring "$ROOT/.claude/settings.json"; then
  echo "register-check selftest: case W FAILED -- .claude/settings.json no longer wires register-check.sh to a PreToolUse/AskUserQuestion declaration (the gate is disarmed)" >&2
  fail=1
else
  # W1 fuzzed matcher / W2 wrong event / W3 wrong command: the checker must refuse each.
  check_wiring "$FIX/settings-mutant-matcher.json" && { echo "register-check selftest: case W1 FAILED -- checker accepted matcher AskUserQuestionX" >&2; fail=1; }
  check_wiring "$FIX/settings-mutant-event.json"   && { echo "register-check selftest: case W2 FAILED -- checker accepted the entry under PostToolUse" >&2; fail=1; }
  check_wiring "$FIX/settings-mutant-command.json" && { echo "register-check selftest: case W3 FAILED -- checker accepted a command pointing at another script" >&2; fail=1; }
fi

# D DRIFT: every standing agent carries the citation block (marker + pointer to the canonical rule).
for f in "$ROOT"/.claude/agents/*.md; do
  if ! grep -qF 'Register check:' "$f" || ! grep -q 'check the register before you ask' "$f"; then
    echo "register-check selftest: case D FAILED -- $(basename "$f") lacks the register-check citation block (docs/claude/sessions/workflow.md is the canonical rule)" >&2
    fail=1
  fi
done

# C CANON: the canonical rule the blocks and the hook cite still exists where they point.
if ! grep -q 'check the register before you ask' "$ROOT/docs/claude/sessions/workflow.md"; then
  echo "register-check selftest: case C FAILED -- docs/claude/sessions/workflow.md no longer carries the canonical register-check rule the blocks cite" >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo "register-check selftest: FAILED (see cases above)" >&2
  exit 2
fi
echo "register-check selftest: all cases pass."
exit 0
