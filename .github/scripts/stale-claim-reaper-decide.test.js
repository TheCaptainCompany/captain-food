#!/usr/bin/env node
'use strict';

// Hermetic stub tests for stale-claim-reaper-decide.js (issue #642). No network, no `github`/
// `context` objects — every case feeds the pure decision functions fixture data shaped like
// GitHub's REST responses (timeline events, branches) and checks the returned verdict. Modelled on
// `.claude/skills/decision-lookup/scripts/stub-tests.sh`: verdict()/skipped() bookkeeping, an
// EXPECTED_CASES completeness count, and a non-zero exit code on any failure or on a case that
// silently stopped running.
//
// Invocation (from the repo root):  node .github/scripts/stale-claim-reaper-decide.test.js
//
// MUTATION TESTING. `STALE_CLAIM_REAPER_DECIDE_MODULE` overrides which module under test is
// required, so an alternate (deliberately buggy) implementation can be pointed at this exact same
// suite to prove the suite CATCHES the regression it is named for, without duplicating the
// fixtures. Three such mutants were run during development of this fix and produced the RED
// evidence recorded in this change's commit message:
//   1. "liveness re-widened to cross-referenced events" — reds case C1 (noise-only mentions).
//   2. "blocked-item check deleted" — reds cases B0-B4 (5/5 blocked-notice cases).
//   3. "branch-commit lookup widened to any commit ever" (drops the claimedAt comparison) — reds
//      case C5 (a branch whose only commit predates the claim must still be reaped).
// The mutant source files are not committed (they exist only to prove the suite is not vacuous);
// the RED transcripts are the evidence, per the dispatch's "record red output in the commit
// message or a test comment".

const assert = require('assert');
const path = require('path');

const modulePath = process.env.STALE_CLAIM_REAPER_DECIDE_MODULE
  ? path.resolve(process.env.STALE_CLAIM_REAPER_DECIDE_MODULE)
  : path.join(__dirname, 'stale-claim-reaper-decide.js');
const { decideClaimLiveness, decideBlockedNotice, CLAIM_MARKER, BLOCKED_MARKER } = require(modulePath);

let pass = 0;
let fail = 0;
const failures = [];

function verdict(ok, name, detail) {
  if (ok) {
    pass += 1;
    console.log(`PASS  ${name}`);
  } else {
    fail += 1;
    failures.push(name);
    console.log(`FAIL  ${name}${detail ? ` -- ${detail}` : ''}`);
  }
}

function check(name, fn) {
  try {
    fn();
    verdict(true, name);
  } catch (err) {
    verdict(false, name, err.message);
  }
}

// ── Fixtures: timestamps relative to `NOW`, in the shape GitHub's timeline API returns ──────────
const NOW = Date.parse('2026-08-28T12:00:00Z');
const iso = ms => new Date(ms).toISOString();
const claimedLongAgo = NOW - 10 * 24 * 60 * 60 * 1000; // 10 days ago -- the issue's own bug report
const claimedRecently = NOW - 2 * 60 * 60 * 1000; // 2h ago -- inside the 24h window

const labeled = at => ({ event: 'labeled', label: { name: 'status/in-progress' }, created_at: iso(at) });
const commented = (at, body) => ({ event: 'commented', created_at: iso(at), body: body || 'update' });
const crossReferenced = at => ({ event: 'cross-referenced', created_at: iso(at) });
const referenced = at => ({ event: 'referenced', created_at: iso(at) });
const connected = at => ({ event: 'connected', created_at: iso(at) });

const issue = { number: 642, created_at: iso(claimedLongAgo) };
const issueRecent = { number: 642, created_at: iso(claimedRecently) };

// ── Claim liveness cases ──────────────────────────────────────────────────────────────────────

check('C0: still within the 24h window is never reaped, whatever else is on the timeline', () => {
  const timeline = [labeled(claimedRecently)];
  const r = decideClaimLiveness(issueRecent, timeline, [], NOW);
  assert.strictEqual(r.alive, true);
  assert.strictEqual(r.reason, 'within-window');
});

check('C1: cross-referenced/referenced/connected mentions do NOT count as liveness (the #642 bug)', () => {
  const timeline = [
    labeled(claimedLongAgo),
    crossReferenced(claimedLongAgo + 60 * 60 * 1000),
    referenced(NOW - 60 * 60 * 1000),
    connected(NOW - 30 * 60 * 1000),
  ];
  const r = decideClaimLiveness(issue, timeline, [], NOW);
  assert.strictEqual(r.alive, false, 'a bare mention must not keep the claim alive');
  assert.strictEqual(r.reason, 'no-branch');
});

check('C2: a comment on the issue itself, since the claim, keeps it alive', () => {
  const timeline = [labeled(claimedLongAgo), commented(claimedLongAgo + DAY_MS_HALF())];
  const r = decideClaimLiveness(issue, timeline, [], NOW);
  assert.strictEqual(r.alive, true);
  assert.strictEqual(r.reason, 'commented-since-claim');
});
function DAY_MS_HALF() {
  return 12 * 60 * 60 * 1000;
}

