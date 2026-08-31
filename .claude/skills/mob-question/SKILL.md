---
name: mob-question
description: >
  The founder is asking a question he wants put to the mob. Fan out to the lens roster, then
  synthesise. Invoked ONLY by the founder as `/mob-question` -- never selected by the model. The
  reversibility class sizes the roster; divergences between lenses are REPORTED as divergences and
  never averaged; the register check runs before the fan-out, not instead of it.
disable-model-invocation: true
---

# `/mob-question` — fan out, then report the disagreements

**What the founder is doing.** He is asking a question and spending the roster on it. The tag says
*"this one is worth thirteen reads"* — or however many the class earns.

## Procedure

1. **Register check FIRST, before the fan-out.** Run the four steps of
   [`coordinator-register-check`](../coordinator-register-check/SKILL.md). This is not ceremony: if
   the register already decides the question, fanning out spends the whole roster re-deriving a
   recorded answer, and — worse — invites thirteen lenses to reason from a premise the records
   already contradict. The trail goes **into the briefing**, so every lens reads the same
   controlling record and can dispute it.
2. **Size the roster by reversibility class**
   ([ADR-20260816-134352](../../../docs/adr/ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)):
   - **Full mob** — money movement, stored event shapes, legal surfaces, and anything Tours-facing.
     This is the `HOLD: human` axis, and **it wins when the two disagree**.
   - **2–3 lenses** — reversible refactors, generated artifacts, doc sweeps.

   At a **briefing** the roster is **invited by default and a lens excuses itself**; *"nothing in my
   lens"* is a complete answer. Coordinator-chosen subsets belong to the checkpoint, not here.
3. **Brief the whole roster in parallel, before any answer is composed.** Each lens names what it
   will catch. Cite the lens that carried a finding — the doctrine voices are separate from the
   operations role: `young` (read/write separation, folds as disposable projections, event
   versioning, set-based validation), `vernon` (aggregate boundaries, one aggregate per transaction,
   process managers, Ask vs Tell), `evans` (ubiquitous language, bounded contexts, ACLs,
   distillation), and `architect` for operations.
4. **Give the lenses the question, not your answer to it.** A briefing that leads with a proposed
   conclusion collects agreement, not review.
5. **Synthesise — and report divergences AS divergences.**

## Never average the lenses

> **Two lenses that disagree are a finding. The disagreement is the output.**

Averaging them, or picking the majority, destroys exactly the information the fan-out was paid for.
On 2026-08-31 both `vernon`/`evans` and `architect`/`vernon` diverged, and in **both** cases the
averaging would have lost the finding. Report it as:

- what each lens said, in its own terms;
- **what they actually disagree about** — usually a premise, not a conclusion;
- what would settle it (a record, a file read, a founder decision);
- a recommendation, marked as yours and separable from the lens outputs.

A unanimous roster is reported as unanimous. Manufactured consensus is not.

## The escalation duty

The same three conditions as `/direct-question`, pointing the other way — here the fan-out is
already happening, so the duty is to **say what the fan-out cannot settle**:

- **A controlling record the question appears to contradict** makes this a **decision reversal**,
  not an option space. Say so in the briefing and in the answer, name the record, and route it to
  `/decision` — a register row and an ADR — rather than letting a lens majority quietly reverse a
  recorded decision. Thirteen lenses agreeing does not amend a record.
- **A `HOLD: human`-axis subject** gets the **full** roster, whatever the tag's apparent scope, and
  the answer says the class was recognised.
- **A legal surface** gets the legal lens — and the answer states, every time, that **no lens
  output and no aggregation of lenses is legal advice or clearance**
  ([ADR-20260812-143619](../../../docs/adr/ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)).
  Some things are legal preconditions in France rather than backlog items — allergen declaration for
  distance selling, VAT and a compliant receipt, GDPR erasure, who holds customer funds. Flag them;
  never defer them silently.

## Limits

- **Not a decision.** The mob produces findings and a recommendation. If he then decides, that is
  `/decision`, and the resulting record carries a `Consulted:` block — **one line per lens** —
  because a lens never asked is indistinguishable from a lens with nothing to say.
- **Not a dispatch.** A mob answer never starts work; `/work` does.
- **Not a substitute for the independent review.** The mob at briefing is the *first* look and the
  checkpoint is the *second*; the independent full-diff review by eyes that did not write the change
  remains the **third**.
- **Context discipline**: lenses read the **dispatch card**, not the repo — one coordinator-authored
  file, SHA-stamped, with replies appended to its Findings block. And a dispatch card **may not
  state a derived number without naming its antecedents**; any bare number it does state is marked
  `UNVERIFIED input`
  ([ADR-20260817-105845](../../../docs/adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)).
  Widening the roster puts *more* readers in front of the same unverified figure, so the wider the
  fan-out, the more this binds.
