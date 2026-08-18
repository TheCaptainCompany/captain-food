# ADR-20260818-193000 — The decision register is a QUEUE: a closed row collapses to its record, and a section number is an anchor that is never reused

**Status**: Accepted · **Date**: 2026-08-18 ·
**Decider**: the **FOUNDER / Tech CEO**, asked whether the register is still relevant, whether it can
be reduced, and what is still open — answering *"Do what's best for us"* ·
**Register**: [DECISIONS](../proposals/DECISIONS.md) — the whole page, restructured by this change ·
**Relates**:
[ADR-20260801-020000](ADR-20260801-020000-proposals-are-living-documents.md) (proposals are LIVING —
the same discipline, applied to the register) ·
[ADR-20260813-233418](ADR-20260813-233418-recorded-intent-must-execute-itself-the-anti-repeat-mechanisms.md)
(recorded intent must execute itself) ·
[ADR-20260803-234035](ADR-20260803-234035-compiler-first-a-check-is-the-fallback.md) (a check where
types cannot reach) ·
[ADR-20260724-135945](ADR-20260724-135945-proposals-are-committed-to-the-repo.md) ·
**Session**: https://claude.ai/code/session_01CjwEKYG9EwBuhKR2CBGqAZ

## Status

Accepted, and landed in the same change.

## Enforced by

`register_section_numbers_are_unique` (`tools/codegen-rs/src/validate/proposals.rs`, §13c) — a
duplicate `## NN.` section number in `docs/proposals/DECISIONS.md` is an **ERROR** and fails
`make validate`. It is a check rather than a type because the register is markdown, which no compiler
reads.

No `rules.yaml` entry: this records a guarantee about a **record**, not about runtime behaviour.

## Context

The register was created to be a queue. Its own first line says so: *"Every decision the proposals
are waiting on, in one place… If a decision is not here, it is not blocking anything."* The
`architect` enforces it as the pipeline throttle, 83 files reference it, and 20 of them cite it by
section number.

It had stopped being a queue. Measured on `main` at 2026-08-18, before this change:

- **579 KB across 2540 lines.** The reconciliation header alone was **58.7 KB** — three stacked
  "Last reconciled" blocks, one of them a single **33 KB line** inside a `<details>` element, each
  appended rather than amended. That is precisely the *"appended superseded blocks"* pattern
  [ADR-20260801-020000](ADR-20260801-020000-proposals-are-living-documents.md) forbids for
  proposals, arrived at independently on the register.
- **1383 of 2540 lines sat in sections with no open row at all** — decided history, nearly all of it
  already recorded in an ADR the row itself links to. Against ~24 open rows, roughly nine tenths of
  the page was not queue.
- **The live rows were unfindable.** They ran from line 971 to line 2532, while §1 — titled *"Decide
  these first"* — had carried no open row since 2026-08-11 and opened the page with 47 lines of
  closed history.
- **Two section numbers were duplicated**: two `§37` (*Recorded intent* / *Strix*) and two `§42`
  (*A process manager is a write-side component* / *Reader-set derivation carry-forwards*). Four
  external records cite `§42` meaning the first, one cites it meaning the second. A cited anchor
  that resolves to two different sections is a broken reference that no gate could see.
- **Two sections contradicted themselves**: §16 and §18 carry headings reading `SUPERSEDED` and
  `DECIDED` above tables whose every `Answer` cell still reads `_(open)_`.
- **It was stale.** Two founder-answer commits — `7bdd808` (ADR-20260818-094500, three rulings) and
  `10866d6` (ADR-20260818-101500, the cleared queue) — landed *after* the last reconciliation and
  never reached the page. **STAFF-AUTH was still shown 🟠 OPEN, FOUNDER-OWNED, nine hours after the
  founder answered it.**

That last point is the one that matters, and it is a failure mode rather than an untidiness. **A
queue nobody can read is a queue nobody works.** The page's stated purpose makes its own silence
load-bearing: *"if a decision is not here, it is not blocking anything"* — so a row that is answered
but still shown open, or open but buried at line 2349, misinforms every session that reads it, and
the `architect` classifies issues 🔴 RED from exactly this surface.

## Decision

**The register is a queue, and its shape is a rule rather than a preference.**

1. **The open queue is the product of the page.** A table at the top indexes every live row — the
   question in one line, and a link to the section that holds its argument. It is an index, not a
   second home: nothing is decided there, and a row leaves it only by being answered below.
