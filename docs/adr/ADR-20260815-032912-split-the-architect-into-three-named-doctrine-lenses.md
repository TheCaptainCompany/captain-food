# ADR-20260815-032912 — Split the architect into three named doctrine lenses: `young`, `vernon`, `evans`

**Status**: Accepted (direct founder directive, no option space) · **Date**: 2026-08-15 ·
**Decider**: the founder / Tech CEO, verbatim below ·
**Amends** (does not supersede)
[ADR-20260808-154005](ADR-20260808-154005-agents-channel-named-experts-published-work.md) —
advisory agents channel named experts' published bodies of work ·
**Session**: https://claude.ai/code/session_018WtW3eyd4yWFKHTUEQYJkM

## The directive (verbatim, founder / Tech CEO, 2026-08-15)

> "Split the architect into Greg Vaughn and eric"

Earlier in the same exchange, the reason:

> "For the architect I would prefer to discuss with Greg Young Vaughn Vernon than a generic
> architect"

## Context — what earned it

`.claude/agents/architect.md` already declared that it channels Greg Young, Vaughn Vernon and Eric
Evans (ADR-20260808-154005). In practice its output read as **generic architecture opinion** until
the coordinator explicitly re-briefed it to argue as one of the three; the channelling sat at the
bottom of the file as a footnote under a heading called "Channels", competing with an audit
procedure, an issue-filing checklist, a proposal template and a dispatch protocol. A lens that has
to be *reminded* which school it argues from is not a lens — the position gets averaged into the
model's default architecture voice, which is precisely the monoculture failure ADR-20260808-154005
was written to attack.

The same day made the cost concrete. Three live questions each turn entirely on **one thinker's**
doctrine, and each was argued as undifferentiated "architecture":

- whether a process manager may read a read model
  ([ADR-20260815-030206](ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md))
  — a **Young** argument: a read model's whole licence is that it is disposable and rebuildable, so
  a write-side dependency on one makes a rebuild a business event;
- whether an actor may be queried synchronously on the checkout path (register row **PMW-3**) — a
  **Vernon** argument, in the vocabulary of *Reactive Messaging Patterns*: Ask vs Tell, addressed
  replies, timeouts, and the absence of a grain directory;
- why `specs/ordering/processmanager.yaml:30-43` says "read the projection" while the code folds the
  aggregate's stream — an **Evans** argument: the spec, the code and the conversation have two
  vocabularies for one act, which is a modelling defect and not a naming nit.

## Decision

**1. `architect` survives, as the OPERATIONS role.** It is not deleted and not renamed. It audits the
live system, files triaged issues, writes the proposals that carry design decisions, ranks the
prioritised backlog under the binding value method (ADR-20260810-215503), and **names the next
chunk** for dispatch. That last one is load-bearing: the autonomous loop
(ADR-20260810-011500, `docs/claude/autonomous-run.md`), CLAUDE.md's own start-of-session sequence and
the `architecture-review` skill (`.claude/skills/architecture-review/`) all name it. "Who names the
next chunk" must never become ambiguous, so the split does not touch it.

**2. Three new doctrine lenses, one per thinker**, read-only advisory agents in the shape of `beck`
and `holub` (`tools: Read, Grep, Glob, Bash`):

