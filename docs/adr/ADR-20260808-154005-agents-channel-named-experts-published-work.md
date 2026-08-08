# ADR-20260808-154005 — Advisory agents channel named experts' published bodies of work

## Status

Accepted (customer decision, 2026-08-08, session
https://claude.ai/code/session_01AKgDqRbCcCxtUePWPRfxtp)

## Context

The agent bench's review ensemble is one model wearing different prompts — "eyes that did not
write it" share priors, so consensus can be correlatedly wrong in ways no lens notices
(the monoculture blind spot, named the day this was decided). Generic personas ("30 years of
experience") sharpen a lens's DOMAIN but not its POSITIONS: two generic experts drift toward the
model's average opinion. The founder's direction: anchor each advisory persona in a NAMED expert
whose published body of work the model genuinely knows — so each lens argues from documented,
checkable-against-source positions instead of averaged instinct.

## Decision

1. **Each advisory agent channels the published work of named experts.** The roster (extendable;
   the agent file is the source of truth):
   - `architect` — Greg Young (CQRS/ES: event versioning, "CQRS is not a top-level architecture"),
     Vaughn Vernon (Implementing DDD, actor model), Eric Evans (strategic DDD, bounded contexts,
     ubiquitous language).
   - `ux-designer` — Don Norman (user-centered design, affordances, error-as-design-failure) and
     Jeff Patton (User Story Mapping — `specs/stories.yaml` IS a story map; journeys slice
     outcomes, not features).
   - `dba` — Martin Kleppmann (Designing Data-Intensive Applications: logs as source of truth,
     derived data, exactly-once semantics).
   - `graphql-architect` — Lee Byron (GraphQL's design rationale: schema-first, additive
     evolution, the reasons the spec says no).
   - `observability-agent` — Charity Majors (observability vs monitoring, high-cardinality
     events, "test in prod honestly").
   - `business-specialist` — Danny Meyer (Setting the Table: enlightened hospitality, the
     restaurant-side P&L and dignity economics); the platform-side lens stays experience-based
     (no single canonical public figure).
   - `reviewer` — Kent Beck (test-desiderata, small safe steps, "make the change easy").
   - Allen Holub anchors the OPERATING MODEL, not an agent: team ownership, no proxy roles —
     ADR-20260808-144738 is his lens made executable here.
2. **Channeling means published positions, applied.** A persona argues what the expert's books,
   talks and writing actually say, cited by work when load-bearing ("Kleppmann, DDIA ch. 11:
   …"), and applies it to this codebase. It never invents new opinions for the person.
3. **Internal advisory device only — never external attribution.** These names appear in
   `.claude/agents/**` and internal reports. Nothing public-facing (README, marketing, issues
   meant for outside eyes, generated docs) may claim these people advise, endorse, or are
   affiliated with Captain.Food. A persona is "a lens channeling X's published work", never X.
4. **The honest limit is recorded, not hidden**: this does NOT cure monoculture — it is still one
   model role-playing distinct priors. It buys real decorrelation (documented positions diverge
   where averaged instinct converges) but the non-model gates (compiler, validator, tests,
   production evidence per ADR-20260808-144738) remain the true independent checks. Running
   review lenses on different model families remains the stronger fix, open for a future
   decision.

## Alternatives considered

- **Keep generic personas** — rejected: domain-shaped but position-less; drifts to model-average
  opinions, which is the monoculture.
- **Different model families per lens** — the stronger decorrelation; not rejected, deferred as
  an infrastructure decision. This ADR is compatible with it.
- **Fictional named experts with invented doctrines** — rejected: unfalsifiable; a real expert's
  published work lets a reviewer check the persona against the source.

## Consequences

- Agent charters gain "channels" sections with the named positions most relevant to this repo;
  disagreements between lenses become traceable to real, documented schools of thought.
- A review claim "X would object" is checkable against X's actual writing — personas become
  auditable.
- Guardrail cost: contributors must keep the external-attribution rule; a validator/grep sweep
  of public-facing artifacts for the roster names is cheap insurance if drift is ever observed.

## Refs

ADR-20260808-144738 (no PM agent; evidence displaces proxy judgment) · `.claude/agents/` ·
`specs/stories.yaml` (Patton's artifact, literally)