check("C3: the reaper's own marker comment does not count as activity", () => {
  const timeline = [labeled(claimedLongAgo), commented(NOW - 60 * 60 * 1000, `${CLAIM_MARKER}\nexpired`)];
  const r = decideClaimLiveness(issue, timeline, [], NOW);
  assert.strictEqual(r.alive, false);
});

check('C4 (green control): a commit on the NN-slug branch inside the window keeps the claim alive', () => {
  const timeline = [labeled(claimedLongAgo)];
  const branches = [{ name: '642-reaper-liveness-is-a-commit', latestCommitAt: iso(NOW - 60 * 60 * 1000) }];
  const r = decideClaimLiveness(issue, timeline, branches, NOW);
  assert.strictEqual(r.alive, true);
  assert.strictEqual(r.reason, 'branch-commit-since-claim');
});

check('C5: a branch whose only commit PREDATES the claim is reaped, not kept alive', () => {
  const timeline = [labeled(claimedLongAgo)];
  const branches = [{ name: '642-reaper-liveness-is-a-commit', latestCommitAt: iso(claimedLongAgo - DAY_MS_HALF()) }];
  const r = decideClaimLiveness(issue, timeline, branches, NOW);
  assert.strictEqual(r.alive, false, 'a stale branch commit from before the claim is not proof of live work');
  assert.strictEqual(r.reason, 'branch-stale');
});

check('C6: no branch at all yet is treated as no liveness proof, reapable after the window', () => {
  const timeline = [labeled(claimedLongAgo)];
  const r = decideClaimLiveness(issue, timeline, [], NOW);
  assert.strictEqual(r.alive, false);
  assert.strictEqual(r.reason, 'no-branch');
});

check('C7: claimedAt falls back to issue creation when no labeled event is on the timeline', () => {
  const r = decideClaimLiveness(issue, [], [], NOW);
  assert.strictEqual(r.claimedAt, Date.parse(issue.created_at));
  assert.strictEqual(r.alive, false);
});

// ── Blocked dead-man's-switch cases ──────────────────────────────────────────────────────────

const blockedIssue = { number: 700, created_at: iso(NOW - 200 * 60 * 60 * 1000) };

check('B0: silent for only 10h stays quiet', () => {
  const timeline = [commented(NOW - 10 * 60 * 60 * 1000)];
  const r = decideBlockedNotice(blockedIssue, timeline, NOW);
  assert.strictEqual(r.notify, false);
  assert.strictEqual(r.reason, 'within-window');
});

check('B1: silent for over 72h with no prior notice gets a fresh one', () => {
  const timeline = [commented(NOW - 73 * 60 * 60 * 1000)];
  const r = decideBlockedNotice(blockedIssue, timeline, NOW);
  assert.strictEqual(r.notify, true);
  assert.strictEqual(r.reason, 'silence-exceeded');
});

check('B2: a notice already posted for this silence window is not repeated (no hourly spam)', () => {
  const timeline = [
    commented(NOW - 100 * 60 * 60 * 1000),
    commented(NOW - 90 * 60 * 60 * 1000, `${BLOCKED_MARKER}\nparked`),
  ];
  const r = decideBlockedNotice(blockedIssue, timeline, NOW);
  assert.strictEqual(r.notify, false);
  assert.strictEqual(r.reason, 'already-notified');
});

check('B3: real activity after a previous notice restarts the 72h clock', () => {
  const timeline = [
    commented(NOW - 200 * 60 * 60 * 1000),
    commented(NOW - 190 * 60 * 60 * 1000, `${BLOCKED_MARKER}\nparked`),
    commented(NOW - 10 * 60 * 60 * 1000, 'still blocked on the vendor, following up'),
  ];
  const r = decideBlockedNotice(blockedIssue, timeline, NOW);
  assert.strictEqual(r.notify, false, 'the fresh real comment must restart the window');
  assert.strictEqual(r.reason, 'within-window');
});

check('B4: real activity restarting the clock, then a second 72h of silence, notifies again', () => {
  const timeline = [
    commented(NOW - 200 * 60 * 60 * 1000),
    commented(NOW - 190 * 60 * 60 * 1000, `${BLOCKED_MARKER}\nparked`),
    commented(NOW - 100 * 60 * 60 * 1000, 'still blocked on the vendor'),
  ];
  const r = decideBlockedNotice(blockedIssue, timeline, NOW);
  assert.strictEqual(r.notify, true);
  assert.strictEqual(r.reason, 'silence-exceeded');
});

// ── Completeness ──────────────────────────────────────────────────────────────────────────────
const EXPECTED_CASES = 13;
const ran = pass + fail;
if (ran !== EXPECTED_CASES) {
  console.log(
    `FATAL: ${ran} case(s) reached a verdict, expected exactly EXPECTED_CASES=${EXPECTED_CASES} -- ` +
      'a case that stops running silently must be loud, not green.'
  );
  process.exit(1);
}

console.log(`RESULT: ${pass} passed, ${fail} failed, ${ran} total (EXPECTED_CASES=${EXPECTED_CASES})`);
if (fail > 0) {
  console.log(`Failing cases: ${failures.join(', ')}`);
  process.exit(1);
}
process.exit(0);
