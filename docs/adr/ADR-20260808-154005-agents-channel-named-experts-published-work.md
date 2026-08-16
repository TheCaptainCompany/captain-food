# ADR-20260808-154005 — Advisory agents channel named experts' published bodies of work

## Status

Accepted (customer decision, 2026-08-08, session
https://claude.ai/code/session_01AKgDqRbCcCxtUePWPRfxtp)

**AMENDED 2026-08-15 by
[ADR-20260815-032912](ADR-20260815-032912-split-the-architect-into-three-named-doctrine-lenses.md)**
— the `architect` roster row below is superseded in ONE respect: Greg Young, Vaughn Vernon and
Eric Evans are now three separate agents (`young`, `vernon`, `evans`) and `architect` keeps the
operations half (audit, issue filing, proposals, backlog ranking, naming the next chunk) while
consulting and citing them. Every other roster row, and the whole of the decision below, stands
unchanged. This section is the historical record and keeps its vocabulary.

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
   - `ux-designer` — Don Norman (user-centered design, affordances, error-as-design-failure),
     Jeff Patton (User Story Mapping — `specs/stories.yaml` IS a story map; journeys slice
     outcomes, not features), and **Jony Ive** (customer-added 2026-08-09): the craft lens —
     simplicity as subtraction until what remains is inevitable, care in the details nobody is
     asked to notice, materials honesty (a thing is what it appears to be), and "design is how it
     works". The three are kept distinct: Norman asks *can they use it?*, Patton *does the journey
     deliver an outcome?*, Ive *is this made with enough care to be trusted?* — which is the
     decisive question for the demo, a credibility artifact aimed at restaurants
     (ADR-20260808-212741 §2). Ive is invoked for behaviour, sequence, restraint and honesty —
     never for styling; a finding that cannot be written as "the user experiences X instead of Y"
     is not an Ive finding.
   - `dba` — Martin Kleppmann (Designing Data-Intensive Applications: logs as source of truth,
     derived data, exactly-once semantics).
   - `graphql-architect` — Lee Byron (GraphQL's design rationale: schema-first, additive
     evolution, the reasons the spec says no).
   - `observability-agent` — Charity Majors (observability vs monitoring, high-cardinality
     events, "test in prod honestly").
   - `business-specialist` — Danny Meyer (Setting the Table: enlightened hospitality, the
     restaurant-side P&L and dignity economics); Trebor Scholz (Platform Cooperativism / Ours to
     Hack and to Own — the mission's own movement; customer-added 2026-08-08); the platform-side
     lens stays experience-based (no single canonical public figure).
   - `reviewer` — Kent Beck (test-desiderata, small safe steps, "make the change easy").
   - `holub` — Allen Holub, the focus coach (customer-added 2026-08-08): Holub still anchors the
     OPERATING MODEL (ADR-20260808-144738 is his lens made executable), and now also speaks as
     an advisory agent — advises on focus and flow, never a PM proxy.
   - `farley` — Dave Farley (Continuous Delivery, Modern Software Engineering; customer-added
     2026-08-08): the production-path coach — releasability, pipeline-as-proof, happy paths in
     production.
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
