'use strict';

// Decision logic for the stale-claim reaper (issue #642, ADR-20260720-233000 as amended by this
// change, extended by issue #703). Extracted out of the `actions/github-script` step so it is
// testable without the network — see `stale-claim-reaper-decide.test.js`, a hermetic stub suite
// driven by fixture data shaped like GitHub's REST responses. The module has TWO clearly-separate
// layers (vernon's boundary, issue #703):
//   (a) `candidateNames` / `mergedAtByBranch` / `resolveBranches` — I/O ORCHESTRATION and its pure
//       DERIVATION helpers. `resolveBranches` is the ONLY function that touches injected I/O;
//       `candidateNames` and `mergedAtByBranch` are pure reductions over already-fetched listings.
//   (b) `decideClaimLiveness` / `decideBlockedNotice` / their helpers — PURE deciders. They are
//       TOLD an already-resolved `branches` array and never call any of the above themselves, no
//       `fetch`, no `github`/`context` objects.
//
// THE DEFECT THIS REPLACES (issue #642): the previous inline script counted ANY
// `cross-referenced`/`referenced`/`connected` timeline event as proof the claim was alive, so an
// unrelated PR merely mentioning the issue number reset the 24h clock forever. Liveness is now
// WORK, not mentions — a comment posted on the issue itself, a commit landed on the issue's own
// `NN-slug` branch, or (issue #703) a PR merge whose head was that branch, per
// ADR-20260810-231300 ("detection must be a positive liveness proof").
//
// RESIDUALS, stated rather than claimed away (beck):
//   - GitHub's timeline/REST response SHAPES are not covered by these stubs — a field rename or
//     removal upstream (e.g. `label.name` moving) is a live-only failure this suite cannot see.
//   - The hourly cron cadence itself carries no dead-man's-switch in this change: nothing here
//     detects "the reaper stopped firing at all". A `specs/observability.yaml` monitoring-contract
//     row for that is future work — out of this fix's scope, which does not touch `specs/**`.
//   - `getBranch` reports the COMMITTER date of the branch tip, which `git rebase` rewrites even
//     when no new work happened — a rebased-but-otherwise-idle UNMERGED branch still reads as
//     live via `latestCommitAt`. PARTIALLY CLOSED by issue #703's `mergedAt` signal: a MERGED
//     branch's liveness is now proven by the immutable `merged_at` of its PR regardless of any
//     rebase — or even deletion — of the branch itself (evans's distinction: "branch gone" is a
//     STATE fact, `merged_at` is the WORK-COMPLETION event, and only the latter survives a rebase
//     unscathed). What remains open is narrower: an UNMERGED branch that is rebased with no real
//     new work still reads as live via `latestCommitAt`. Distinguishing that needs comparing tree
//     contents across runs, which this stateless decision function still does not do.
//   - `pulls.list({ state: 'closed', ... per_page: 100 })` for the run-level closed-PR listing is
//     ONE call, not paginated further: it covers the most-recently-updated 100 closed PRs. Since
//     `liveAfter` never looks back further than `CLAIM_WINDOW_MS` (24h), any closure the liveness
//     decision could possibly care about is well within that window unless this repo closes more
//     than 100 PRs between two hourly runs — not a rate this project is anywhere near. Not a
//     residual in practice, but stated because the bound is a real one, not an accident.
//
// FOLLOW-UP (issue #642, re-review of #697): both liveness signals below used to compare only
// against `claimedAt`, so ANY single artifact at any instant after the claim — including the
// comment + branch push the claim protocol itself manufactures within a minute of `claimedAt`
// (BACKLOG.md, "Claim protocol") — kept the claim alive FOREVER, no matter how many days of
// silence followed. Both signals now require activity WITHIN the trailing `CLAIM_WINDOW_MS`, not
// merely at any point since the claim (`liveAfter` below). And the two bot markers now share one
// recognizer (`isReaperComment`) so a `decideBlockedNotice` notice can no longer feed
// `decideClaimLiveness`'s liveness, or vice versa.
//
// FOLLOW-UP (issue #703, round-4 pick off the #702 review): the branch-commit signal alone missed
// the case where a claim's work landed and MERGED, and GitHub then deleted the (now merged) head
// branch as routine cleanup. Liveness now also accepts a RECENT merge (bounded by the same
// `liveAfter` as the commit signal — beck's recency rule: an ancient merge proves nothing) of a PR
// whose head was the claim's own branch, per `decideClaimLiveness`'s third signal.
//
// FOLLOW-UP (issue #703, #705 review — the SAME finding fixed properly): the first cut above
// sourced CANDIDATES from `repos.listBranches` alone, so a branch deleted HOURS OR DAYS before a
// run — the ROUTINE case, since GitHub deletes a merged PR's head branch at merge time — was
// simply ABSENT from that list: `candidates = []`, `decideClaimLiveness` returned `no-branch`, and
// a multi-PR issue whose work had already landed and merged got reaped anyway. The `mergedAt`
// signal as first built only ever fired in the seconds-wide race where a branch was still in the
// run-start snapshot and vanished before `getBranch` ran — never the motivating case. Fixed by
// deriving candidates from TWO sources, merged and deduplicated by `candidateNames`: still-live
// branches (`repos.listBranches`) AND the `head.ref`s of a run-level CLOSED-PR listing
// (`pulls.list({ state: 'closed' })`, fetched ONCE per run, never per candidate/issue) — a closed
// PR keeps its head ref even after the branch itself is gone. That same listing's `merged_at` is
// PRE-RESOLVED into a `{ name: mergedAt|null }` map by `mergedAtByBranch` and threaded into
// `resolveBranches`, so a candidate sourced from it costs NO further API call at all — the earlier
// design's per-candidate `pulls.list` call is now a NARROW FALLBACK (see `resolveBranches`'s own
// contract) for the seconds-wide race the run-level listing did not already cover.

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
 * PURE DERIVATION (issue #703 / #705 review). `repos.listBranches` alone MISSES a branch deleted
 * at merge time, hours or days before this run — that is the ROUTINE case (GitHub deletes a merged
 * PR's head branch), not a rare race, and it is exactly the shape of the motivating bug (a
 * multi-PR issue reaped after its own work had already landed and merged). Merging in a run-level
 * CLOSED-PR listing's head refs recovers those names: a closed PR keeps its `head.ref` string even
 * after the branch itself is gone.
 *
 * @param {string} prefix - `branchPrefix(issue.number)`, e.g. `"703-"`. Matched with
 *   `String.prototype.startsWith` — the trailing hyphen is the delimiter, so `"703-"` never
 *   matches a `"70-"` or `"7-"` branch/head-ref (issue #705 review, the prefix-collision case).
 * @param {{ branchNames: string[], closedPrHeadRefs: string[] }} sources - `branchNames` from
 *   `repos.listBranches` (still-live branches); `closedPrHeadRefs` from a closed-PR listing's
 *   `head.ref`s (merged OR not — an unmerged closed PR's now-deleted branch is still a candidate,
 *   just one that resolves to `mergedAt: null`).
 * @returns {string[]} deduplicated candidate names, order not significant.
 */
function candidateNames(prefix, { branchNames, closedPrHeadRefs } = {}) {
  const names = new Set();
  for (const n of branchNames || []) {
    if (n.startsWith(prefix)) names.add(n);
  }
  for (const n of closedPrHeadRefs || []) {
    if (n.startsWith(prefix)) names.add(n);
  }
  return Array.from(names);
}

/**
 * PURE DERIVATION (issue #703 / #705 review). Reduces a raw closed-PR listing
 * (`github.rest.pulls.list({ state: 'closed' })` shape) to a `{ headRef: merged_at|null }` map —
 * the PRE-RESOLVED `mergedAt` that `resolveBranches` needs so a candidate sourced from this
 * listing costs NO further API call (the #705 review's cost finding). A closed PR that was never
 * merged contributes `null` for its head ref explicitly — a known non-merge, not "unknown". If a
 * branch name was reused across more than one closed PR (rare, but a repo's history can do it),
 * the MOST RECENT non-null `merged_at` wins.
 */
function mergedAtByBranch(closedPrs) {
  const map = {};
  for (const pr of closedPrs || []) {
    const name = pr.head && pr.head.ref;
    if (!name) continue;
    const mergedAt = pr.merged_at || null;
    const known = Object.prototype.hasOwnProperty.call(map, name);
    if (!known || (mergedAt && (!map[name] || mergedAt > map[name]))) {
      map[name] = mergedAt;
    }
  }
  return map;
}

/**
 * I/O ORCHESTRATION (issue #703) — the ONLY function in this module that touches injected I/O.
 * Resolves each candidate branch NAME (as produced by `candidateNames`) to the two
 * liveness-relevant facts about it: the branch tip's own commit date (a STATE fact — evans) and
 * the `merged_at` of the PR that merged it, if any (the WORK-COMPLETION EVENT — evans; kept
 * distinct from the state fact because a merge is immutable while a branch can be rebased or
 * deleted out from under it).
 *
 * @param {string[]} candidates - branch names already filtered to the issue's own `NN-slug`
 *   prefix, as produced by `candidateNames` (live branches UNION closed-PR head refs — never the
 *   full repo branch list; bounding the fetcher calls to candidates only is the per-run cost
 *   guarantee documented at the call site in the workflow).
 * @param {{
 *   branchCommitAt: (name: string) => Promise<string|null>,
 *   mergedAt: (name: string) => Promise<string|null>,
 * }} fetcher - workflow-side wrapper around `github.rest`, injected so this function is testable
 *   without the network. `branchCommitAt` resolves the branch tip's commit date (or throws, with
 *   `.status`, on API failure — a 404 means the branch is gone). `mergedAt` is called ONLY as the
 *   narrow fallback described below — most candidates never trigger it.
 * @param {Object<string, string|null>} [mergedAtByName] - the PRE-RESOLVED `{ name: mergedAt }`
 *   map from `mergedAtByBranch`, built ONCE per run from the run-level closed-PR listing. THIS,
 *   not the `fetcher.mergedAt` fallback, is the mechanism that makes the motivating case work: a
 *   branch deleted hours or days ago at merge time is not in `branchNames` any more, but its name
 *   IS already a key in this map (via `candidateNames`'s other source), with its real `merged_at`
 *   attached — no further call needed. Defaults to `{}` (no pre-resolved names).
 * @returns {Promise<Array<{ name: string, latestCommitAt: string|null, mergedAt: string|null }>>}
 *   One entry per candidate, always — a candidate is never dropped.
 *
 * ERROR CONTRACT (observability's rule: an API error is NEVER mapped to absence-of-proof):
 *   - `branchCommitAt` 404 is the ONE targeted idempotence, caught HERE, inside this function: the
 *     branch is gone (a STATE fact), not a liveness signal that failed to resolve. This 404 path
 *     covers ONLY the SECONDS-WIDE RACE where a branch was still present in the run's
 *     `listBranches` snapshot and vanished before this candidate's own `getBranch` call ran (e.g.
 *     a merge that happened mid-run) — it is NOT the mechanism for the routine hours/days-later
 *     deletion, which `candidateNames` + `mergedAtByName` already cover without ever reaching a
 *     404 here at all (a name sourced only from the closed-PR listing is expected to 404 on
 *     `branchCommitAt`, and that is fine — its `mergedAt` was resolved before this loop ran).
 *   - `fetcher.mergedAt` is called ONLY when the name is NOT already a key in `mergedAtByName` AND
 *     `branchCommitAt` 404'd for it — i.e. only for that same narrow race. A name already known
 *     from the run-level closed-PR listing (merged OR closed-without-merging) never triggers this
 *     call; a name that is still a live branch (no 404) never triggers it either, because its own
 *     PR, if any, is by definition still open.
 *   - ANY other error — a non-404 from `branchCommitAt`, or anything at all from `fetcher.mergedAt`
 *     — rethrows out of this function, uncaught, into the workflow's per-issue `try`/`catch`
 *     collection. No other status is ever swallowed.
 */
async function resolveBranches(candidates, fetcher, mergedAtByName = {}) {
  const resolved = [];
  for (const name of candidates) {
    let latestCommitAt = null;
    let branchGone = false;
    try {
      latestCommitAt = await fetcher.branchCommitAt(name);
    } catch (err) {
      if (err.status !== 404) throw err;
      branchGone = true;
    }
    const known = Object.prototype.hasOwnProperty.call(mergedAtByName, name);
    let mergedAt = known ? mergedAtByName[name] : null;
    if (!known && branchGone) {
      // Narrow fallback, see the ERROR CONTRACT above: not the motivating case, only the
      // seconds-wide race the run-level closed-PR listing did not already cover.
      mergedAt = await fetcher.mergedAt(name);
    }
    resolved.push({ name, latestCommitAt, mergedAt });
  }
  return resolved;
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
 * Decide whether an issue's `status/in-progress` claim is still alive. PURE — takes an
 * already-resolved `branches` array (see `resolveBranches`) and never touches I/O itself.
 *
 * @param {{ number: number, created_at: string }} issue
 * @param {Array<object>} timeline - raw `listEventsForTimeline` events (already paginated)
 * @param {Array<{ name: string, latestCommitAt: string|null, mergedAt: string|null }>} branches -
 *   candidate branches whose name starts with `branchPrefix(issue.number)`, as resolved by
 *   `resolveBranches` (candidates themselves produced by `candidateNames`). `latestCommitAt` is
 *   the ISO timestamp of the branch's most recent commit, or `null` if the branch is gone or has
 *   no commits. `mergedAt` (issue #703) is the ISO `merged_at` of the PR that merged this branch,
 *   or `null` if none exists. Pass `[]` when no such branch exists yet.
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
  // merely at any point since. Issue #703's `mergedAt` signal below is bound by this SAME
  // `liveAfter` (beck's recency rule): an old merge proves nothing about a claim that has since
  // gone stale.
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

  // Signal 2: a RECENT commit (since `liveAfter`) on the claim's own `NN-slug` branch. This is
  // ONE positive proof-of-work this function accepts in place of the old mention-counting —
  // `cross-referenced` / `referenced` / `connected` timeline events are DELIBERATELY not consulted
  // anywhere in this function.
  const prefix = branchPrefix(issue.number);
  const claimBranches = (branches || []).filter(b => b.name.startsWith(prefix));
  const commitSince = claimBranches.some(
    b => b.latestCommitAt && Date.parse(b.latestCommitAt) > liveAfter
  );
  if (commitSince) {
    return { alive: true, claimedAt, reason: 'branch-commit-since-claim' };
  }

  // Signal 3 (issue #703): a RECENT merge (since the SAME `liveAfter`) of a PR whose head was the
  // claim's own branch. `mergedAt` is a WORK-COMPLETION EVENT (evans), immutable once it happens
  // — unlike `latestCommitAt` it survives a `git rebase` and even the branch's own deletion
  // (GitHub deletes a merged PR's head branch routinely). This closes the #702 review finding: a
  // branch deleted by a JUST-merged PR now reads as live via `mergedAt` even though `getBranch`
  // 404s on it (reachable end-to-end since the #705 review's `candidateNames` fix — see the module
  // header), and it closes the rebase residual for the merged case specifically (a merge proof
  // cannot be altered by a later rebase of a branch that no longer needs rebasing).
  const mergedSince = claimBranches.some(
    b => b.mergedAt && Date.parse(b.mergedAt) > liveAfter
  );
  if (mergedSince) {
    return { alive: true, claimedAt, reason: 'branch-merged-since-claim' };
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
  candidateNames,
  mergedAtByBranch,
  resolveBranches,
  decideClaimLiveness,
  decideBlockedNotice,
};
