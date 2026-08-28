#!/usr/bin/env node
'use strict';

// Hermetic stub tests for stale-claim-reaper-decide.js (issue #642, extended by issue #703). No
// network, no `github`/`context` objects — every case feeds the pure decision functions (or, for
// `resolveBranches`, a stub `fetcher`) fixture data shaped like GitHub's REST responses (timeline
// events, branches, PR merge state) and checks the returned verdict. Modelled on
// `.claude/skills/decision-lookup/scripts/stub-tests.sh`: verdict()/skipped() bookkeeping, an
// EXPECTED_CASES completeness count, and a non-zero exit code on any failure or on a case that
// silently stopped running.
//
// Invocation (from the repo root):  node .github/scripts/stale-claim-reaper-decide.test.js
//
// MUTATION TESTING. `STALE_CLAIM_REAPER_DECIDE_MODULE` overrides which module under test is
// required, so an alternate (deliberately buggy) implementation can be pointed at this exact same
// suite to prove the suite CATCHES the regression it is named for, without duplicating the
// fixtures. Mutants run during development of this fix, with the RED evidence recorded in this
// change's commit message:
//   1. "liveness re-widened to cross-referenced events" — reds case C1 (noise-only mentions).
//   2. "blocked-item check deleted" — reds cases B0-B4 (5/5 blocked-notice cases).
//   3. "branch-commit lookup widened to any commit ever" (drops the claimedAt comparison) — reds
//      case C5 (a branch whose only commit predates the claim must still be reaped).
//   4. (issue #642 follow-up, re-review of #697) the module AS MERGED at 05996a3b — no recency
//      bound, and each decider filtering only its own marker — pointed at THIS suite (with the new
//      cases already added): reds C8 (a claim-time comment and branch push, followed by 13 days of
//      silence, reported `alive: true` forever — finding 1), C9 (a `BLOCKED_MARKER` comment fed
//      `decideClaimLiveness`'s liveness — finding 2) and B5 (a `CLAIM_MARKER` comment fed
//      `decideBlockedNotice`'s liveness — finding 2, mirrored). Case C2 was ITSELF a fixture
//      encoding the finding-1 bug before this change (asserting `alive: true` for a comment 9.5
//      days stale) and has been corrected to test a genuinely RECENT comment instead. Transcript
//      (13 passed, 3 failed, 16 total) is in this change's commit message.
//   5. (issue #703) `mergedAt` check dropped from `decideClaimLiveness` entirely (signal 3
//      deleted, falls straight through to the final `no-branch`/`branch-stale` return) — reds
//      C10 (fresh merge, deleted branch, must be `alive: true`) and C12 (the reviewer's
//      merged-vs-rebased case).
//   6. (issue #703) `mergedAt` recency bound dropped (`b.mergedAt && Date.parse(b.mergedAt) >
//      liveAfter` weakened to `b.mergedAt` alone, i.e. ANY non-null `mergedAt` counts regardless
//      of age) — reds C11 (a merge from long before the trailing window must NOT keep a stale
//      claim alive; the mutant reports `alive: true`).
//   7. (issue #703) `resolveBranches`'s error contract weakened: a non-404 from `branchCommitAt`
//      mapped to `latestCommitAt: null` instead of rethrown (`if (err.status !== 404) throw err;`
//      deleted) — reds RB1 (the loud-failure case: resolveBranches must rethrow, not silently
//      resolve to absence-of-proof).
// The mutant source files are not committed (they exist only to prove the suite is not vacuous);
// the RED transcripts are the evidence, per the dispatch's "record red output in the commit
// message or a test comment".

const assert = require('assert');
const path = require('path');

const modulePath = process.env.STALE_CLAIM_REAPER_DECIDE_MODULE
  ? path.resolve(process.env.STALE_CLAIM_REAPER_DECIDE_MODULE)
  : path.join(__dirname, 'stale-claim-reaper-decide.js');
const {
  decideClaimLiveness,
  decideBlockedNotice,
  resolveBranches,
  CLAIM_MARKER,
  BLOCKED_MARKER,
} = require(modulePath);

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

