#!/usr/bin/env bash
# Self-imposed WEEKLY time budget for autonomous loops/routines (ADR-0014).
# Claude Code has NO native "minutes per week" cap, so we track it ourselves in committed state that
# survives ephemeral cloud-routine runners. The budget resets each ISO week.
#
#   loop-budget.sh check                 # exit 0 if budget remains this week, 2 if exhausted (skip the run);
#                                        # over-cap exits 0 when config sets "capIsAStopSign": false (see below)
#   loop-budget.sh start                 # check + open a run timer (writes NOTHING tracked)
#   loop-budget.sh stop [--note "..."]   # close the timer and APPEND the segment to the ledger
#   loop-budget.sh stop --elapsed 900    # record a run whose timer was never opened / was stale
#   loop-budget.sh status                # print the week's breakdown, including any open timer
#   loop-budget.sh reset                 # discard an open timer WITHOUT billing it
#   loop-budget.sh prune                 # delete ledger weeks older than the retention window
#   loop-budget.sh selftest              # run the guard tests (also run by .claude/hooks/stop-gate.sh)
#
# ---------------------------------------------------------------------------------------------
# WHY THIS SHAPE (it replaced a single mutable {secondsUsed, startedAt} counter that produced seven
# distinct failures in one day; every property below is STRUCTURAL, not a guard that can be skipped):
#
#   1. THE RUNNING TIMER IS NEVER TRACKED. It lives in `$(git rev-parse --git-common-dir)`, i.e.
#      inside `.git/`, which git cannot track by construction. An open timer therefore cannot be
#      committed, cannot travel between branches, and `start` cannot dirty a working tree. The
#      common dir is SHARED by every linked worktree, so `start` in one checkout and `stop` in
#      another are provably the same timer.
#   2. THE TOTAL IS AN APPEND-ONLY LEDGER, not a counter. Each recorded run writes ONE new file
#      `.claude/loop-budget/<ISO-week>/<stamp>-<rand>.json`; the week's usage is their sum. Nothing
#      ever rewrites a number, so `stop` CANNOT lower the total -- monotonicity is arithmetic, not a
#      check. Two branches recording concurrently create two DIFFERENT files, so they never conflict
#      and a merge is additive automatically. That matters because a `.gitattributes` merge driver
#      would NOT have worked here: custom merge drivers are local-only and never run in GitHub's
#      server-side merge, which is where this repo's PRs actually land.
#   3. `.claude/loop-budget.json` IS PURE CONFIG (`weeklyBudgetSeconds`). Nothing writes it. A file
#      no process mutates is a file that does not conflict.
#
# Configure the cap by editing "weeklyBudgetSeconds" in .claude/loop-budget.json.
# "capIsAStopSign" (same file, default true) decides whether over-cap REFUSES (exit 2) or is only
# REPORTED (exit 0, message unchanged). It gates ONLY the over-cap verdict: billing, the append-only
# ledger, and every integrity refusal (exit 3: stale timer, double-open, vanishing stop) are
# identical in both states. Founder override 2026-08-13; path back = flip the field to true
# (ADR-20260813-132540 records when).
# ---------------------------------------------------------------------------------------------
set -uo pipefail

BUDGET_CMD="${1:-check}"
[ $# -gt 0 ] && shift
BUDGET_ELAPSED=""
BUDGET_NOTE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --elapsed)   BUDGET_ELAPSED="${2:-}"; shift 2 || shift ;;
    --elapsed=*) BUDGET_ELAPSED="${1#*=}"; shift ;;
    --note)      BUDGET_NOTE="${2:-}"; shift 2 || shift ;;
    --note=*)    BUDGET_NOTE="${1#*=}"; shift ;;
    *) echo "loop-budget: unknown argument '$1'" >&2; exit 64 ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"   # repo root (.claude/hooks/../..)

# --- selftest runs before anything else: it is hermetic and never touches this repo's state. ---
if [ "$BUDGET_CMD" = "selftest" ]; then
  exec bash "$ROOT/.claude/hooks/loop-budget-selftest.sh"
fi

