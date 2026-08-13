#!/usr/bin/env bash
# Guard tests for .claude/hooks/loop-budget.sh (invoked as `loop-budget.sh selftest`, and run on
# every turn by .claude/hooks/stop-gate.sh).
#
# WHY THIS EXISTS. The budget hook produced SEVEN distinct failures in one day across independent
# sessions, every one of them a guard that either did not exist or silently did nothing. "A guard
# never seen to fire is an unverified claim" (#292), so each numbered case below reproduces one of
# those failures against the REAL script and asserts it is now refused. Prose in loops.md cannot do
# this: prose can be skipped, a red gate cannot.
#
# Hermetic: everything happens in a throwaway `git init` repo under $TMPDIR. It never reads or
# writes this repo's ledger, config or timer. Runs in about a second (bash + node, no network,
# no cargo).
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/.claude/hooks/loop-budget.sh"
SELF="$ROOT/.claude/hooks/loop-budget-selftest.sh"
[ -f "$SRC" ] || { echo "loop-budget selftest: cannot find $SRC" >&2; exit 2; }

TMP="$(mktemp -d "${TMPDIR:-/tmp}/loop-budget-selftest.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
REPO="$TMP/repo"
HOOK="$REPO/.claude/hooks/loop-budget.sh"
TIMER="$REPO/.git/loop-budget-timer.json"
LEDGER="$REPO/.claude/loop-budget"
BUDGET=1000     # seconds; small so the exhaustion case is cheap to reach

mkdir -p "$REPO/.claude/hooks"
cp "$SRC" "$HOOK"
cp "$SELF" "$REPO/.claude/hooks/loop-budget-selftest.sh"
printf '{\n  "weeklyBudgetSeconds": %s\n}\n' "$BUDGET" > "$REPO/.claude/loop-budget.json"
git -C "$REPO" init -q
git -C "$REPO" config user.email "selftest@captain.food"
git -C "$REPO" config user.name  "loop-budget selftest"
git -C "$REPO" config commit.gpgsign false
git -C "$REPO" add -A >/dev/null
git -C "$REPO" commit -qm "selftest fixture" >/dev/null