async function checkAsync(name, fn) {
  try {
    await fn();
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

check('C2: a RECENT comment on the issue itself keeps it alive (fixed — see the mutant-4 note above: this fixture used to encode the #642 recency bug, asserting `alive` for a comment 9.5 days stale)', () => {
  const timeline = [labeled(claimedLongAgo), commented(NOW - DAY_MS_HALF())];
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
  const branches = [{ name: '642-reaper-liveness-is-a-commit', latestCommitAt: iso(NOW - 60 * 60 * 1000), mergedAt: null }];
  const r = decideClaimLiveness(issue, timeline, branches, NOW);
  assert.strictEqual(r.alive, true);
  assert.strictEqual(r.reason, 'branch-commit-since-claim');
});

check('C5: a branch whose only commit PREDATES the claim is reaped, not kept alive', () => {
  const timeline = [labeled(claimedLongAgo)];
  const branches = [{ name: '642-reaper-liveness-is-a-commit', latestCommitAt: iso(claimedLongAgo - DAY_MS_HALF()), mergedAt: null }];
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

check('C8: RECENCY BOUND (issue #642 follow-up) — a comment AND a branch push at claim time, then 13 days of silence, is reaped, not immune forever', () => {
  const claimedThirteenDaysAgo = NOW - 13 * 24 * 60 * 60 * 1000;
  const oneMinuteAfterClaim = claimedThirteenDaysAgo + 60 * 1000;
  // This is exactly the shape the claim protocol (BACKLOG.md, "Claim protocol") manufactures for
  // EVERY well-formed claim: a claim comment and a branch push within a minute of `claimedAt`.
  // Before the recency bound, both signals compared only against `claimedAt`, so this fixture
  // reported `alive: true` FOREVER regardless of how much silence followed — the exact defect the
  // #144 precedent (BACKLOG.md) recorded and #697 failed to close.
  const timeline = [
    labeled(claimedThirteenDaysAgo),
    commented(oneMinuteAfterClaim, 'claiming this, branch 642-reaper-recency-bound'),
  ];
  const branches = [{ name: '642-reaper-recency-bound', latestCommitAt: iso(oneMinuteAfterClaim), mergedAt: null }];
  const issue13 = { number: 642, created_at: iso(claimedThirteenDaysAgo) };
  const r = decideClaimLiveness(issue13, timeline, branches, NOW);
  assert.strictEqual(
    r.alive,
    false,
    'a claim-time comment and branch push must not grant permanent immunity -- both are 13 days stale and neither signal is RECENT'
  );
});

check("C9: a BLOCKED_MARKER comment does not count as claim liveness either (finding 2 -- the reaper's own comments never count, whichever job posted them)", () => {
  const timeline = [labeled(claimedLongAgo), commented(NOW - 60 * 60 * 1000, `${BLOCKED_MARKER}\nstill blocked`)];
  const r = decideClaimLiveness(issue, timeline, [], NOW);
  assert.strictEqual(r.alive, false, 'a BLOCKED_MARKER comment is still a bot comment standing in for work, not first-party activity');
  assert.strictEqual(r.reason, 'no-branch');
});

check('C10 (issue #703): a RECENT merge of a PR whose head was the claim branch keeps the claim alive even though the branch itself is deleted (404 -- latestCommitAt: null)', () => {
  const timeline = [labeled(claimedLongAgo)];
  const branches = [{ name: '642-reaper-mergedat', latestCommitAt: null, mergedAt: iso(NOW - 60 * 60 * 1000) }];
  const r = decideClaimLiveness(issue, timeline, branches, NOW);
  assert.strictEqual(r.alive, true, 'a fresh merge is proof of completed work, independent of the now-deleted branch');
  assert.strictEqual(r.reason, 'branch-merged-since-claim');
});

check("C11 (issue #703, beck's 2022-merge trap): an ANCIENT mergedAt does not grant permanent immunity -- it must be bounded by the SAME liveAfter as the commit signal", () => {
  const timeline = [labeled(claimedLongAgo)];
  const branches = [{ name: '642-reaper-old-merge', latestCommitAt: null, mergedAt: iso(Date.parse('2022-01-01T00:00:00Z')) }];
  const r = decideClaimLiveness(issue, timeline, branches, NOW);
  assert.strictEqual(r.alive, false, 'a merge from 2022 proves nothing about a claim that has since gone stale');
  assert.strictEqual(r.reason, 'branch-stale');
});

check('C12 (reviewer\'s required case, issue #703): merged-and-deleted branch vs rebased-but-unmerged branch, both with a recent-looking date -- only the merged one is live', () => {
  const timeline = [labeled(claimedLongAgo)];
  const branches = [
    // Rebased-but-unmerged: still open, tip committer date is recent because of the rebase, not
    // real new work -- this is the residual the module header states is NOT closed (existing
    // behaviour, unchanged: it reads as live via latestCommitAt, same as case C4).
    { name: '642-reaper-rebased-not-merged', latestCommitAt: iso(NOW - 30 * 60 * 1000), mergedAt: null },
  ];
  const rebasedOnly = decideClaimLiveness(issue, timeline, branches, NOW);
  assert.strictEqual(rebasedOnly.alive, true, 'sanity: the rebase residual still reads as live via latestCommitAt');
  assert.strictEqual(rebasedOnly.reason, 'branch-commit-since-claim');

  // Merged-and-deleted: branch is GONE (latestCommitAt: null, as getBranch 404'd), but its PR's
  // mergedAt is recent -- live via the merge signal even though the branch itself no longer
  // exists. This is the case the #702 review flagged and issue #703 closes.
  const mergedDeleted = [{ name: '642-reaper-merged-deleted', latestCommitAt: null, mergedAt: iso(NOW - 30 * 60 * 1000) }];
  const r = decideClaimLiveness(issue, timeline, mergedDeleted, NOW);
  assert.strictEqual(r.alive, true, 'the merged-and-deleted branch is live via mergedAt despite having no latestCommitAt at all');
  assert.strictEqual(r.reason, 'branch-merged-since-claim');
});

// ── resolveBranches (I/O orchestration, stub fetcher) ────────────────────────────────────────

function stubFetcher(overrides) {
  const calls = { branchCommitAt: [], mergedAt: [] };
  const fetcher = {
    async branchCommitAt(name) {
      calls.branchCommitAt.push(name);
      const fn = overrides.branchCommitAt;
      return typeof fn === 'function' ? fn(name) : fn[name];
    },
    async mergedAt(name) {
      calls.mergedAt.push(name);
      const fn = overrides.mergedAt;
      return typeof fn === 'function' ? fn(name) : fn[name];
    },
  };
  return { fetcher, calls };
}

const rbChecks = [];
function checkRb(name, fn) {
  rbChecks.push(checkAsync(name, fn));
}

checkRb('RB0 (issue #703): a 404 on branchCommitAt resolves the candidate with a null latestCommitAt but STILL consults mergedAt -- no silent skip', async () => {
  const { fetcher, calls } = stubFetcher({
    branchCommitAt: name => {
      if (name === '642-gone') {
        const err = new Error('Branch not found');
        err.status = 404;
        throw err;
      }
      return iso(NOW - 60 * 60 * 1000);
    },
    mergedAt: name => (name === '642-gone' ? iso(NOW - 30 * 60 * 1000) : null),
  });
  const result = await resolveBranches(['642-gone', '642-still-here'], fetcher);
  assert.deepStrictEqual(result, [
    { name: '642-gone', latestCommitAt: null, mergedAt: iso(NOW - 30 * 60 * 1000) },
    { name: '642-still-here', latestCommitAt: iso(NOW - 60 * 60 * 1000), mergedAt: null },
  ]);
  assert.deepStrictEqual(calls.branchCommitAt, ['642-gone', '642-still-here'], 'branchCommitAt must be called for every candidate');
  assert.deepStrictEqual(calls.mergedAt, ['642-gone', '642-still-here'], 'mergedAt must be consulted even for the 404 candidate -- no silent skip');
});

checkRb('RB1 (issue #703): a NON-404 error from branchCommitAt rethrows out of resolveBranches -- never mapped to absence-of-proof', async () => {
  const { fetcher, calls } = stubFetcher({
    branchCommitAt: () => {
      const err = new Error('Internal Server Error');
      err.status = 500;
      throw err;
    },
    mergedAt: () => null,
  });
  await assert.rejects(
    () => resolveBranches(['642-flaky'], fetcher),
    err => err.status === 500,
    'a non-404 API failure must rethrow, loud, not resolve silently'
  );
  assert.deepStrictEqual(calls.branchCommitAt, ['642-flaky'], 'the stub was called -- this is not a silent skip');
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

check('B5: a CLAIM_MARKER comment does not count as blocked-notice liveness either (finding 2, mirrored)', () => {
  const timeline = [
    commented(NOW - 200 * 60 * 60 * 1000),
    commented(NOW - 10 * 60 * 60 * 1000, `${CLAIM_MARKER}\nexpired`),
  ];
  const r = decideBlockedNotice(blockedIssue, timeline, NOW);
  assert.strictEqual(r.notify, true, 'a CLAIM_MARKER comment is still a bot comment standing in for work, not real activity');
  assert.strictEqual(r.reason, 'silence-exceeded');
});

// ── Completeness ──────────────────────────────────────────────────────────────────────────────
const EXPECTED_CASES = 21;

Promise.all(rbChecks).then(() => {
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
});
