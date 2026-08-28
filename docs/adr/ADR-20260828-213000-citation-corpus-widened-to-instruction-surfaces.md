# ADR-20260828-213000 — The citation corpus widens to the instruction-surface class: `docs/dispatch/**`, `docs/claude/**`, `docs/PLAYBOOK.md`

**Status**: Accepted · **Date**: 2026-08-28 ·
**Decider**: the team, per its own register-row analysis, accelerated by the **FOUNDER / Tech CEO** ·
**Closes**: [`docs/decisions/DISPATCH-CARD-CITATION.yaml`](../decisions/DISPATCH-CARD-CITATION.yaml) ·
**Issue**: [#477 "Validator gate: no first-read doc may cite a SUPERSEDED ADR (CLAUDE.md +
docs/claude/\*\*)"](https://github.com/TheCaptainCompany/captain-food/issues/477) ·
**Session**: https://claude.ai/code/session_01Dbhq2Y7U5NcnqhByscaB4v

## Status

Accepted.

## The directive, verbatim

Founder, 2026-08-28: "work #477... ASAP, it influences agent behaviour."

## Context

`decision-superseded-authority` (`tools/codegen-rs/src/validate/decisions.rs`, shipped by PR #679,
hardened to a `make validate` error by ADR-20260827-081500 / `CITATION-RULE-LEVEL`) fails a tracked
file in `claude_citation_corpus` that cites a `superseded` decision-register row as live authority.
The corpus excludes `docs/**` wholesale, on the stated argument that "a record ABOUT a supersession
must name the superseded row" — an argument about ADRs and proposals, whose whole job is narrating
history.

`docs/decisions/DISPATCH-CARD-CITATION.yaml`, raised by review #59 of PR #679, named the gap: a
dispatch card under `docs/dispatch/**` is not that kind of document — it is an INSTRUCTION SURFACE a
session executes, the same property that puts `.claude/**` in the corpus — and the same exception,
with more weight, applies to `docs/claude/**` (CLAUDE.md's own "Topic authorities — read the
relevant one before working" routing, `sessions.md` marked OPERATIONAL) and `docs/PLAYBOOK.md`
(the same routing position). The row was LATENT, not live: no citing site existed in either subtree
at the time it was opened, so `make validate` stayed green while the corpus definition itself
understated its own scope.

## Decision

Option (a) from the row: widen `claude_citation_corpus` to include `docs/dispatch`, `docs/claude`
and `docs/PLAYBOOK.md` as three named exceptions to the general `docs/**` exclusion. The rest of
`docs/**` — ADRs, proposals, the register, `STATUS.md`, the journal — stays excluded: those
subtrees' whole job is narrating history, including citing superseded rows to explain what changed.
A citing site inside the three newly-in subtrees that is itself narrating a supersession (not
instructing a live action) uses the SAME clause-scoped `superseded` exemption every other
instruction surface already relies on — this decision adds three pathspecs, not a second exemption
mechanism.

**Verified against the real corpus (2026-08-28)**: after widening, `make validate` stays at 0
errors and the pre-existing 92-warning baseline is unmoved — zero live citations of a superseded
row exist today in `docs/dispatch/**`, `docs/claude/**` or `docs/PLAYBOOK.md`, matching the row's
own "grepped at the time of writing" finding.

**A scope gap surfaced while planting the red-first check, worth recording rather than silently
absorbing**: the issue that opened #477 asked for a gate on citing a superseded ADR **by id** (its
own `Status:` line), but the mechanism that actually shipped in PR #679 checks citations of
**decision-register row keys** (`docs/decisions/*.yaml`, `status: superseded`) — a different,
narrower universe that does not include ADR ids at all (no decision row is keyed by an ADR id, and
`validate_no_superseded_row_is_cited_as_authority` only ever searches for row `key` values). Planted
and confirmed empirically: citing `ADR-20260731-061609` (superseded IN PART, the issue's own
motivating example) inside a tracked `docs/claude/**` file does **not** red under the widened
corpus — `make validate` stays at 0 errors — while citing the one currently-superseded decision row
(`RETRIEVAL-QMD`) the same way reds immediately as a hard error. Issue #477's original ADR-id ask is
therefore still open; this change widens WHERE the shipped row-key rule looks, not WHAT it looks
for. Left for the architect to triage as a separate, correctly-scoped follow-up (or an explicit
decision that the row-key rule is the intended final form and the ADR-id ask is superseded by it).

**"Superseded IN PART"**: the shipped rule has no distinct IN-PART handling — it only tests
`status == "superseded"` on decision rows (a closed vocabulary with no `"superseded_in_part"`
value), and it never reads ADR `Status:` lines at all, so the IN-PART/hard-superseded distinction
described in the issue does not exist in the shipped mechanism. Noted per instruction rather than
added here — adding it would be new validator behaviour, out of this row's scope (a corpus
pathspec widening).

## Alternatives considered

- Option (b) — extend `validate_dispatch_card_rows` to also check row STATUS, not just resolution.
  Narrower, card-specific, and does not reach a card that names a row in prose rather than in its
  `Decision row:` envelope line — nor `docs/claude/**`/`docs/PLAYBOOK.md`, which carry no such
  envelope at all.
- Option (c) — leave it, on the grounds that the ask-gate resolves the row file at the point of
  need. Rejected: `make validate` green over a first-read doc pointing a session at a dead row is
  exactly the defect class #477 exists to close, and the row's own evidence already made the
  stronger case for `docs/claude/**` than for `docs/dispatch/**` alone.

## Enforced by

`the_records_state_the_same_citation_corpus_as_the_code` reads the pathspecs out of
`claude_citation_corpus` and reds until both `docs/decisions/RETRIEVAL-QMD-CI.yaml` (clause d) and
`docs/adr/ADR-20260824-205911-...md` name every one of them — both updated in this change.
`a_superseded_row_may_not_be_cited_as_live_authority` and the wider `decision-superseded-authority`
suite are unchanged in level (`err`, per `CITATION-RULE-LEVEL`) and now execute over the widened
`git ls-files` pathspec list.

## Consequences

### Positive
- A stale row-key citation inside `docs/dispatch/**`, `docs/claude/**` or `docs/PLAYBOOK.md` now
  reds `make validate` instead of passing silently — closing the latent gap the register row named.
- No behaviour change on the real tree today: 0 new errors, 0 new warnings.

### Negative
- None observed. The general `docs/**` narration exclusion is unchanged for every other subtree.

### Follow-up actions
- Issue #477's original ADR-id ask (citing a superseded ADR by its own `Status:` line) remains
  unimplemented by the shipped row-key mechanism; the architect should decide whether to file it as
  new, correctly-scoped work or close it as superseded by the row-key rule's narrower but
  operative coverage.
