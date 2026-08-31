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
# Identity is AMBIENT: inside a Claude Code session $CLAUDE_CODE_SESSION_ID is set, and the hook
# reads it as the run's owner. A fixture that INHERITS it tests the caller's identity, not the
# code, and every "different run" case would silently collapse into one run. So every invocation
# below states its identity explicitly: `run` = a run with NO identity, `run_as <id>` = that run.
run()    { OUT="$(env -u CLAUDE_CODE_SESSION_ID -u LOOP_BUDGET_RUN_ID "$@" 2>&1)"; CODE=$?; return 0; }
run_as() { local _id="$1"; shift; OUT="$(env -u CLAUDE_CODE_SESSION_ID LOOP_BUDGET_RUN_ID="$_id" "$@" 2>&1)"; CODE=$?; return 0; }
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
# A segment measured from a TIMER is wall-clock, so it lands a second or two above the stamped age.
# The window is far too small to confuse 120s with 600s -- which is exactly the property under test.
near() { [ "$1" -ge "$2" ] && [ "$1" -le "$(($2 + ${3:-5}))" ]; }
entries() { find "$LEDGER" -name '*.json' -type f 2>/dev/null | wc -l | tr -d ' '; }
stamp_timer() { printf '{"startedAt": %s, "branch": "selftest"}\n' "$(( $(date +%s) * 1000 - ${1:-0} * 1000 ))" > "$TIMER"; }
# Tracked state must be untouched by everything except an APPENDED ledger file.
config_modified() { git -C "$REPO" status --porcelain -- .claude/loop-budget.json | grep -q . && echo yes || echo no; }
# The timer file of an IDENTIFIED run. Keying the path on the owner is what makes another run's
# timer unaddressable rather than merely detected.
own_timer() { printf '%s/.git/loop-budget-timer--%s.json\n' "$REPO" "$1"; }
stamp_owned() { printf '{"startedAt": %s, "branch": "%s", "owner": "%s"}\n' \
  "$(( $(date +%s) * 1000 - $2 * 1000 ))" "${3:-selftest}" "$1" > "$(own_timer "$1")"; }
# The ledger path `stop` PRINTS. Asserting on the artifact the hook names beats globbing the
# directory: several fixture segments share a one-second stamp, so a sort would pick by random suffix.
receipt_path() { printf '%s' "$OUT" | sed -n 's/.*recorded as \([^ ]*\) (a NEW file.*/\1/p' | tail -1; }

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
run bash "$HOOK" stop --elapsed-seconds 30 --note "seq 2"
end="$(total)"
{ [ "$mid" -ge "$seq_start" ] && [ "$end" -ge "$mid" ]; } && ok "4a the weekly total is monotonically non-decreasing" || bad "4a monotonic total" "$seq_start -> $mid -> $end"
[ "$end" = "$((mid+30))" ] && ok "4b ...and each stop ADDS its segment" || bad "4b stop adds" "$mid + 30 != $end"
modified="$(git -C "$REPO" status --porcelain | grep -cv '^??' || true)"
[ "$modified" = 0 ] && ok "4c no TRACKED file is ever modified -- the ledger is append-only" || bad "4c append-only" "$modified tracked file(s) modified"

