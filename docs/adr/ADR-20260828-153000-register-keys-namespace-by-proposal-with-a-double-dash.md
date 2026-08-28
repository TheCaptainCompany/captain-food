# ADR-20260828-153000 — Register keys namespace by proposal, with `--` as the separator

**Status**: Accepted · **Date**: 2026-08-28 ·
**Decider**: the team (executing under the founder's ASAP directive on
[#658 "The decision register cannot say what is still open: 62 of 148 rows carry no status token, 22
keys are ambiguous, and nothing confronts a question with the register before it reaches the
founder"](https://github.com/TheCaptainCompany/captain-food/issues/658)) ·
**Consulted**: architect (dispatched this chunk from the #658 batch) — this is the last outstanding
row of that batch, not a fresh option space; the roster invitation happened at the batch relay,
2026-08-28 ·
**Realizes**: `docs/decisions/KEY-NAMESPACE.yaml` (closed by this record) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Context

`docs/decisions/README.md`'s v1 key grammar forbade `--` outright, reserving it for exactly this
question: PROP-20260819-110442 measured `docs/proposals/DECISIONS.md` at 148 row-anchored keys, only
126 of them unique — **22 duplicates, all in the per-proposal `D1`–`D7` family**, so a bare `D1`
names seven different decisions depending on which proposal's table it sits in (D5's own evidence:
*"Namespace them by proposal: `PROP-20260809-003000/D1`"* — recommended, with `/` ruled out because
it is illegal in both the key grammar and a filename).

## Decision

1. **The namespaced form is `<PROPOSAL-ID>--<LOCAL>`** — e.g. `PROP-20260809-003000--D1`. `--` is
   the one reserved separator; at most one per key. A key with zero `--` follows the unchanged v1
   grammar (`^[A-Z][A-Z0-9-]{2,63}$`, no trailing `-`) — so a bare 2-character local name like `D1`
   is **still illegal on its own**, unchanged from before, because it fails the length floor before
   namespacing is even considered.
2. **The namespace half must look like a `PROP-YYYYMMDD-HHMMSS` stamp** (syntax, checked with no
   disk access) **and must resolve to a committed `docs/proposals/PROP-*.md` file** (semantics,
   checked with the same injected resolver `decided_by` already uses). A syntactically-shaped but
   non-resolving namespace is `decision-key-namespace-dangling` — a namespace is migrated (the
   proposal exists) before a key points at it, same discipline as `superseded_by`.
3. **Two separators, or an empty/hyphen-adjacent half, is `decision-key-grammar`** — `--` is a
   singular reserved separator, never a general delimiter.
4. Implemented in `tools/codegen-rs/src/validate/decisions.rs` (`valid_key` for the syntax half, a
   new check in `validate_decision_rows` for the resolution half), red-first: a planted bare
   ambiguous key, a planted double-separator key, and a planted dangling-namespace key each proved
   red before the green corpus was asserted.

## Migration executed in the same change

Verified count (not re-quoted from the dispatch's "22" without checking it, per
ADR-20260817-105845): the "22 duplicates" figure is `148 total D1–D7-style keys, 126 unique` =
`29 occurrences of 7 distinct name strings, minus the 7 first-seen = 22 surplus` — a corpus-health
STATISTIC over occurrences, not a curated list of 22 specific rows to move while leaving the other 7
untouched (there is no non-arbitrary way to pick which single occurrence of `D1` is "the real one").
All physical occurrences of the family need namespacing to stop colliding.

Of the four proposals whose tables use bare `D1`–`D7` as their entire numbering scheme:

- **`PROP-20260809-003000` (§23, 7 keys)** and **`PROP-20260809-021351` (§24, 6 keys)** are
  migrated to full `docs/decisions/<KEY>.yaml` files in this change — both have a clean, single,
  dated closing record (`ADR-20260809-050000`) with no content since superseded.
- **`PROP-20260810-234225` (§27, 7 keys)** and **`PROP-20260811-000946` (§28, 7 keys)** are
  **deliberately NOT migrated here**: §27bis (`MET-R`, closed 2026-08-11) records that the
  `DECISIONS.md` §27 table's own D4/D6 text was **reversed** by the projection-vs-instrument
  decision (*"D4, D6, D8 and D9 now recommend the projection approach; the generated-instrument
  option is recorded there as rejected"*) — and §28 cross-references §27 D6/D7 for its own D3/D4.
  Promoting these fourteen keys from the stale §27/§28 table text as `decided` would assert a
  superseded technical position as the register's current authority — a stronger and more concrete
  failure than the ownership-ambiguity case `docs/decisions/README.md` already tells an executor not
  to guess through, so the same discipline applies: left un-migrated, reported here, needs a
  dedicated pass that reconciles the DECISIONS.md summary against the source proposals
  (`PROP-20260810-234225`, `PROP-20260811-000946`) and 27bis before promotion, not a mechanical
  namespace-and-copy.
- Two standalone bold `**D5**` rows (§6 `PROP-20260726-201500`, superseded proposal; §8
  `PROP-20260728-120931`, already closed 2026-07-28) are single-highlight artifacts inside tables
  whose `D1`–`D4` are NOT bolded — not evidence of a real `D1`–`D7` numbering family at those two
  sites, and excluded on that basis rather than migrated as a stray single key while its siblings
  stay bare.

Thirteen files land in this change: `docs/decisions/PROP-20260809-003000--D{1..7}.yaml` (all
`decided`, `decided_by: ADR-20260809-050000`) and `docs/decisions/PROP-20260809-021351--D{1,3,4}.yaml`
(`decided`, same record — the answer sheet chose an option that in D1's case differs from the
table's original recommendation) plus `--D{2,5,6}.yaml` (`withdrawn` — the proposal's own text says
these three "lapse with the deferral").

None of the fourteen migrated/promoted keys had any prior citation anywhere in the repo as a formal
register reference (`rg` for `Decision row: D[1-7]`, `row D[1-7]`, `reconsiders: .D[1-7].` returns
zero hits outside this same table) — the "update every citation" step of the dispatch found nothing
to update, because these keys were never citable before this change; they existed only as row labels
scoped to their own proposal's own table.

## Consequences

- `docs/decisions/KEY-NAMESPACE.yaml` closes `decided`, `decided_by: ADR-20260828-153000`.
- `docs/decisions/README.md`'s schema block is updated to describe the v2 grammar.
  `docs/decisions/_legacy.yaml` is untouched, in content and in comments — it never listed the
  `D1`–`D7` family (the length floor already excluded a bare `D1` from its own extraction regex),
  so none of the fourteen migrated keys nor the fourteen deferred ones were ever on that allowlist.
- A follow-up chunk is owed for `PROP-20260810-234225`/`PROP-20260811-000946`'s `D1`–`D7` — reported
  to the architect rather than actioned here.
