# 📋 Captain.Food — Backlog prioritisation

> Hand-maintained (NOT generated, outside `specs/` so it never affects the DSL).
> This file records **the prioritisation process and how value is defined** — it does NOT hold the
> ranking itself. Recorded decision: [ADR-20260720-213024](adr/20260720-213024-value-first-issue-prioritisation.md).

## Where priorities are defined

**The GitHub Project “Prioritized backlog” (TheCaptainCompany org) is the single place where the backlog
order is defined and maintained.** Nothing in the repository duplicates the ranking — no rank
stamps in issue bodies, no ordered list here. Sessions (human or agent) read the board and **pick
work from the top**: `Urgent` → `High` → `Medium` → `Low`, row order within a bucket. Skipping the
top open item requires a stated reason (blocked, plan-mode approval pending, founder
directive) — not preference.

**Re-prioritisation is delegated to the team** (founder directive, 2026-08-10,
[ADR-20260810-215503](adr/ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md):
*"Don't care about the project field anymore the team decides without me"*). The **`Priority`
bucket and the row order within a bucket are the team's to set**, in the project, alongside the
`Type`/`Value Size`/`Impact`/`Effort` fields it already sets at triage. The founder may
re-bucket or reorder anything at any time, without justification, and the team adopts it
immediately — the delegation is revocable per item and in general.

**What is NOT delegated**: genuine option spaces ([proposals/DECISIONS.md](proposals/DECISIONS.md));
external, legal and admin-gated matters; `specs/**` approval — **a `Priority` is not an approval,
and ranking an AMBER item `Urgent` does not make it dispatchable**; and **the method below**, which
is now **binding rather than descriptive** — it used to describe how the founder ranked, and
it is now the constraint under which the team ranks. If the *method* changes, that change is
recorded as an ADR amending/superseding ADR-20260720-213024.

**Ranking and dispatching must not be the same act.** The architect agent both ranks the backlog and
names the next chunk, so: **an agent must never change a Priority bucket or a row position in order
to make an item dispatchable, or to make its own recommendation legitimate.** If the top item is
blocked, the answer is "blocked" — never a re-rank. A re-rank is justified by the value method or by
a dependency that was wrong; never by what the ranker wants to work on next. Because the rationale no
longer lives in the founder's head, it has to be written down or it does not exist: every
bucket change or material row move is stated in the architect's run report with the method clause
that justifies it, and a re-ranking that reverses a previously stated order also gets a dated line
at the top of the current `docs/status/journal-YYYY-Www.md` — [STATUS.md](STATUS.md) changes only
when durable state does. Any mob lens may contest a ranking at briefing time exactly as it contests
a design.

## How value is defined (the ordering method)

The backlog is ordered by **value, not effort** (founder directive, 2026-07-20):

1. **First: foundations & cross-functional / non-functional** — work everything later stands on:
   API/write contracts, security (ACL), correctness invariants, observability, data
   retention/compliance, and the codegen operating-model wave (which cheapens all downstream work).
   Value here = risk retired × what it unblocks.
2. **Then: features, in value-stream order.** The V0 value stream runs
   **customer ordering** (the PMF funnel — nothing matters until a Tours customer can order)
   → **restaurant onboarding** (supply side, self-serve HubRise connect)
   → **delivery automation** (post-V0; manual/out-of-band in V0).

Within a tier, order stays dependency-consistent (an issue never ranks above one it needs).

## How an issue represents its value and cost

- **Priority** (org field) = the **value bucket** (from the method above):
  `Urgent` = tier-1 contract/security/correctness/observability/NFR foundations ·
  `High` = operating-model / codegen foundations ·
  `Medium` = V0 features in value-stream order ·
  `Low` = post-V0.
  Within a bucket the fine order is the **row order** on the board — no numeric value field is
  used for ordering.
- **Value Size** (org field, T-shirt `XS`–`XL`) = **how much value the issue brings if
  completed**, graded from its Impact section (what it unblocks / what breaks if delayed).
  Informational — it explains the Priority placement, it does not sort.
