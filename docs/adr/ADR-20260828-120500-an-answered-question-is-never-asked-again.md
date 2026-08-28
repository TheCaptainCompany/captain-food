# ADR-20260828-120500 — An answered question is never asked again

**Status**: Accepted · **Date**: 2026-08-28 ·
**Decider**: the **FOUNDER / Tech CEO**, verbatim below ·
**Prompted by**: the round-5 call-sheet answers ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted.

## The directive, verbatim

The round-5 call-sheet answers, in full (this ADR is the same-turn record the register rows'
`decided_by` resolves to):

> *"1. Flip identity resolution: Wait - flip after a few quiet days.*
> *2. Outage state: Decide later.*
> *3. Next up: Back to the product backlog top.*
> *Note: We need to ensure that agents do not ask questions already answered"*

## The rule, made executable

Prose is the fallback; the mechanism is the rule (compiler-first spirit, ADR-20260803-234035):

1. **Every founder answer lands as a register row in the same turn** — a `docs/decisions/<KEY>.yaml`
   row whose `evidence` quotes the founder's words verbatim and states scope honestly (what the
   answer does and does NOT settle). The coordinator transcribes; the verbatim quote is what makes
   the transcription auditable rather than lossy (process-lens actor question, answered). The
   round-5 answers are the first instance: `IDENT1-RESOLUTION-ACTIVATION` (decided),
   `IDENT1-OUTAGE-EXPERIENCE` (open, revisit-at-flip), plus `ERASURE-LAUNCH-GATE` (open, the
   legal lens's escalation — a NEW question, recorded before it is ever asked).
2. **Keys name the QUESTION's scope, not the answer** (evans): `IDENT1-RESOLUTION-ACTIVATION`,
   never `IDENT1-FLIP-WAIT` — so a future register-check on the question pattern-matches the row.
3. **A deferred question carries its revisit TRIGGER in the row** (`open` + the event that
   returns it to the queue). Re-raising it before the trigger is a re-ask; raising it AT the
   trigger is executing the row.
4. **Briefings snapshot the register at briefing time** (process lens): an answer arriving
   mid-session is for the NEXT briefing, not a mid-flight re-brief — the no-re-ask check has a
   consistency point.
5. **The enforcement gate goes on the ASK** (beck's design, building on ADR-20260821-010543):
   `.claude/hooks/register-check.sh` today validates the TRAIL's shape; it does not refuse a
   question whose controlling row is already `decided`. The strengthening — read the row file and
   REFUSE the `AskUserQuestion` call with the citation when its status is non-open — is real gate
   work with its own red-first tests, carried by the tracking issue named below, not smuggled
   into this record.

## What it does not change

`HOLD: human`, the decision queue, and the carve-outs stand. A genuinely NEW question, or a
decided row whose PREMISE has changed (state the change in the trail), still goes to the founder
— the rule kills repetition, not consultation.

## Consulted

- **architect** — sound; the register is the durable answer store.
- **beck** — supplied the three-level executable design; recommends Level 2 (refuse on decided
  row) as cheapest-strongest; notes the one untestable residue: registration discipline itself
  (a row never created cannot be matched — which is exactly why rule 1 exists).
- **business-specialist** — will cite-and-move-on; asked that the outage row name the
  paid-order-notification signal before "later" arrives (folded into the row's revisit trigger).
- **dba** — nothing in my lens; the discipline moves duplicates from cognition to configuration.
- **evans** — keys name the question scope; a check needs a defined protocol (what matches, who
  runs it, what a miss triggers) — carried into the tracking issue.
- **farley** — the wait row must state honestly what quiet days prove (old-path stability of the
  refactored seam) and not let the flip over-cite them; written into the row.
- **graphql-architect** — nothing in my lens.
- **holub** — focus note delivered separately: the product gap is the UI; operational questions
  are recorded and closed, not re-asked.
- **legal-specialist** — escalated #708 to an explicit launch-gate row (`ERASURE-LAUNCH-GATE`);
  filed-and-visible is not chosen-and-recorded.
- **observability-agent** — flip is unobservable without the #707 contract; it landed with #707.
- **reviewer** — carries the register-check in every pass already.
- **ux-designer** — outage row's revisit trigger (at flip) is correct.
- **vernon** — the check-before-ask discipline already exists in workflow.md; this ADR is the
  agent-discipline record that makes it roster-wide.
- **young** — procedural hygiene, not doctrine; nothing in my lens.
