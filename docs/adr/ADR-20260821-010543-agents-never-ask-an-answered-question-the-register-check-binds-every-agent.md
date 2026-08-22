# ADR-20260821-010543 — Agents never ask an answered question: the register check binds every agent, and the ask surface is gated

## Status

Accepted

## Enforced by

n/a — no behavioral guarantee in `rules.yaml` (this is operating-model surface, not domain
behaviour). The executable enforcement is `.claude/hooks/register-check.sh` (PreToolUse gate on
`AskUserQuestion`, wired in `.claude/settings.json`) and `.claude/hooks/register-check-selftest.sh`
(hook verdicts on fixtures, the settings wiring, the agent files' citation blocks, and the canonical
section's existence), run by `.claude/hooks/stop-gate.sh` on every turn and directly via
`make hooks-test`.

## Context

Founder directive, 2026-08-21, verbatim: *"I want to ensure that the agents will no longer ask
questions already answered. Use the best practices known for that."*

The failure is banked and diagnosed: a settled question was re-asked on 2026-08-18
(ADR-20260818-210000 coordinator defect 2 — the record was 891 words and one grep away), the
founder asked why on 2026-08-19, and
[PROP-20260819-110442](../proposals/PROP-20260819-110442-the-decision-register-is-the-unit-of-decision.md)
([DECISIONS §48](../proposals/DECISIONS.md)) answered: no index, taxonomy or ADR-count reduction
would have prevented it — **only making the lookup a precondition of asking** (REG-1 option (a),
recommended but not decided). Earlier the same day, commit `7c6f0bf` sharpened the session-level
rule in [docs/claude/sessions/workflow.md](../claude/sessions/workflow.md) — *"check the register
before you ask — and before you assert"* — but it bound sessions in prose only, bound no agent
prompt, and had no executable gate. Per ADR-20260818-210000: a rule that lives only in prose is a
convention.

## Decision

Enforcement goes **on the ask**, landed now in the form buildable before the register rows carry
machine identity. This decides the **direction** of DECISIONS §48 **REG-1 = (a) in principle**; the
full mechanical form (a decision-queue question `$ref`s a register row whose status is `open`)
remains with PROP-20260819-110442 and stays gated on REG-2/REG-4, which this ADR does **not**
decide. Final-vision-first is satisfied, not bypassed: the final step is already designed and
recorded in that proposal; the staging is externally forced by its undecided register machinery.

Five pieces, all in this change:

1. **One canonical protocol and trail format**, declared once in
   [docs/claude/sessions/workflow.md](../claude/sessions/workflow.md) ("The trail rides the
   question"): `Register check: <record id> (<date>, <status>) -- covers <X>, silent on <Y>`, or
   the explicit negative `Register check: no controlling record -- terms: …; nearest: …`. A found
   controlling answer terminates in the work record as its citation and never reaches the founder;
   the negative is a PASSING trail — a genuinely new question is asked, with it, never dropped.
   Date and status ride the citation because a counsel-gated or still-open record legitimately
   re-opens a question, and "the facts changed" is a legitimate re-ask naming its record.
2. **Every agent is bound**: all 16 `.claude/agents/*.md` carry a thin citation block — a pointer
   to the canonical rule plus the trail format, zero restated protocol (the blocks are disposable
   read models of workflow.md; paraphrase is drift). The executor's block exempts
   protocol-mandated mechanical hand-backs and explicitly includes AMBER hand-backs.
3. **The tool ask-surface is mechanically gated**: a PreToolUse hook on `AskUserQuestion` blocks a
   question whose payload lacks a well-shaped trail (a record id or the explicit negative — a bare
   `Register check: done` is rejected) and feeds the protocol back. Fail-closed: exit 2 on missing
   trail AND on unreadable input, never exit 1 (any other nonzero exit would allow with a warning —
   the ADR-20260810-231300 silent-fallback defect class). Dependency-free (bash + grep, no jq).
   Each firing appends one greppable line to `.claude/register-check.log` (gitignored), so hollow
   trails are spot-checkable and the firing rate is countable.
4. **The gate is proven, not trusted**: the selftest was run RED first (wiring and drift cases
   failing before the wiring existed), asserts block/allow/fail-closed verdicts on fixture
   payloads, the `settings.json` wiring (a dropped entry turns the selftest red instead of
   silently disarming the gate), every agent file's block, and the canonical section's existence.
   It runs on every turn via the stop gate and directly via `make hooks-test`.
5. **The alias table** in workflow.md (`contribution`←`tip`, `delivery`←`rider`,
   `founder`←`product owner`/`customer`, `register`←`decision queue`…) is the Published Language
   for the search; every rename appends its pair, and every question later found answered appends
   the term that would have found it — the miss log that tunes the search no hook can verify.

**Honest scope — believe no more than this**: the hook proves the trail's *presence and shape* on
the `AskUserQuestion` transport only. It does not prove a search happened, and it cannot see
questions travelling as prose (run reports, decision-queue sections, PR/issue comments, register
rows, decision forms) — those are bound by the agent blocks and by this rule at the session level.
Honesty is enforced by the mob briefing and the independent review, aided by the firing log. This
is a deliberate compiler-first placement (ADR-20260803-234035): no type system reaches an agent's
free-text question, so a shape-checking gate is the legitimate fallback — do not "fix" it into
something heavier, and do not read its green as more than it proves.

## Alternatives considered

- **(b) Archive-side enforcement** (frontmatter, topic taxonomy, fewer/larger ADRs) — rejected on
  PROP-20260819-110442's own evidence: none of the incidents would have been prevented; the
  re-litigated record was already findable in one command.
- **(c) Prose only** (the workflow.md rule alone) — foreclosed by ADR-20260818-210000; it is the
  state that existed the morning of this directive.
- **(d) The full REG-1(a) row-`$ref` mechanism now** — requires machine-readable row status
  (REG-2/REG-4, founder-undecided) and REG-SEQ holds it does not displace
  [#556 "Local acceptance harness"](https://github.com/TheCaptainCompany/captain-food/issues/556).
  Landing it by the side door would decide founder-owned rows silently.

## Consequences

### Positive
- Re-asking a settled question now fails mechanically on the tool path and visibly (missing trail)
  on every prose path; the founder sees, on every surviving question, what was searched and what
  nearly answered it.
- The flow is countable: trail lines and the firing log give the avoided-vs-asked measure without
  new machinery.

### Negative
- One trail line of friction on every founder-facing question, including trivially interactive
  ones — accepted; the escape (the explicit negative) is one line.
- A string-shape gate can be satisfied by a fabricated citation; the log and review make that
  auditable, not impossible.

### Follow-up actions
- DECISIONS §48 REG-2, REG-3, REG-4 remain **open and founder-owned**; when they land,
  PROP-20260819-110442's validator form supersedes the shape check on the tool path.
- Spot-check `.claude/register-check.log` in reviews; append alias pairs and misses as they occur.

## Consulted

Per ADR-20260812-143619 the whole roster was briefed in parallel before any artifact landed; every
line below changed the design or confirmed it.

- **architect**: string gates invite compliance theater — require the trail to name a verifiable
  artifact; the hook covers one transport, the agent blocks are the real gate for prose; keep the
  no-record escape open; blocks must be pointers with an executable drift fence. (All four landed.)
- **beck**: the failing test comes first — fixture payloads shown red/green against the real
  script, wired like `loop-budget-selftest.sh`; the hook enforces the ritual's presence, the blocks
  its honesty — say so. (Landed; the selftest ran red before the wiring existed.)
- **business-specialist**: count avoided-vs-asked; the refusal must say "ask anyway with the
  trail"; citations carry dates — a facts-changed re-ask lane exists; a cited answer terminates in
  the work record, not the queue. (All landed.)
- **dba**: verify a token and you have a lease without a fencing token — require the record-id
  regex or an explicit negative; supersession-to-head explicit; the invisible failure is recall —
  keep a miss log. (Landed as the shape check and the alias/miss table.)
- **evans**: alias handling was the weak joint — a Published Language alias table in workflow.md,
  fed by the existing rename sweeps; the marker literal is declared once and cited, never
  re-spelled; blocks are citations, not paraphrases. (All landed.)
- **executor**: the executor's escalations are prose (PR comments, hand-backs) — bind them in the
  agent block; exempt protocol-mandated mechanical refusals; the trail makes AMBER hand-backs
  actionable. (All landed in the executor block.)
- **farley**: only exit 2 blocks — any other nonzero silently allows; fail closed on unparseable
  input; no jq dependency; executable fixtures under a make target; the refusal text carries the
  protocol so the retry is one turn. (All landed in the hook.)
- **generator**: 16 hand-copies fork on the first sharpening — one-line citing blocks plus a
  cheap executable presence check; the sentinel literal defined in one place; record the
  compiler-first placement so a later session doesn't "fix" the gate into something heavier.
  (All landed; the check is shell in the selftest rather than a codegen-rs test — same family,
  runs every turn via the stop gate.)
- **graphql-architect**: validate the shape, not the token; state coverage honestly (tool path vs
  prose); make "cite, never fork" checkable; pin a stable anchor — the blocks cite the rule's
  quoted heading, which the selftest asserts exists; the refusal names the legitimate empty-citation
  form. (All landed.)
- **holub**: tightens the loop, not ceremony — priced only on founder-facing questions; the
  perverse incentive is the unasked real question, so the refusal says "do the check, then ask";
  one countable flow signal. (All landed.)
- **legal-specialist**: "cited = settled" must not swallow grading — a counsel-gated or re-verify
  record re-opens the question; legal records decay (currency check on legal rows); the
  external-clock carve-out is never delayed (the negative trail covers it); a found legal answer is
  a map, never clearance. (All landed in the format rules.)
- **observability-agent**: a gate that fires only on bad arrivals is silent-when-broken — an
  executable wiring check and one structured log line per firing, high-cardinality enough to
  decompose; the refusal quotes the exact expected shape. (All landed.)
- **reviewer**: negative verification before trust — the hook was demonstrated blocking and
  passing before landing; the hook's green must not be over-read; grep for paraphrasing blocks;
  ADR and journal land in the SAME change. (All landed.)
- **ux-designer**: the trail is a one-line signifier, not a dumped search log — "nearest record,
  covers X, silent on Y" so the founder triages in one glance; the near-answer citation is the
  valuable one; prose surfaces need the same format. (All landed in the canonical format.)
- **vernon**: an Ask-pattern fix — the register is a read model and the check converts illegitimate
  Asks to the slowest actor into reads; the prose channel is the mailbox-bypass equivalent, so name
  it; resolve at the point of need — re-read the record when it licenses the action. (All landed.)
- **young**: blocks are read models and stay disposable — zero protocol content; the hook is
  accept-and-compensate, not a reservation — say so honestly; a found answer is a snapshot needing
  invalidation semantics (supersession followed at ask-time); mandate recording the negative.
  (All landed.)
