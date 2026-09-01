# ADR-20260831-204546 — The founder elects user-invoked commands, and `/direct-question` is a fourth carve-out to the mob rule

- **Status**: Accepted (founder directive, 2026-08-31)
- **Amends**: [ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)
  — adds a **fourth** carve-out to a list that had three, and is the first one the **founder elects
  per message** rather than a lens asking for it by class
- **Relates**: [ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
  (the decision queue is the only thing brought to him) ·
  [ADR-20260816-134352](ADR-20260816-134352-the-checkpoint-goes-to-declared-concerns-and-review-is-priced-by-reversibility.md)
  (reversibility prices the roster) ·
  [ADR-20260831-141500](ADR-20260831-141500-the-coordinator-gets-the-register-check-gate-on-its-committing-surface.md)
  (the coordinator's register-check discipline)
- **Register row**: `CMD-INVOKE` (`docs/decisions/CMD-INVOKE.yaml`, `docs/proposals/DECISIONS.md §49`)
- **Realized by**: [#819 "Six founder-invoked slash commands"](https://github.com/TheCaptainCompany/captain-food/issues/819)
  / [#820](https://github.com/TheCaptainCompany/captain-food/pull/820)

## Directives

Two, verbatim, 2026-08-31:

1. *"So to avoid any risk I will choose user invoked approach for now with these 3 types:
   /direct-question: ask direct question(s) to you don't need the mob · /mob-question: ask the
   question(s) to the mob · /work: tell you to launch the work on something. Do you see other types
   of invocation?"*
2. *"/decision instead of decide, decide could mean that you have to decide and I here I want you
   record decisions. Ok for /status and /correct"*

Provenance note: these reached this record through the coordinator's dispatch relay. Issue #819's
body **paraphrases** them and quotes only *"to avoid any risk"*; this ADR is the first place either
message exists verbatim in the repo.

## Decision 1 — the six commands, and the naming ground

The approved set is **six**: `/direct-question`, `/mob-question`, `/work`, `/decision`, `/whatsup`,
`/correct`. He named the first three; the last three were proposed and he approved them as
`/decision`, `/status` and `/correct` — and that fifth command was renamed **twice**, each time by
him: `/status` → `/where` later the same day, on the collision described below, then `/where` →
**`/whatsup`** on 2026-09-01 on preference (Decision 4's postscript). Two deliberate namings, not
one confused one.

`/decide` became **`/decision`** on his stated ground: *decide* reads as an instruction to the
coordinator to go and decide something, while *decision* names **the artifact he is recording**.
That distinction is binding on the copy of every one of the six — they describe what **he** is
doing, never what the coordinator should do.

Each is a skill at `.claude/skills/<name>/SKILL.md` carrying `disable-model-invocation: true`, so it
is reachable by the founder typing it and unreachable by model selection. That key is real and
enforced on the running runtime; see the note below, because *which* runtime is not obvious here.

## Decision 2 — `/direct-question` is a fourth carve-out to ADR-20260812-143619, and it is a different KIND

This is the substance of this record, and it is why a skill-files PR needed an ADR at all.

ADR-20260812-143619 rules that **every founder message goes to the whole roster before any answer**,
with three carve-outs, each attributed to the lens that asked for it: the **external-clock relay**
(business), the **already-recorded rollback/abort path** (release), and the **legal-clearance limit**
(legal). `/direct-question` is a **fourth**, and it does not resemble the other three:

- the existing three are **class-based** — a *kind* of message that may be answered in-turn, decided
  once, by a lens, on the merits of the class;
- `/direct-question` is **founder-elected, per message**. He decides, message by message, not to
  spend the fan-out. No class predicts it.

That is a real amendment to the rule, not an application of it, so it is recorded rather than
absorbed. **The rule it does not touch**: the fan-out is skipped, the **register check is not**. The
coordinator's catalogued failures were overwhelmingly answer-shaped, and the `PreToolUse` hook gates
`AskUserQuestion` and the `Agent` tool — never a prose answer. So a direct answer is the surface
where the check is *least* enforced, and dropping the mob removes the other reader who might have
caught the error. Both cannot go.

**The tag is a routing convenience, never a bypass.** Three conditions override it, and they are
written into the skill rather than only here: a controlling record the question appears to
contradict; a subject on the `HOLD: human` axis (money movement, stored event shapes, legal
surfaces, anything Tours-facing); or an honest *"I do not know and one lens would"*. In each, the
coordinator says which fired and fans out anyway. Saying *"this one needs the mob"* is a valid
response to `/direct-question`.

`/mob-question` is the same rule pointed the other way, and inherits
ADR-20260816-134352's pricing: the reversibility class sizes the roster, full mob for the
`HOLD: human` axis. Divergences between lenses are **reported as divergences, never averaged** —
averaging destroys the information the fan-out was paid for.

## Decision 3 — what this record does NOT decide

- **It does not weaken the register check anywhere.** `/direct-question` skips the mob only.
- **It does not create a route around ADR-20260810-011500.** These are him asking; none of the six
  is the coordinator asking *"shall I proceed?"*.
- **It does not create a route around ADR-20260810-011500 in the other direction either**: the
  `/status` → `/where` rename below was put to the founder as a decision-queue row with options and
  a recommendation, not fixed by the coordinator, precisely because *"Ok for /status"* was his
  verbatim and the name was therefore his.

## Decision 4 — `/status` is renamed `/where`, because a colliding skill shadows a built-in silently

Claude Code ships a **built-in `/status`**: the panel for the session itself — version, model,
account, API connectivity, tool stats. Our skill would have taken that name, and the mechanics make
that worse than it sounds:

- resolution is **first-match-wins over a flat array**, with **skills ahead of built-ins**;
- skill dedup is **by file path, never by name**;
- so there is **no collision detection and no warning**. The built-in does not error, it simply
  **disappears** inside this repo — and the guarantee that ours wins at all rests on **array order
  inside a vendor bundle that can reorder on any upgrade**. The failure is silent in both
  directions.

Losing the panel you reach for when the session itself looks wrong is a worse trade than picking
another name, and this command's job is *"where are we"*. **Founder decision, 2026-08-31: rename
ours.** `/where` was verified free; `/status` was the **only** collision among the six.

**The reusable rule: check a proposed command name against the built-ins before writing the skill.**
Names seen on this bundle include `status`, `review`, `security-review`, `stats`, `skills`,
`agents`, `todos`. Verified free at the time of writing: `/direct-question`, `/mob-question`,
`/work`, `/decision`, `/correct`, `/where`. Verify against the artifact `readlink -f "$(which
claude)"` resolves to — see the verification note below, because that is not the artifact it looks
like.

**Postscript, 2026-09-01 — `/where` is renamed `/whatsup`.** Founder verbatim: *"Instead of /where
use /whatsup"*. This is a **preference, not a collision**, and nothing above changes: `/status` was and
remains unusable for the reason Decision 4 gives, and that reasoning is the durable part of this
record. `/whatsup` was re-verified free against the running binary before the rename — the literal
string appears **nowhere** in the artifact `readlink -f "$(which claude)"` resolves to, and since a
built-in's name is stored there as a plain string, a name absent from the binary cannot be one. The
name was his to choose (ADR-20260810-011500), so this executed rather than opened an option space.

## Consequences

- ADR-20260812-143619's carve-out list is now **four**, and the fourth is of a different kind. Its
  forward banner and CLAUDE.md's `Carve-outs:` bullet are updated in this change so no reader lands
  on a list of three and takes it as current.
- The founder gains a per-message lever over the fan-out. The cost is that the lever is *his* and
  the discipline is the coordinator's: the escalation duty is the only thing preventing the tag from
  becoming a silent bypass of a rule he himself set.
- **A verbatim founder directive existed for a full working session in paraphrase only.** The
  directive arrived, six skills were built from it, and no record held his words. That is the gap
  this ADR closes late rather than on time.

## How this defect was found, and why it is worth writing down

The branch that introduced this carve-out **also introduced the skill that exists to catch exactly
this** — `.claude/skills/decision/SKILL.md`, whose *"Step one, always: the reversal check"* uses the
[ADR-20260810-112836](ADR-20260810-112836-cart-priced-live-on-read.md) landmine as its worked
example. The PR wrote the warning and reproduced the defect.

The mechanism is diagnosable and it is not carelessness. Issue #819's register-check trail read
*"no controlling record — terms: slash command, user-invoked, disable-model-invocation,
/direct-question, /mob-question"*. Every term is a **mechanism** term. None is a **substance** term
(*mob*, *fan-out*, *founder message*, *carve-out*), and the substance is what the change amends —
while the branch's own deliverable devotes a section to ADR-20260812-143619 **as controlling**. The
check searched for prior art on *how to build a slash command* and correctly found none, which is
not the question the change raised.

**The rule that earns its place**: a register check searches the **substance the change amends**,
not the **mechanism it is built from**. A trail whose terms are all implementation nouns has not yet
run. This is failure class #1/#8 of the table of nine — a controlling record read as absent —
committed by the register check itself, and it is the reason the negative trail is not
self-certifying.

## A verification note that cost real time

`disable-model-invocation` is a **real and enforced** `SKILL.md` frontmatter key. On the **running**
runtime — the native binary `/opt/claude-code/bin/claude`, **2.1.251** — the guard is:

```js
if (e.disableModelInvocation && !userTypedThisTurn)
  return { reason: "disable_model_invocation", errorCode: 4,
           message: "... reserved for explicit user invocation." };
```

`user-invocable` defaults to true when absent, so the six stay founder-invocable while excluded from
the model-visible list.

**The trap, which cost three wrong citations in one session**: this container has **two** installs.
`/opt/node22/lib/node_modules/@anthropic-ai/claude-code` is a JS bundle whose `package.json` and
embedded `VERSION` both read **2.1.42**; `/opt/node22/bin/claude` is a **symlink** to the **native
ELF binary** `/opt/claude-code/bin/claude`, which is what actually runs. They are different
programs, and the JS bundle is not one of them.

**The runtime is also a moving target.** Within this single session the symlink was repointed and
the binary rebuilt under it, `claude --version` going **2.1.251 → 2.1.252**. So no version number
belongs in prose: it is stale before the next reader arrives. **Record the method, not the value** —
`readlink -f "$(which claude)"` for the artifact, `claude --version` for its version, `strings` on
the binary rather than `grep` on a `cli.js` that may not be the running code — and re-derive at the
moment the fact licenses an action. Both installs carry the key and both agree that `/status` is a
built-in, so neither conclusion here turned on the confusion; that is luck, not method.

## Consulted

**No lens was consulted. This block records that fact rather than concealing it**, per this ADR's
parent: *a lens that was never asked is indistinguishable from a lens with nothing to say*, and
inventing lens lines would be the fabricated-antecedent failure (#5/#6 of the table of nine) written
into the one record whose purpose is auditability.

- **Roster** — **NOT ASKED.** The dispatch classed the work *reversible* (prose skill files, no
  runtime behaviour, no stored shape) and set the roster at "this card plus the independent review",
  which is correct pricing for *writing six skill files* and **wrong for amending a founder rule
  about how founder messages are answered**. The class was assessed on the artifact, not on the
  decision the artifact carried.
- **executor** — built the six, and flagged in-run that the card's *"five of nine were
  answer-shaped"* was a derived number the source does not state (it gives only the qualitative
  split); corrected to the claim the source supports. Did **not** catch the carve-out at
  implementation time, and wrote the sentence that names it (*"the founder electing, per message,
  not to spend that fan-out"*) without recognising it as an amendment.
- **reviewer** (independent full-diff pass, round 1) — **caught it**: no `docs/adr/**` and no
  `docs/decisions/**` in the diff, against a change whose own text describes a standing exception to
  a recorded rule. Also diagnosed the mechanism-vs-substance trail defect above.

**Consequence, stated plainly**: this record is accepted on the founder's directive, which is not in
doubt — he chose the approach and named the commands. What has **not** had a mob read is the
*second-order* question of whether a founder-elected, per-message opt-out should sit alongside three
class-based carve-outs, and what stops it becoming a silent bypass beyond coordinator discipline.
If that is wanted, it is a `/mob-question` on this record, and the escalation duty in
`.claude/skills/direct-question/SKILL.md` is the interim answer.
