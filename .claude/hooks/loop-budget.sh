#!/usr/bin/env bash
# Self-imposed WEEKLY time budget for autonomous loops/routines (ADR-0014).
# Claude Code has NO native "minutes per week" cap, so we track it ourselves in committed state that
# survives ephemeral cloud-routine runners. The budget resets each ISO week.
#
#   loop-budget.sh check                 # exit 0 if budget remains this week, 2 if exhausted (skip the run);
#                                        # over-cap exits 0 when config sets "capIsAStopSign": false (see below)
#   loop-budget.sh start                 # check + open THIS RUN's timer (writes NOTHING tracked)
#   loop-budget.sh stop [--note "..."]   # close THIS RUN's timer and APPEND the segment to the ledger
#   loop-budget.sh <cmd> --run <id>      # address a named run's timer (handover; `start` prints the id)
#   loop-budget.sh stop --adopt          # deliberately bill a timer that carries NO run id
#   loop-budget.sh stop --elapsed-seconds 900   # record a run whose timer was never opened / was stale
#                                        # (`--elapsed` is the older spelling and still works, but it
#                                        #  REFUSES a value under 60 -- see the seconds/minutes note)
#   loop-budget.sh status                # print the week's breakdown, including any open timer
#   loop-budget.sh reset                 # discard an open timer WITHOUT billing it
#   loop-budget.sh prune                 # delete ledger weeks older than the retention window
#   loop-budget.sh selftest              # run the guard tests (also run by .claude/hooks/stop-gate.sh)
#
# EXIT CODES: 0 ok - 2 the WEEK IS SPENT (only while the cap is a stop sign) - 3 INTEGRITY (the timer
# state is wrong: not yours, already open, stale, missing) - 64 usage. 2 and 3 share the "the guard
# said no" shape and mean opposite things, so every exit-3 path SAYS it is integrity and prints the
# week's state next to it. Do not report an exit 3 as "budget exhausted".
#
# ---------------------------------------------------------------------------------------------
# WHY THIS SHAPE (it replaced a single mutable {secondsUsed, startedAt} counter that produced seven
# distinct failures in one day; every property below is STRUCTURAL, not a guard that can be skipped):
#
#   1. THE RUNNING TIMER IS NEVER TRACKED. It lives in `$(git rev-parse --git-common-dir)`, i.e.
#      inside `.git/`, which git cannot track by construction. An open timer therefore cannot be
#      committed, cannot travel between branches, and `start` cannot dirty a working tree. The
#      common dir is SHARED by every linked worktree, so `start` in one checkout and `stop` in
#      another are provably the same timer -- `git worktree` does NOT isolate it.
#   1b. EACH RUN OWNS ITS TIMER, and the FILE NAME carries the owner (#821). A shared anchor is
#      what `stop` needs to FIND a timer; it is not a licence to BILL one. With a single slot and N
#      concurrent runs, `stop` billed whatever it found: the 2026-W36 ledger holds a segment noting
#      "a concurrent session in this shared checkout closed the timer I inherited", and another with
#      a 33.3-minute unbilled remainder after a `stop` billed 3.2 minutes of a ~39-minute run and
#      printed success. Keying the PATH on the owner makes another run's timer unaddressable rather
#      than merely detected -- the nearest thing to compiler-first that shell reaches
#      (ADR-20260803-234035; PROP-20260802-130500 §1 caps a shell binding at level 3). Two
#      consequences on purpose: concurrent runs each open their OWN timer and each bill their OWN
#      real duration (no second session forced to estimate with `--elapsed`), and a `stop` that
#      cannot prove ownership REFUSES and names whose timer it found instead of billing it.
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
BUDGET_ELAPSED_UNIT_STATED=""
BUDGET_NOTE=""
BUDGET_RUN_ID=""
BUDGET_ADOPT=""
while [ $# -gt 0 ]; do
  case "$1" in
    # The UNAMBIGUOUS spelling: the unit is in the flag, so any value is taken at face value.
    --elapsed-seconds)   BUDGET_ELAPSED="${2:-}"; BUDGET_ELAPSED_UNIT_STATED=1; shift 2 || shift ;;
    --elapsed-seconds=*) BUDGET_ELAPSED="${1#*=}"; BUDGET_ELAPSED_UNIT_STATED=1; shift ;;
    # The original spelling, kept so every recorded incantation keeps working. It reads like
    # minutes -- every number this tool PRINTS is minutes -- and a caller who means minutes
    # under-bills by 60x in silence (#597: `--elapsed 16` recorded 0.3m and printed success). So
    # this spelling refuses the range where that mistake lives; see the check below.
    --elapsed)   BUDGET_ELAPSED="${2:-}"; shift 2 || shift ;;
    --elapsed=*) BUDGET_ELAPSED="${1#*=}"; shift ;;
    --note)      BUDGET_NOTE="${2:-}"; shift 2 || shift ;;
    --note=*)    BUDGET_NOTE="${1#*=}"; shift ;;
    # Address a NAMED run's timer. `start` prints the id; this is the handover path (and the only
    # way to touch a timer another run opened), so it is always an explicit assertion.
    --run)       BUDGET_RUN_ID="${2:-}"; shift 2 || shift ;;
    --run=*)     BUDGET_RUN_ID="${1#*=}"; shift ;;
    # Bill/discard a timer that carries NO run id (opened before this hook had them, or by a run
    # with no identity at all). Deliberate by construction: "bill whatever you found" is the defect.
    --adopt)     BUDGET_ADOPT=1; shift ;;
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
  BUDGET_TIMER_PREFIX="$BUDGET_COMMON/loop-budget-timer"