# --- 4bis. THE SECONDS/MINUTES TRAP (#597): a 60x silent under-report is worse than a refusal ----
# The flag takes SECONDS while every number this tool prints is MINUTES. `--elapsed 16` meaning
# "16 minutes" recorded 0.3m and printed success -- the ledger under-reported an entire session and
# nothing said so. The ambiguous spelling now refuses the range where that mistake lives; the
# spelling that states its unit accepts any value, because a genuinely short segment is real.
before_trap="$(total)"
run bash "$HOOK" stop --elapsed 16
expect_code "4d --elapsed with a sub-minute value is REFUSED (the 60x trap)" 64
expect_out  "4e ...and the refusal names the unit and offers both readings" "Did you mean minutes?"
[ "$(total)" = "$before_trap" ] && ok "4f ...and bills NOTHING -- a refused entry is not a silent 0.3m" || bad "4f trap bills nothing" "total moved"
run bash "$HOOK" stop --elapsed-seconds 16 --note "genuinely 16 seconds"
expect_code "4g --elapsed-seconds states its unit, so a short segment is accepted" 0
[ "$(total)" = "$((before_trap+16))" ] && ok "4h ...and bills exactly what it said" || bad "4h short segment billed" "expected $((before_trap+16))"
run bash "$HOOK" stop --elapsed 90 --note "the old spelling, above the trap range"
expect_code "4i the old --elapsed spelling still works above 60s -- recorded incantations keep running" 0

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
  run bash "$HOOK" reset
  t0="$(total)"
  run bash "$REPO/.claude/hooks/loop-budget.sh" start
  expect_code "7a start in the MAIN worktree" 0
  run bash "$WT/.claude/hooks/loop-budget.sh" stop --note "cross-worktree"
  expect_code "7b stop in a LINKED worktree finds the same timer (was: two independent counters)" 0
  expect_out  "7c ...and records the segment there" "loop budget: +"
  # ...but a SHARED ANCHOR must not become a shared LICENCE. `git worktree` does NOT isolate the
  # timer -- it is one file in the common dir -- so the same identity across worktrees is the same
  # run and still bills, while a DIFFERENT run in the linked worktree must not bill this one.
  t0="$(total)"
  run_as wt-run bash "$REPO/.claude/hooks/loop-budget.sh" start
  expect_code "7d start in the MAIN worktree as an identified run" 0
  run_as other-run bash "$WT/.claude/hooks/loop-budget.sh" stop --note "not my timer"
  expect_code "7e a DIFFERENT run stopping in the LINKED worktree is REFUSED, not billed" 3
  [ -f "$(own_timer wt-run)" ] && ok "7f ...and leaves the timer open for the run that owns it" || bad "7f timer survives" "another run consumed it"
  [ "$(total)" = "$t0" ] && ok "7g ...and bills nothing" || bad "7g nothing billed" "total moved"
  run_as wt-run bash "$WT/.claude/hooks/loop-budget.sh" stop --note "same run, other worktree"
  expect_code "7h the SAME run stopping in the linked worktree still bills (the shared anchor holds)" 0
  [ ! -f "$(own_timer wt-run)" ] && ok "7i ...and closes its own timer" || bad "7i timer closed" "timer survived its owner's stop"
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
run bash "$HOOK" status
expect_code "9j status over the cap under the override exits 0 -- the REPORT command must not emit a refusal signal" 0
expect_out  "9k ...while still printing the over-cap state loudly" "loop budget"
printf '{\n  "weeklyBudgetSeconds": %s\n}\n' "$BUDGET" > "$REPO/.claude/loop-budget.json"
run bash "$HOOK" check
expect_code "9l the flag ABSENT restores the stop sign (exit 2) -- the recorded path back works" 2
run bash "$HOOK" status
expect_code "9m ...and status over the cap exits 2 again with the flag absent" 2

# --- 10. D1: ONE TIMER SLOT FOR N CONCURRENT RUNS -- `stop` billed whatever it found -------------
# The 2026-W36 ledger records this collision twice: one segment notes "a concurrent session in this
# shared checkout closed the timer I inherited at 12:05:26Z", and another carries a 33.3-minute
# UNBILLED REMAINDER after a `stop` reported success having billed 3.2 minutes of a ~39-minute run.
# A `stop` that bills a third of a run and prints success is the defect -- a silent under-count that
# the executor trusting the output then records as fact, and it fails worst exactly when the session
# is most parallel, which is when the cap matters most.
#
# The fix is STRUCTURAL rather than a comparison: a run has an owner identity (--run /
# $LOOP_BUDGET_RUN_ID / $CLAUDE_CODE_SESSION_ID) and the timer FILE is keyed on it, so an identified
# run cannot address another run's timer at all. The ownership COMPARISON survives for the one case
# the keying cannot cover: a timer with no owner (opened by the pre-#821 hook, or by a run with no
# identity at all), which is billable only on an explicit --adopt.
#
# Cases 8-9 spent the fixture's cap on purpose; these cases are about billing, not about the cap, so
# they run with headroom. (`start` under an exhausted cap is a separate assertion -- 8e/9c.)
BIG=100000
printf '{\n  "weeklyBudgetSeconds": %s\n}\n' "$BIG" > "$REPO/.claude/loop-budget.json"
rm -f "$REPO"/.git/loop-budget-timer*.json