pass=0; fail=0
OUT=""; CODE=0
run() { OUT="$("$@" 2>&1)"; CODE=$?; return 0; }
ok()   { pass=$((pass+1)); printf '  ok   %s\n' "$1"; }
bad()  { fail=$((fail+1)); printf '  FAIL %s\n     %s\n' "$1" "${2:-}" >&2; printf '     output: %s\n' "$OUT" >&2; }
expect_code() { [ "$CODE" = "$2" ] && ok "$1" || bad "$1" "expected exit $2, got $CODE"; }
expect_out()  { case "$OUT" in *"$2"*) ok "$1" ;; *) bad "$1" "expected output to contain: $2" ;; esac; }
# The week's recorded total, computed the same way the hook does: the SUM of the ledger files.
total() { find "$LEDGER" -name '*.json' -type f 2>/dev/null -exec cat {} + | node -e '
  let d=""; process.stdin.on("data",c=>d+=c).on("end",()=>{
    let t=0; for (const m of d.matchAll(/"seconds":\s*(\d+)/g)) t+=Number(m[1]);
    console.log(t);
  });'; }
entries() { find "$LEDGER" -name '*.json' -type f 2>/dev/null | wc -l | tr -d ' '; }
stamp_timer() { printf '{"startedAt": %s, "branch": "selftest"}\n' "$(( $(date +%s) * 1000 - ${1:-0} * 1000 ))" > "$TIMER"; }
# Tracked state must be untouched by everything except an APPENDED ledger file.
config_modified() { git -C "$REPO" status --porcelain -- .claude/loop-budget.json | grep -q . && echo yes || echo no; }

echo "loop-budget selftest ($REPO)"

# --- 1. happy path: start opens an untracked timer, stop appends exactly one ledger file ---------
run bash "$HOOK" start
expect_code "1a start succeeds on a clean repo" 0
[ -f "$TIMER" ] && ok "1b the timer lives INSIDE .git (untrackable by construction)" || bad "1b the timer lives INSIDE .git" "no $TIMER"
[ "$(config_modified)" = no ] && ok "1c start leaves the tracked config UNMODIFIED (failure 7)" || bad "1c start leaves the tracked config unmodified" "config was rewritten"
before="$(entries)"
run bash "$HOOK" stop --note "selftest happy path"
expect_code "1d stop succeeds when a timer is open" 0
[ "$(entries)" = "$((before+1))" ] && ok "1e stop APPENDS exactly one ledger file" || bad "1e stop appends one ledger file" "entries $before -> $(entries)"
[ ! -f "$TIMER" ] && ok "1f stop closes the timer" || bad "1f stop closes the timer" "timer still present"
[ "$(config_modified)" = no ] && ok "1g stop still leaves the tracked config UNMODIFIED" || bad "1g stop leaves the config unmodified" "config was rewritten"

# --- 2. FAILURE 2: stop with no open timer must FAIL LOUDLY, never silently record zero ----------
before="$(entries)"; t0="$(total)"
run bash "$HOOK" stop
expect_code "2a stop without a timer is REFUSED (was: silent exit 0, recorded nothing)" 3
expect_out  "2b ...and says the run would vanish from the cap" "silently vanish"
expect_out  "2c ...and names the honest escape hatch" "--elapsed"
[ "$(entries)" = "$before" ] && ok "2d ...and records nothing" || bad "2d records nothing" "entries changed"
run bash "$HOOK" stop --elapsed 120 --note "run that forgot start"
expect_code "2e the escape hatch records the run for real" 0
[ "$(total)" = "$((t0+120))" ] && ok "2f ...for exactly the stated duration" || bad "2f exact duration" "total $t0 -> $(total)"

# --- 3. FAILURE 3: a stale open timer must never be billed (261m billed for a 16m run) -----------
t0="$(total)"; before="$(entries)"
stamp_timer $((5 * 3600))                       # 5h old, left open by a dead run
run bash "$HOOK" stop
expect_code "3a stop on a 5h-old timer is REFUSED, not billed" 3
expect_out  "3b ...and says why (stale)" "STALE"
[ "$(total)" = "$t0" ] && ok "3c ...and the weekly total is unchanged" || bad "3c total unchanged" "total $t0 -> $(total)"
[ "$(entries)" = "$before" ] && ok "3d ...and no ledger entry was written" || bad "3d no entry written" "entries changed"
[ ! -f "$TIMER" ] && ok "3e ...and the stale timer is discarded so the repo is not stuck" || bad "3e stale timer discarded" "timer survived"

stamp_timer $((5 * 3600))
run bash "$HOOK" start
expect_code "3f start over a stale timer succeeds" 0
expect_out  "3g ...announcing that it DISCARDED it rather than billing it" "DISCARDING a stale open timer"
[ "$(total)" = "$t0" ] && ok "3h ...and bills none of the stale wall clock" || bad "3h nothing billed" "total moved"

# --- 4. FAILURE 4: the total can never go DOWN --------------------------------------------------
# Structural: `stop` only ever CREATES a file, so there is no code path that writes a smaller number.
# Assert it over a mixed sequence, and assert the shape that makes it true (no tracked file is
# ever modified -- only added).
seq_start="$(total)"
run bash "$HOOK" start; run bash "$HOOK" stop --note "seq 1"
mid="$(total)"
run bash "$HOOK" stop --elapsed 30 --note "seq 2"
end="$(total)"
{ [ "$mid" -ge "$seq_start" ] && [ "$end" -ge "$mid" ]; } && ok "4a the weekly total is monotonically non-decreasing" || bad "4a monotonic total" "$seq_start -> $mid -> $end"
[ "$end" = "$((mid+30))" ] && ok "4b ...and each stop ADDS its segment" || bad "4b stop adds" "$mid + 30 != $end"
modified="$(git -C "$REPO" status --porcelain | grep -cv '^??' || true)"
[ "$modified" = 0 ] && ok "4c no TRACKED file is ever modified -- the ledger is append-only" || bad "4c append-only" "$modified tracked file(s) modified"

# --- 5. an already-open timer must be refused, not silently overwritten --------------------------
run bash "$HOOK" start
expect_code "5a start succeeds" 0
run bash "$HOOK" start
expect_code "5b a SECOND start over a live timer is REFUSED" 3
expect_out  "5c ...naming the open timer" "ALREADY OPEN"
run bash "$HOOK" reset
expect_code "5d reset discards it without billing" 0

# --- 6. FAILURE 6: `startedAt` can never reach a tracked file ------------------------------------
# Scoped to the tracked STATE (config + ledger). The hook's own source is excluded on purpose: it
# tracks a copy of loop-budget.sh, whose text naturally mentions the field it refuses to persist.
run bash "$HOOK" start
git -C "$REPO" add -A >/dev/null
if git -C "$REPO" grep -qI 'startedAt' -- .claude/loop-budget.json .claude/loop-budget 2>/dev/null; then
  bad "6a startedAt never appears in tracked state" "found startedAt in the fixture's committed state"
else
  ok "6a startedAt never appears in tracked state, so an open timer cannot travel between branches"
fi
case "$(git -C "$REPO" status --porcelain --ignored=no)" in
  *loop-budget-timer*) bad "6b the timer file is invisible to git" "git sees the timer file" ;;
  *) ok "6b the timer file is invisible to git (it is inside .git/)" ;;
