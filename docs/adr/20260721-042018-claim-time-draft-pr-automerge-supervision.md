# ADR-20260721-042018 — Claim-time draft PR + auto-merge supervision (amends ADR-20260720-233000)

## Status

Accepted (product-owner directive, 2026-07-21)

## Context

ADR-20260720-233000 made the claim atomic (`status/in-progress` label + claim comment naming the
`NN-slug` branch) but left the branch/PR creation moment unspecified. In practice sessions coded
first and opened the PR at the end, so for hours: the Development sidebar showed nothing, the
board's native "PR linked → In progress" workflow never fired, the claim comment was the only
pointer, and the stale-claim reaper saw no linked-PR activity. Separately, the repo now has
**auto-merge** enabled (product-owner configuration): once a PR satisfies main's merge
requirements it merges without a human click — sessions ended their turn at "pushed, CI pending",
leaving nobody watching the checks that gate that merge.

## Decision

The issue workflow becomes a fixed three-phase session contract (method in `BACKLOG.md`):

1. **Claim = label + comment + branch + draft PR, immediately** (before any implementation work):
   - add `status/in-progress` + the claim comment naming `NN-slug` (unchanged — the label stays
     the atomic claim);
   - create branch `NN-slug` from latest `main` and push it;
   - open a **draft PR** `NN-slug → main` right away, body starting with `Closes #NN` plus the
     intended approach. From minute one the Development sidebar shows branch + PR, the board
     flips to In progress (native workflow), and the reaper sees linked-PR activity.
   - Draft status is a safety interlock: GitHub refuses auto-merge on a draft, so an
     early-created PR can never merge half-done work.
2. **Work happens on the PR**: push commits to `NN-slug`; the `ci` workflow gates every push.
3. **Completion = ready + auto-merge + supervision** (a session does not end its turn at "pushed"):
   - local gates green (`make rust` — build + tests + validate + no drift), STATUS.md/ADR updated
     in the same change;
   - mark the PR **ready for review** and **enable auto-merge** (repo default merge method);
   - **supervise until MERGED**: watch checks (subscribe to PR activity where the harness supports
     it), fix and push on any failure, re-check after each round. The merge auto-closes the issue
     (`Closes #NN`), which ends the claim. If checks cannot be made green or scope explodes,
     comment the diagnosis on the PR instead of going silent.

## Security analysis — is repo-level auto-merge risky? (the "fake empty PR" question)

Question examined: with "Allow auto-merge" enabled, could an outsider open an empty/malicious PR
against `main` and have it merge itself?

**No — auto-merge adds no merge authority.** Three server-side facts bound the risk:

1. "Allow auto-merge" (repo setting) only makes the per-PR button available. Auto-merge must be
   **enabled on each PR by someone with write access to the base repo**. An outside (fork)
   contributor cannot enable it, and cannot merge; their PR just sits open until a maintainer acts.
2. When auto-merge fires, it merges **as that enabling user** — subject to exactly the same branch
   protection/ruleset requirements as a manual merge (required status checks, reviews,
   up-to-date rules). It is a deferred click, not a bypass.
3. Fork PRs run `pull_request` workflows with a read-only token and **no repo secrets**, and this
   repo does not use `pull_request_target`; keep the default "require approval for first-time
   contributors' workflow runs" so outsiders can't even burn Actions minutes freely.

**The real risks are configuration-side, not outsider-side:**

- **If `main` has no required status checks, auto-merge merges almost immediately** — the setting
  is only as strong as the branch protection behind it. REQUIRED: a `main` ruleset/branch
  protection requiring the **`codegen`** check (the `ci` workflow's job — build + tests +
  validator + drift gate) before merge. Verify in Settings → Branches/Rules; this cannot be
  asserted from inside the repo.
- **Any write-access actor — including agent sessions — lands unreviewed code once CI is green.**
  That is the deliberate operating-model trade (the executable gates ARE the review:
  validator 0 errors, behaviour-test completeness, drift check, stop-gate hooks); the PR trail +
  STATUS/ADR discipline is the audit record. If that trade ever needs tightening, add CODEOWNERS
  on `specs/**` requiring product-owner review — accepting that it breaks unattended merges for
  spec-touching PRs.
- Keep force-pushes/deletions on `main` blocked (ruleset default) and don't grant outside
  collaborators write access.

An "empty PR to main" from outside is therefore spam at worst: it cannot enable auto-merge, cannot
pass nothing (required checks still run), and cannot merge itself.

## Alternatives considered

- PR only at completion (status quo) — rejected: invisible claims, dead board automation, unwatched
  auto-merges.
- Non-draft PR at claim time — rejected: with auto-merge armed repo-wide, a green-but-incomplete
  branch could merge the moment someone enables it; draft is the interlock.
- Disable auto-merge entirely — rejected: reintroduces the human click for every agent PR;
  the risk analysis above shows required checks + write-access gating bound the exposure.

## Consequences

- Positive: issue↔branch↔PR linkage exists from minute one; the board reflects reality; the
  reaper's linked-PR signal works; merges are supervised to MERGED, not abandoned at "CI pending";
  the auto-merge threat model is written down.
- Negative: one more API round-trip at claim time; an abandoned claim now also leaves a draft PR
  to close (the reaper handles the label; the PR is closed when the claim is released).
- Follow-up (product owner, repo Settings — not verifiable from the repo): confirm the `main`
  ruleset requires the `codegen` status check and blocks force-pushes.