run_as run-A bash "$HOOK" start
expect_code "10a start succeeds for an identified run" 0
expect_out  "10b ...and NAMES the run id it opened, so a later stop can be matched to it" "run-A"
run_as run-B bash "$HOOK" start
expect_code "10c a DIFFERENT run opens its OWN timer instead of being refused" 0
{ [ -f "$(own_timer run-A)" ] && [ -f "$(own_timer run-B)" ]; } \
  && ok "10d ...so two concurrent runs hold two timers, and neither has to guess with --elapsed" \
  || bad "10d two concurrent timers" "expected $(own_timer run-A) and $(own_timer run-B)"
run_as run-B bash "$HOOK" start
expect_code "10e a second start from the SAME run is still REFUSED -- a double-open is still a defect" 3

# Give the two runs DIFFERENT durations, so a cross-billed stop is arithmetically visible.
stamp_owned run-A 600 branch-A
stamp_owned run-B 120 branch-B
t0="$(total)"
run_as run-B bash "$HOOK" stop --note "run B"
expect_code "10f run B's stop succeeds" 0
near "$(( $(total) - t0 ))" 120 && ok "10g ...and bills run B's OWN 120s, never run A's 600s (the cross-billing defect)" || bad "10g bills its own run" "total $t0 -> $(total), expected +120"
[ -f "$(own_timer run-A)" ] && ok "10h ...and leaves run A's timer OPEN -- it was never run B's to close" || bad "10h other timer survives" "run B closed run A's timer"
t1="$(total)"
run_as run-A bash "$HOOK" stop --note "run A"
expect_code "10i run A then bills its own run" 0
near "$(( $(total) - t1 ))" 600 && ok "10j ...for its FULL 600s -- no unbilled remainder" || bad "10j no remainder" "total $t1 -> $(total), expected +600"

# The honest escape hatch must stay honest: `stop --elapsed-seconds` cleared the timer
# unconditionally, so the run told to "record it honestly instead" destroyed a live run's timer.
stamp_owned run-A 600 branch-A
t0="$(total)"
run_as run-C bash "$HOOK" stop --elapsed-seconds 300 --note "run C has no timer of its own"
expect_code "10k a run with no timer of its own still records honestly with --elapsed-seconds" 0
[ "$(total)" = "$((t0+300))" ] && ok "10l ...billing exactly its own stated duration" || bad "10l stated duration" "total $t0 -> $(total)"
[ -f "$(own_timer run-A)" ] && ok "10m ...and does NOT delete run A's live timer (was: unconditional clearTimer)" || bad "10m elapsed spares other timers" "the escape hatch closed another run's timer"

run_as run-C bash "$HOOK" reset
expect_code "10n reset with no timer of its own succeeds" 0
[ -f "$(own_timer run-A)" ] && ok "10o ...and leaves run A's timer alone -- reset was the other weapon" || bad "10o reset is ownership-scoped" "reset discarded another run's timer"

run_as run-C bash "$HOOK" stop
expect_code "10p stop with no timer of its own is refused" 3
expect_out  "10q ...and NAMES the other run holding an open timer (theft vs a forgotten start)" "run-A"

# A timer with NO owner: written by the pre-#821 hook, or by a run with no identity. Billing it is
# a real need (the upgrade path), so it must be possible -- but never automatic.
rm -f "$REPO"/.git/loop-budget-timer*.json
stamp_timer 300
t0="$(total)"
run_as run-D bash "$HOOK" stop
expect_code "10r an identified run REFUSES to silently bill an UNOWNED timer" 3
[ "$(total)" = "$t0" ] && ok "10s ...and bills nothing" || bad "10s nothing billed" "total moved"
run_as run-D bash "$HOOK" stop --adopt --note "adopted an unowned timer"
expect_code "10t ...but --adopt bills it deliberately (the upgrade path for a pre-#821 timer)" 0
near "$(( $(total) - t0 ))" 300 && ok "10u ...for its real duration" || bad "10u real duration" "total $t0 -> $(total), expected +300"

