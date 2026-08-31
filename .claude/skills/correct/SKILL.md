---
name: correct
description: >
  The founder is telling the coordinator that something it did or said is WRONG. Treat as
  authoritative, then PROPAGATE: find everything downstream that rests on the wrong claim and fix
  that too, because a wrong claim travels. Invoked ONLY by the founder as `/correct` -- never
  selected by the model. Ends in a record, in the same change.
disable-model-invocation: true
---

# `/correct` — the correction is authoritative, and the propagation is the work

**What the founder is doing.** Telling you that something you did or said is wrong.

## Take it as authoritative

Accept the correction and act on it. Do not defend the original claim, do not re-argue it, do not
open with an apology paragraph. If some **narrow factual** part of the correction cannot be
reconciled with the code or the register — the file genuinely says otherwise — say that in one
sentence **with the evidence**, and proceed on the correction for everything else. That is a
staleness report, not a disagreement, and it is the only shape in which pushback is useful here.

The failure to avoid is the opposite one: agreeing verbally and changing nothing, or changing only
the sentence he quoted.

## The distinguishing handling: PROPAGATE

> **A wrong claim does not sit still. Find everything downstream of it and fix that too.**

This is what makes `/correct` different from *"fix this"*. The correction names **one** occurrence.
Your job is the **closure** of it.

**The worked example — 2026-08-31.** A wrong claim of the coordinator's propagated into two later
messages and went on to **frame a founder decision** before a lens caught it. By then the wrong
premise was load-bearing in an argument the founder was being asked to rule on. Correcting only the
original sentence would have left the framing standing — and the framing was the part that mattered.

So, after fixing the named occurrence, sweep for the rest. **Grep the old term** across `specs/**`,
`docs/**` and open issue/PR bodies — this is the standing rule after any decision that renames or
reshapes something, and a correction is that. Then check, concretely:

- **Records** — ADRs, proposals, register rows, `STATUS.md`, journal entries written on the wrong
  claim. A proposal is **rewritten in place** (LIVING doctrine), never appended to.
- **Live GitHub surfaces** — issue and PR bodies, dispatch cards, review comments. A wrong claim in
  an open PR body is read by every reviewer who arrives after it. A comment that already went out
  wrong is repaired **in place** via `PATCH /repos/{o}/{r}/issues/comments/{id}`.
- **Decisions taken on it.** The expensive class. If the wrong claim framed a decision, the decision
  itself may need revisiting — **surface that to the founder rather than silently re-deciding it**.
  He decides whether the ruling stands on corrected facts.
- **In-flight work.** An agent dispatched on the wrong premise is working on a bad card **right
  now**. Stop it or correct the card; a card is a **cached fold** and a stale one is DISCARDED
  rather than patched.

Report the propagation explicitly: *here is what else rested on it, here is what I changed, here is
what I checked and found clean.* The last clause matters — a sweep with no negative results is
indistinguishable from a sweep that never ran.

## Then record it

[ADR-20260730-034635](../../../docs/adr/ADR-20260730-034635-every-session-records-what-it-learned.md)
requires the learning **in the same change as the work** — not just failures, not only on the second
occurrence. And every **recurring** agent/loop failure becomes a rule, a test or an ADR.

Choose the weakest sufficient artifact, but choose one:

- **Prefer executable over prose.** A validator rule, a test or a hook beats a bullet, because prose
  can be ignored and a gate cannot. **Compiler first**: ask whether the type system can make the
  mistake unspellable before writing a gate at all.
- **Sharpen an existing rule rather than appending a near-duplicate.** A near-duplicate buries the
  rule it duplicates; padding a session doc is a net loss.
- **Record only what is not derivable from the code and would cost the next session time** —
  **with the cost that earned it**. `None` is a legitimate outcome for the *learning*, though never
  for the propagation.

Where does it go: operational findings → [`docs/claude/sessions.md`](../../../docs/claude/sessions.md)
or the topic file; decisions → an ADR; option spaces → a proposal; durable state → `STATUS.md`;
dated history → the **top** of the current `docs/status/journal-YYYY-Www.md`.

## Limits

- **A correction is not a decision reversal.** If it turns out he is changing a recorded decision
  rather than correcting a mistake, that is `/decision` — with its reversal check and its register
  row. Say which one you think it is.
- **Findings are triaged, not chased**
  ([ADR-20260826-084500](../../../docs/adr/ADR-20260826-084500-one-review-pass-per-presentation-and-findings-are-triaged-not-chased.md)).
  The propagation sweep will surface adjacent problems. They are **blocking** (fix here),
  **non-blocking** (one linked issue), or **not-a-finding** (say so, change nothing). A `/correct`
  that expands into a general cleanup has lost its terminating condition — the same failure that
  cost #679 a night and 114 commits.
- **Never weaken a gate to make the correction land**, and never hand-edit generated output. If a
  behaviour test now fails, fix the generator or the runtime.
- **One correction, one closure.** Finish the propagation and stop. Do not start work the correction
  merely revealed; report it and let him call `/work`.
