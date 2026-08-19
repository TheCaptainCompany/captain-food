# ADR-20260819-201218 — A decision record is an aggregate: answers are immutable versions, and split/merge are first-class

> **AMENDED 2026-08-19 — the clause-7 time source is ruled, and the DESIGN PHASE IS CLOSED.** The gap
> this record flagged in Consequences is closed by the founder: `recorded_at` (immutable,
> repository-controlled ordering) is separated from `effective_at` (optional policy/domain
> applicability), and **lifecycle and supersession validation use `recorded_at`** — see
> [§ Amendment](#amendment-2026-08-19--the-time-source-is-ruled-and-the-design-phase-closes).
> **No REG implementation, schema, YAML records, generated index, migration, librarian enforcement,
> GraphRAG/QMD work or C5-lens dispatch is authorised.** The active priority returns to
> [#556](https://github.com/TheCaptainCompany/captain-food/issues/556).

## Status

Accepted — **design only, and the design phase is now CLOSED.** Closes `REG-REVERSAL`. **Authorises no
implementation**: `REG-WHEN` is **not scheduled** and the
[#643](https://github.com/TheCaptainCompany/captain-food/issues/643) deferral remains in force.

## Enforced by

n/a — no behavioral guarantee yet. The guarantees this model implies are enforced by the four
demonstration fixtures required below, which are **not built**.

## Context

[ADR-20260819-191227](ADR-20260819-191227-the-register-ruling-canonical-records-a-nine-status-vocabulary-and-a-capped-boot-index.md)
approved the register's design and, in amendment C4, gated **all** schema work on one question:
*does a changed answer create a successor record, or does one record retain identity while its answer
changes?* `vernon` was dispatched on it and agreed with `evans` from the opposite direction; the option
space, the three shapes and the two checkable consequences are in
[PROP-20260819-110442 §14](../proposals/PROP-20260819-110442-the-decision-register-is-the-unit-of-decision.md).

The founder ruled on 2026-08-19. This record captures that ruling and **stops**.

## Decision

**Record-as-aggregate is adopted**, in the founder's own terms:

1. **The key identifies the concern.** The immutable, globally unique record key identifies the decision
   **concern/question** — never its positional location, and never a particular answer.
2. **Answers are immutable events/versions** belonging to that record.
3. **`decided_by`, authority references, evidence references and effective dates belong to the
   answer/version**, not only to the record. *(This is the checkable consequence `vernon` named: the
   evidence for answer 1 is a different ADR from the evidence for answer 2, and under an
   answer-as-record design it could not live here.)*
4. **The current applicable answer is a deterministic projection over the answer history** — never a
   manually overwritten scalar. *(The register's own failure mode, hand-maintaining a derived value, is
   thereby unspellable rather than discouraged.)*
5. **Reversal is a new answer version** that supersedes the prior effective answer.
6. **Split and merge are first-class operations**, not prose:
   - a **split** creates successor records with **explicit provenance from the parent**;
   - a **merge** creates a successor record with **explicit provenance from every source record**;
   - the old records **retain their history and point to their successors**;
   - **no history is erased or rewritten to make the new topology appear original.**
7. **A record superseded at time `t` must reject any answer whose effective/recorded time is later than
   `t`**, unless an explicit reopen or correction event is supported by the final model. **Silent
   reopening must never be inferred from a merge conflict.**

### Why clause 6 is in the ruling at all

`vernon` found the shape neither the proposal's nor the coordinator's option space contained, and it is
**not hypothetical**: this register performed both operations *in a single batch on 2026-08-19* —
`REFUND-BEARER` residue **merged into** `CAPTAINNET-ZERO` while `BREAKDOWN-ZERO` **split**. His
assessment is that split/merge is **more common in this register than question-replacement**, and it is
*"the one the schema will otherwise meet unprepared."* Clause 6 makes it a modelled operation before
the schema exists rather than a migration surprise in slice 2.

### Why clause 7 is in the ruling at all

`vernon` found the one **silent** corruption in the concurrency story. Two sessions superseding the same
predecessor conflict loudly on the same YAML line — which is fine. But one session superseding `X` while
another appends an answer to `X` **merges cleanly**, yielding a record that is both `superseded` and
freshly re-answered, with nothing red anywhere. Clause 7 forbids it, and the founder's added sentence —
*"do not infer silent reopening from a merge conflict"* — closes the tempting repair, which would have
turned a detected corruption into an invented state transition.

### The acceptance condition — executable examples before schema

> **The first schema proposal must demonstrate, with fixtures and planted-defect tests, one ordinary
> reversal, one split, one merge, and one concurrent-append-after-supersession rejection. Do not
> hard-code a schema before these examples are executable.**

This inverts the usual order deliberately: **the four scenarios are the specification, and the schema is
whatever satisfies them.** It is the same discipline `beck` asked for at briefing — name the failing
test before the code exists — applied to the design rather than to a rule, and it is the direct answer
to his warning that with `docs/decisions/**` empty until slice 2, every corpus-wide assertion in slice 1
would be **vacuously true**. Four concrete fixtures cannot be vacuous.

## Alternatives considered

Presented in [PROP-20260819-110442 §14](../proposals/PROP-20260819-110442-the-decision-register-is-the-unit-of-decision.md);
both rejected, and the reasons are worth keeping because each fails on a rule the register already holds.

- **B — answer-as-record.** Every changed answer becomes a new file, the predecessor superseded.
  Rejected: the key must then carry a version (`CAPTURE-TIMING@2`), so **position-as-identity returns
  through the back door** — precisely what `DECISION-UNIT` exists to forbid. *"What applies now"* becomes
  a **scan** for the record nothing supersedes; the **common** operation (a reversal) pays a two-file
  concurrency cost; and the boot index would need dedupe logic, acquiring resolution behaviour and
  contradicting `BOOT-INDEX-BOUND`'s discovery-only rule.
- **C — current answer only, history in git.** Smallest schema. Rejected on three escalating grounds,
  the first dated and concrete: **`REGISTER-MIGRATION` destroys the blame surface** — rewriting ~154 rows
  into new files makes git report every record as born that day, with the whole decision lineage behind a
  rename. Second, `decided_on` ≠ commit date *systematically* (rows already read *"CLOSED 2026-08-12
  (raised 2026-08-11)"*), so the decision date is business data. Third, *"what was believed on date D"*
  must be a **query**: as a fold over one file a validator can run it; as git archaeology across renames
  nobody will. The repo already learned this once — `docs/adr/HISTORY.md` is the hand-built,
  retrospective version of exactly the field this ruling now requires.

## Consequences

### Positive
- The register's own vocabulary stays true end to end: the key names the question, the record **is** the
  question, and a decision identity outlives its answer.
- Clause 4 removes the failure class that produced this whole thread — a hand-maintained derived value
  that nothing re-derives, which is how `DECISIONS.md` came to assert `✅ IMPLEMENTED: the retention sweep
  is live` with no rebuild path.
- Clause 3 makes per-answer evidence expressible, which is what `STATUS-VOCABULARY`'s `realized` needs
  if it is to survive amendment C2's objectivity test at all.
- The four required demonstrations mean slice 1 cannot ship green while proving nothing.

### Negative
- **Slice 1's schema is now larger than the ruling first implied**: `answers[]` with per-answer
  `decided_by`, provenance edges for split/merge, and per-record history all land in the first schema
  rather than being retrofitted. `vernon` judged the alternative worse — retrofitting history after
  `REGISTER-MIGRATION` has already flattened the blame surface.
- **Split/merge provenance is an N-record invariant no single record owns.** The commit is the write
  transaction and CI is the fence (`vernon`), so it needs a corpus-level validator rule; it cannot be
  a per-file schema constraint.
- **Clause 7 needed a defined time source.** ✅ **Closed by the amendment below** — the gap was real and
  the founder ruled it rather than deferring it into the schema unresolved.
- Nothing here shortens the path to the user-facing outcome, which by amendment C6 exists only when
  canonical records, migration and Stage B all work together.

### Follow-up actions
- **Nothing is dispatchable.** `REG-WHEN` is **not scheduled**: no `REG` implementation slice is
  authorised before the [#556](https://github.com/TheCaptainCompany/captain-food/issues/556) local
  acceptance-harness milestone is completed **and** the founder explicitly schedules the work. The #643
  deferral remains in force.
- **The one carve-out, stated as ruled**: a separately approved **repository-integrity task** may
  proceed if it is complete and valuable *even if all `REG` work is cancelled* — and it **must not**
  create `docs/decisions/**`, a decision schema, a generated decision index, or agent enforcement. That
  is the `adr-citation-unresolved` ratchet and nothing more.
- The first schema proposal owes the **four executable demonstrations** above, and must implement the
  **two-clock model** ruled in the amendment.
- **Before REG work may be proposed for scheduling at all**, the #556 milestone report is owed: a fresh
  measurement of **boot context**, **decision status distribution**, **repeated-question incidents**, and
  **the feasibility of the bounded index**. Today's figures are explicitly not durable inputs to that
  request.
- `beck`'s eight planted defects (ADR-20260819-191227 § Consulted) plus his ninth — *a `superseded`
  record may not carry an answer appended after its `superseded_on`* — remain the slice-1 checklist.

## Consulted

**No new dispatch was run for this record, and that is deliberate rather than an omission.** The ruling
adopts, without alteration, the position four lenses had already returned on this exact question, and
the founder forbade further dispatch in the same message. Their returns are carried in
[ADR-20260819-191227 § Consulted](ADR-20260819-191227-the-register-ruling-canonical-records-a-nine-status-vocabulary-and-a-capped-boot-index.md#consulted):

- **`vernon`** — dispatched specifically on this question; supplied the aggregate boundary, the three
  shapes including split/merge, the per-answer `decided_by` consequence, the three auditability grounds,
  and the concurrent-append corruption that became clause 7. Recommended option A.
- **`evans`** — reached the same conclusion from naming: a key names the question, so a reversal is a new
  answer on the same record and supersession is a change of identity. His divergence with `vernon`, filed
  unanswered in the previous record, is now **answered and closed** — `vernon` agreed with him and added
  the third shape he had not enumerated.
- **`young`** — supplied the rule clause 4 encodes: a derived value maintained by hand is the defect,
  and the current answer must be a projection with a rebuild path.
- **`beck`** — supplied the discipline the acceptance condition applies, and the warning about vacuously
  true corpus assertions that the four fixtures answer.

---

## Amendment 2026-08-19 — the time source is ruled, and the design phase closes

### Two clocks, and only one of them orders the lifecycle

The gap this record flagged — *"effective/recorded time is unspecified, and comparing an
author-supplied effective date against another author's supersession time is not obviously
well-ordered"* — is closed by the founder rather than deferred:

```yaml
recorded_at:   # immutable, repository-controlled ordering
effective_at:  # optional policy/domain applicability date
```

- **Lifecycle and supersession validation must use `recorded_at`**, or an equally monotonic
  repository-controlled sequence.
- **`effective_at` must not permit a later edit to bypass supersession or reopen a record implicitly.**

**Why this is the right cut, and why it was worth ruling before the schema.** Clause 7 says a record
superseded at `t` rejects a later answer. Left with one date, `t` would have been author-supplied — and
an author-supplied clock is not monotonic, is not adversarially safe, and is exactly the attack surface
clause 7's second sentence (*"do not infer silent reopening from a merge conflict"*) exists to close. A
single field would have re-opened, through the back door, the hole the clause was written to shut: set
`effective_at` earlier than the supersession and the rejection stops firing. **Separating the clocks
makes the ordering repository-controlled and the applicability date pure business data.** It is the same
distinction the repo already draws between an event's `occurredAt` envelope field and the business dates
inside a payload.

**Consequence for the four required demonstrations**: the concurrent-append-after-supersession fixture
must be written against `recorded_at`, and a fifth shape is now implied and worth including — *an answer
whose `effective_at` predates the supersession must still be rejected on `recorded_at` grounds*. That is
the fixture that proves the two clocks do not collapse into one under pressure.

### The design phase is closed

> *"This closes the design phase for the decision-register programme."*

`REG-REVERSAL` is decided; `REG-WHEN` remains explicitly unscheduled. **Not authorised**: REG
implementation, schema, YAML records, generated index, migration, librarian enforcement, GraphRAG/QMD
work, C5-lens dispatch.

**The active priority returns to [#556](https://github.com/TheCaptainCompany/captain-food/issues/556).**
[#659](https://github.com/TheCaptainCompany/captain-food/issues/659) may be considered later as a
separately approved, narrowly scoped integrity task; it **neither authorises nor blocks** REG work.

### What is owed before REG work may be proposed for scheduling

At the #556 milestone, and **not before**, a fresh measurement of four things — after which scheduling
may be *asked for*, not assumed:

1. **Boot context** — the six-file reading order, re-measured.
2. **Decision status distribution** — the census. Today's *"75 of 154 rows carry no status token"* is a
   dated observation, not a durable fact.
3. **Repeated-question incidents** — how often a settled decision was actually re-litigated in the
   interval. This is the only one of the four that measures the *problem* rather than the *artifact*,
   and the programme's whole justification rests on it.
4. **Feasibility of the bounded index** — modelled against the then-current open set, not against the
   136 B/row estimate recorded here.

### The bounded hardening item, recorded and not implemented

The gate-command rule ([sessions/gates.md §1b](../claude/sessions/gates.md)) is accepted as a
**documented interim control**. One future item is recorded, **conditional and deliberately narrow**:

> **When hook/dispatch wiring is explicitly scheduled** — and only then — make required gate commands
> **fail closed on their direct exit status**, **preserve their output**, and add a **negative test
> proving that a trailing successful command cannot mask a failed validation command.**

**Not to be implemented now, and not to be broadened into hook-platform work.** It is written into the
rule itself so that whoever wires hooks meets it there rather than rediscovering the defect.