esac
# ...and the same assertion against the REAL repo, which is the regression this case exists for: a
# committed `"startedAt": 1786454264646` sat unclosed on a branch for hours and billed 261 phantom
# minutes to the next run that read it. This is the check that would have gone red the moment it landed.
if git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1; then
  if git -C "$ROOT" grep -qI 'startedAt' -- '.claude/loop-budget*' 2>/dev/null; then
    bad "6c THIS repo's committed budget state carries no open timer" "found startedAt in $ROOT/.claude/loop-budget* -- run: loop-budget.sh status"
  else
    ok "6c THIS repo's committed budget state carries no open timer"
  fi
fi

# --- 7. FAILURE 1: start in one worktree and stop in another bill the SAME timer -----------------
WT="$TMP/linked"
git -C "$REPO" worktree add -q -b linked "$WT" >/dev/null 2>&1
if [ -d "$WT/.claude/hooks" ]; then
  bash "$HOOK" reset >/dev/null 2>&1
  t0="$(total)"
  run bash "$REPO/.claude/hooks/loop-budget.sh" start
  expect_code "7a start in the MAIN worktree" 0
  run bash "$WT/.claude/hooks/loop-budget.sh" stop --note "cross-worktree"
  expect_code "7b stop in a LINKED worktree finds the same timer (was: two independent counters)" 0
  expect_out  "7c ...and records the segment there" "loop budget: +"
  git -C "$REPO" worktree remove --force "$WT" >/dev/null 2>&1 || true
else
  bad "7 cross-worktree timer" "worktree fixture not created"
fi

# --- 8. exhaustion still blocks, and `check` stays strictly read-only ----------------------------
run bash "$HOOK" stop --elapsed "$BUDGET" --note "exhaust the cap"
expect_code "8a a stop that crosses the cap reports exhaustion (exit 2)" 2
run bash "$HOOK" check
expect_code "8b check refuses once the cap is spent" 2
expect_out  "8c ...with the documented message" "weekly loop budget exhausted"
[ "$(config_modified)" = no ] && ok "8d check never writes the tracked config" || bad "8d check is read-only" "config was rewritten"
run bash "$HOOK" start
expect_code "8e start refuses once the cap is spent" 2

# --- 9. FOUNDER OVERRIDE: capIsAStopSign=false makes over-cap a REPORT, never a refusal ----------
# ADR-20260813-132540 ("Don't care about the budget right now understood?", founder 2026-08-12,
# operationalized 2026-08-13). Three properties, each of
# which the cheap wrong implementation (exit 0 for everything) would break:
#   over-cap under the override exits 0 AND billing still appends;
#   INTEGRITY refusals (exit 3 family: double-open, no timer, stale timer) are untouched;
#   the flag ABSENT restores exit 2 -- flipping back is a one-line config edit, proven reversible.
# The cap is exhausted at this point (case 8 spent it), which is exactly the state to test under.
printf '{\n  "weeklyBudgetSeconds": %s,\n  "capIsAStopSign": false\n}\n' "$BUDGET" > "$REPO/.claude/loop-budget.json"
run bash "$HOOK" check
expect_code "9a check over the cap under the override exits 0 (reported, not refused)" 0
expect_out  "9b ...and the exhaustion message stays LOUD on stderr (never silent)" "weekly loop budget exhausted"
run bash "$HOOK" start
expect_code "9c start over the cap under the override opens a timer" 0
expect_out  "9d ...still shouting the over-cap state" "weekly loop budget exhausted"
run bash "$HOOK" start
expect_code "9e INTEGRITY is untouched: a second start over a live timer still refuses (exit 3)" 3
before="$(entries)"; t0="$(total)"
run bash "$HOOK" stop --note "override run"
expect_code "9f stop under the override exits 0 even though the week is over cap" 0
[ "$(entries)" = "$((before+1))" ] && ok "9g ...and billing still APPENDS its ledger file (the override never stops the meter)" || bad "9g billing unchanged" "entries $before -> $(entries)"
run bash "$HOOK" stop
expect_code "9h INTEGRITY is untouched: stop with no open timer still refuses (exit 3)" 3
stamp_timer $((5 * 3600))
run bash "$HOOK" stop
expect_code "9i INTEGRITY is untouched: a stale timer still refuses (exit 3), never billed" 3
printf '{\n  "weeklyBudgetSeconds": %s\n}\n' "$BUDGET" > "$REPO/.claude/loop-budget.json"
run bash "$HOOK" check
expect_code "9j the flag ABSENT restores the stop sign (exit 2) -- the recorded path back works" 2

printf 'loop-budget selftest: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ] || { echo "loop-budget selftest: FAILED -- a budget guard is not firing (see above)." >&2; exit 2; }
exit 0
