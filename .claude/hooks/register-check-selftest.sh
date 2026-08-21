#!/usr/bin/env bash
# Guard tests for .claude/hooks/register-check.sh, plus the wiring- and drift-checks that keep the
# register-check discipline alive (run from stop-gate.sh on every turn -- pure shell, ~100ms).
#
# WHY THIS EXISTS. "A gate never seen to fire is an unverified claim" (#292, beck): each case below
# shows the hook red or green against the REAL script before any session trusts it. And the hook is
# exactly the silent-when-broken shape ADR-20260810-231300 warns about -- a matcher typo or a
# removed settings entry disarms it with no signal -- so case W asserts the wiring exists, and case
# D asserts every standing agent still carries its citation block (16 hand-pasted blocks are copy
# drift waiting to happen; this is the executable fence for "cite, never fork").
#
# Hermetic: fixtures are inline strings piped to the script; nothing here reads or writes state
# beyond the hook's own append-only log.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HOOK="$ROOT/.claude/hooks/register-check.sh"
[ -f "$HOOK" ] || { echo "register-check selftest: cannot find $HOOK" >&2; exit 2; }

fail=0
expect() { # expect <case> <want-exit> <payload>
  local case="$1" want="$2" payload="$3" got
  printf '%s' "$payload" | REGISTER_CHECK_LOG=/dev/null bash "$HOOK" >/dev/null 2>&1
  got=$?
  if [ "$got" -ne "$want" ]; then
    echo "register-check selftest: case $case FAILED (want exit $want, got $got)" >&2
    fail=1
  fi
}

# 1 BLOCK: no trail at all -- the incident shape (ADR-20260818-210000 defect 2).
expect 1-no-trail 2 '{"questions":[{"question":"Which funding model applies to tips?"}]}'
# 2 BLOCK: bare marker token without a record id or the explicit negative -- the cargo-cult trail.
expect 2-hollow-trail 2 '{"questions":[{"question":"Which funding model? Register check: done"}]}'
# 3 ALLOW: trail citing a controlling record id.
expect 3-record-id 0 '{"questions":[{"question":"Confirm scope. Register check: ADR-20260819-103112 (2026-08-19, decided) -- covers refunds, silent on thresholds"}]}'
# 4 ALLOW: legacy ADR id form.
expect 4-legacy-id 0 '{"questions":[{"question":"... Register check: ADR-0032 (completeness) covers this"}]}'
# 5 ALLOW: explicit negative with terms -- a genuinely new question is the system working.
expect 5-no-record 0 '{"questions":[{"question":"New option space. Register check: no controlling record -- terms: payout, settlement, virement; nearest: none"}]}'
# 6 ALLOW: DECISIONS register section citation.
expect 6-register-row 0 '{"questions":[{"question":"... Register check: DECISIONS.md section 48 REG-1 (open)"}]}'
# 7 BLOCK: empty stdin -- fail closed, never fail open (ADR-20260810-231300).
expect 7-empty-input 2 ''

# W WIRING: the settings entry that arms the hook exists and points at the real script.
if ! grep -q 'AskUserQuestion' "$ROOT/.claude/settings.json" || \
   ! grep -q 'register-check\.sh' "$ROOT/.claude/settings.json"; then
  echo "register-check selftest: case W FAILED -- .claude/settings.json no longer wires register-check.sh to AskUserQuestion (the gate is disarmed)" >&2
  fail=1
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
