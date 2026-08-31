---
name: coordinator-register-check
description: >
  The coordinator's own register-check discipline — run the check BEFORE composing an answer to the
  founder, BEFORE writing a dispatch card, and BEFORE asserting that something is already decided.
  Use when about to answer any founder message, when about to dispatch work to a write-capable
  agent (executor, generator, architect), when about to say "we already decided that" or "that is
  an open question", and when a lens or the founder has just corrected a claim. Carries the
  procedure, the two legal trail shapes, and the nine worked failures that earned it. The Agent-tool
  hook enforces the dispatch half; this skill is the only mechanism for the prose half.
---

# Coordinator register-check — compose from the register, not from the conversation

**The failure this exists to stop.** The coordinator reads CLAUDE.md, then a long conversation
accumulates, and answers start coming from the conversation instead of from the records. Every
*agent* is gated: `PreToolUse` on `AskUserQuestion` refuses a founder-facing question with no
`Register check:` trail. The coordinator had **no gate on any surface** — and in one session
produced nine failures of exactly the class the gate exists to prevent.

Founder directive 2026-08-31, verbatim: *"Ensure that you will use the ADRs and proposals from
now"* and *"You really need to use the same approach as the agents have."* Recorded in
[ADR-20260831-141500](../../../docs/adr/ADR-20260831-141500-the-coordinator-gets-the-register-check-gate-on-its-committing-surface.md).

**A skill alone was already tried and it failed.** `.claude/skills/decision-lookup/` existed
throughout that session and was invoked **zero times**. An un-invoked skill is prose with extra
steps. That is why the dispatch half is now a hook and not a paragraph — and why the honest limit
below matters rather than being a caveat.

## The procedure — four steps, in order

**1. Get candidates (advisory).**

```
.claude/skills/decision-lookup/scripts/decision-lookup.sh "<the question in your own words>"
```

At most three candidates. **A candidate is never evidence.** An empty result is *not* "undecided" —
the index is Markdown-only, corpus-masked and possibly stale.

**2. Read the candidate directly.** Open the record and read *around* the hit, not the matching
line. A grep hit inside a rejected alternative, a quoted question or a struck clause reads as an
answer out of context. Check for a later word: an `Amendment`/`Superseded` banner, a strike, a
`reconsiders:` row pointing at a later decision.

**3. Resolve the exact row.** `docs/decisions/<KEY>.yaml` at HEAD is authoritative for **current
status**; the prose row in `DECISIONS.md` is its *history*. The retrieval index cannot see row
files, so this step is never skippable "because the tool already found it".

**4. State the trail** — one line, in exactly one of the two shapes defined in
[`docs/claude/sessions/workflow.md`](../../../docs/claude/sessions/workflow.md), which is the only
place the format is defined:

```
Register check: <record id> (<date>, <status>) -- covers <X>, silent on <Y>
Register check: no controlling record -- terms: <terms searched>; nearest: <record id or none>
```

The `covers <X>, silent on <Y>` clause is the load-bearing half: it is where a *partial* match is
forced to admit what it does not cover. Failure 1 below is exactly a record that covered the
question being read as if it did not exist; failure 9 is a record that covered it being about to be
contradicted.

**The negative is a PASSING trail.** "No controlling record — terms: …" is a complete, correct
answer. Never silently drop a question because the search got harder.

## What the hook enforces, and what it cannot

**Enforced (Lane D of `.claude/hooks/register-check.sh`, `PreToolUse` on the `Agent` tool):** a
dispatch to a **write-capable** agent must carry a trail whose record id **resolves to a file** under
`docs/adr`, `docs/proposals`, `docs/legal` or `docs/status`, or an explicit negative that names its
`terms:`. A literal `Register check: none` is refused; an invented id is refused.

The discriminator is the **target agent's own `tools:` frontmatter** — write-capable (`architect`,
`executor`, `generator` today) is gated, read-only is not. So lens consults and the `reviewer` pass
never see this gate, and there is no exemption list to drift: granting an agent a write tool pulls
it into the gate in the same commit.

