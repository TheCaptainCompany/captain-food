# Review packet — the `STATUS.md` split (and the `sessions.md` split it builds on)

- **Date**: 2026-08-19
- **Branch**: `claude/agent-decision-retrieval-w55ey3`
- **Review base**: `a981c50` (= `origin/main` at the time of writing). Local `main` is `900994b` and
  has **no merge base** with this branch — review against `a981c50..HEAD`, not against `main`.
- **Commits under review**: `e7486c3` (sessions.md split), `3a207eb` (STATUS.md split)
- **Reversibility class**: `REVERSIBLE INTERNAL` — docs only. No stored event shape, no money path,
  no legal surface, nothing Tours-facing. Per ADR-20260816-134352 this sizes to a **2–3 lens** review.
- **Who wrote it**: the coordinator session, unassisted.
- **Review status**: **COMPLETE — see §8.** `reviewer` returned **FAIL** with nine findings;
  `beck` confirmed the same defect class independently. All findings were corrected before the
  follow-up commit. This packet is retained as the record of what was asked and what came back.

---

## 1. What changed, and why

The six-file boot reading order in `docs/claude/autonomous-run.md` §"Ground yourself first" measured
**1 441 655 B** at `a981c50` — larger than the context window. No session had ever completed steps 2
and 3, and every session reported grounding itself. Two of those files were split so the instruction
becomes executable.

| File | Before | After | Move |
|---|---:|---:|---|
| `docs/claude/sessions.md` | 133 802 B | 9 777 B | index at same path → 4 topic files in `docs/claude/sessions/` |
| `docs/STATUS.md` | 628 654 B (`a981c50`) | ~33 KB | current state at same path → 220 journal entries into 5 ISO-week files in `docs/status/` |
| **Boot order total** | **1 441 655 B** | **722 124 B** | −50% |

`docs/proposals/DECISIONS.md` (631 346 B) was **deliberately not touched** — it is the subject of
`PROP-20260819-110442`, `Proposed` with D1–D5 open. It is now **87%** of the remaining boot cost.

---

## 2. Changed files

**Structural (the substance of the review)**
- `docs/STATUS.md` — rewritten: 10 durable sections hoisted above the fold, journal replaced by a
  one-line index of the current + preceding ISO week, plus an archive table.
- `docs/status/journal-2026-W30..W34.md` — **new**, 5 files, 615 608 B, the extracted journal.
- `docs/claude/sessions.md` — rewritten as an index; `docs/claude/sessions/{gates,environment,evidence,workflow}.md` — **new**.
- `docs/adr/ADR-20260819-174300-status-md-is-current-state-the-journal-moves-to-iso-week-files.md` — **new**.

**Incidental**
- `docs/adr/README.md` — one index row added.
- `docs/claude/autonomous-run.md` — boot steps 2 and 5 rewritten to say *read the index, fetch the part you need*.
- `docs/legal/BRIEF-20260811-erasure-zone-and-retention.md` (3 citations), `docs/proposals/PROP-20260811-093000-…md` (1 citation) — repointed.
- `crates/db_test_gate/src/lib.rs`, `crates/adapters/stripe/tests/journal_leak_canary.rs` — one doc-comment path each.

---

## 3. Invariants the change must not break

| # | Invariant | How it was checked |
|---|---|---|
| **I1** | No rule and no journal entry is lost. Content is byte-identical in its new home. | Multiset comparison of non-blank lines, original vs split, run before **and** after the move |
| **I2** | Both files stay at their original paths, so existing citations resolve. | 53 `sessions.md` citations, all file-level; `STATUS.md` unmoved |
| **I3** | `§N` numbering in `sessions.md` is preserved — `§2`, `§8b`, `§18` are cited across STATUS, ADRs and proposals | Heading text carried verbatim; only unnumbered `###` were promoted |
| **I4** | Every relative link in a moved file still resolves from its new depth. | `os.path.exists` walk over every non-`http` link target |
| **I5** | The 10 durable `STATUS.md` sections survive verbatim, headings and anchors included. | 129 non-blank lines, set-membership check against the new file |
| **I6** | Journal entries keep their **original written order** within a week. Bucketing used each entry's own date; nothing was re-sorted. | By construction — extraction preserves index order |
| **I7** | No generated artifact changed. | `make generate` then `git status` → only hand-edited docs dirty |

---

## 4. Acceptance criteria

1. All four proofs green (§5 of the final report): extraction, durable sections, links, gates.
2. `make validate` — 0 errors, warning ratchet unmoved.
3. `make rust` — exit 0, `check-drift` clean on a committed tree.
4. A session opening `docs/STATUS.md` reaches "🔐 Authorization" and "🧭 Architecture decisions"
   without scrolling past a journal.
5. A session told to record state can determine **where to write** from the file itself.

---

## 5. Known risks and residue — already recorded, please challenge

- **R1 — `2026-W33` is 271 611 B.** One archive file nobody can read end to end. Argued acceptable
  because it is fetched on purpose, never at boot. *Is that argument good enough, or does the week
  bucket need a size cap?*
- **R2 — stale `STATUS.md:NNNN` citations.** These were **already broken before this change**
  (`STATUS.md:44`, cited for the erasure-window quote, pointed at a 2026-08-19 ADR-volume entry at
  `HEAD`). Four were repointed on a **verified unique quote match**; **eight were left** — including
  all ADR sites, on the reasoning that a historical record's citation must not be repointed on a
  mapping derived from a file state it never referred to. *Is that the right call, or is a broken
  pointer worse than an approximate one?*
- **R3 — the eight date inversions were preserved, not corrected.** Entries are in the right week;
  within a week the order is nearly but not strictly reverse-chronological.