- **`young`** — Greg Young. CQRS proper, including its most-abused claim: **CQRS is not eventual
  consistency** (lag is a deployment choice, not a property of the pattern), and "CQRS is not a
  top-level architecture". Read models as **disposable, rebuildable folds** whose entire licence is
  that a rebuild changes nothing on the write side. Commands derive from use cases and can be
  rejected. **Event versioning and upcasting** — stored events are immutable historical facts,
  upcast on read, never mutate — **and the boundary of that doctrine**: it governs stored events,
  NOT live query replies, which need additive-only change plus a tolerant reader.
  **Snapshots** — disposable, rebuildable, never authoritative (live right now: register row
  **SNAP-1**, the founder's every-100-events catalog snapshot policy). Set-based validation against
  an event-sourced write side (`verify_phone` / `slug_taken`). And his caution against synchronously
  interrogating the write side under load — the reason read models exist at all.
- **`vernon`** — Vaughn Vernon. The aggregate design rules from *Implementing Domain-Driven Design*:
  small aggregates, **reference other aggregates by identity** and resolve at the point of need,
  **one aggregate per transaction**. Process managers / sagas — what a PM may legitimately depend on,
  and that its durable state is **its own process-state table**, never another consumer's query
  model. The **actor model as a consistency discipline**, and specifically *Reactive Messaging
  Patterns with the Actor Model* as the pattern language for Request-Reply, **Ask vs Tell**, and when
  a synchronous ask is legitimate versus a design smell (live: **PMW-3**). *DDD Distilled* on
  eventual consistency BETWEEN aggregates and strong consistency INSIDE one. Bounded contexts as
  deployment and consistency boundaries.
- **`evans`** — Eric Evans. **Ubiquitous language** — one vocabulary across spec, code and
  conversation, drift being a modelling defect. **Strategic design** — bounded contexts (he
  originated them), **context maps** and their relationship patterns (Shared Kernel,
  Customer/Supplier, **Conformist**, **Anticorruption Layer**, **Published Language**, Separate
  Ways, Open Host Service). **Distillation of the core domain** — core versus supporting versus
  generic, and refusing to spend the team's best effort on the generic. Model integrity and
  knowledge crunching. The ACL discipline this repo already applies to HubRise/Stripe
  (`specs/integrations/hubrise.md`) is his pattern. His sharpest live contribution here:
  **a shared naming convention is the worst kind of context-map edge** — this repo's PM↔aggregate
  edge is a `format!("{CATEGORY}-{}", id.0)` on a stream name
  (`crates/domain/src/payment.rs:26-28`) against a lane keyed on `"{actor_type}:{key}"`
  (`crates/actor_client/src/enqueue.rs:478`): invisible to the loader, unversionable, a Conformist
  relationship where a Published Language is wanted.

**3. All three advise and are consulted; none of them acts.** They never edit `specs/**`, never claim
or implement an issue, never set priorities (ADR-20260808-144738) and **never rank the backlog** —
that stays `architect`. They ground every claim in this tree with `file:line`, or say plainly that
they are arguing from doctrine with no local evidence.

**4. Disagreement is output, not noise.** Where two lenses reason from different premises they say
so rather than blending; the coordinator has already found that Young-versus-Vernon on the hot-path
ask question is the most useful thing either of them produces. `architect` must report that
divergence as divergence and **cite which lens carried a finding**.

**5. `architect` loses the doctrine, keeps its own experience.** The "Channels" section is replaced by
a routing table into the three lenses. The **microservice and actor-model failure-mode** material
(distributed monoliths, split theatre, env-var boundaries, shared databases behind "independent"
services) stays: it is that agent's own experience and belongs to none of the three thinkers.

## Relationship to ADR-20260808-154005 — amendment, not supersession

That ADR's decision is untouched: advisory agents channel named experts' published, checkable
positions; channelling means published positions applied, never invented; the names are an internal
advisory device and **never external attribution**; and the honest limit (this does not cure
monoculture — it is still one model) still stands. Only the `architect` roster row changes, and only
in that its three anchors are now three agents. **Every other channelled lens is untouched** —
Kleppmann on `dba`, Byron on `graphql-architect`, Majors on `observability-agent`,
Norman/Patton/Ive on `ux-designer`, Meyer/Scholz on `business-specialist`, Beck on `reviewer` and
`beck`, Holub on `holub`, Farley on `farley`.

It also refines the naming rule of ADR-20260809-021500 by demonstrating its inverse: when a
multi-anchor lens's output reads as generic, the answer is to **split it into one agent per thinker**
— never to rename it after the loudest anchor, which is the demotion that rule already forbids.

## Consequences

- A doctrinal finding now arrives attributed and checkable ("young: this makes a rebuild
  non-neutral") instead of as "the architect thinks", and a reader can verify the persona against
  the source.
- The mob briefing gains three lenses. Silence must stay cheap — *"nothing in my lens"* is a complete
  one-line answer (ADR-20260809-013142) — and on a typical dispatch at most one of the three has
  anything to say.
- `architect` gets shorter and more clearly one job. The risk it now carries is the opposite of the
  old one: **not invoking** the lens and re-deriving a position from memory. Its charter says so
  explicitly.
- No `specs/**` change, no code change, no gate movement: this is agent configuration and docs.

## Consulted

This is a **direct founder directive on team composition**, with no option space: the founder named
the split and the three people. Under the proportionality rule (CLAUDE.md, founder directive
2026-07-31) that means **no proposal** — there are no alternatives to arbitrate — and this ADR plus
the three agent files are the whole record. The mob was not fanned out for the same reason: there is
nothing for a lens to weigh in on when the decision is *which lenses exist*, and the one judgement
call the executor did make (keep `architect` as the operations role rather than dissolve it) is
forced by the loop's dependency on it, recorded in Decision 1 above.

## Refs

`.claude/agents/young.md` · `.claude/agents/vernon.md` · `.claude/agents/evans.md` ·
`.claude/agents/architect.md` · `docs/claude/autonomous-run.md` (roster + naming rule) ·
[ADR-20260808-154005](ADR-20260808-154005-agents-channel-named-experts-published-work.md) ·
[ADR-20260809-021500](ADR-20260809-021500-beck-is-the-testing-lens-and-the-agent-naming-rule.md) ·
[ADR-20260809-013142](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md) ·
[ADR-20260815-030206](ADR-20260815-030206-a-process-manager-is-a-write-side-component-and-never-reads-the-read-side.md) ·
`docs/proposals/DECISIONS.md` §42 (**PMW-1**, **PMW-2**, **PMW-3**)
