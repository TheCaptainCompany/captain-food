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
//   - `getBranch` reports the COMMITTER date of the branch tip, which `git rebase` rewrites even
//     when no new work happened — a rebased-but-otherwise-idle branch reads as live. Not closed
//     here: distinguishing a genuine rebase from a genuine commit needs comparing tree contents
//     across runs, which this stateless decision function does not do.
//   - `getBranch` 404ing mid-run (the branch was deleted -- routine on merge, since GitHub deletes
//     a merged PR's head branch) is handled WORKFLOW-SIDE, in `stale-claim-reaper.yml`: the
//     candidate is dropped rather than pushed into `branches`, which is indistinguishable to this
//     function from "no branch matched the prefix" (`decideClaimLiveness` already handles an empty
//     `branches` array). Not exercised by THIS suite because it is a fetch-time decision the
//     workflow makes before this function ever runs, not a branch this function's own logic takes.
//
// FOLLOW-UP (issue #642, re-review of #697): both liveness signals below used to compare only
// against `claimedAt`, so ANY single artifact at any instant after the claim — including the
// comment + branch push the claim protocol itself manufactures within a minute of `claimedAt`
// (BACKLOG.md, "Claim protocol") — kept the claim alive FOREVER, no matter how many days of
// silence followed. Both signals now require activity WITHIN the trailing `CLAIM_WINDOW_MS`, not
// merely at any point since the claim (`liveAfter` below). And the two bot markers now share one
// recognizer (`isReaperComment`) so a `decideBlockedNotice` notice can no longer feed
// `decideClaimLiveness`'s liveness, or vice versa.

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
 * True for either of the reaper's own bot comments. A comment standing in for work is not work,
 * for EITHER decider — before this, `decideClaimLiveness` filtered only `CLAIM_MARKER` and
 * `decideBlockedNotice` filtered only `BLOCKED_MARKER`, so each job's own comment fed the OTHER
 * signal (the #642 class again: a bot comment counted as liveness).
 */
function isReaperComment(comment) {
  const body = (comment && comment.body) || '';
  return body.includes(CLAIM_MARKER) || body.includes(BLOCKED_MARKER);
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

  // RECENCY BOUND (issue #642 follow-up, finding 1): activity must be RECENT, not merely
  // subsequent. Comparing only against `claimedAt` let a single artifact at ANY instant after the
  // claim — including the comment + branch push the claim protocol itself manufactures within a
  // minute of `claimedAt` (BACKLOG.md, "Claim protocol") — keep the claim alive forever, no matter
  // how many days of silence followed. `liveAfter` is the later of the claim and the start of the
  // trailing `CLAIM_WINDOW_MS`: a fresh claim keeps its full grace period (the early return above
  // already handles `now - claimedAt < CLAIM_WINDOW_MS`, so past this point `liveAfter` reduces to
  // `now - CLAIM_WINDOW_MS`), while an old claim now requires activity within the LAST window, not
  // merely at any point since.
  const liveAfter = Math.max(claimedAt, now - CLAIM_WINDOW_MS);

  // Signal 1, preserved from the previous behaviour: a comment on the issue ITSELF, RECENT (since
  // `liveAfter`), that is not one of the reaper's own bot comments. Unambiguous first-party
  // activity.
  const commentedSince = timeline.some(e => {
    if (e.event !== 'commented' || !e.created_at) return false;
    if (Date.parse(e.created_at) <= liveAfter) return false;
    return !isReaperComment(e);
  });
  if (commentedSince) {
    return { alive: true, claimedAt, reason: 'commented-since-claim' };
  }

  // Signal 2: a RECENT commit (since `liveAfter`) on the claim's own `NN-slug` branch. This is the
  // ONE positive proof-of-work this function accepts in place of the old mention-counting —
  // `cross-referenced` / `referenced` / `connected` timeline events are DELIBERATELY not consulted
  // anywhere in this function.
  const prefix = branchPrefix(issue.number);
  const claimBranches = (branches || []).filter(b => b.name.startsWith(prefix));
  const committedSince = claimBranches.some(
    b => b.latestCommitAt && Date.parse(b.latestCommitAt) > liveAfter
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
  // Neither of the reaper's own bot comments counts as real activity here (issue #642 follow-up,
  // finding 2) — a `CLAIM_MARKER` comment from the OTHER job is still a bot comment standing in
  // for work, not work.
  const realComments = comments.filter(e => !isReaperComment(e));
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
  isReaperComment,
  decideClaimLiveness,
  decideBlockedNotice,
};