- **R4 — two dangling ADR links found in `PROP-20260811-093000`** (pointing at a renamed file),
  pre-existing at `HEAD`, recorded and **not** fixed as out of scope.
- **R5 — concentration.** The change makes `DECISIONS.md` the single dominant unread file rather than
  one of two. Intended, but it is a real consequence.
- **R6 — the index could rot.** `STATUS.md`'s recent-changes index is hand-maintained alongside the
  week files. Nothing enforces that a new week entry gets its index row. This is the same decay class
  the whole exercise is about.

---

## 6. Specific questions for the review

**For `reviewer` (independent full-diff pass):**
1. Read the new `docs/STATUS.md` **top-of-file** (first ~40 lines). Does a cold session learn what the
   system is, and where to write a new entry, without opening another file? Name what is missing.
2. Verify I1 and I5 independently — do not trust my proof scripts. Re-derive the check your own way
   and say whether it agrees.
3. Is the ADR's follow-up section an honest and complete account of the residue, or does it bury
   something a future session will pay for?
4. `sessions.md`: I promoted 18 `###` sections, 16 of which had drifted under `§17`. Did any promotion change
   the meaning or the authority of a rule?
5. Anything in the diff that is **not** what the commit messages say it is.

**For `beck` (testing lens):**
1. **R6 is the one I most want challenged.** What test or gate would fail if a session wrote a journal
   entry to the week file and forgot the index row — or wrote it to `STATUS.md` the old way? Right now:
   nothing. Is that acceptable residue for a docs change, or is it the same defect class in a new place?
2. My four proofs are ad-hoc Python run once, not committed tests. One of them had a real bug — a
   negative lookahead that failed to un-rewrite `](../SECURITY.md)`, which I caught and fixed. **A
   proof that had a bug in it is exactly a proof nobody has seen red.** Should any of these become a
   committed check, and if so which one earns its keep?
3. Is there a cheap executable invariant here that would have caught a bad split *before* the commit,
   rather than a one-shot script after it?

---

## 7. What is fenced — do not propose work inside these

Per the founder, 2026-08-19: no changes to `DECISIONS.md`, no YAML decision records, no decision
index, no QMD/GraphRAG, no librarian agent or skill, no change to agent enforcement. Those are
pending his D1–D5 ruling on `PROP-20260819-110442`. **A finding that lands inside a fence should be
reported as a finding, not as a proposed change.**

---

## 8. Review outcome (added after the pass)

Two lenses read the diff on 2026-08-19: `reviewer` (independent full-diff) and `beck` (testing).

**`reviewer`: FAIL, nine findings.** The structural claim held — it re-derived I1 and I5 by its own
method and agreed, and independently confirmed I4, I6 and I7. Every failure was in **newly-authored
derived text**, not in the extraction:

| # | Finding | Status |
|---|---|---|
| F1 | Entry counts wrong: 219→**220**, W34 20→**21**, index 97→**98** rows | fixed |
| F2 | Five size figures wrong; `STATUS.md`'s "before" quoted at two different SHAs as one quantity | fixed — SHA now named, self-measured sizes removed |
| F3 | **Five index rows corrupted** by the headline extractor: `` `specs/**` `` truncated the bold early, `orders_placed_total` lost its underscores and was rendered as a *wrong identifier*, nested `**ALREADY**` truncated a GDPR entry, `(night)`/`(evening)` prefixes leaked | fixed — extractor rewritten to mask code spans and find the true outer bold; all 220 rows regenerated |
| F4 | Repointing a legal brief left `:2230`/`:2014` orphaned under a NEW antecedent, reading as line numbers in the journal file where they mean nothing | fixed — sub-refs removed, provenance stated |
| F5 | The `sessions.md:NNNN` residue class (5 sites in `PROP-20260818-013222`) was created by this change and omitted from the residue account | fixed — recorded in the ADR follow-up |
| F6 | The residue list said "eight" and enumerated seven | fixed — `ADR-20260819-103112:494` added |
| F7 | "eleven durable sections" is **ten** | fixed in ADR, README row, packet |
| F8 | Hoisted durable sections are materially stale (Architecture decisions says latest is 2026-08-02) and the change was silent about it | recorded in the ADR consequences; refresh is out of scope |
| F9 | Three incompatible promotion counts published (4 / 22 / 22); actual is **18** | fixed |

**`beck`: the same defect class, found independently and named structurally.** R6 did not merely
remain a risk — *it went red inside the commit under review*, because the split's own journal entry is
appended to the corpus whose measurements that entry states. A self-measuring document cannot be kept
consistent by discipline. `beck` also verified what did **not** rot: date-multiset equality between
index rows and week-file entries held for both indexed weeks, and `STATUS.md` carries zero
entry-openers — the writer followed the new write path; only the arithmetic failed.

**Both lenses converged on the same executable remedy**, which is **not built** because the founder
scoped this chunk to the existing gates: a `tools/codegen-rs/src/validate/status.rs` (~120 lines,
modelled on `validate/proposals.rs`, planted-defect template at `tools/codegen-rs/src/tests.rs:6367`)
asserting (T1) declared count == parsed entries == index rows per indexed week, (T2) zero entry-openers
in `STATUS.md`, (T3) every entry sits in the ISO week its date belongs to. `beck` measured T3 at 0
misbucketed across 220 entries and T2 green today; **T1 was red on arrival.**

**The lesson worth keeping**: all four original proofs were green and **none could ever have gone red
on F1 or F3**, because they checked extraction fidelity and link resolution — and the index rows are
new derived text no proof covered. The proofs were built around the risk already known.
