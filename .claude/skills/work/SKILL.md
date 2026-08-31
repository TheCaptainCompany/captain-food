---
name: work
description: >
  The founder is telling the coordinator to launch work on something. Run the existing dispatch
  pipeline -- architect names the chunk if he did not, claim, branch, draft PR, mob checkpoints,
  independent review, then ready plus auto-merge or HOLD human. Invoked ONLY by the founder as
  `/work` -- never selected by the model. Adds nothing to the protocol; it names the entry point and
  the three things this tag decides.
disable-model-invocation: true
---

# `/work` — the entry point to the existing pipeline

**What the founder is doing.** He is telling you to start. Not asking whether you should.

The pipeline is **already documented and unchanged by this command**. Do not restate it here or in
the answer:

- **method and prioritisation** — [`docs/BACKLOG.md`](../../../docs/BACKLOG.md), with value-first
  ordering from [ADR-20260720-213024](../../../docs/adr/20260720-213024-value-first-issue-prioritisation.md)
  and the delegation in
  [ADR-20260810-215503](../../../docs/adr/ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md);
- **claim protocol** — [ADR-20260720-233000](../../../docs/adr/20260720-233000-claim-protocol-stale-reaper.md),
  [ADR-20260721-042018](../../../docs/adr/20260721-042018-claim-time-draft-pr-automerge-supervision.md),
  [ADR-20260721-044613](../../../docs/adr/20260721-044613-auto-merge-never-armed-before-completion.md);
- **merge posture** — [ADR-20260815-115220](../../../docs/adr/ADR-20260815-115220-auto-merge-on-green-by-default-hold-human-for-the-named-class.md)
  as amended by [ADR-20260815-134655](../../../docs/adr/ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md);
- **mob mechanics** — [ADR-20260816-134352](../../../docs/adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md);
- **review cadence** — [ADR-20260826-084500](../../../docs/adr/ADR-20260826-084500-one-review-pass-per-presentation-and-findings-are-triaged-not-chased.md)
  and [`review-triage`](../review-triage/SKILL.md);
- **operational traps** — [`docs/claude/sessions/workflow.md`](../../../docs/claude/sessions/workflow.md).

## What this tag adds — three decisions, and nothing else

**1. Is the chunk named?** If he named it, that is the chunk; do not re-scope it. If he did not
("carry on", "next thing"), the **architect** names it from the top of the prioritised backlog —
never the coordinator picking. Skipping the top item needs a stated reason, and an item is **never
re-ranked to make it dispatchable**: a blocked top item is reported blocked.

**If the chunk has no issue yet, file one before claiming.** The claim protocol needs an `#NN` for
the `Closes #NN` line and the `status/in-progress` label, and `docs/BACKLOG.md` has no
issue-creation step — so this is the gap that must not stall the run. File it with the title in the
CLAUDE.md naming form, the reversibility class, and the register-check trail; then claim it. Note
that `GET /search/issues` is refused in this container, so a duplicate check lists
`GET /repos/{owner}/{repo}/issues` and filters locally.

**2. Is it dispatchable?** Run the register check before writing the card — the `Agent`-tool hook
will refuse a dispatch to a write-capable agent without a resolvable trail, and that refusal at
dispatch time is late. Two blockers stop the run before it starts:

- **A `Priority` is not an approval.** Ranking an AMBER item `Urgent` does not make it
  dispatchable. If it needs a `specs/**` change with no recorded approval, it goes back as AMBER
  with the search trail attached, not out as a card.
- **A genuine option space is a decision, not a dispatch.** It goes to the founder's decision queue
  with options, trade-offs and a recommendation.

**3. What is the merge posture?** State it **on the card**, decided before the work starts:

- **default** — ready-for-review and auto-merge armed **together, as one indivisible step**, then
  supervised to MERGED;
- **`HOLD: human`** — the named class (stored event shapes, fold/upcasting semantics, migrations;
  payments and customer-funds custody; GDPR erasure; legal surfaces; non-additive GraphQL; the
  mailbox/lease/fencing runtime; the merge/CI machinery itself). `HOLD: human` names the **team's**
  independent reviewer pass. **It is never a founder wait.** After that pass returns no blocking
  finding and the gates are green, **the coordinator merges** — same session, no further approval.

The card also states its **reversibility class** and BANKS at the checkpoint whether the narrow
lens set missed anything, **with an attribution**: card defect / invited-lens depth miss / roster
width. Only a **roster-width** miss goes back to the founder, because reverting a class amends his
ruling.

## Limits

- **`/work` is not permission-seeking, in either direction.** Sessions start by themselves
  ([ADR-20260810-011500](../../../docs/adr/ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md));
  this tag is him choosing *when*, not you having waited. Never answer it with *"shall I proceed?"*.
- **The coordinator never authors the diff.** Brief, checkpoint, relay, GitHub mechanics. The
  **executor** writes every phase — code, specs under recorded approval, records. A coordinator
  push is limited to claim commits and GitHub surfaces.
- **One item per run.** An adjacent problem spotted mid-run is noted in the PR body for filing,
  never fixed in the same PR.
- **A `/work` that turns out to need an uncovered `specs/**` change stops** and hands back as AMBER
  with the trail. Editing the DSL to get unstuck is out of bounds.
- **Spec- and docs-only changes skip all of this** — they go straight to `main`, no branch, no PR,
  no claim ceremony. Note that **`.claude/**` is NOT on that list**: it is the code path.