2. **A closed row collapses to its outcome plus the record that holds the reasoning.** The argument
   belongs in the ADR and the proposal, the history belongs in git. Where a closed row carries a
   fact that outlives the decision — a measured defect, a correction of the register's own text, an
   accepted consequence — that fact is carried forward in the collapsed form. Nothing else is.
3. **No appended history.** Amend in place, as proposals already do. No "previously" blocks, no
   stacked reconciliation headers, no `<details>` archive of prior headers.
4. **A section number is an anchor other records cite. It is never reused, and never renumbered
   without grepping `docs/**` and `.claude/**` for `DECISIONS §NN` in the same change.**

**Every OPEN row was preserved byte-exact** by this change — lifted by line number, not retyped — so
no live argument was paraphrased away in the course of shortening the page.

**The two duplicate numbers are resolved by renumbering the less-cited of each pair**, each carrying
a visible numbering note: Strix `§37` → **`§47`** (no external record cited it), and Reader-set
derivation `§42` → **`§48`** (one citation, `docs/STATUS.md`, updated in this change).

**The staleness is repaired**: STAFF-AUTH is answered for the rider and the restaurant, a new §49
records the three rulings of 2026-08-18 and the cleared queue, and the residue the rulings did not
cover — **account-manager sign-in** — is opened as its own founder-owed row rather than left implied.

Result: **579 KB → 216 KB**, 2540 → 1225 lines, with the live queue on the first screen.

## Alternatives considered

- **Option A — move closed sections to a `DECISIONS-ARCHIVE.md`, leaving anchor stubs.** Rejected.
  83 files link into this page; a split doubles the number of places a reference can rot, and the
  stubs would have to be maintained forever to keep the anchors alive. Collapsing in place keeps
  every anchor valid with no second file to drift.
- **Option B — leave it and only fix the staleness.** Rejected on the measured finding: the page had
  already gone nine hours out of date on a founder answer *while being nominally maintained*. The
  cost of reconciling it had grown with its size, which is the mechanism that produces staleness, so
  fixing only the symptom guarantees the recurrence.
- **Option C — delete the closed sections outright.** Rejected. Section numbers are cited by 20
  records, and several closed rows carry facts that outlive their decision (§35 DB-HA's unpriced
  60 Gi, §31's uncomputed ETA, §24's *nobody is told about a paid order*). Deletion would drop
  those silently.
- **Option D — a validator rule that caps the file's size.** Rejected as the wrong instrument: it
  would fire on legitimate growth in the open queue, which is the part that should be allowed to
  grow, and it says nothing about the failure that actually happened.

## Consequences

### Positive

- The live queue is visible on the first screen instead of scattered across 1500 lines, so a session
  can tell in one read what is owed and by whom.
- Reconciling the page is now cheap, which is what makes it likely to happen. The cost of an update
  scaled with the file; that coupling is broken.
- The duplicate-number ambiguity is gone and cannot recur — a duplicate now fails `make validate`.
- Two self-contradicting sections (§16, §18) are resolved rather than left for a reader to notice.
- Four founder rulings that had never reached the register are now recorded in it.

### Negative

- **The collapse is lossy by design, and the loss is real.** Per-lens argument on closed rows now
  lives only in the ADRs, the proposals and git history. A reader who wants the full reasoning behind
  a closed decision must follow the link rather than read it in place. This is the intended trade —
  but it is a trade, and if a collapsed row turns out to have carried a load-bearing fact, restore
  that fact rather than reverting the shape.
- **Anchors changed for the two renumbered sections.** `docs/STATUS.md` was updated; any future
  reference to Strix or reader-set derivation by their old numbers is wrong.
- **The gate is narrow.** It catches duplicate section numbers and nothing else. It does not detect
  a stale row, an unreconciled ADR, or an appended history block — those remain prose discipline.

### Follow-up actions

- The `architect` reconciles the register on each run under the collapsed shape; the Maintenance
  section states it.
- A stronger check — *"every ADR of the last N days is either cited by the register or explicitly
  exempt"* — would have caught the STAFF-AUTH staleness mechanically, and would have caught the four
  2026-08-16 lane ADRs that are also uncited. It is not built here because the exemption predicate
  is not obvious (most engineering ADRs owe the register nothing), and a check with a guessed
  predicate is worse than none. Recorded as the next candidate if staleness recurs.
