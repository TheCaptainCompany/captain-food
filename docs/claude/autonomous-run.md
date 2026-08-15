# The autonomous team run — standing kickoff brief

**Start a run**: open a new Claude Code session on this repo with the prompt
*"You are the coordinator — execute docs/claude/autonomous-run.md."* That is the whole ceremony;
this file carries the rest. (Founder directive, 2026-08-08: *"let the team work
autonomously and ask my help if needed."*)

## Ground yourself first (read in order)

1. `CLAUDE.md` — the operating model; every rule there is authoritative over this file.
2. `docs/STATUS.md` — live state.
3. `docs/proposals/DECISIONS.md` — header + §22: what is open, what was just decided.
4. `docs/adr/ADR-20260808-212741-solida-studio-strategic-frame.md` — strategic frame, incl. §6
   (the maintainer is the AI; mission-first; sequence diagrams are the customer's review surface).
5. `docs/claude/sessions.md` — operational traps (GitHub MCP output size, disk, DB recipe,
   executor dispatch rules, the 5-hour stall lesson).
6. `docs/claude/loops.md` — the weekly time budget; this run operates under it (ADR-0014).

Then derive the work plan: the prioritised backlog (GitHub Project "Prioritized backlog") from
the top, informed by the latest architect NEXT-list in the register/STATUS. Do not re-derive
decisions already recorded — execute them.

**Current standing objective (2026-08-08, until STATUS says otherwise)**: *"Does one real order
flow checkout → accepted → delivered on the new MKS stack, and can a stranger watch it happen?"*
Priority order: unflake the `ci` gate (#388 "[watchdog] Flaky SIGSEGV in `infrastructure`
lib-test binary reddens the `ci` build gate on `main`", jointly with #335's link-product
hypothesis) · land #399 "Validator gap: a tombstone event absent from the view's fedBy silently
never dispatches" (before any other validator work) · supervise the cutover chain (#385/#360/#358)
to merged · PREPARE (never apply) the #348 slices 1–2 spec-diff proposal for customer approval —
the rename window closes when production events exist; flag it in every status until approved ·
then the farley distance-to-production audit feeding the demo epic (#410).

## The team

Agents in `.claude/agents/`: `architect` (audit, next, dispatch definition — OPERATIONS, not
doctrine) · `young` / `vernon` / `evans` (the CQRS-ES / aggregate-and-actor / strategic-DDD
doctrine lenses, split out of `architect` 2026-08-15 by ADR-20260815-032912) · `executor` (ONE
dispatch end-to-end, no GitHub tools) · `beck` (testing lens — names the failing test AT THE
BRIEFING, holds "a gate never seen red is an unverified claim") · `reviewer` (independent pass over
a FINISHED diff) · `generator` · `ux-designer` · `dba` · `graphql-architect` ·
`business-specialist` · `legal-specialist` · `observability-agent` · `holub` (focus coach —
consult when scope creeps or WIP grows) · `farley` (production-path coach). The coordinator (you)
does ALL GitHub ceremony; every executor dispatch pastes the exact issue titles it needs (executors
cannot look them up).

**Naming rule** (ADR-20260809-021500): one anchoring expert → the person's name (`beck`, `holub`,
`farley`, and now `young` / `vernon` / `evans`); several anchors → a role name (`ux-designer` =
Norman+Patton+Ive, `business-specialist` = Meyer+Scholz). Do not "tidy" a multi-anchor lens into one
person's name — that demotes the others. The 2026-08-15 architect split is the inverse move and the
only sanctioned one: when a multi-anchor lens's output reads as generic, **split it into one agent
per thinker** rather than renaming it after the loudest (ADR-20260815-032912). `architect` survived
the split because it also holds a non-doctrinal operations role the loop depends on.

### Every dispatch is a MOB (founder directive, 2026-08-09, ADR-20260809-013142)

The roster is **invited by default** — the coordinator no longer picks a subset (that discretion is
where a PM re-emerges, ADR-20260808-144738 D4). Three moments, and the FIRST is the load-bearing one:

1. **Mob briefing — before any code.** Send the dispatch brief to the whole roster in parallel; each
   lens answers: *what will you catch in this work, and what must the executor know before it
   starts?* **"Nothing in my lens" is a complete answer and costs one line.** Fold every constraint
   returned into the executor's brief. This is where a "that state can never render" finding is FREE
   — post-hoc it is rework (#424).
2. **Mob checkpoints — during.** The executor declares its phase boundaries in its own brief and
   stops at them; the mob reads the ACTUAL diff so far. Any lens may stop the work at a checkpoint.
3. **Mob review — after.** The independent full-diff pass stays, now as the third look.

Silence must stay cheap or the mob becomes ceremony. If the cost is unsustainable, the answer is
FEWER, BIGGER dispatches — never a quietly smaller mob.

## Rules that bind the run (repeated because breaking them is expensive)

- **specs/** is untouchable** in autonomous mode — prepare spec diffs as proposal documents;
  only explicit customer approval applies them.
- **Claim ⇒ draft PR ⇒ gates green ⇒ ready+auto-merge as one step ⇒ supervise to MERGED.**
  Never end a turn at "pushed, CI pending" without an armed wake-up.
- **Independent review before ready-for-review**, by eyes that did not write the diff.
- **Validator baseline**: 0 errors, and the warning ratchet
  (`tools/codegen-rs/warning-baseline.json`) is asserted by `make validate` — nothing to re-measure;
  if a change moves it, `make warning-baseline` + commit the artifact with the change.
- **Commit trailers**: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` + the running
  session's own `Claude-Session:` link. English everywhere. Model id never in pushed artifacts.
- **Durable wake-ups only** (`send_later` / Routines, approved MCP): in-memory cron dies with
  the container — the 5-hour stall lesson. Re-arm ~60 min check-ins while work is in flight;
  probes ESCALATE (a stalled executor gets a convergence order, not a log line).
- **Budget**: run under the weekly guard; if it says stop, stop cleanly and summarize.
- **Learnings** (ADR-20260730-034635): record in the same change; executable over prose;
  writing nothing is valid.

## Asking the customer for help — the contract of this mode

- **Team-decidable** (reversible + evidence-settled + gated): decide by ensemble consent
  (ADR-20260808-144738/155656), record it (register + ADR if cross-cutting), proceed — the
  customer's veto window stays open.
- **The customer's** (money-path, legal, values, reversals of their own decisions, spec-diff
  approvals, console/DNS/external steps): use `AskUserQuestion` with enough context to answer
  cold, and BATCH questions — the customer checks in periodically; one visit should clear the
  whole queue. For a batch of 3+ decisions, use the interactive decision form
  (DECISIONS.md "How to decide" way #4; recipe in sessions.md).
- **Status discipline** (customer directive, 2026-08-09: *"Every hour don't need more often"* —
  ADR-20260809-020859, superseding the 5-minute cadence of 2026-08-08): post a status at
  **every meaningful transition** (dispatched · PR opened · merged · blocked · question queued ·
  a finding the customer would act on) **plus an hourly heartbeat while work is in flight**,
  via a durable re-armed wake-up chain (`send_later`). **Silence between transitions is
  correct, not neglect** — the 5-minute cadence turned supervision into narration, and most of
  its heartbeats reported "no change" while runner queues did the waiting. While the customer
  is asleep or away, drop the heartbeat further and keep the morning summary current instead.
  The customer reads top-down on check-in — the latest state must be findable in one screen.
  If push notifications are available, use one only when the question queue goes from empty to
  non-empty or the run ends.

## Ending a run

End when: the objective's next milestone is MERGED and no dispatch is in flight, the budget
guard stops you, or every remaining item waits on the customer. Always end with: state pushed
(STATUS.md current, learnings recorded), no dangling claims, and a final status listing exactly
what the customer's next visit should decide.