- **Impact** = **the size of the change on the code** (blast radius). One 5-step T-shirt scale
  (`XS`–`XL`, founder decision), in two places with the same value: the **org Impact field**
  and the **`impact/*` repo label** (visible on issue lists/cards). It replaces the former
  `size/*` labels; the finer XXS–XXXL granularity of ADR-20260720-143000's estimation table lives
  only in the body's Estimation section (an estimate beyond XL is a "split before starting" flag,
  not a field value).
- **Effort** (org field, `Low`/`Medium`/`High`) = delivery cost, the coarse projection of the
  Impact T-shirt (XXS–S → `Low`, M → `Medium`, L and up → `High`). Impact/Effort are displayed
  for planning but **never drive the order** — value does.
- **Type** = `Foundation` (non-functional: contracts, security, invariants, observability,
  retention, codegen/operating-model) or `Feature` (functional, user-visible capability) —
  matching the two value tiers; `Bug`/`Task` for the rest.
- Estimation rules are unchanged: ADR-20260720-143000 (the detailed shirt-size estimate still
  lives in the issue body's Estimation section; label/field above are its visible form).

## Triage of new issues

A new issue gets, at triage time: the standard pre-task sections (ADR-20260720-143000), a **Type**
(`Foundation`/`Feature`/`Bug`/`Task`), an **`impact/*` label** (change size, from the Estimation
section), and the org fields **Priority + Value Size + Impact + Effort**, using the definitions
above. The founder adjusts its row position in the project if the default bucket placement
isn't enough.

## Claim protocol (multi-session safety) — claim → draft PR → supervised auto-merge

(ADR-20260720-233000, amended by ADR-20260721-042018 and ADR-20260721-044613.)

1. **Claim = label + comment + branch + draft PR, immediately** (before any implementation work):
   - add the **`status/in-progress`** label AND post a claim comment naming the **`NN-slug`**
     branch (issue number first). The label is the atomic, API-visible claim.
   - **The claim comment MUST carry the session link** (founder directive, 2026-07-27):
     `https://claude.ai/code/session_<id>`. Without it a claim is anonymous — you can see that an
     issue is taken but not by which run, so a stalled claim cannot be traced back to the session
     that made it, and a human cannot open the transcript to see what was already tried. Commits
     already carry `Claude-Session:` in their trailer; the claim comment is the FIRST artifact of an
     issue and must carry it too, because it exists before any commit does.
   - create branch `NN-slug` from latest `main`, push it, and **open a draft PR
     `NN-slug → main` right away** — body starting with **`Closes #NN`** plus the intended
     approach. From minute one the Development sidebar shows the branch + PR, the board flips to
     In progress (native workflow), and pushing further commits to `NN-slug` feeds the stale-claim
     reaper's branch-commit liveness signal (see "Stale-claim reaper" below) — the PR *link* itself
     is never consulted; only a genuine issue comment or a commit on the branch counts.
     Draft status is the interlock: GitHub refuses to merge a draft, so the early PR can never
     merge half-done work. **Do NOT enable auto-merge here** — see the rule below.
2. **Never work an issue that carries `status/in-progress`** — pick the next unclaimed rank.
3. **Work happens on the PR**: push commits to `NN-slug`; the `ci` workflow gates every push.
4. **Completion = ready + auto-merge + supervision** (never end at "pushed, CI pending"):
   local gates green (`make rust`), the work recorded in the same change — durable state in
   `STATUS.md`, the dated entry at the TOP of the current `docs/status/journal-YYYY-Www.md`, and an
   ADR for an actual cross-cutting decision — then **mark the PR
   ready for review and enable auto-merge together, as one indivisible step** (repo default merge
   method) — never one without the other — and **supervise until MERGED**: watch the checks, fix
   and push on any failure. The merge auto-closes the issue (`Closes #NN`), which ends the claim.
   Checks can't be made green / scope exploded? Comment the diagnosis on the PR — don't go silent.
5. Merge (or close) ends the claim naturally. Abandoning? Remove the label and close the draft PR.
6. **Board mirror (native Project workflows — no label trigger exists)**: enable
   "Pull request linked to issue → Status: In progress", "Pull request merged → Done" and
   "Item closed → Done". With the PR created at claim time, the Status column flips at claim time.
   Sessions never write the Status column directly — the label is the authoritative claim
   (full label→Status sync would need a PAT-scoped Action; deliberately not adopted).

**Auto-merge security posture** (analysis in ADR-20260721-042018): repo-level "Allow auto-merge"
grants no merge authority — it must be armed per-PR by a **write-access** user and merges under the
same `main` protection rules as a manual merge; fork/outsider PRs can't arm it and can't merge, so
a "fake empty PR" from outside just sits open. The load-bearing config is the **`main` ruleset**:
it MUST require the **`codegen`** status check (build + tests + validator + drift) — without a
required check, an armed auto-merge fires immediately. Residual (deliberate) trade: any
write-access session lands unreviewed code once CI is green — the executable gates are the review.

**Never arm auto-merge early** (ADR-20260721-044613): a claim-time draft PR is close to a no-op
diff, so its CI passes trivially — draft status blocks the *merge* regardless, but an auto-merge
armed at claim time would stay armed for the whole task and fire the instant the PR leaves draft,
even if that happens before the work is actually done. Auto-merge is armed **exactly once**, in
the same action as marking the PR ready — never earlier, never separately.

## Stale-claim reaper

**Closed (2026-08-28, issue #642 follow-up on the re-review of #697):** the 2026-08-09 #144
precedent below survived #697's rewrite verbatim, because both liveness signals compared activity
only against the moment of the claim, never against how recent it was — a claim comment and branch
push at claim time (which the claim protocol above manufactures within a minute of every
well-formed claim) therefore kept a claim "alive" forever, no matter how many days of silence
followed. Liveness is now bound to the **trailing 24h window**, not merely "any point since the
claim": see `.github/scripts/stale-claim-reaper-decide.js` (`liveAfter`) and its hermetic suite for
the fixture that reproduces the #144 shape and proves it is now reaped.

**Historical shape of the hole (2026-08-09, the #144 precedent, kept for context):** a claim whose
linked draft PR simply sat there survived **13 days** parked — the reaper never fired, so "carries
`status/in-progress`" was NOT proof of an active session. Meeting a stale-looking claim with a
long-quiet linked PR: check the claim comment's session link and the PR's last activity; if both
are days old, re-claim explicitly with a fresh comment naming your branch and session (as #144 →
PR #430 did) rather than treating the label as untouchable — and rather than silently working
alongside it. This manual check remains good practice even though the automated reaper now closes
the gap on its own.

`.github/workflows/stale-claim-reaper.yml` (hourly): a `status/in-progress` issue with **no RECENT
activity in the trailing 24h** — a genuine issue comment, a commit landed on its own `NN-slug`
branch, or (issue #703) the merge of a PR whose head was that branch, WITHIN the last 24h; the
reaper ignores its own marker comments from either job, and
`cross-referenced`/`referenced`/`connected` timeline events (an unrelated PR mentioning the issue
number) are never consulted — loses the label and gets a "claim expired" comment → back to the
queue. A crashed session can never hold an issue hostage. The merge signal closes the case where a
claim's work landed and merged, and GitHub then deleted the (now merged) head branch as routine
cleanup, hours or days before a run: the candidate set is now derived from BOTH still-live branches
AND the `head.ref`s of a run-level closed-PR listing (`candidateNames`, issue #703 / #705 review),
so a deleted-at-merge branch is still a candidate even though `listBranches` can no longer see it,
and its PR's `merged_at` (bounded by the same trailing window as every other signal) proves the
claim alive. A second, independent job in the same workflow surfaces `status/blocked` issues silent
for **over 72h** (a dead-man's-switch for parked items) with a "still blocked" comment, once per
silence window.

**Residual — `getBranch` reports the branch tip's COMMITTER date**, which `git rebase` rewrites
even when no new work happened: a rebased-but-otherwise-idle UNMERGED branch reads as live.
PARTIALLY CLOSED (issue #703): a MERGED branch's liveness is now proven by its PR's immutable
`merged_at` regardless of any rebase or deletion of the branch itself. Still open: an unmerged
branch, rebased with no real new work, keeps reading as live via its commit date — distinguishing
that from a genuine commit would need comparing tree contents across runs, which the reaper's
stateless decision function does not do.
