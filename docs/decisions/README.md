# docs/decisions/ — the decision register's declaration site

One file per register row key (REG-2(a)), a closed status vocabulary (REG-4(a)), and the generated
index in [DECISIONS.md](../proposals/DECISIONS.md) (REG-3(a)) — decided by founder directive
2026-08-21, design in
[PROP-20260819-110442](../proposals/PROP-20260819-110442-the-decision-register-is-the-unit-of-decision.md),
record: the ADR named in the git history of this file. `make validate` enforces everything below
(§22, `tools/codegen-rs/src/validate/decisions.rs`); the register-check hook
(`.claude/hooks/register-check.sh`) reads these FILES — never the generated index — to refuse a
founder question about a row that is not open.

## The schema — flat scalars, closed sets, unknown fields are errors

```yaml
key: "CAPTAINNET-ZERO"        # == filename stem; v1 grammar ^[A-Z][A-Z0-9-]{2,63}$,
                              #    no `--` (reserved for the future D1–D7 namespacing), no trailing -
status: "open"                # open | decided | deferred | superseded | withdrawn (closed set)
question: "One line, phrased as the ANSWERABLE question — never a topic label."
owner: "founder"              # founder | team | counsel | external — who owes the NEXT MOVE on the
                              #   ANSWER; counsel/external mark rows the founder cannot act on
opened: "2026-08-18"          # YYYY-MM-DD — when the ROW was opened (kept even after closing)
register: "docs/proposals/DECISIONS.md §47"   # pointer to the authoritative prose (the history)
evidence: "verbatim quote from that prose"    # so the extraction is reviewable against the source
# status-coupled fields (biconditional — presence also constrains status):
decided: "2026-08-19"         # required iff decided|superseded
decided_by: "ADR-20260819-103112"  # required iff decided|superseded; must RESOLVE to a file under
                              #   docs/adr/ or docs/proposals/ (optional but resolved on withdrawn)
superseded_by: "OTHER-KEY"    # required iff superseded; another DECLARED key; DAG, no cycles
until: "after one order flows end to end (#556)"  # required iff deferred — the wake condition
note: "free text"             # required iff withdrawn (why the question stopped being a question)
capacity: "founder"           # optional: founder | team | counsel | architect — who decided
```

**`decided` is a recorded decision, never legal clearance** — on a legal-exposed row the `capacity`
field keeps visible in what capacity it was taken, and a counsel-gated row stays `open` with
`owner: counsel` (the founder question it licenses is about the external action, never the answer).

## Semantics that are not in the schema

- **Authority split**: for a key with a file here, the file is authoritative for **CURRENT
  status**; the prose row in DECISIONS.md is its **history** (its glyphs are not current status).
  Temporal questions are answered by declared fields, never by parsing git history.
- **Partial closure = split at close time.** The closed vocabulary cannot say "half decided" on
  purpose: close the row and open a NEW key for the residue in the same change (the pattern:
  REFUND-BEARER's residue → CAPTAINNET-ZERO; REG-4's namespacing half → KEY-NAMESPACE).
- **Reversal, never re-ask**: a decided row whose answer's premise has changed is reopened by a
  NEW row citing the old key — the old file is never flipped back to `open`.
- **Supersession is a two-file change**: the old row flips to `superseded` + `superseded_by`, the
  successor file is created, both in one commit (the validator checks resolvability per commit).
- **Legacy** ([`_legacy.yaml`](_legacy.yaml)) is the closed allowlist of prose-only rows. A key
  migrates when a dispatch explicitly includes it ("touch" is decided at dispatch time — citing a
  legacy row is not a touch), leaving the allowlist in the same change.

## Editing discipline

Any change here is a **generating** change: run `make generate` (or `make rust`) in the same
commit — the DECISIONS.md index region regenerates and `check-drift` is red otherwise, including
on the docs-straight-to-main path. Resolve any merge conflict inside the generated region by
regenerating, never by hand-merging. Closing a row from a dispatch: the card names the key; the
closing commit edits the file (status + `decided` + `decided_by`) and regenerates.
