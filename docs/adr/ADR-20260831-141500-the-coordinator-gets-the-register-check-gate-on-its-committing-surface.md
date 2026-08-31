# ADR-20260831-141500 — The coordinator gets the register-check gate on its committing surface

**Status**: Accepted · **Date**: 2026-08-31 ·
**Decider**: the **FOUNDER / Tech CEO**, verbatim below ·
**Prompted by**: nine register-check failures by the coordinator in one session ·
**Realizes**: [#814 "The coordinator has no register-check gate: dispatch cards and direct answers are composed from the conversation, not from the register"](https://github.com/TheCaptainCompany/captain-food/issues/814) ·
**Enforced by**: `.claude/hooks/register-check.sh` Lane D (`PreToolUse` on the `Agent` tool), wired
in `.claude/settings.json`, proven by `.claude/hooks/register-check-selftest.sh` cases D1–D12 /
LD1–LD3 / W4–W7 in CI's `gate-scripts` job ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted.

## The directive, verbatim

> *"Ensure that you will use the ADRs and proposals from now"*
>
> *"You really need to use the same approach as the agents have."*

## Context — the asymmetry, and its measured cost

Since 2026-08-21 every **agent** has been gated on the ask: `PreToolUse` on `AskUserQuestion` →
`.claude/hooks/register-check.sh` refuses a founder-facing question that carries no `Register
check:` trail or `Decision row:` envelope (ADR-20260821-010543, ADR-20260821-095957,
ADR-20260821-103403, ADR-20260828-120500).

**The coordinator had no gate on any surface.** In one session it produced nine failures of exactly
the class the gate exists to prevent — each an answer or a dispatch composed from the conversation
instead of from the register:

1. Framed *"must the PM stop saving?"* as an open option space. **Decided**: ADR-20260829-230418 +
   `specs/common/processmanager.yaml:7-9`. **Founder corrected it.**
2. Told the founder C4 was a "load-only port". His ruling: the PM must not load either. **Founder
   corrected it.**
3. Asked *"Want me to have that specified and dispatched?"* — forbidden by ADR-20260810-011500
   (never "shall I proceed?").
4. Proposed a new counsel posture without reading BRIEF-20260819 §4.2, which already records the two
   carve-outs (authorisation questions, fiscal receipts) that may not be self-answered at any
   labelling. **The legal lens corrected it.**
5. Cited `tools/codegen-rs/src/emit/pm_orchestrators.rs:844-852` as the `state.by` gate. It is
   `:964-972`; the cited range is `pm_adapt`'s `FromRead` arm and **reads as confirming the claim
   while showing the opposite**. **An executor caught it.**
6. Stated "four config keys" where the model admits three. **An executor caught it.**
7. Asserted the PM should call the typed actor clients. `crates/actor_client/Cargo.toml` declares
   `application` as a dependency — a dependency-rule inversion, never buildable. **A lens caught it.**
8. Presented the PM `read:` step as a contradiction needing a decision **while quoting
   ADR-20260815-030206, which decides it**.
9. Was about to dispatch "retire the `read:` step" — contradicting PROP-20260815-142349 (Approved
   2026-08-15, founder verbatim *"I'm ok for the dsl for process manager"*), whose decision is that
   `read:` **stays** with a `source:` enumeration and the richer grammar arrives additively.
   **Caught by running the check before dispatching** — the first one the discipline stopped.

**Four of nine were caught by the founder or a lens.** That is the cost, and it is paid in the one
currency the operating model is built to protect: the founder's attention.

**Prose was already tried, and the evidence is unusually clean.** `.claude/skills/decision-lookup/`
existed throughout that session and was invoked **zero times**. An un-invoked skill is prose with
extra steps. So the remedy cannot be another paragraph asking the coordinator to remember.

## Decision

**The coordinator's committing surface gets the same gate the agents have.**

1. **A `PreToolUse` hook on the `Agent` tool.** A dispatch card is the coordinator's diff, and it was
   ungated. It is refused unless it carries a register-check trail.

2. **It EXTENDS `register-check.sh`; it does not fork it.** The existing script's trail parsing is
   already payload-generic — it greps the hook payload rather than an `AskUserQuestion`-specific
   structure — so the two surfaces are dispatched on `tool_name` inside one script (Lane D beside
   Lanes 1–3). A second near-duplicate validator would drift from the first, and the gate-script
   self-verification set stays at **four files**, unchanged, rather than growing a fifth that both
   guards would have to learn about.

3. **The discriminator is the target agent's own declaration.** Not every `Agent` call is a dispatch
   card: lens consults, the `reviewer` pass and read-only research use the same tool. The gate fires
   **iff `.claude/agents/<subagent_type>.md` grants a write tool** (`Write`/`Edit`, substring, so
   `MultiEdit`/`NotebookEdit` count) in its frontmatter `tools:` line. Today that is exactly
   `architect`, `executor`, `generator`; the other thirteen declare `Read, Grep, Glob, Bash` and pass
   untouched, logged `agent-advisory`.

   **Nothing enumerates those names.** Granting an agent a write tool pulls it into the gate in the
   same commit; revoking one drops it. This is deliberate: a hand-maintained exemption list is the
   shape this repo has retired twice, and it is the shape that goes stale silently. The rule reads:
   **a call that can produce a diff carries the trail that licenses it.**

   It **fails closed** on all three unknowns — no `subagent_type`, no agent file (`general-purpose`
   is the live case: `docs/claude/sessions/environment.md` documents pasting a charter into it as
   the standard workaround, and it holds the full tool set), or a file with no `tools:` line.

4. **The escape hatch is shut by making the trail's SHAPE checkable.** A gate satisfiable by pasting
   a literal `Register check: none` is theatre. So a **positive** trail must name a record id that
   **resolves to a file on disk** (`docs/adr`, `docs/proposals`, `docs/legal`, `docs/status`), and a
   **negative** trail must be the explicit no-controlling-record form **and** name the `terms:`
   searched. `Register check: none` is neither; an id in the right *shape* naming a record that was
   never written resolves to nothing and is refused too. (This ADR cannot spell that example out:
   the validator's own §23 `record-citation-unresolved` rule refuses a dangling ADR-shaped id
   anywhere under `docs/**` — it caught the illustration while this record was being written, which
   is the same principle one corpus over.) This is strictly stronger than the ask surface's Lane 2,
   which accepts any id-*shaped*
   token — **deliberately not back-ported**, because tightening the ask gate has its own blast
   radius and belongs in its own change.

5. **Lane D deliberately does NOT run the envelope lane or the passive key check.** On a founder
   *question*, naming a decided row means asking something already answered. On a dispatch *card*,
   citing a decided record is precisely the behaviour being enforced. Refusing a card for citing its
   own controlling record would invert the gate — case D12 pins this.

6. **A `coordinator-register-check` skill** (`.claude/skills/coordinator-register-check/SKILL.md`)
   carries the procedure — `decision-lookup` for candidates → read the candidate directly (advisory,
   never evidence) → resolve the exact `docs/decisions/<KEY>.yaml` → state the trail — with the nine
   failures as worked examples, because a rule with its cost attached is the one that gets followed.

## The honest limit — stated here rather than hidden

**A hook gates a TOOL CALL.** The coordinator's **prose answers to the founder are not tool calls**
and cannot be blocked the way `AskUserQuestion` is. Of the nine failures, the dispatch-shaped ones
are now gated; **the answer-shaped ones are not**. The skill is the only mechanism there and it is
weaker — demonstrably so, since the un-invoked `decision-lookup` skill is this ADR's own evidence
that prose does not self-execute.

That is an argument, not a resignation: it is a positive reason to **route more coordinator→founder
questions through `AskUserQuestion`**, where the gate already bites, instead of composing them as
prose in the transcript.

Nor does Lane D prove the trail is the *right* record, that it was read, or that the card's claims
follow from it — it proves a **resolvable citation was produced**. Failures 5 and 6 are the residual
class it does not reach: a fabricated line range and a wrong count, both stated with confidence, #5's
citation actively misleading because the wrong range *looks* like it confirms the claim. That class
is governed by ADR-20260817-105845 (no derived number without its antecedents) and caught by review,
not by this gate. Same honesty limit the rest of the script already states about itself.

## Consequences

- Every dispatch to `architect`, `executor` or `generator` now carries a resolvable trail or is
  refused, with the protocol fed back on stderr.
- Mob briefings, lens consults and reviewer passes are unaffected — the false-positive floor is
  pinned by case D3 and by live case LD2.
- A roster change that grants a lens a write tool silently arms the gate for it. That is intended;
  case LD1/LD2 red if `executor` stops being write-capable or `reviewer` starts being so, which is
  a roster decision worth looking at.
- **No gate was weakened.** The ask surface's contract is byte-for-byte unchanged: every
  pre-existing selftest case passes, and case D11 pins that an `AskUserQuestion` payload still reds
  on the *original* reason rather than a dispatch one.

## Alternatives considered

- **A separate `dispatch-check.sh`.** Rejected: a second near-duplicate trail validator drifts from
  the first, and it would grow the gate-script self-verification set from four to five files, which
  both guards and a codegen pin would have to learn about — cost with no capability.
- **Gate every `Agent` call.** Rejected: it fires on every mob briefing, which is how a gate becomes
  something to work around. The founder's own instruction is that the roster is invited by default
  (ADR-20260816-134352); a gate taxing that would push against a standing ruling.
- **An allowlist of dispatching agent names in the hook.** Rejected: a hand-maintained exemption
  list, the shape retired twice here, and stale the moment the roster changes.
- **Skill only, no hook.** Rejected by the evidence: the existing skill was invoked zero times in
  the session that produced nine failures.

## Consulted

Per ADR-20260812-143619, one line per lens. **This block is honest about what it is**: the dispatch
card carried no mob-briefing evidence and stated no reversibility class (a card defect, reported
with the work), so the lines below are **recorded positions read from the register**, not in-session
opinions — writing invented lens quotes into a record about not inventing citations would be the
failure this ADR exists to stop.

- **beck** — recorded position applied: *"a gate never seen to fire is an unverified claim"*
  (`register-check-selftest.sh` header, #292). Honoured: three mutants were planted and each was
  observed red (Lane D disarmed → D1/D4/D5 red; the `Agent` settings entry deleted → W red; the
  resolver stubbed to accept anything → D7 red) before the suite was trusted.
- **farley** — recorded position applied: gates belong in the pipeline, not on an author's machine
  (ADR-20260827-081500, GATE-STEP-LOCUS option (a)). The new cases live in the existing suite, which
  already runs in the always-run `gate-scripts` job; no new CI step and no new skip path.
- **holub** — the shortest slice that removes the defect: one script extended, one settings entry,
  one skill, one ADR. No new tool, no new job, no new gate script.
- **evans** — vocabulary: "dispatch card", "trail", "register check" are the terms already in
  `workflow.md`; Lane D introduces no synonym, and the trail format is cited from its single
  canonical definition rather than re-spelled (the drift shape `ci.yml`'s own comments warn about).
- **young / vernon / dba / graphql-architect / ux-designer / business-specialist /
  observability-agent** — nothing in their lenses: no stored event shape, no aggregate boundary, no
  schema, no API surface, no screen, no unit economics, no runtime telemetry contract is touched.
- **legal-specialist** — nothing in its lens directly (no legal surface changes), but failure 4 is
  its correction, and the skill records BRIEF-20260819 §4.2's carve-outs as a worked example so the
  same self-answer is refused next time.
- **architect** — not consulted in-session; this chunk was dispatched to an executor with the scope
  fully specified, and it re-scopes nothing.

## Enforced by

- `.claude/hooks/register-check.sh` — Lane D, dispatched on `tool_name == Agent`.
- `.claude/settings.json` — the `PreToolUse` / `Agent` entry (the ask entry stays at index 0).
- `.claude/hooks/register-check-selftest.sh` — cases **D1–D12** (fixture corpus), **LD1–LD3** (live
  roster and live `docs/` resolution), **W4–W7** (the `Agent` entry's disarming mutants: fuzzed
  matcher, re-pointed command, deleted entry, moved event). Runs from `stop-gate.sh` every turn and
  in CI's always-run `gate-scripts` job.
- `.claude/skills/coordinator-register-check/SKILL.md` — the procedure for the half no hook reaches.
