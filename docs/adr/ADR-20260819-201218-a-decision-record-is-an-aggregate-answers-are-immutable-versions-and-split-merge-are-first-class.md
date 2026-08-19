# ADR-20260819-201218 — A decision record is an aggregate: answers are immutable versions, and split/merge are first-class

## Status

Accepted — **design only.** Closes `REG-REVERSAL`. **Authorises no implementation**: `REG-WHEN` is
**not scheduled** and the [#643](https://github.com/TheCaptainCompany/captain-food/issues/643) deferral
remains in force.

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
- **Clause 7 needs a defined time source.** *"Effective/recorded time"* is unspecified here, and
  comparing an effective date supplied by an author against a supersession time supplied by another
  author is not obviously well-ordered. **This is a real gap and it belongs to the first schema
  proposal**, not to this record.
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
- The first schema proposal owes the **four executable demonstrations** above, plus a decision on the
  clause-7 time source.
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
