---
name: direct-question
description: >
  The founder is asking the coordinator a question directly and does not want the mob fanned out for
  it. Answer from the register, in this turn, without a lens consult. Invoked ONLY by the founder as
  `/direct-question` -- never selected by the model. Skips the MOB, never the REGISTER CHECK: the
  answer carries a `Register check:` trail, and a controlling record the question appears to
  contradict, or a HOLD-class subject, means say so and fan out anyway rather than answering under
  the tag.
disable-model-invocation: true
---

# `/direct-question` — the mob is skipped, the register is not

**What the founder is doing.** He is asking *you* a question and has decided, in advance, that this
one does not need thirteen lenses. The tag is his routing choice. It is a convenience about **who
answers**, never a licence about **what the answer is built from**.

## The one rule

> **`/direct-question` skips the mob. It never skips the register check.**

This is the whole point of the command having a written procedure. The coordinator's catalogued
failures split into dispatch-shaped and answer-shaped ones, and **only one of the nine was
caught at the dispatch gate** (#9). The rest were answer- or question-shaped — a wrong claim
composed as prose to the founder
([`coordinator-register-check`](../coordinator-register-check/SKILL.md), the table of nine). The
`PreToolUse` hook cannot see a prose answer: it gates `AskUserQuestion` and `Agent` tool calls, and
an answer is neither. So a direct answer is the surface where the check is **least enforced and most
needed**. Removing the mob removes the other reader who might have caught it. Both cannot go.

## Procedure

1. **Run the register check** — the four steps of
   [`coordinator-register-check`](../coordinator-register-check/SKILL.md): advisory candidates, read
   the record *around* the hit, resolve `docs/decisions/<KEY>.yaml` for current status, state the
   trail. Scale the search to the question; one well-aimed grep plus reading its record is the
   floor, not a sweep.
2. **Apply the escalation test** below, before composing anything.
3. **Answer**, with the trail line in the answer's own text, in exactly one of the two shapes
   defined in [`docs/claude/sessions/workflow.md`](../../../docs/claude/sessions/workflow.md) —
   the only place the format is defined:

   ```
   Register check: <record id> (<date>, <status>) -- covers <X>, silent on <Y>
   Register check: no controlling record -- terms: <terms searched>; nearest: <record id or none>
   ```

   The negative is a **passing** trail. "No controlling record — terms: …" is a complete answer, not
   an admission of failure.
4. **If the answer turns out to be a decision he is recording**, it is `/decision`, not this. Say so
   and switch; do not let a question quietly become a record.

## The escalation duty — say so and fan out anyway

Three conditions **override the tag**. In each, do not answer under `/direct-question`; state which
condition fired, and fan out.

- **The register holds a controlling record the question appears to contradict.** Answering "yes"
  to a question that reverses a recorded decision is how a reversal lands unflagged. Name the
  record, quote the clause, and say that the answer now looks like a decision reversal — which is
  a register row and an ADR, not a reply.
- **The subject is on the `HOLD: human` axis**: money movement, customer-funds custody, a stored
  event shape or fold/upcasting semantics, a DB migration, GDPR erasure, a legal surface (allergens,
  VAT/receipt, P2B terms), a non-additive GraphQL change, the actor mailbox/lease/fencing runtime,
  the merge/CI machinery — or anything Tours-facing. These are exactly the classes
  [ADR-20260816-134352](../../../docs/adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)
  prices at a **full mob**, and the founder's own rule
  ([ADR-20260812-143619](../../../docs/adr/ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md))
  is that every founder message reaches the roster before an answer. A tag he typed for convenience
  is not him rescinding that rule for the classes it was written for.
- **The honest answer is "I do not know and one lens would".** Guessing under a tag that suppressed
  the lens is worse than the round-trip.

Saying *"this one needs the mob, here is why"* IS a valid response to `/direct-question`. It is
faster than a wrong answer and it is the founder's own rule being honoured, not overridden.

## Relationship to ADR-20260812-143619, stated rather than implied

That ADR requires **every founder message to reach the whole roster before any answer is composed**.
`/direct-question` is the founder electing, per message, not to spend that fan-out. Two carve-outs
already existed in the ADR that permit a same-turn answer with no consult, and they are the model
for how this tag behaves:

- an **external-clock fact** — billing suspension, token expiry, partner deadline, opposition
  window — is relayed **in the same turn, verbatim from the register**, with the mob's opinion
  following after;
- **executing an already-recorded rollback/abort path** needs no consult, while going *forward*
  through an incident is a new decision and does get the mob. The test is: *am I executing a
  recorded path, or inventing one?*

Note what both carve-outs have in common: they are cases where **the register already holds the
answer** and the fan-out would only re-derive it. That is the shape `/direct-question` fits. It is
not a carve-out for questions the register has *not* answered — for those, the tag buys speed on
retrieval, and the escalation test above still decides whether a lens is owed.

Third clause of the same ADR, unchanged and worth restating here because this command is where it
will be tested: **no lens output, and no aggregation of lenses, is legal advice or clearance.**
Neither is a direct answer from the coordinator.

## Limits

- **Not a route around team ownership.** `/direct-question` is him asking you; it is never you
  asking him *"shall I proceed?"* — [ADR-20260810-011500](../../../docs/adr/ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
  forbids that shape, and answering a direct question by proposing to do work and awaiting approval
  reintroduces it through the back door. If the answer implies work, say what will be done and use
  `/work`'s pipeline when he asks for it.
- **Not a decision record.** Nothing here writes an ADR, a register row or a journal entry. If the
  exchange produced a decision, that is `/decision`.
- **No citation, no assertion.** The same rule binds asserting *"we already decided that"*.
  Reciting from CLAUDE.md is answering from a **projection**, correct only while the index is
  current; a disagreement between the index and the underlying record is a **staleness report**, not
  an answer.
- **Never invent a citation to satisfy the trail.** A fabricated antecedent — a line range, a count,
  a record id — is the failure class
  [ADR-20260817-105845](../../../docs/adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)
  governs. Prefer a **symbol** to a line range; a range decays every time the file moves, and a wrong
  one *looks* confirming. If a range is unavoidable, name the commit it was read at.
