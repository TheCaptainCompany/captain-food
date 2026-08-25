# Status journal — 2026-W35

Journal entries for ISO week 2026-W35, newest first, in the order they were written.
Current state: [`../STATUS.md`](../STATUS.md).

> **Week boundary, recorded once so the next reader does not hunt**: 2026-08-24 is ISO week **35**,
> but several entries dated 2026-08-24 sit in [`journal-2026-W34.md`](journal-2026-W34.md) — earlier
> sessions filed them there before this file existed.

> ⚠️ **2026-08-25 — `claude-review` is hardened while STILL REQUIRED: the founder took the bypass
> over the recorded path, and the exposure is carried knowingly.** [#677](https://github.com/TheCaptainCompany/captain-food/issues/677)
> is real — the claude-code-action refuses to run on a PR that edits its own workflow file and
> **exits 0**, so a required review gate clears itself unreviewed. Fixed on
> [#680](https://github.com/TheCaptainCompany/captain-food/pull/680) by asserting the outcome the
> prompt already promises. Record: `ADR-20260825-005323`.
>
> **The ordering was the defect, not the fix.** `farley`'s register check surfaced
> [DECISIONS §45 REV-1](../proposals/DECISIONS.md): `claude-review` was decided OUT of the required
> checks on 2026-08-17, against the team's own recommendation, and **never executed** (403 from the
> agent proxy on the ruleset write; open on
> [#593](https://github.com/TheCaptainCompany/captain-food/issues/593)). Hardening a
> decided-out-but-still-required check turns every "the reviewer could not post" into a **repo-wide
> merge stop** — #593 verbatim — and is self-blocking, because the revert PR would need the same
> check green. The team recommended executing REV-1 first. **The founder chose the bypass**, twice,
> with the cost stated. Recorded as declined, not as unstated. REV-1 stays open on #593 and
> executing it later removes the exposure without touching the workflow.
>
> **The gate proved itself on its own PR — and my first write-up of that proof was WRONG.** I cited
> run 32778735735 (`claude-review` success in 4s) as the false green. It was not: its log reads
> `IS_DRAFT: true` / *"draft PR - the review step is skipped by design"*, and the action step's
> conclusion is `skipped`. That run demonstrates the DRAFT path. The action never ran, never
> self-skipped, never exited 0 there. **Third false record claim in this chain, and the second one
> where I asserted evidence without opening the log.**
>
> The genuine proof is free, better, and inside **one** run — 32792130350: the step
> `Run Claude Code Review` has conclusion **`success`** while its own log says
> `Exiting due to workflow validation skip`, and the assert step reds in the same job. Success and
> no-review, side by side, one run id. That is #677 in a single artifact. `beck`'s ruling on the
> self-red: keep it; a bootstrap carve-out *is* the hole under a nicer name.
>
> **Then the reviews found the fix carried FOUR false verdicts of its own** — on a check that is
> required *today*, so each was a repo-wide merge stop I had introduced while fixing one:
> - `printf '%s' "$bodies" | grep -qF` makes grep exit at the first match, printf die of SIGPIPE and
>   `pipefail` report **141 even though grep MATCHED**. Reproduced here: green at 64 000 trailing
>   bytes, **FALSE RED at 128 000 and 512 000**. PR #674 already carries ~35 KB of bot comments, and
>   it grows with PR length. The match now runs entirely inside `jq` — no pipe, no SIGPIPE.
> - The marker named `head.sha`, but `actions/checkout` on a `pull_request` event takes the **MERGE
>   ref** (`refs/remotes/pull/680/merge` in the live log), so a reviewer resolving `git rev-parse
>   HEAD` reports a different sha. Worse, across **23 real bot comments** it wrote a bare 40-char
>   sha **zero** times — 19 backticked, 41 abbreviated. An exact-string match would have redded
>   every real review. **The pass path had never once been exercised**, and structurally cannot be
>   on this PR.
>
> **Recorded because it is not derivable from the code, and cost real time twice**: a workflow file
> **cannot** make itself required or un-required, and cannot stop the ruleset accepting `skipped`.
> Every edit to `claude-code-review.yml` is a *verdict-honesty* fix and never a requiredness fix.
> Requiredness lives in ruleset `19179892`. Anyone reading #677 as "option 2 solved requiredness" is
> reading it wrong.
>
> **And the shape worth carrying forward**: this gate can only ever prove *"a GitHub App bot comment
> ending in a marker for this commit exists"*. It cannot prove WHICH bot — the team's own mob-lens
> sessions post under the identical `claude[bot]` identity. The step is named and commented for what
> it proves, not for what one wishes it proved. The independent reviewer also **declined to post its
> review as a PR comment**, because doing so would have carried the marker and flipped the check
> green — destroying the evidence the PR rests on. That is the limit, demonstrated rather than
> argued.