**NOT enforced, and this is the honest limit: a hook gates a TOOL CALL.** The coordinator's **prose
answers to the founder are not tool calls** and cannot be blocked the way `AskUserQuestion` is. Of
the nine failures below, the dispatch-shaped ones are now gated; **the answer-shaped ones are not**.
This skill is the only mechanism there and it is *weaker* — which is a positive argument for
**routing more coordinator→founder questions through `AskUserQuestion`**, where the gate already
bites, rather than composing them as prose.

Nor can the hook prove the trail is the *right* record, that it was read, or that the card's claims
follow from it. It proves a resolvable citation was produced. The rest is this procedure.

## The nine worked examples

They are here because a rule with its cost attached is the one that gets followed. **Four of nine
were caught by the founder or a lens** — that is the price paid for not running the check.

| # | What was said | What the register actually held | Caught by |
|---|---|---|---|
| 1 | Framed *"must the PM stop saving?"* as an open option space | **Decided**: `ADR-20260829-230418` + `specs/common/processmanager.yaml:7-9` | **Founder** |
| 2 | Told the founder C4 was a "load-only port" | His ruling: the PM must not load **either** | **Founder** |
| 3 | Asked *"Want me to have that specified and dispatched?"* | `ADR-20260810-011500` forbids "shall I proceed?" — sessions start by themselves | — |
| 4 | Proposed a new counsel posture | `BRIEF-20260819` §4.2 already records the two carve-outs (authorisation questions, fiscal receipts) that may not be self-answered at any labelling | **Legal lens** |
| 5 | Cited `pm_orchestrators.rs:844-852` as the `state.by` gate | It is `:964-972`; the cited range is `pm_adapt`'s `FromRead` arm and **reads as confirming the claim while showing the opposite** | **Executor** |
| 6 | Stated "four config keys" | The model admits **three** | **Executor** |
| 7 | Asserted the PM should call the typed actor clients | `crates/actor_client/Cargo.toml` declares `application` as a dependency — a dependency-rule inversion, never buildable | **Lens** |
| 8 | Presented the PM `read:` step as a contradiction needing a decision | **while quoting `ADR-20260815-030206`, which decides it** | — |
| 9 | Was about to dispatch "retire the `read:` step" | Contradicts `PROP-20260815-142349` (Approved 2026-08-15, founder verbatim *"I'm ok for the dsl for process manager"*): `read:` **stays**, with a `source:` enumeration, and the richer grammar arrives **additively** | **The check itself** |

**#9 is the one that matters.** It is the only one caught *before* it did damage, and it was caught
by running this procedure before dispatching. That is the whole argument for the skill.

**#5 and #6 are a distinct sub-class**: not a missed record but a **fabricated antecedent** — a line
range and a count, both stated with confidence, both wrong, and #5's citation actively misleading
because the wrong range *looks* like it confirms the claim. That is what
[ADR-20260817-105845](../../../docs/adr/ADR-20260817-105845-a-dispatch-card-may-not-state-a-derived-number-without-its-antecedents.md)
governs: **a dispatch card may not state a derived number without naming its antecedents, and any
bare number it does state is marked `UNVERIFIED input`.** Re-read a cited record — and re-open a
cited line range — *at the moment it licenses an action*.

## Before asserting "already decided"

The same rule binds the assertion, not just the question: **no citation, no assertion.** Reciting
from CLAUDE.md is answering from a *projection* — it is correct only while the index is current, and
a disagreement between the index and the underlying record is a **staleness report**, not a founder
question. Say what disagrees and point at the newer record.

## Related

- [`docs/claude/sessions/workflow.md`](../../../docs/claude/sessions/workflow.md) — the canonical
  rule and the only definition of the trail format.
- [`docs/decisions/README.md`](../../../docs/decisions/README.md) — the row schema and the
  `Decision row: <KEY>` envelope for founder-facing **decision** questions.
- [`.claude/skills/decision-lookup/`](../decision-lookup/SKILL.md) — advisory retrieval, step 1.
- [`.claude/hooks/register-check.sh`](../../hooks/register-check.sh) — both surfaces; Lane D is the
  dispatch gate. Proven by `.claude/hooks/register-check-selftest.sh` (cases D1–D12, LD1–LD3),
  which runs in CI's `gate-scripts` job and from the Stop hook every turn.
