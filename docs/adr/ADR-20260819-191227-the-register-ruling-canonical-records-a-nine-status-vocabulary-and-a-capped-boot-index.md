# ADR-20260819-191227 — The register ruling: canonical records, a nine-status vocabulary, and a capped boot index

> **AMENDED 2026-08-19 (same day, founder), and the amendment is load-bearing.** The ruling is
> **DESIGN APPROVAL ONLY — not a schedule and not dispatch authorisation.** It does **not** revoke the
> [#643](https://github.com/TheCaptainCompany/captain-food/issues/643) deferral and does **not** displace
> [#556](https://github.com/TheCaptainCompany/captain-food/issues/556). **No `REG-*` slice may be claimed,
> started or implemented until the founder explicitly schedules it, after the local acceptance harness
> has reached its stated milestone.** Six corrections apply to the approved design — see
> [§ Amendment](#amendment-2026-08-19--six-corrections-the-scheduling-constraint-and-two-open-questions).
> All four defects this ADR recorded are **resolved by those corrections**. Both open questions have
> since been ruled: **OQ-1 closed** (record-as-aggregate,
> [ADR-20260819-201218](ADR-20260819-201218-a-decision-record-is-an-aggregate-answers-are-immutable-versions-and-split-merge-are-first-class.md))
> and **OQ-2 ruled NOT SCHEDULED**. The six keys are approved **as named**.

## Status

Accepted — **design approval only**, amended 2026-08-19 (see banner)

## Enforced by

n/a — no behavioral guarantee in this record. The guarantees this ruling authorises are enforced by
the validator rules of slices 1 and 3, which are **not built** (see Follow-up).

## Context

The founder ruled on [PROP-20260819-110442](../proposals/PROP-20260819-110442-the-decision-register-is-the-unit-of-decision.md)
on 2026-08-19, answering the question that opened the thread — *"do the agents ask questions the ADRs
already answered?"* — with an approved design rather than a further option space. The proposal moves
from `Proposed` to `Approved`; register rows `REG-1`…`REG-4` close and `REG-SEQ` unblocks
([DECISIONS §48](../proposals/DECISIONS.md)).

This record captures the ruling **verbatim**, states the **four** defects the ruling's own text carries,
and stops. **No implementation is authorised by this ADR.** The founder amended the ruling the same day
to make that explicit and to correct all four defects — read the banner and § Amendment before acting
on anything in the Decision section below, which is preserved as ruled, not as corrected.

## Decision

### The ruling, verbatim, with keys

The founder's message numbered its decisions `D1`–`D6`. **Those are positional keys, and the ruling's
own first clause forbids positional keys** — the proposal already has a `D1`–`D5` meaning different
things, so `D3` acquired three referents in this repo the moment the ruling was written.

`evans` was asked to rule on the fix and **rejected the coordinator's first proposal** (namespacing them
as `RULING-20260819/…`): any document-scoped namespace still ratifies *where a thing was written down*
as its identity, and the mapping is non-bijective anyway — the message's `D2` covers the proposal's
`D2`+`D3`, and a positional scheme has no way to say *"this one decision is now two records"* except
prose. The collision is also worse than the briefing stated: a bare `D1` is already the key cell of
**four unrelated rows** in `DECISIONS.md`, `D5` appears six times, and someone has already hand-patched
one into `**D6 endpoint**` — the language telling you the identifier was underspecified.

**The keys below therefore name the QUESTION, never the answer and never a position** — `CAPTURE-TIMING`
survives a reversal where `CAPTURE-ON-ACCEPTANCE` dies at it, and a decision identity must outlive its
own answer. Uniqueness is free from one file per key under `docs/decisions/<KEY>.yaml`: the filesystem
is the constraint, and no validator rule is needed for it.

| Key | Ruling |
|---|---|
| **`DECISION-UNIT`** (msg `D1`) | The canonical decision unit is one declared record with a globally unique, immutable, namespaced key. A decision record is a decision **identity**; an ADR is **evidence/rationale** and may contain multiple decision records. No positional keys such as `D1`–`D7` without a namespace. |
| **`REGISTER-STORAGE`** (msg `D2`) | Canonical records under `docs/decisions/**` in YAML. `docs/proposals/DECISIONS.md` becomes a **generated** human-readable view, never a separately maintained authority. Generator and validator deterministic; `check-drift` fails when generated output is stale. |
| **`STATUS-VOCABULARY`** (msg `D3`) | Closed vocabulary with explicit transitions, at minimum: `open` · `proposed` · `decided` · `implementing` · `realized` · `blocked` · `superseded` · `rejected` · `deprecated`. **`decided` = binding policy exists; `realized` = implementation evidence exists; they must not be conflated.** `open`, `blocked` and `proposed` do **not** authorise implementation. |
| **`REGISTER-MIGRATION`** (msg `D4`) | Staged. Migrate all currently open/blocked/proposed/recently-decided records first, namespacing the ambiguous `D1`–`D7` keys in that same pass. Historical residue follows in later slices, but **no existing record may silently disappear**: every source row maps to one canonical record, an explicitly merged successor, or a declared archive/exemption. **Re-derive the census immediately before implementation; today's counts are not durable facts.** |
| **`ASK-ENFORCEMENT`** (msg `D5`) | Two stages. **A**: a decision-queue question is invalid unless it cites one existing `open` decision key; the validator enforces it. **B**: before a decision-sensitive plan is sent or implementation begins, the agent queries the decision records and emits a compact `Decision Context`; it may ask the founder only when no matching record exists or the match is `open`/`blocked`. **No claim that this prevents repeated questions until Stage B is implemented and tested against known re-litigation cases.** |
| **`BOOT-INDEX-BOUND`** (msg `D6`) | The generated resident index is discovery-only, hard-capped at **8 KB**, carrying only key · status · scope/domain · one-line question or binding-answer summary · pointer · supersession/authority pointer. No rationale, discussion or copied ADR prose. It **must** include (1) all `open`, `proposed`, `blocked` records and (2) a bounded "active constraints" subset of `decided`/`implementing` records that are global or high-risk (money, legal/compliance, security, irreversible data, production operations, foundational architecture), marked by an explicit schema-backed `resident: true` **with a stated reason** — never a vague "important" convention. Cap breach fails validation; **raising the cap is a separate explicit decision**. |

**Where `D1`–`D7` remain legitimate**: as an **anchor inside a cited document**, never as a key —
`decided_by: [{ $ref: <ADR-or-PROP>, anchor: D1 }]`. That keeps every existing `PROP-…/D1` reference
readable as a *citation syntax* without promoting document position to an identity.

**Prefixes, where a key needs one, come from a closed set, and that set is the spec scopes**
(`ordering · catalog · network · customer · delivery · payments · comms · common`) — so the register and
`specs/{scope}/` share one context partition instead of the repo growing a second taxonomy.

Fenced throughout, restated by the founder: **GraphRAG, QMD and external memory systems are not
prerequisites and never become decision authority.**

### Four defects in the ruling, recorded now rather than discovered in slice 3

The ruling is approved and is being executed as written. These are the places where executing it as
written will not produce what it intends, surfaced at briefing where they are free.

**1. Stage B points the write side at a lossy read model — `young`.**
`ASK-ENFORCEMENT` Stage B says the agent must query *the decision index*.
`BOOT-INDEX-BOUND` defines that index as a deliberately lossy 8 KB fold. **A gate that
decides whether work may proceed, reading a truncated projection, is a first-order write/read
violation** (ADR-20260815-030206), and it has a governance consequence: a curation change to
`resident:` would silently change what an agent is *permitted to do*, making a regeneration a
governance event. **Both stages must read the declared records under `docs/decisions/**`.** The index
and `DECISIONS.md` are human surfaces only, and the schema should say so. *This is a correction to the
ruling's mechanism, not to its intent, and is recorded as such.*

**2. `realized` as an authored token is a status no gate can contradict — `beck` and `young`, independently.**
A hand-written `realized` is a self-assertion about the world outside the register, with no rebuild
path. The failure is already in the wild in the artifact being replaced: `DECISIONS.md` asserts
`✅ IMPLEMENTED: the retention sweep is live`, which nothing re-derives — the same defect class as the
resident index telling every session that *"gates are hooks in `.claude/settings.json`"* when
`grep -c -i hook .claude/settings.json` returns **0**. Both lenses reached the same remedy from
opposite directions: **`realized` must require resolvable evidence** — a `realized_by:`/`evidence:`
naming a merged commit, a repo path, a spec `$ref`, or a named test that goes red if the thing is
removed — validated exactly as `decided_by` is. `beck`'s position if the schema will not carry it:
**drop `realized` rather than ship a status no gate can contradict.** `young` adds that `implementing`
is weaker still — it mirrors the *work-item* lifecycle, which is #643's scope, and co-maintaining it
beside the issue guarantees divergence; point at the issue instead of mirroring its state.

**3. The 8 KB cap is tighter than it looks, and the census decides whether it is feasible at all.**
Re-derived at `456abda` against the ruling's own field list — key, status, scope, one-line summary,
pointer, supersession pointer — four realistic sample rows measure **118–153 B, mean 136 B**, not the
120 B the D6 brief assumed. With a ~400 B header:

| open + proposed + blocked | `resident:` records | Index size | vs 8 192 B cap |
|---:|---:|---:|---|
| 41 | 0 | 5 996 B | 73% |
| 41 | 10 | 7 361 B | 90% |
| 41 | 20 | 8 726 B | **breached** |
| 50 | 10 | 8 590 B | **breached** |
| 60 | 0 | 8 590 B | **breached** |

The uncontrolled variable is not the resident subset but the **open set**: **75 of 154 rows carry no
status token at all today**, so the migration will resolve them to *something*, and if even half land
open/blocked/proposed the cap is breached with zero resident records. This is why the founder's
instruction to **re-derive the census immediately before implementation is load-bearing rather than
procedural** — it decides whether 8 KB is generous or already impossible. The gate firing is the design
working; the risk is that the first thing it rejects is a legitimate resident record.

**4. `resident:` creates permanent WIP, and it should be priced where it is introduced — `holub`.**
Once residency is a per-record curation decision that must be *"explicit, schema-backed, and reviewed"*,
the cap stops being self-limiting and becomes **a recurring review queue with no end date**: every
future `decided` record touching money, legal/compliance, security, irreversible data or production
operations now carries a residency question a human must answer. That is ongoing work created by a
governance artifact, and the slice that introduces `resident:` should carry its cost rather than let it
be discovered later.

**A naming consequence of the same finding — `evans`.** `resident: true` names the **mechanism** (it
goes in the boot index), not the **meaning** (this decision binds work right now). Name the concept —
*active constraint* — and let placement be derived from it, or the curation criterion drifts back to
"important", which is exactly what the ruling forbade.

## Alternatives considered

The option space was the proposal's and the founder resolved it. Recorded here only where the ruling
chose against the proposal's own recommendation:

- **Status vocabulary of five** (`open · decided · deferred · superseded · withdrawn`) — the proposal's
  recommendation. **Ruled against**: nine values, adding `proposed`, `implementing`, `realized`,
  `rejected`, `deprecated`, and dropping `deferred`/`withdrawn` in favour of `rejected`/`deprecated`.
  The `decided`/`realized` split is the substantive addition and the one carrying defect 2 above.
- **Index bounded by the open set alone** — the D6 brief's recommendation, chosen because it made the
  bound self-limiting. **Ruled against**: the resident subset is added, so the bound is now partly a
  curation decision. The founder required it be schema-backed with a stated reason, which is what keeps
  it from becoming the "important" convention the brief warned about.
- **A single `docs/decisions.yaml`** and **keeping markdown with a status token** — both rejected by
  the proposal and not revived by the ruling.

## Consequences

### Positive
- The register acquires a machine identity: a unique immutable key, a closed status vocabulary and a
  declaration site. It becomes the largest surface in the repo to stop being exempt from
  ADR-20260811-014129's *"every reference is a `$ref`"* doctrine.
- `DECISIONS.md` becomes a projection, so the human page can no longer disagree with the data, and
  `check-drift` — which already exists and is already green — enforces it at no new gate cost.
- `decided` vs `realized` names a distinction the repo currently conflates, which is why a resident
  index could tell every session that hooks existed when none did.
- The boot index is capped by a rule that fails rather than by a convention that erodes.

### Negative
- **`holub` dissents from the sequence, with figures, and it is recorded rather than argued past.**
  His position hardened rather than softened: the ruling's own `ASK-ENFORCEMENT` forbids claiming the
  value *"until Stage B is implemented and tested"*, so the single outcome this programme exists to
  deliver — the founder not being re-asked a settled question — **arrives at the fourth slice**.
  *"Four slices where the value lands last is not four independently reviewable slices, it is one batch
  of four wearing slice vocabulary"*, and slice 1's generated view is a view over records that do not
  exist yet. Antecedents he named: zero orders have ever flowed end to end; the last green nightly smoke
  was **2026-07-29** (21 days); the [#556](https://github.com/TheCaptainCompany/captain-food/issues/556)
  walk card landed **2026-08-17**; `docs/` commit volume has far outrun `crates/` commit volume over
  the same window; and a high ADR production rate in August (his figure for it is quoted below and
  carries no antecedent — see the strike note).
  ⚠️ **Three figures struck as `UNVERIFIED`, and one claim withdrawn as false.** The **107 ADRs**
  figure quoted below names no command, ref or scope, and the record's own prose said *in August* while
  the quote says *in fourteen days* — two denominators for one number. The quote stays verbatim as his
  words; the figure is not relied on here. The lens's *"36 `docs/`
  commits vs 5 `crates/`"* pair does not reproduce: three independent derivations at `df082e6` gave
  36 (as recorded), 45, and 33 for the first, and 5, 5 and 4 for the second, because no scope was ever
  stated — `--since` boundary, ref, and whether merge commits count all move it. **The figures are not
  used as a decision premise**: the ratio's direction is not in dispute at any of those values, and
  `holub`'s argument rests on the direction. The separate claim that the walk card had *"no branch and
  no commit"* is **FALSE** — branch `556-local-walk-harness` and draft PR #621 existed with four
  commits; the claim was true only of a stale local clone and was relayed without re-verifying against
  GitHub. His conclusion: *"a register is an index over inventory; 107 ADRs in fourteen days IS the
  inventory, and industrialising the index does not shorten the loop that produced it."* Asked for the
  smallest honest subset that could ride alongside #556, he answered **none of the four** — and named a
  different candidate instead (below). **This does not overturn the ruling and no priority was changed
  by this record.**
- **Approving a design is not scheduling it.** `holub` notes the founder ruled one day earlier on the
  sibling proposal — *"we will not apply it yet we will finish what we have started first"*
  ([#643](https://github.com/TheCaptainCompany/captain-food/issues/643)) — and that nothing in this
  ruling revokes it. Whether this ruling schedules the slices or only approves their design is a
  question this ADR does not answer, and it should be settled before slice 1 is claimed.
- **Four slices of process machinery** on a project whose process output already outruns its code.
- **The migration cost is unknown and growing.** Rows went 148 → 154 in one day, and 75 carry no
  derivable status. The census must be re-derived, not inherited.
- **Slice 1's tests will be vacuously true** until slice 2 populates the corpus — `beck`'s trap, already
  documented at `tools/codegen-rs/src/tests.rs:6342-6348`. Every corpus-level assertion needs a
  non-empty precondition or the slice ships green and proves nothing.
- **Uniqueness is a set invariant no single record owns.** `young`: one-file-per-record is the right
  shape *only if the key IS the filename*, so the filesystem arbitrates uniqueness; if the key is a
  field inside the file, two concurrent sessions can both win the race.

### Follow-up actions
- **Nothing here is dispatchable.** The founder's sequence: (1) `#659` first, and only after normal
  approval, restricted to `STATUS.md`; (2) register work in independently reviewable slices; (3) slice 1
  = schema, parser, validator, generated view, planted-defect tests — **no broad migration, no agent
  behavioural claims**; (4) slice 2 = migrate the active set, resolve duplicate namespaces; (5) slice 3
  = queue-question gate; (6) slice 4 = plan/execute decision context + regression tests against the
  known re-litigation cases.
- **The four defects above are inputs to slice 1's design** — all four are corrected by the amendment, not separate work. Defect 1 changes what
  Stage A/B read; defect 2 changes the schema's `realized`; defect 3 changes nothing structurally but
  makes the pre-implementation census a gate on feasibility.
- **`beck`'s eight planted defects** are named in Consulted and should be the slice-1 PR body's checklist.
- The D6 decision itself lives only in this ADR and the founder's message; **`PROP-20260819-110442` §3
  is rewritten in the same change as this record** so the proposal carries the ruled design, per the
  living-proposal rule (ADR-20260801-020000).

## Consulted

Founder directive, so the roster was invited before this record landed (ADR-20260812-143619).

- **`young`** — `DECISIONS.md` is a projection and `check-drift` is the right fence; the proof is that
  deleting the view and the index, then regenerating, must change no answer anywhere. Carried defect 1:
  Stage B reading the capped index is a write-side gate on a lossy read model, and a `resident:`
  curation change would then alter what an agent may do. Carried defect 2 with `decided_by`'s precedent:
  `decided` is self-authoritative, `realized` is a claim about the world and must be derived, never
  authored; `implementing` mirrors the work-item lifecycle and should point at the issue rather than
  copy its state. Named the hard case: the key must BE the filename or uniqueness degrades to a
  projection scan with a race.
- **`beck`** — named eight planted defects for slice 1 before any code:
  `a_positional_key_without_a_namespace_is_rejected` (fixtured on the ruling's own `D3`, a pre-earned
  red), `a_status_outside_the_closed_vocabulary_is_rejected`,
  `two_files_declaring_the_same_decision_key_are_both_reported` (message must name both paths),
  `supersession_missing_its_reverse_edge_is_an_error` (both directions),
  `resident_true_without_a_stated_reason_is_an_error`,
  `a_hand_edit_to_the_generated_register_is_reported_as_drift`,
  `the_view_is_byte_identical_under_shuffled_record_order` (determinism breaks through map iteration
  order), and `the_index_at_8193_bytes_fails` asserting the message names the overflowing keys. Ruled
  that `realized` as scoped is untestable — *"I would rather the vocabulary drop `realized` than ship a
  status no gate can contradict"* — and that D4's no-silent-disappearance clause needs
  `every_source_row_maps_to_a_record_a_merge_or_a_declared_archive` written **now** against a
  one-row-dropped fixture, so slice 2 inherits a gate already seen red.
- **`evans`** — ruled on the key scheme and **rejected the coordinator's first answer**: any
  document-scoped namespace (`ADR-…/D1`) ratifies outline position as identity, which the ruling forbids
  in substance. The right unit already exists — the mnemonic register key, already globally
  unique — and the rule is that **a key names the QUESTION, never the answer and never a position**,
  because a decision identity must outlive its own answer. `D1`–`D7` stay legal as a *citation anchor*
  inside `decided_by`, never as a key; uniqueness comes free from one file per key; prefixes come from a
  closed set that should be the spec scopes, so the register and `specs/{scope}/` share one partition
  rather than growing a second taxonomy. On vocabulary: *"canonical decision unit", "canonical record",
  "register row" and "decision identity" are four names for one thing* — the schema should say
  **`decision`, bare**, and **"register row" is the actively harmful one**, because it names the artifact
  this ruling demotes to a generated view and will quietly restore `DECISIONS.md` to authority in every
  reader's head. Reserve *register* for the collection and *the generated view* for `DECISIONS.md`.
  **Divergence he asked to be recorded, against `vernon`**: under a question-named key a *reversal* is a
  new answer on the same record, not a supersession — two records apply only when the **question itself**
  is replaced by a differently-scoped one. That is a much narrower two-record transaction than the
  briefing implied, and it should be settled before the schema fixes it. **`vernon` was not dispatched,
  so this divergence is recorded unanswered.**
- **`holub`** — *"No, and the ruling is my strongest argument."* Full position and antecedents in
  Consequences above; he priced `resident:` as permanent WIP (defect 4). His alternative, offered
  unprompted: if something must ride alongside #556 it is the **proposal's** slice 1 —
  `adr-citation-unresolved` plus its `_exempt.yaml` and the one stale-citation fix — because it is a
  standing ratchet over prose that already exists, needs no schema, no migration and no records, and
  *"would still be correct if all four ruled slices were cancelled tomorrow"*. And the thing he refused
  to pretend: *"I am not saying the register is waste; I am saying it is inventory management, and the
  correct first response to accreting inventory is to stop producing it, not to build a faster index over
  it."* He marked one figure he did **not** re-derive — the 2.4:1 process-to-code ratio — as
  `UNVERIFIED input` and did not lean on it, which is ADR-20260817-105845 working as intended.
- **`vernon`** — dispatched separately on reversal semantics under amendment C4; full analysis carried in
  the proposal's §14 decision request. **Agrees with `evans` and reaches it from the other side**: the
  record is the aggregate, its identity is the question, its answers are the events in its own stream —
  *"a changed answer is a state transition inside one boundary; supersession is a change of identity."*
  He **sharpens `evans` on one point that changes the schema**: there are three shapes, not two — reversal
  (one record), question replacement (two), and **split/merge (N)**, the last being *"the real
  multi-record operation in this register, more common than (b)"*, evidenced by `REFUND-BEARER` merging
  into `CAPTAINNET-ZERO` while `BREAKDOWN-ZERO` split, in one batch, on 2026-08-19. Two checkable
  consequences: `decided_by` moves onto the **answer**, and **git alone cannot carry auditability**
  because `REGISTER-MIGRATION` destroys the blame surface — a cost that lands in slice 2 unless the
  schema carries history in slice 1. Found one new silent corruption and its rule: one session
  superseding `X` while another appends an answer to `X` **merges cleanly**, so *a `superseded` record
  may not carry an answer appended after its `superseded_on`*. On the two-record edge: there is no
  transaction and must not be — *"the commit IS the write transaction; CI is the fence"*. **Independently
  reached the founder's C3**: `resident: true` is a **set** invariant expressed as a per-record field, so
  a record author cannot know whether their own flag is legal and the corpus rule *"will reject whichever
  record lands last, not the least important one"* — derive residency instead.
- **`dba` · `graphql-architect` · `ux-designer` · `business-specialist` · `legal-specialist` ·
  `observability-agent` · `farley`** — invited via
  [the briefing](../dispatch/BRIEF-20260819-the-register-ruling.md); not separately dispatched for this
  records-only chunk. **A lens never asked is indistinguishable from a lens with nothing to say**, so
  this is stated as a gap rather than as silence. Amendment **C5 converts it into a precondition**:
  `legal-specialist`, `business-specialist` and `dba` must define the **controlled fields and
  classifications** that determine boot residency and implementation gates **before slice 2** — and,
  under C3, those classifications are what residency is *derived from*, so they are now load-bearing
  rather than advisory. Deliberately **not dispatched in this records-only chunk**.

---

## Amendment 2026-08-19 — six corrections, the scheduling constraint, and two open questions

The founder amended the ruling the same day, on reading this record. **The amendment is not a
refinement of emphasis: it changes one key's mechanism outright and puts the whole programme behind an
explicit scheduling gate.**

### The scheduling constraint, stated first because it governs everything below

> *"This is design approval only, not a schedule and not dispatch authorization. It does not revoke the
> #643 deferral or displace #556. No `REG-*` slice may be claimed, started, or implemented until I
> explicitly schedule it after the local acceptance harness has reached its stated milestone."*

This answers the question this ADR left open in Consequences — *whether the ruling schedules the slices
or only approves their design*. **It approves the design only.** The keys `DECISION-UNIT`,
`REGISTER-STORAGE`, `STATUS-VOCABULARY`, `REGISTER-MIGRATION`, `ASK-ENFORCEMENT` and
`BOOT-INDEX-BOUND` are approved **as named**; positional labels `D1`–`D7` remain **citation anchors
only and are never decision identities**.

### The six corrections

**C1 — Canonical reads, not index reads.** `ASK-ENFORCEMENT` must resolve and read the **canonical
declared decision records**. The boot index is discovery/navigation only, **may return candidate keys,
is not authoritative, and must not determine permission to ask or implement**: *"a lossy projection
cannot be a write-side gate."* → **Resolves defect 1** (`young`) exactly as he framed it, and settles
that both Stage A and Stage B read `docs/decisions/**`.

**C2 — `realized` requires verifiable evidence, or it does not ship.** Retain `realized` **only** with
a mandatory, resolvable evidence reference and a defined validator — an implementation issue/PR/commit
plus an applicable verification artifact. *"If such an invariant cannot be implemented without
subjective interpretation, remove `realized` from the first status vocabulary and represent
implementation separately until evidence semantics are designed."* → **Resolves defect 2**, and adopts
`beck`'s fallback as a first-class branch rather than a fallback: the vocabulary ships at eight values
if the invariant cannot be made objective. **The nine-value list in this ADR's `STATUS-VOCABULARY` row
is therefore conditional on that test.**

**C3 — The 8 KB cap is a design target, not an unconditional invariant, and residency is DERIVED.**
Two changes, and the second is the larger:
- `resident: true` **must not be a required manual decision on every new high-risk record** — *"that
  creates governance WIP and a second decision queue."* Residency is **derived from stable schema
  fields** (status, domain, applicability, explicit severity/classification), with an **exceptional
  override only, and only with a recorded reason.**
- Before implementation: **re-derive the actual census and model the maximum feasible index size.** If
  all `open`/`proposed`/`blocked` records cannot fit, the index carries **a compact summary plus
  deterministic pointers** while the canonical records remain queryable. **The cap is never raised
  silently.**

→ **Resolves defect 3 and defect 4 together.** Defect 3 (the cap is breached at 41 open + 20 resident
on measured 136 B rows) stops being a blocker because the index degrades to summary-plus-pointers
rather than failing or growing. Defect 4 — `holub`'s *"permanent WIP … a recurring review queue with no
end date"* — is removed at the root: derived residency has no per-record queue. **This is the correction
that most changes the approved design**, and `BOOT-INDEX-BOUND` as recorded above should be read through
it.

**C4 — Reversal semantics are resolved before any schema implementation.** `vernon` is dispatched
specifically on it, answering `evans`' divergence recorded in Consulted. The output must distinguish
**stable question identity · answer versions · supersession · historical auditability · how agents
resolve "what applies now."** **No schema implementation until this is decided** — and it is decided by
the founder, not by the lens. See Open question **OQ-1**.

**C5 — No high-risk curation without the relevant lenses.** Before slice 2, `legal-specialist`,
`business-specialist` and `dba` must review the **domains and classifications** that determine boot
residency and implementation gates. Explicitly *"not an invitation to add their prose to the boot
index; it is input to define the controlled fields and classifications."* → Converts this ADR's stated
gap ("their input is owed before slice 2") from an observation into a **precondition**, and pairs with
C3: derived residency needs a controlled classification vocabulary, and those three lenses own it.
**Not dispatched now** — that work belongs to the slice-2 precondition, not to this records-only chunk.

**C6 — `holub`'s dissent is preserved as an EXECUTION CONSTRAINT, not merely as a recorded position.**
> *"Do not fragment this into nominal slices and claim early value. The intended user outcome — an agent
> checks resolved decisions before asking the founder — only exists when canonical records, migration,
> and Stage B enforcement all work together. Until then, do not claim that repeated questions are
> prevented."*

This is the strongest form of what `holub` argued (*"one batch of four wearing slice vocabulary"*) and
it now **binds execution**: slices remain independently *reviewable*, but no slice may be reported as
delivering the user-facing outcome. It also hardens `ASK-ENFORCEMENT`'s own no-claim clause into a
standing prohibition rather than a caveat.

### #659 and the ADR-citation slice

`#659` remains **independently eligible only through normal approval**, and it **neither authorises nor
blocks** `REG` work — the sequencing statement in this ADR's Follow-up ("`#659` first") is amended to
that. Separately, the proposal's ADR-citation-integrity slice **may be evaluated as a narrow
repository-integrity task in its own right** — which is the candidate `holub` named unprompted — but it
**must not become a backdoor into register migration**.

### Open questions — the only two things the founder is asked to decide

- **OQ-1 — Reversal semantics. ✅ CLOSED 2026-08-19 — record-as-aggregate.** The key identifies the
  **concern**; answers are **immutable versions**; `decided_by`, authority, evidence and effective dates
  live on the **answer**; the current answer is a **deterministic projection**, never an overwritten
  scalar; **split and merge are first-class** with explicit provenance and no rewritten history; a record
  superseded at `t` **rejects** a later answer, and **silent reopening is never inferred from a merge
  conflict**. The first schema proposal must demonstrate **one reversal, one split, one merge and one
  concurrent-append-after-supersession rejection as executable fixtures BEFORE any schema is
  hard-coded**. → [ADR-20260819-201218](ADR-20260819-201218-a-decision-record-is-an-aggregate-answers-are-immutable-versions-and-split-merge-are-first-class.md).
  `evans`' divergence with `vernon`, filed unanswered above, is **answered and closed**: `vernon` agreed
  with him and added the split/merge shape he had not enumerated.
- **OQ-2 — Execution priority after #556. 🔴 RULED 2026-08-19: NOT SCHEDULED.** The #643 deferral remains
  in force. **No `REG` implementation slice is authorised** before the #556 local acceptance-harness
  milestone completes **and** the founder explicitly schedules the work. One carve-out: a separately
  approved **repository-integrity task**, complete and valuable even if all `REG` work is cancelled,
  which must **not** create `docs/decisions/**`, a decision schema, a generated decision index, or agent
  enforcement.

**Nothing in the programme is a live question, and nothing in it is dispatchable.** Both open questions
are now closed; what remains is a schedule the founder has reserved.