# --- audit: the ~10ms invariant check the stop-gate runs on EVERY turn. -------------------------
# Two shapes must never appear in tracked budget state, because each one silently corrupts the cap:
#   `startedAt` -- an OPEN TIMER that got committed. It then travels to every branch and session that
#      checks the file out, and the next `stop` bills the wall clock since a run that ended hours ago
#      (measured: 261 minutes, 36% of the weekly cap, charged for a 16-minute run).
#   `secondsUsed` -- the MUTABLE COUNTER this design replaced. It is the file both "take ours" and
#      "take theirs" silently corrupt on merge; a resurrected one would start eating ledger entries
#      again. In-flight branches still carry it, so a careless merge resolution can bring it back.
# Pure git+grep on purpose: no node, no temp dirs, nothing to make a per-turn hook expensive.
if [ "$BUDGET_CMD" = "audit" ]; then
  if ! git -C "$ROOT" rev-parse --git-dir >/dev/null 2>&1; then
    echo "loop-budget audit: not a git checkout -- nothing to audit." >&2; exit 0
  fi
  found="$(git -C "$ROOT" grep -nI -E '"(startedAt|secondsUsed)"' -- '.claude/loop-budget*' 2>/dev/null || true)"
  if [ -n "$found" ]; then
    echo "⛔ loop-budget audit: TRACKED budget state carries a field that must never be committed:" >&2
    printf '%s\n' "$found" | sed 's/^/     /' >&2
    echo "   .claude/loop-budget.json is CONFIG ONLY (weeklyBudgetSeconds). The running timer lives" >&2
    echo "   untracked in .git/, and usage is the append-only ledger .claude/loop-budget/<week>/*.json." >&2
    echo "   A committed 'startedAt' bills phantom hours to the next run; a resurrected 'secondsUsed'" >&2
    echo "   re-creates the counter whose merges silently discarded other sessions' recorded time." >&2
    echo "   Fix: drop the field (see docs/claude/loops.md); record real time with 'loop-budget.sh stop'." >&2
    exit 2
  fi
  echo "loop-budget audit: tracked budget state is clean (no committed timer, no mutable counter)." >&2
  exit 0
fi

command -v node >/dev/null 2>&1 || { echo "loop-budget: node not found on PATH -- cannot read the budget state." >&2; exit 64; }

BUDGET_CONFIG="$ROOT/.claude/loop-budget.json"
BUDGET_LEDGER="$ROOT/.claude/loop-budget"

