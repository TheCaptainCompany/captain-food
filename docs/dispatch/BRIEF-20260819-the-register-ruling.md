# Mob briefing — the founder's ruling on the decision register (2026-08-19)

**Class**: governance / operating-model. Records-only in this chunk; the implementation slices it
authorises are separate and not started. **No code, no migration, no `specs/**` change here.**

The founder ruled on [PROP-20260819-110442](../proposals/PROP-20260819-110442-the-decision-register-is-the-unit-of-decision.md).
This is a founder directive, so the whole roster is invited before any record lands
(ADR-20260812-143619), and the record will carry a `Consulted:` block with one line per lens.
**"Nothing in my lens" is a complete answer and costs one line.**

---

## The ruling, verbatim

> **D1 — Canonical decision unit: approved.**
> The canonical decision unit is one declared record with a globally unique, immutable, namespaced key.
> A register row is a decision identity; an ADR is evidence/rationale and may contain multiple decision
> records. Do not use positional keys such as `D1`–`D7` without namespace.
>
> **D2 — Canonical storage and generated view: approved with one constraint.**
> Create canonical records under `docs/decisions/**` in YAML. `docs/proposals/DECISIONS.md` becomes a
> generated human-readable view, never a separately maintained authority. The generator and validator
> must be deterministic and `check-drift` must fail when generated output is stale.
>
> **D3 — Status model: approved.**
> Use a closed status vocabulary and explicit transitions. At minimum: `open`, `proposed`, `decided`,
> `implementing`, `realized`, `blocked`, `superseded`, `rejected`, `deprecated`.
> `decided` means binding policy exists; `realized` means implementation evidence exists. They must not
> be conflated. `open`, `blocked`, and `proposed` do not authorize implementation.
>
> **D4 — Migration and identity repair: approved, staged.**
> First migrate all currently open, blocked, proposed, and recently decided records. Namespace all
> ambiguous `D1`–`D7` keys during that first migration. Historical/closed residue can be migrated in
> subsequent slices, but no existing record may silently disappear: every source row must map to one
> canonical record, an explicitly merged successor, or a declared archive/exemption. Re-derive the
> census immediately before implementation; do not use today's counts as durable facts.
>
> **D5 — Enforcement: approved in two stages.**
> Stage A: a decision-queue question is invalid unless it cites one existing `open` decision key; the
> validator enforces this.
> Stage B: before a decision-sensitive plan is sent or implementation begins, the agent must query the
> decision index and emit a compact `Decision Context`. It may ask me only when no matching record
> exists or the matched record is `open`/`blocked`.
> Do not claim this prevents all repeated questions until Stage B is implemented and tested with known
> re-litigation cases.
>
> **D6 — Boot index: approved with correction.**
> The generated resident index is discovery-only and hard-capped at 8 KB. It contains only: key, status,
> scope/domain, one-line question or binding answer summary, pointer, and supersession/authority pointer
> where applicable. No rationale, discussion, or copied ADR prose.
> However, it must include: (1) all `open`, `proposed`, and `blocked` records; and (2) a bounded
> "active constraints" subset of `decided`/`implementing` records that apply globally or are high-risk
> (money, legal/compliance, security, irreversible data, production operations, or foundational
> architecture). This subset must be explicit, schema-backed, and reviewed — not selected by a vague
> "important" convention. Use a `resident: true` field with a stated reason. If the hard cap is breached,
> validation fails; do not raise the cap without a separate explicit decision.
>
> **Sequence:**
> 1. Implement #659 first only after normal approval; it remains restricted to `STATUS.md`.
> 2. Implement the decision-register work in independently reviewable slices.
> 3. First slice: schema, parser, validator, generated view, and planted-defect tests — no broad
>    migration and no agent behavioural claims.
> 4. Second slice: migrate the active/open/blocked/recently-decided set and resolve duplicate namespaces.
> 5. Third slice: queue-question gate.
> 6. Fourth slice: plan/execute decision-context enforcement and regression tests for the previously
>    repeated questions.
>
> Keep GraphRAG/QMD and external memory systems fenced. They are not prerequisites and do not become
> decision authority.

---

## Two things the coordinator found before briefing you

**C1 — the ruling's own keys are positional and collide.** The proposal already has `D1`–`D5` with
*different meanings*: proposal `D1` = enforcement archive-vs-ask, `D2` = register source shape,
`D3` = whole-file-vs-index generation, `D4` = status vocabulary, `D5` = the 22 ambiguous keys. The
ruling's `D1`–`D6` map across those non-bijectively (ruling `D3` = proposal `D4`; ruling `D2` covers
proposal `D2`+`D3`; ruling `D1` covers part of proposal `D5`). So **`D3` now has three possible
referents in this repo**, which is precisely what the ruling's own `D1` forbids. The coordinator
intends to namespace the ruling's decisions on recording rather than propagate the collision.

**C2 — the `resident: true` correction changes the budget arithmetic**, and the coordinator has not
yet re-derived it. The 8 KB cap was proposed on the assumption that the index holds *open rows only*
(~41 rows × ~120 B ≈ 4 920 B). The ruling adds `proposed` + `blocked` (already inside the open-ish set)
**and** a resident subset of `decided`/`implementing`. Today 39 rows carry `✅` in the key cell; if even
a third are resident that is ~+1.6 KB. The cap probably still holds, but its headroom is now a function
of a *curation* decision rather than of how many decisions are open — which was the property that made
the bound self-limiting.

---

## What each lens is asked

Answer only what your lens owns.

- **`young`** — `DECISIONS.md` becomes a generated view over declared records. Is that a projection in
  your sense, and does the ruling keep the write/read wall intact? The `decided` vs `realized` split:
  is that a legitimate distinction between a recorded fact and evidence of a fold having run, or is
  `realized` a status that will inevitably be derived from something other than the record itself?
- **`vernon`** — one file per record under `docs/decisions/**`, written by concurrent sessions. Is the
  consistency boundary right, and is there a transaction that spans two records (e.g. supersession,
  which writes both the superseded and the successor)?
- **`evans`** — C1 is yours. Also: "decision unit", "register row", "canonical record", "decision
  identity" and "active constraint" all appear in the ruling. Is that one vocabulary or several, and
  which term should the schema actually use?
- **`beck`** — slice 1 is "schema, parser, validator, generated view, planted-defect tests". What is the
  failing test for each, named before the code exists? And what test distinguishes `decided` from
  `realized` — i.e. what would go red if someone marked a record `realized` with no implementation?
- **`holub`** — your standing position is that nothing displaces #556 until one order flows end to end.
  Four slices of register machinery is real WIP. Does this ruling change your position, and if it does
  not, what is the smallest honest subset?
- **`dba` / `graphql-architect` / `ux-designer` / `business-specialist` / `legal-specialist` /
  `observability-agent` / `farley`** — likely "nothing in my lens", and that is a complete answer. If
  the `resident: true` high-risk subset (money, legal/compliance, security, irreversible data,
  production ops) touches your surface, say what belongs in it from your side.

**Fenced, do not propose**: GraphRAG, QMD, external memory, or any of it as decision authority.
No implementation in this chunk — this briefing produces records only.
