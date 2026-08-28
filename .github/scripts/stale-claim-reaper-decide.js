'use strict';

// Pure decision logic for the stale-claim reaper (issue #642, ADR-20260720-233000 as amended by
// this change). Extracted out of the `actions/github-script` step so it is testable without the
// network — see `stale-claim-reaper-decide.test.js`, a hermetic stub suite driven by fixture data
// shaped like GitHub's REST responses. Every function here is a pure function of already-fetched
// data: no `fetch`, no `github`/`context` objects, no I/O.
//
// THE DEFECT THIS REPLACES (issue #642): the previous inline script counted ANY
// `cross-referenced`/`referenced`/`connected` timeline event as proof the claim was alive, so an
// unrelated PR merely mentioning the issue number reset the 24h clock forever. Liveness is now
// WORK, not mentions — a comment posted on the issue itself, or a commit landed on the issue's own
// `NN-slug` branch, per ADR-20260810-231300 ("detection must be a positive liveness proof").
//
// RESIDUALS, stated rather than claimed away (beck):
//   - GitHub's timeline/REST response SHAPES are not covered by these stubs — a field rename or
//     removal upstream (e.g. `label.name` moving) is a live-only failure this suite cannot see.
//   - The hourly cron cadence itself carries no dead-man's-switch in this change: nothing here
//     detects "the reaper stopped firing at all". A `specs/observability.yaml` monitoring-contract
//     row for that is future work — out of this fix's scope, which does not touch `specs/**`.

const DAY_MS = 24 * 60 * 60 * 1000;
const CLAIM_WINDOW_MS = DAY_MS; // unchanged: a claim gets 24h of silence before it is reapable
const BLOCKED_WINDOW_MS = 72 * 60 * 60 * 1000; // 72h dead-man's-switch for parked items (#642 scope 2)

const CLAIM_MARKER = '<!-- stale-claim-reaper -->';
const BLOCKED_MARKER = '<!-- stale-claim-reaper:blocked -->';

/** The `NN-slug` branch prefix for an issue, per the claim protocol (BACKLOG.md). */
function branchPrefix(issueNumber) {
  return `${issueNumber}-`;
}

/**
 * Decide whether an issue's `status/in-progress` claim is still alive.
 *
 * @param {{ number: number, created_at: string }} issue
 * @param {Array<object>} timeline - raw `listEventsForTimeline` events (already paginated)
 * @param {Array<{ name: string, latestCommitAt: string|null }>} branches - candidate branches
 *   whose name starts with `branchPrefix(issue.number)`; `latestCommitAt` is the ISO timestamp of
 *   the branch's most recent commit, or `null` if it could not be resolved (e.g. an empty branch).
 *   Pass `[]` when no such branch exists yet.
 * @param {number} now - `Date.now()`-shaped epoch milliseconds
 * @returns {{ alive: boolean, claimedAt: number, reason: string }}
 */
function decideClaimLiveness(issue, timeline, branches, now) {
  const claims = timeline.filter(
    e => e.event === 'labeled' && e.label && e.label.name === 'status/in-progress'
  );
  const claimedAt = claims.length
    ? Date.parse(claims[claims.length - 1].created_at)
    : Date.parse(issue.created_at);

  if (now - claimedAt < CLAIM_WINDOW_MS) {
    return { alive: true, claimedAt, reason: 'within-window' };
  }

  // Signal 1, preserved from the previous behaviour: a comment on the issue ITSELF, since the
  // claim, that is not the reaper's own marker. Unambiguous first-party activity.
  const commentedSince = timeline.some(e => {
    if (e.event !== 'commented' || !e.created_at) return false;
    if (Date.parse(e.created_at) <= claimedAt) return false;
    return !(e.body || '').includes(CLAIM_MARKER);
  });
  if (commentedSince) {
    return { alive: true, claimedAt, reason: 'commented-since-claim' };
  }

  // Signal 2, NEW: a commit on the claim's own `NN-slug` branch, after the claim. This is the
  // ONE positive proof-of-work this function accepts in place of the old mention-counting —
  // `cross-referenced` / `referenced` / `connected` timeline events are DELIBERATELY not consulted
  // anywhere in this function.
  const prefix = branchPrefix(issue.number);
  const claimBranches = (branches || []).filter(b => b.name.startsWith(prefix));
  const committedSince = claimBranches.some(
    b => b.latestCommitAt && Date.parse(b.latestCommitAt) > claimedAt
  );
  if (committedSince) {
    return { alive: true, claimedAt, reason: 'branch-commit-since-claim' };
  }

  return {
    alive: false,
    claimedAt,
    reason: claimBranches.length ? 'branch-stale' : 'no-branch',
  };
}

/**
 * Decide whether a `status/blocked` issue needs a fresh dead-man's-switch notice: no real
 * activity (a comment, from anyone, that is not the reaper's own notice) for over 72h, and no
 * notice already posted for the CURRENT silence window — so a parked item is surfaced once per
 * expiry, not on every hourly run.
 *
 * @param {{ number: number, created_at: string }} issue
 * @param {Array<object>} timeline - raw `listEventsForTimeline` events (already paginated)
 * @param {number} now
 * @returns {{ notify: boolean, lastActivityAt: number, reason: string }}
 */
function decideBlockedNotice(issue, timeline, now) {
  const comments = timeline.filter(e => e.event === 'commented' && e.created_at);
  const realComments = comments.filter(e => !(e.body || '').includes(BLOCKED_MARKER));
  const lastActivityAt = realComments.length
    ? Math.max(...realComments.map(e => Date.parse(e.created_at)))
    : Date.parse(issue.created_at);

  if (now - lastActivityAt < BLOCKED_WINDOW_MS) {
    return { notify: false, lastActivityAt, reason: 'within-window' };
  }

  const alreadyNotified = comments.some(
    e => (e.body || '').includes(BLOCKED_MARKER) && Date.parse(e.created_at) > lastActivityAt
  );
  if (alreadyNotified) {
    return { notify: false, lastActivityAt, reason: 'already-notified' };
  }

  return { notify: true, lastActivityAt, reason: 'silence-exceeded' };
}

module.exports = {
  DAY_MS,
  CLAIM_WINDOW_MS,
  BLOCKED_WINDOW_MS,
  CLAIM_MARKER,
  BLOCKED_MARKER,
  branchPrefix,
  decideClaimLiveness,
  decideBlockedNotice,
};