# The RUNNING TIMER lives in the git COMMON dir: shared by every linked worktree of this repo, and
# untrackable by construction. `--git-common-dir` prints ".git" (relative to -C) in the main
# worktree and an ABSOLUTE path to the same directory in a linked worktree -- both resolve to one
# shared location, which is exactly the property `start`/`stop` need.
if BUDGET_COMMON="$(git -C "$ROOT" rev-parse --git-common-dir 2>/dev/null)" && [ -n "$BUDGET_COMMON" ]; then
  case "$BUDGET_COMMON" in
    /*|[A-Za-z]:[/\\]*) ;;                      # already absolute (linked worktree, or Windows)
    *) BUDGET_COMMON="$ROOT/$BUDGET_COMMON" ;;  # relative (main worktree prints ".git")
  esac
  BUDGET_TIMER="$BUDGET_COMMON/loop-budget-timer.json"
else
  # No git at all (tarball export, sandbox): fall back to a temp path keyed on the checkout, so
  # start/stop in the SAME checkout still agree. Different checkouts get different timers, which is
  # the best that can be done without a shared anchor.
  BUDGET_TIMER="${TMPDIR:-/tmp}/loop-budget-timer-$(printf '%s' "$ROOT" | cksum | cut -d' ' -f1).json"
fi

BUDGET_LABEL="$(git -C "$ROOT" rev-parse --abbrev-ref HEAD 2>/dev/null || echo '?')"
export BUDGET_CMD BUDGET_CONFIG BUDGET_LEDGER BUDGET_TIMER BUDGET_ELAPSED BUDGET_NOTE BUDGET_LABEL ROOT

node <<'NODE'
'use strict';
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const CONFIG = process.env.BUDGET_CONFIG;
const LEDGER = process.env.BUDGET_LEDGER;
const TIMER  = process.env.BUDGET_TIMER;
const cmd    = process.env.BUDGET_CMD;
const label  = process.env.BUDGET_LABEL || '?';

// A timer older than this was left open by a run that died or never called `stop`. It is STALE by
// definition and is NEVER billed. 4h is chosen against the ORDER OF MAGNITUDE of the cap (read the
// real value from .claude/loop-budget.json -- it is not restated here, and it changes): a single
// unclosed segment beyond 4h would eat a large fraction of the week in one write -- which is
// precisely the observed failure (261 minutes, 36% of the then-cap, billed for a 16-minute run). Real
// segments measured in this repo are tens of minutes; a genuinely longer one is still recordable,
// but only via an explicit `--elapsed`, because inventing 4h+ of budget deserves a human assertion.
const STALE_TIMER_SECONDS = 4 * 3600;
const RETAIN_WEEKS = 6;          // `prune` keeps this many ISO-week directories

const EXIT_OK = 0, EXIT_OVER = 2, EXIT_REFUSED = 3, EXIT_USAGE = 64;

function isoWeek(date) {
  const d = new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate()));
  const dayNum = (d.getUTCDay() + 6) % 7;
  d.setUTCDate(d.getUTCDate() - dayNum + 3);
  const firstThursday = d.getTime();
  d.setUTCMonth(0, 1);
  if (d.getUTCDay() !== 4) d.setUTCMonth(0, 1 + ((4 - d.getUTCDay()) + 7) % 7);
  const week = 1 + Math.ceil((firstThursday - d.getTime()) / 604800000);
  return new Date(firstThursday).getUTCFullYear() + '-W' + String(week).padStart(2, '0');
}
const readJson = (f) => { try { return JSON.parse(fs.readFileSync(f, 'utf8')); } catch (e) { return null; } };
const mins = (x) => (x / 60).toFixed(1);

const now = Date.now();
const wk = isoWeek(new Date(now));
const cfg = readJson(CONFIG) || {};
const budget = Number(cfg.weeklyBudgetSeconds) > 0 ? Number(cfg.weeklyBudgetSeconds) : 1800;
// Default TRUE: an absent field keeps today's behaviour, so branches that predate the field parse
// fine and flipping the override off is deleting one line (ADR-20260813-132540).
const capIsAStopSign = cfg.capIsAStopSign !== false;

// ---- the ledger: one file per recorded segment, the week's usage is their sum -------------------
function weekDir(week) { return path.join(LEDGER, week); }
function segments(week) {
  let names = [];
  try { names = fs.readdirSync(weekDir(week)).filter((n) => n.endsWith('.json')); } catch (e) { return []; }
  return names.sort().map((n) => {
    const e = readJson(path.join(weekDir(week), n));
    return { file: n, seconds: e && Number(e.seconds) > 0 ? Math.round(Number(e.seconds)) : 0, entry: e || {} };
  });
}
function used(week) { return segments(week).reduce((a, s) => a + s.seconds, 0); }

function record(seconds, note) {
  // APPEND-ONLY. A new file every time, so this can never decrease the total and can never conflict
  // with a segment another branch recorded concurrently.
  const before = used(wk);
  const stamp = new Date(now).toISOString().replace(/[-:]/g, '').replace(/\.\d+Z$/, 'Z');
  const file = path.join(weekDir(wk), `${stamp}-${crypto.randomBytes(4).toString('hex')}.json`);
  fs.mkdirSync(weekDir(wk), { recursive: true });
  fs.writeFileSync(file, JSON.stringify({
    week: wk, seconds, recordedAt: new Date(now).toISOString(), branch: label,
    ...(note ? { note } : {}),
  }, null, 2) + '\n');
  const after = used(wk);
  if (after < before) {   // unreachable by construction; assert anyway rather than trust the comment
    fs.unlinkSync(file);
    console.error(`⛔ loop-budget: refusing to record -- the total would DROP ${before}s -> ${after}s. State not changed.`);
    process.exit(EXIT_REFUSED);
  }
  return { file, before, after };
}

// ---- the running timer: untracked, shared across worktrees --------------------------------------
function openTimer() {
  const t = readJson(TIMER);
  if (!t || !Number(t.startedAt)) return null;
  return { startedAt: Number(t.startedAt), branch: t.branch || '?', age: Math.round((now - Number(t.startedAt)) / 1000) };
}
const clearTimer = () => { try { fs.unlinkSync(TIMER); } catch (e) {} };
const describe = (t) => `started ${new Date(t.startedAt).toISOString()} on '${t.branch}', ${mins(t.age)}m ago`;

const total = used(wk);
const over = total >= budget;
const summary = () => `${mins(used(wk))}m / ${mins(budget)}m used (week ${wk})`;

// ---- commands -----------------------------------------------------------------------------------
if (cmd === 'check' || cmd === 'start') {
  // `check` is STRICTLY READ-ONLY, and `start` writes only the untracked timer. Neither can dirty a
  // working tree -- an earlier version called save() on both paths and re-stamped an unrelated
  // checkout, tripping the stop-gate on a branch doing no loop work at all.
  if (over) {
    // The message is IDENTICAL in both flag states, on purpose: the override changes only the exit
    // code, never the loudness -- a silent fallback is worse than the gate it replaced
    // (ADR-20260810-231300(b); the defect class is "degraded and nobody can tell").
    console.error(`⛔ weekly loop budget exhausted: ${summary()}; resets Monday.`);
    if (capIsAStopSign) process.exit(EXIT_OVER);
    console.error(`   capIsAStopSign=false (ADR-20260813-132540): over-cap is REPORTED, not a refusal. Billing and integrity guards unchanged.`);
    if (cmd === 'check') process.exit(EXIT_OK);
  }
  if (cmd === 'start') {
    const t = openTimer();
    if (t && t.age <= STALE_TIMER_SECONDS) {
      console.error(`⛔ loop-budget: a run timer is ALREADY OPEN (${describe(t)}).`);
      console.error(`   Two overlapping runs would bill one segment twice. Close it first:`);
      console.error(`     loop-budget.sh stop                 # bill it from ${new Date(t.startedAt).toISOString()}`);
      console.error(`     loop-budget.sh stop --elapsed <s>   # bill the true duration instead`);
      console.error(`     loop-budget.sh reset                # discard it without billing`);
      process.exit(EXIT_REFUSED);
    }
    if (t) {
      console.error(`⚠ loop-budget: DISCARDING a stale open timer (${describe(t)}, older than ${mins(STALE_TIMER_SECONDS)}m).`);
      console.error(`   It is NOT billed: an unclosed timer measures wall-clock since a dead run, not work done.`);
    }
    fs.writeFileSync(TIMER, JSON.stringify({ startedAt: now, branch: label, pid: process.pid }, null, 2) + '\n');
    console.error(`✓ ${over ? 'loop budget OVER CAP (override active)' : 'loop budget OK'}: ${summary()}. Timer open (untracked: ${TIMER}).`);
    process.exit(EXIT_OK);
  }
  console.error(`✓ loop budget OK: ${summary()}.`);
  const t = openTimer();
  if (t) console.error(`  (a run timer is open: ${describe(t)})`);
  process.exit(EXIT_OK);
}

if (cmd === 'stop') {
  const raw = process.env.BUDGET_ELAPSED || '';
  let seconds = null;
  let note = process.env.BUDGET_NOTE || '';

  if (raw !== '') {
    const n = Number(raw);
    if (!Number.isFinite(n) || n < 0) {
      console.error(`⛔ loop-budget: --elapsed must be a non-negative number of seconds (got '${raw}').`);
      process.exit(EXIT_USAGE);
    }
    if (n > budget) {
      console.error(`⛔ loop-budget: --elapsed ${mins(n)}m exceeds the ENTIRE weekly cap (${mins(budget)}m). Refusing.`);
      process.exit(EXIT_REFUSED);
    }
    seconds = Math.round(n);
    clearTimer();                       // an explicit figure supersedes whatever the timer held
  } else {
    const t = openTimer();
    if (!t) {
      // NEVER a silent no-op. An unrecorded run defeats the cap, which is the whole point of ADR-0014.
      console.error(`⛔ loop-budget: NO RUN TIMER IS OPEN -- this run would be recorded as ZERO and silently vanish from the weekly cap.`);
      console.error(`   Either the run never called \`loop-budget.sh start\`, or \`stop\` already ran.`);
      console.error(`   Record it honestly instead:  loop-budget.sh stop --elapsed <seconds> --note "<what ran>"`);
      console.error(`   Current: ${summary()}.`);
      process.exit(EXIT_REFUSED);
    }
    if (t.age > STALE_TIMER_SECONDS) {
      // Do NOT bill it and do NOT clamp it: 4h of clamped phantom time is barely better than 4h21m
      // of unclamped phantom time. Discard, say so unmissably, and demand the true figure.
      clearTimer();
      console.error(`⛔ loop-budget: the open timer is STALE (${describe(t)}) -- older than ${mins(STALE_TIMER_SECONDS)}m.`);
      console.error(`   It was left open by an earlier run, so billing it would charge ${mins(t.age)}m of wall clock that nobody worked.`);
      console.error(`   NOTHING was recorded and the stale timer is now discarded. Record THIS run's true duration:`);
      console.error(`     loop-budget.sh stop --elapsed <seconds> --note "<what ran>"`);
      console.error(`   Current: ${summary()}.`);
      process.exit(EXIT_REFUSED);
    }
    if (t.branch !== label) note = note || `start on '${t.branch}', stop on '${label}'`;
    seconds = t.age;
    clearTimer();
  }

  const r = record(seconds, note);
  console.error(`• loop budget: +${mins(seconds)}m -> ${mins(r.after)}m / ${mins(budget)}m used (week ${wk}).`);
  console.error(`  recorded as ${path.relative(process.env.ROOT || '.', r.file)} (a NEW file -- commit it).`);
  process.exit(r.after >= budget && capIsAStopSign ? EXIT_OVER : EXIT_OK);
}

if (cmd === 'status') {
  console.error(`loop budget ${summary()}`);
  const segs = segments(wk);
  for (const s of segs) console.error(`  ${String(mins(s.seconds)).padStart(7)}m  ${s.entry.recordedAt || '?'}  ${s.entry.branch || '?'}  ${s.entry.note || ''}`);
  console.error(`  ${segs.length} segment(s) this week; timer file: ${TIMER}`);
  const t = openTimer();
  console.error(t ? `  OPEN TIMER: ${describe(t)}${t.age > STALE_TIMER_SECONDS ? '  <-- STALE, will not be billed' : ''}` : `  no open timer`);
  // status is the REPORT command (the ADR names it as the constraint's replacement), so under the
  // override it must be the last place still emitting a refusal signal (ADR-20260813-132540).
  process.exit(over && capIsAStopSign ? EXIT_OVER : EXIT_OK);
}

if (cmd === 'reset') {
  const t = openTimer();
  clearTimer();
  console.error(t ? `✓ loop-budget: discarded an open timer (${describe(t)}) WITHOUT billing it. ${summary()}.` : `✓ loop-budget: no open timer to discard. ${summary()}.`);
  process.exit(EXIT_OK);
}

if (cmd === 'prune') {
  let weeks = [];
  try { weeks = fs.readdirSync(LEDGER).filter((n) => /^\d{4}-W\d{2}$/.test(n)).sort(); } catch (e) {}
  const drop = weeks.slice(0, Math.max(0, weeks.length - RETAIN_WEEKS)).filter((w) => w !== wk);
  for (const w of drop) fs.rmSync(weekDir(w), { recursive: true, force: true });
  console.error(drop.length ? `✓ loop-budget: pruned ${drop.length} ledger week(s): ${drop.join(', ')} (keeping ${RETAIN_WEEKS}).` : `✓ loop-budget: nothing to prune (${weeks.length} week(s) retained).`);
  process.exit(EXIT_OK);
}

console.error('usage: loop-budget.sh check|start|stop [--elapsed N] [--note "..."]|status|reset|prune|selftest');
process.exit(EXIT_USAGE);
NODE