else
  # No git at all (tarball export, sandbox): fall back to a temp path keyed on the checkout, so
  # start/stop in the SAME checkout still agree. Different checkouts get different timers, which is
  # the best that can be done without a shared anchor.
  BUDGET_TIMER_PREFIX="${TMPDIR:-/tmp}/loop-budget-timer-$(printf '%s' "$ROOT" | cksum | cut -d' ' -f1)"
fi

# WHO IS RUNNING. Most explicit first:
#   --run <id>                an explicit assertion (handover, or billing a named run's timer)
#   $LOOP_BUDGET_RUN_ID       for a script that opens and closes a run in one process tree
#   $CLAUDE_CODE_SESSION_ID   ambient inside a Claude Code session: stable across tool calls and
#                             DISTINCT between concurrent sessions, so ownership costs the caller
#                             no discipline at all -- which is the only kind of discipline that
#                             survives a night of parallel sessions
#   (none)                    no identity available (plain cron, tarball): one shared slot, exactly
#                             as before this change, so nothing that works today stops working
BUDGET_RUN="${BUDGET_RUN_ID:-${LOOP_BUDGET_RUN_ID:-${CLAUDE_CODE_SESSION_ID:-}}}"

BUDGET_LABEL="$(git -C "$ROOT" rev-parse --abbrev-ref HEAD 2>/dev/null || echo '?')"
export BUDGET_CMD BUDGET_CONFIG BUDGET_LEDGER BUDGET_TIMER_PREFIX BUDGET_ELAPSED BUDGET_ELAPSED_UNIT_STATED BUDGET_NOTE BUDGET_LABEL BUDGET_RUN BUDGET_ADOPT ROOT

node <<'NODE'
'use strict';
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const CONFIG = process.env.BUDGET_CONFIG;
const LEDGER = process.env.BUDGET_LEDGER;
const PREFIX = process.env.BUDGET_TIMER_PREFIX;
const cmd    = process.env.BUDGET_CMD;
const label  = process.env.BUDGET_LABEL || '?';
const runId  = process.env.BUDGET_RUN || '';
const adopt  = process.env.BUDGET_ADOPT === '1';

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

