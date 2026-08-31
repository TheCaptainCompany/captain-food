---
name: decision
description: >
  The founder is RECORDING a decision he has made -- the command names the artifact, not an
  instruction to the coordinator to decide anything. Invoked ONLY by the founder as `/decision` --
  never selected by the model. Step one is always the REVERSAL CHECK: does this contradict or amend
  a recorded decision? Then the register row, the ADR with its `Consulted:` block, the CLAUDE.md
  three questions in order, and SPEC-LOG if the spec surface moved.
disable-model-invocation: true
---

# `/decision` — he has decided; you are recording it

**Read the name.** The founder renamed this from `/decide` on his own stated ground: *decide* reads
as an instruction to the coordinator to go and decide something, and **he is not asking you to
decide**. `/decision` names **the artifact he is recording**. Every word of the handling follows
from that. You are a scribe with a checklist, not a decision-maker with a mandate.

Corollary: the answer to `/decision` is never *"are you sure?"* or a re-argument of the option he
rejected. If a lens objection is genuinely new information, say it once, plainly, and record what he
then says. The decision is his.

## Step one, always: the reversal check

> **Does this contradict or amend a decision already on the record?**

Run it **before** writing anything. This is the failure the command exists to stop, and it has a
worked example with a name.

**The worked example — 2026-08-31, Call 1.** A founder call moved the price-freeze locus from
*commitment* to *quote time*. That is materially **Alternative A**, which
[ADR-20260810-112836](../../../docs/adr/ADR-20260810-112836-cart-priced-live-on-read.md) had
**considered and rejected** three weeks earlier. Nobody flagged it in the moment. The reversal was
real, it was recorded as a plain forward decision, and the contradicted ADR sat unamended until a
later pass caught it and wrote the `§2 SUPERSEDED IN PART` banner it now carries — pointing at
[ADR-20260831-121957](../../../docs/adr/ADR-20260831-121957-the-pm-read-step-is-retired-source-fixed-the-physics-and-left-the-ownership.md)
§4d and register row `QUOTE-TOKEN`.

What that cost: a window in which two records contradicted each other with nothing linking them, so
any lens or session reading the older one would have reasoned from a superseded rule and had no way
to know. **A reversal recorded as a reversal is cheap. A reversal recorded as a fresh decision is a
landmine with a date on it.**

So: search the register with the decision's **own vocabulary plus the repo's aliases** for the
subject, read the surrounding record rather than the matching line, and check for a later word — an
`Amendment`/`Superseded` banner, a strike, a `reconsiders:` row. If it reverses or amends something:

- **say so to the founder in the same turn** — he is entitled to know he is reversing himself, and
  on the evidence he wants to know;
- the new record carries the relationship **explicitly** (what it supersedes, in whole or in part);
- **the superseded record is amended in place** with a banner pointing forward. A reader who lands
  on the old record must not be able to miss it.

## Then the CLAUDE.md three questions, in order

1. **Does it contradict or create a recorded decision?** → a decision reversal: a register row,
   whatever the diff size. (This is step one above; it is question 1 for a reason.)
2. **Is the shape already emitted, stored or promised?** — `domain_events`, a shipped client, an
   alert route, a partner contract, a legal artifact → a **migration**: the versioning story is
   recorded **before** it lands, stored events are immutable, and upcasting is never mutation.
3. **Otherwise it is the team's**, `specs/common/` included.

## The artifacts

- **A register row** — `docs/decisions/<KEY>.yaml` is authoritative for **current status**; the
  prose row in [`docs/proposals/DECISIONS.md`](../../../docs/proposals/DECISIONS.md) is its history.
  A `reconsiders:` must point at the **chain head**, never a superseded row — the validator rejects
  the latter.
- **An ADR** in `docs/adr/`, id `ADR-YYYYMMDD-HHMMSS` from `date -u`. Founder wording stays
  **verbatim**; historical records keep their own vocabulary.
- **A `Consulted:` block on any record created from a founder directive — one line per lens.**
  Not optional and not summarised: *a lens never asked is indistinguishable from a lens with nothing
  to say*, so the block is the only thing that tells them apart later.
- **[`docs/SPEC-LOG.md`](../../../docs/SPEC-LOG.md)** — one sentence, in the **same commit**, if the
  spec surface moved: what the product now promises differently. No `specs/**` change ⇒ no sentence.
- **`docs/STATUS.md`** only if durable state changed; the dated entry goes at the **TOP** of the
  current `docs/status/journal-YYYY-Www.md` (`date +%G-W%V`), newest first, never appended.

**Proportionality** decides how much of that applies: a real option space → proposal + tracking
issue; a decision with no alternatives → an ADR; a small subject with no real decision → **neither**,
a commit message and a one-paragraph PR body.

## Under LIVING-proposal doctrine, a superseded proposal is REWRITTEN

[ADR-20260801-020000](../../../docs/adr/ADR-20260801-020000-proposals-are-living-documents.md):
proposals hold the **clean current design**. A refinement **rewrites the file in place**, in the
same change as its ADR — **never** an appended "superseded" block, never a changelog tail. History
lives in git. A proposal that accretes strata stops being readable as a design, which is the only
job it has.

## Limits

- **Not a mob consult.** If he wants the roster's view *before* deciding, that is `/mob-question`.
  Once he has decided, lenses are consulted for **completeness of the record** — what the decision
  touches, what it breaks — never to relitigate it.
- **GitHub is never the record.** Anything drafted in an issue or PR body lands in the repo in the
  same change; issue bodies carry links and a checklist at most.
- **No citation, no assertion.** If you tell him *"this is already decided"*, cite it — id, date,
  status. That assertion is bound by exactly the same rule as a question.
- **A `Proposed` proposal is an argument, not a decision**, and a legal-lens brief is never advice
  or clearance. Neither can be the thing `/decision` records as settled.
- **External artifacts** — mentions légales, partner onboarding, filings — must name the capacity
  the statutes actually confer.