# --- 11. D2: the receipt stamped the branch of the checkout `stop` RAN FROM ----------------------
# Live instance, committed as-is rather than edited because the ledger is append-only:
# .claude/loop-budget/2026-W36/20260831T142143Z-0568abb8.json says "branch": "main" while its own
# note describes work that was on 819-founder-invoked-slash-commands. The receipt is the only durable
# record of where a run's time went, and it named a branch the run was never on.
rm -f "$REPO"/.git/loop-budget-timer*.json
git -C "$REPO" checkout -q -B work-branch
stamp_owned run-E 240 work-branch
git -C "$REPO" checkout -q -B some-other-branch
run_as run-E bash "$HOOK" stop --note "started on work-branch, stopped elsewhere"
expect_code "11a stop from a different branch succeeds" 0
receipt="$REPO/$(receipt_path)"
if [ -f "$receipt" ]; then
  grep -q '"branch": "work-branch"' "$receipt" \
    && ok "11b the receipt names the branch the RUN was on, not the checkout stop ran from" \
    || bad "11b receipt branch is true of the run" "receipt says $(grep '"branch"' "$receipt" | tr -d ' ')"
else
  bad "11b receipt branch is true of the run" "stop named no receipt (looked for '$receipt')"
fi

# --- 12. D3: the DOCUMENTED artifact must be the artifact `stop` actually writes -----------------
# The executor protocol's step 8 said "commit .claude/loop-budget.json" -- pure config that nothing
# writes. An executor following the documented protocol literally commits nothing and leaves its run
# unbilled: a systematic under-count of the cap by exactly the people who follow the protocol instead
# of reading the hook source. This case compares the prose against the path the hook JUST wrote, so
# it goes red if either one moves -- it is not asserting a spelling somebody chose.
PROTOCOL="$ROOT/.claude/agents/executor.md"
run_as run-F bash "$HOOK" start
run_as run-F bash "$HOOK" stop --note "what does stop actually write?"
written="$(receipt_path)"
case "$written" in
  .claude/loop-budget/*/*.json) ok "12a stop writes the append-only ledger path ($written)" ;;
  *) bad "12a stop writes the ledger path" "it wrote '$written'" ;;
esac
if [ -f "$PROTOCOL" ]; then
  ledger_dir="$(dirname "$(dirname "$written")")"
  step="$(grep -A6 'Record the budget' "$PROTOCOL" | tr '\n' ' ')"
  case "$step" in
    *"$ledger_dir/"*) ok "12b the protocol's budget step names the ledger directory the hook writes" ;;
    *) bad "12b protocol names the written artifact" "the hook writes $ledger_dir/... but step 8 says: $step" ;;
  esac
  # NOT "the step must never mention the config file" -- prose that names it in order to warn against
  # it is GOOD prose, and a test that forbids the string forbids the fix. The property is about what
  # the instruction POINTS AT: the first budget artifact named after the word "commit" must be the
  # ledger (.claude/loop-budget/...), never the config (.claude/loop-budget.json).
  after_commit="${step#*commit}"
  target=".claude/loop-budget${after_commit#*.claude/loop-budget}"
  case "$target" in
    .claude/loop-budget/*) ok "12c 'commit' in step 8 points at the ledger, not at the config file nothing writes" ;;
    *) bad "12c 'commit' in step 8 points at the ledger" "it points at: $(printf '%.40s' "$target")" ;;
  esac
else
  bad "12 protocol prose" "expected the executor protocol at $PROTOCOL"
fi

printf 'loop-budget selftest: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ] || { echo "loop-budget selftest: FAILED -- a budget guard is not firing (see above)." >&2; exit 2; }
exit 0