function record(seconds, note, branch) {
  // APPEND-ONLY. A new file every time, so this can never decrease the total and can never conflict
  // with a segment another branch recorded concurrently.
  const before = used(wk);
  const stamp = new Date(now).toISOString().replace(/[-:]/g, '').replace(/\.\d+Z$/, 'Z');
  const file = path.join(weekDir(wk), `${stamp}-${crypto.randomBytes(4).toString('hex')}.json`);
  fs.mkdirSync(weekDir(wk), { recursive: true });
  fs.writeFileSync(file, JSON.stringify({
    // The branch of the RUN, captured at `start` -- not the branch of the checkout `stop` happened
    // to run from. Run `stop` from the primary checkout while the work lives in a linked worktree
    // and the old code stamped "main" on a receipt whose note described branch work (#821 D2:
    // .claude/loop-budget/2026-W36/20260831T142143Z-0568abb8.json, left as-is because the ledger is
    // append-only). The receipt is the only durable record of where a run's time went.
    // NB: never add `startedAt` here -- `audit` REFUSES that field in tracked state, by design.
    week: wk, seconds, recordedAt: new Date(now).toISOString(), branch: branch || label,
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

// ---- the running timer: untracked, shared anchor, OWNED per run ---------------------------------
// The anchor is shared so a timer can always be FOUND; the file name carries the owner so another
// run's timer cannot be ADDRESSED. A run with no identity keeps the historical unsuffixed path, so
// cron/tarball callers behave exactly as they did.
// A Claude session id is a UUID, and this repo publishes it in its own claim comments
// (`https://claude.ai/code/session_<id>`), so it is an identifier, not a secret. It stays in the
// untracked timer and in stderr; NOTHING about it reaches the committed ledger.
const slug = (id) => id.replace(/[^A-Za-z0-9._-]/g, '_').slice(0, 64);
const timerPath = (id) => (id ? `${PREFIX}--${slug(id)}.json` : `${PREFIX}.json`);
const TIMER = timerPath(runId);

function readTimer(file) {
  const t = readJson(file);
  if (!t || !Number(t.startedAt)) return null;
  return {
    file, startedAt: Number(t.startedAt), branch: t.branch || '?',
    owner: typeof t.owner === 'string' ? t.owner : '',   // '' = written before run ids, or by an unidentified run
    pid: Number(t.pid) || 0,
    age: Math.round((now - Number(t.startedAt)) / 1000),
  };
}
function allTimers() {
  const dir = path.dirname(PREFIX), base = path.basename(PREFIX);
  let names = [];
  try { names = fs.readdirSync(dir); } catch (e) { return []; }
  return names
    .filter((n) => n === `${base}.json` || (n.startsWith(`${base}--`) && n.endsWith('.json')))
    .sort()
    .map((n) => readTimer(path.join(dir, n)))
    .filter(Boolean);
}
const openTimer = () => readTimer(TIMER);
// Every OTHER run's timer. Reported, never touched: reporting is what turns an invisible collision
// into a fact the caller can act on, and touching is what caused the collision in the first place.
const otherTimers = () => allTimers().filter((t) => t.file !== TIMER && t.age <= STALE_TIMER_SECONDS);
const clearTimer = (file) => { try { fs.unlinkSync(file || TIMER); } catch (e) {} };
// Everything a caller needs to IDENTIFY the timer it found -- started-at, branch, pid, owner. An
// executor reconstructed exactly this tuple by hand from the ledger's last segment before deciding
// whether a `stop` was even its to run; the guard holds all four and should simply say them.
const describe = (t) => `started ${new Date(t.startedAt).toISOString()} on '${t.branch}', ${mins(t.age)}m ago, `
  + (t.owner ? `run '${t.owner}'` : 'NO run id') + (t.pid ? `, pid ${t.pid}` : '');
const reportOthers = (lead) => {
  const live = otherTimers();
  if (!live.length) return live;
  console.error(lead);
  for (const o of live) console.error(`     ${describe(o)}`);
  return live;
};

const total = used(wk);
const over = total >= budget;
const summary = () => `${mins(used(wk))}m / ${mins(budget)}m used (week ${wk})`;

// EVERY exit-3 path ends here. Exit 2 (the week is spent) and exit 3 (integrity) share the "the
// guard said no" shape and mean opposite things -- one says stop working, the other says the timer
// state is wrong. A run that hit exit 3 today reported it correctly only because the executor
// reasoned it through; the tool holds the fact and must say it, because the next reader may not.
function refuse() {
  console.error(`   (exit 3 = INTEGRITY, not budget exhaustion -- the week is ${over ? 'OVER CAP' : 'within cap'}: ${summary()}.)`);
  process.exit(EXIT_REFUSED);
}

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
    // THIS RUN's own timer, still live: a genuine double-open, and still a refusal. What is NO
    // LONGER a refusal is another run's timer -- see below.
    if (t && t.age <= STALE_TIMER_SECONDS) {
      console.error(`⛔ loop-budget: THIS RUN's timer is ALREADY OPEN (${describe(t)}).`);
      console.error(`   Two overlapping starts would bill one segment twice. Close it first:`);
      console.error(`     loop-budget.sh stop                 # bill it from ${new Date(t.startedAt).toISOString()}`);
      console.error(`     loop-budget.sh stop --elapsed-seconds <s>   # bill the true duration instead`);
      console.error(`     loop-budget.sh reset                # discard it without billing`);
      refuse();
    }
    if (t) {
      console.error(`⚠ loop-budget: DISCARDING a stale open timer of this run (${describe(t)}, older than ${mins(STALE_TIMER_SECONDS)}m).`);
      console.error(`   It is NOT billed: an unclosed timer measures wall-clock since a dead run, not work done.`);
    }
    fs.writeFileSync(TIMER, JSON.stringify({ startedAt: now, branch: label, owner: runId, pid: process.pid }, null, 2) + '\n');
    console.error(`✓ ${over ? 'loop budget OVER CAP (override active)' : 'loop budget OK'}: ${summary()}. Timer open (untracked: ${TIMER}).`);
    console.error(runId
      ? `  run id: ${runId}  -- \`stop\` matches on this; pass it as --run <id> if another process closes the run.`
      : `  run id: (none) -- no identity available, so this run uses the shared unowned slot. Concurrent`
        + `\n          runs here CANNOT be told apart: set LOOP_BUDGET_RUN_ID to bill them separately.`);
    // Concurrency is REPORTED, never refused. Refusing here is what created the incident: a session
    // that could not open a timer went and "resolved" somebody else's, and the run whose timer was
    // closed lost its remainder. Each run now bills its own real time instead of estimating.
    reportOthers(`  note: ${otherTimers().length} other run(s) hold open timers. They are NOT yours -- do not stop or reset them:`);
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

  // The timer this run may bill is its OWN. The one exception is a timer carrying NO run id
  // (written before this hook had them, or by an unidentified run): that is billable, but only on
  // an explicit --adopt, because "bill whatever you found" is precisely the defect being closed.
  const ownTimer = openTimer();
  const orphan   = (!ownTimer && runId) ? readTimer(timerPath('')) : null;
  let billBranch = label;
  let adopted = false;

  if (raw !== '') {
    const n = Number(raw);
    if (!Number.isFinite(n) || n < 0) {
      console.error(`⛔ loop-budget: --elapsed must be a non-negative number of seconds (got '${raw}').`);
      process.exit(EXIT_USAGE);
    }
    // The 60x trap (#597): the flag takes SECONDS, every message here prints MINUTES, and a value
    // meant as minutes bills a sixtieth of the truth while printing a cheerful success. A ledger
    // that under-reports silently is worse than one that refuses. The unambiguous spelling opts
    // out of this check, because a genuinely sub-minute segment is a real thing to record.
    if (n > 0 && n < 60 && (process.env.BUDGET_ELAPSED_UNIT_STATED || '') !== '1') {
      console.error(`⛔ loop-budget: --elapsed ${raw} is SECONDS -- that is ${mins(n)}m. Did you mean minutes?`);
      console.error(`   If you meant ${raw} minutes:  loop-budget.sh stop --elapsed-seconds ${Math.round(n * 60)}`);
      console.error(`   If you really meant ${raw} seconds, say so:  loop-budget.sh stop --elapsed-seconds ${raw}`);
      process.exit(EXIT_USAGE);
    }
    if (n > budget) {
      console.error(`⛔ loop-budget: --elapsed ${mins(n)}m exceeds the ENTIRE weekly cap (${mins(budget)}m). Refusing.`);
      refuse();
    }
    seconds = Math.round(n);
    // An explicit figure supersedes THIS RUN's timer -- and only this run's. It used to clear the
    // timer unconditionally, so the run that followed the tool's own advice to "record it honestly
    // instead" destroyed a concurrent run's live timer: the escape hatch was the weapon.
    if (ownTimer) { billBranch = ownTimer.branch; clearTimer(ownTimer.file); }
    reportOthers(`  note: left ${otherTimers().length} other run(s)' open timer(s) untouched:`);
  } else {
    let t = ownTimer;
    if (!t && orphan) {
      if (!adopt) {
        console.error(`⛔ loop-budget: the only open timer carries NO run id (${describe(orphan)}), and this run is '${runId}'.`);
        console.error(`   Billing a timer this run did not open is how a concurrent session's time got charged to the`);
        console.error(`   wrong run (#821). NOTHING was recorded and the timer is UNTOUCHED. Choose deliberately:`);
        console.error(`     loop-budget.sh stop --adopt                 # it IS this run (e.g. started before run ids existed)`);
        console.error(`     loop-budget.sh stop --elapsed-seconds <n>   # bill THIS run's own duration instead`);
        refuse();
      }
      t = orphan; adopted = true;
    }
    if (!t) {
      // NEVER a silent no-op. An unrecorded run defeats the cap, which is the whole point of ADR-0014.
      console.error(`⛔ loop-budget: NO RUN TIMER IS OPEN for run '${runId || '(none)'}' -- this run would be recorded as ZERO and silently vanish from the weekly cap.`);
      console.error(`   Either the run never called \`loop-budget.sh start\`, or \`stop\` already ran.`);
      // Naming the other holder is the diagnostic that was missing: without it "my timer is gone"
      // and "I forgot to start" look identical, and tonight's sessions reconstructed ownership by
      // hand from startedAt + branch + pid against the ledger's last segment.
      reportOthers(`   Another run DOES hold an open timer. It is not yours to bill:`);
      console.error(`   Record it honestly instead:  loop-budget.sh stop --elapsed-seconds <n> --note "<what ran>"`);
      refuse();
    }
    if (t.age > STALE_TIMER_SECONDS) {
      // Do NOT bill it and do NOT clamp it: 4h of clamped phantom time is barely better than 4h21m
      // of unclamped phantom time. Discard, say so unmissably, and demand the true figure.
      clearTimer(t.file);
      console.error(`⛔ loop-budget: the open timer is STALE (${describe(t)}) -- older than ${mins(STALE_TIMER_SECONDS)}m.`);
      console.error(`   It was left open by an earlier run, so billing it would charge ${mins(t.age)}m of wall clock that nobody worked.`);
      console.error(`   NOTHING was recorded and the stale timer is now discarded. Record THIS run's true duration:`);
      console.error(`     loop-budget.sh stop --elapsed-seconds <n> --note "<what ran>"`);
      refuse();
    }
    if (t.branch !== label) note = note || `start on '${t.branch}', stop on '${label}'`;
    if (adopted) note = note ? `${note}; adopted a timer carrying no run id` : `adopted a timer carrying no run id`;
    seconds = t.age;
    billBranch = t.branch;              // D2: the receipt names the branch the RUN was on
    clearTimer(t.file);
  }

  const r = record(seconds, note, billBranch);
  console.error(`• loop budget: +${mins(seconds)}m -> ${mins(r.after)}m / ${mins(budget)}m used (week ${wk}).`);
  console.error(`  recorded as ${path.relative(process.env.ROOT || '.', r.file)} (a NEW file -- commit it).`);
  process.exit(r.after >= budget && capIsAStopSign ? EXIT_OVER : EXIT_OK);
}

if (cmd === 'status') {
  console.error(`loop budget ${summary()}`);
  const segs = segments(wk);
  for (const s of segs) console.error(`  ${String(mins(s.seconds)).padStart(7)}m  ${s.entry.recordedAt || '?'}  ${s.entry.branch || '?'}  ${s.entry.note || ''}`);
  console.error(`  ${segs.length} segment(s) this week; this run is '${runId || '(no id)'}', timer file: ${TIMER}`);
  // EVERY open timer, not just this run's: concurrency was invisible here, so a session could not
  // see that another run held the slot it was about to bill.
  const all = allTimers();
  if (!all.length) console.error(`  no open timer`);
  for (const t of all) {
    console.error(`  ${t.file === TIMER ? 'OPEN TIMER (this run)' : 'open timer (ANOTHER run)'}: ${describe(t)}`
      + `${t.age > STALE_TIMER_SECONDS ? '  <-- STALE, will not be billed' : ''}`);
  }
  // status is the REPORT command (the ADR names it as the constraint's replacement), so under the
  // override it must be the last place still emitting a refusal signal (ADR-20260813-132540).
  process.exit(over && capIsAStopSign ? EXIT_OVER : EXIT_OK);
}

if (cmd === 'reset') {
  // Ownership-scoped like `stop`: discarding was the OTHER weapon that closed a concurrent run's
  // timer. `reset` throws time away, so doing it to a run that is not yours is strictly worse than
  // mis-billing it -- the run then has nothing left to record.
  const own = openTimer();
  const orphan = (!own && runId) ? readTimer(timerPath('')) : null;
  const target = own || (adopt ? orphan : null);
  if (target) {
    clearTimer(target.file);
    console.error(`✓ loop-budget: discarded an open timer (${describe(target)}) WITHOUT billing it. ${summary()}.`);
  } else if (orphan) {
    console.error(`✓ loop-budget: this run ('${runId}') has no timer to discard. One open timer carries NO run id`);
    console.error(`  (${describe(orphan)}); discard it deliberately with:  loop-budget.sh reset --adopt`);
  } else {
    console.error(`✓ loop-budget: no open timer to discard. ${summary()}.`);
  }
  reportOthers(`  note: left ${otherTimers().length} other run(s)' open timer(s) untouched:`);
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

console.error('usage: loop-budget.sh check|start|stop [--elapsed-seconds N] [--note "..."]|status|reset|prune|selftest');
process.exit(EXIT_USAGE);
NODE
