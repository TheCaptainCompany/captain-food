# ADR-20260810-215503 — Backlog prioritisation is delegated to the team; the method becomes binding

## Status

Accepted (2026-08-10, product-owner directive relayed in-session)

## Context

Until now, one rule sat in three first-read places and said the same thing:

- `CLAUDE.md` — *"Re-prioritising is a **product-owner decision made in the project**, never by an agent."*
- `docs/BACKLOG.md:16-18` — *"Re-prioritisation is a product-owner decision, made in the project …
  Agents never re-prioritise on their own."*
- `.claude/agents/architect.md` (hard boundaries) — *"Never re-prioritise. You read `Priority` and row
  order; you never set them."*

On 2026-08-10 the product owner said, verbatim:

> **"Don't care about the project field anymore the team decides without me"**

This is a delegation, and it contradicts the rule as written. Left unrecorded it produces exactly the
defect the repo has just spent two commits removing: a rule stated in the first-read file and
contradicted in practice, which stops the next reader looking.

It also arrives in a specific context. [ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
already moved *execution* ownership into the team (sessions start unasked; the coordinator never
authors the diff), and [ADR-20260808-144738](ADR-20260808-144738-product-ownership-lives-in-the-team-no-pm-agent.md)
already put product ownership in the team with evidence displacing proxy judgment. This directive
completes that arc on the one surface still held back: **which work is next**.

## Decision

**The GitHub Project "Prioritized backlog" field values and row order are the team's to set.** The
product owner no longer transcribes them and no longer needs to be asked.

### What is delegated

| Surface | Before | Now |
|---|---|---|
| `Type`, `Value Size`, `Impact`, `Effort` | already team-set at triage (`docs/BACKLOG.md` §"Triage of new issues") | unchanged — team |
| **`Priority` (the value bucket)** | product owner | **team** |
| **Row order within a bucket** | product owner | **team** |

Two things actually change hands, not five. The verbatim quote reads singular — *"the project
field"* — but **row order is the one that decides what gets dispatched next**, so it is named here
explicitly rather than left to inference.

### What is NOT delegated

1. **Genuine option spaces.** [`docs/proposals/DECISIONS.md`](../proposals/DECISIONS.md) is untouched.
   A ranking says *when* known work happens; a decision chooses between designs with different
   consequences. The register remains the product owner's surface — and since their attention to the
   board has now been withdrawn, the register's **ordering by leverage becomes more load-bearing, not
   less**.
2. **External, legal and admin-gated matters** — entity and brand naming, counsel questions, money
   posture, consumer-mediator registration, provisioning that needs a console.
3. **`specs/**` approval.** Plan-mode-with-approval is unchanged. Ranking an AMBER item `Urgent` does
   not make it dispatchable.
4. **The method.** The value-first ordering method (`docs/BACKLOG.md` §"How value is defined",
   [ADR-20260720-213024](20260720-213024-value-first-issue-prioritisation.md)) is **not** delegated;
   it is now **binding**. It used to *describe* how the product owner ranked. It is now the
   *constraint* under which the team ranks. This substitution is deliberate: it is what stands in for
   the judgment that left the loop, and without it "the team decides" degrades into "whoever ran the
   architect agent last has a taste."
5. **The override.** The product owner may re-bucket or reorder anything, at any time, without
   justification. The team adopts it immediately and does not ask why. Delegation is revocable per
   item and in general.

### The rule this creates — ranking and dispatching must not be the same act

The architect agent now both **ranks** the backlog and **names the next chunk**. That concentration is
the real cost of this delegation, and it is forbidden to resolve it the convenient way:

> **An agent must never change a Priority bucket or a row position in order to make an item
> dispatchable, or to make its own recommendation legitimate.** If the top item is blocked, the answer
> is "blocked" — never a re-rank.

A re-rank is justified by the value method or by a dependency that was wrong; never by what the ranker
wants to work on next.

### The audit trail moves into the repo

The reason an item sat where it sat used to live in the product owner's head and needed no record.
It now has to be written down or it does not exist:

- Every Priority-bucket change or material row move is **stated in the architect's run report**, with
  the method clause that justifies it.
- A re-ranking that **reverses a previously stated order** gets a line in `docs/STATUS.md`.
- The mob may contest a ranking at briefing time exactly as it contests a design
  ([ADR-20260809-013142](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md)) — any lens
  may say "this is not next" and be heard.

## Alternatives considered

- **Option A — record the delegation as an ADR, make the method binding, forbid self-serving
  re-ranks (chosen).** Keeps a written constraint in place of the human judgment that left, and names
  the one new failure mode. Cost: the architect must now justify rankings in prose it did not
  previously owe.
- **Option B — edit the three rule lines and write nothing else.** Cheapest. Rejected: it deletes a
  constraint without replacing it, and it leaves no record of *what* was delegated when the question
  "who decided this was Urgent?" is asked in a month.
- **Option C — read the quote narrowly, as "stop asking me to transcribe field values", and keep
  Priority with the product owner.** Defensible from the words alone ("the project field"), but
  contradicted by *"the team decides without me"* and by the direction of the two preceding ownership
  ADRs. Rejected as under-reading a deliberate directive.
- **Option D — delegate to a dedicated prioritisation role rather than to the architect.** Would
  separate ranking from dispatching, which is the cleanest answer to the concentration risk. Rejected
  for now: [ADR-20260808-144738](ADR-20260808-144738-product-ownership-lives-in-the-team-no-pm-agent.md)
  explicitly declined to create a PM agent, and adding one to solve a problem the recorded-rationale
  rule already bounds would reverse that decision for a cost we have not yet paid. Revisit if a
  self-serving re-rank is ever observed.

## Consequences

### Positive

- The backlog stops being a bottleneck on the product owner's availability; the loop can start,
  rank and dispatch in one pass.
- Field values on newly filed issues stop accumulating as "proposed values in the body awaiting
  transcription" — a queue that had reached ten issues (#468–#477).
- The value method is promoted from documentation to a constraint that a ranking can be checked
  against, which is a stronger artifact than the rule it replaces.

### Negative

- One agent now ranks and dispatches. The mitigation is prose (recorded rationale) plus a mob veto,
  not a gate — the weakest enforcement level this repo accepts, and it is chosen knowingly because
  the alternative (Option D) reverses a standing decision.
- The product owner loses passive visibility into what the team thinks is important. The compensating
  surface is the architect's run report and `docs/STATUS.md`, both of which they must actually read.
- "Skipping the top item requires a stated reason" becomes weaker when the same party sets the top
  item. The self-serving-re-rank prohibition above is the whole defence.

### Follow-up actions

- [ ] Update the three rule sites to match, citing this ADR: `CLAUDE.md` ("Respect the prioritised
      backlog" bullet), `docs/BACKLOG.md:16-18`, `.claude/agents/architect.md` hard boundaries.
      **These are a diff, not a record — they belong to an executor phase, not to the architect.**
- [ ] Set `Priority` / `Value Size` / `Impact` / `Effort` on
      [#469](https://github.com/TheCaptainCompany/captain-food/issues/469)–[#477](https://github.com/TheCaptainCompany/captain-food/issues/477)
      from the proposed values already in their bodies. **BLOCKED this session** — the Projects v2
      GraphQL API is refused (`"This GraphQL query is not enabled for this session — only the pinned
      set of PR-review operations is served"`) and no `gh` binary is on PATH; Projects v2 has no REST
      surface. Carried as a known gap, not an assumed completion.
- [ ] Amends [ADR-20260720-213024](20260720-213024-value-first-issue-prioritisation.md) on *who*
      ranks; its ordering method is unchanged and now binding.
