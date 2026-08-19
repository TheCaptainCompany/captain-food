# ADR-20260819-174300 — `STATUS.md` is current state; the journal moves to ISO-week files

## Status

Accepted

## Enforced by

n/a — no behavioral guarantee

## Context

`docs/claude/autonomous-run.md` §"Ground yourself first" names six files every session must read, in
order, before any work. Measured at `a981c50`, those six files were **1 441 655 B** — about 272 000
tokens at the conservative 1.4 tokens/word factor PROP-20260819-110442 §1.1 uses. That is larger than
the context window. **No session has ever completed steps 2 and 3, and every session reported that it
had grounded itself.**

`docs/STATUS.md` was **628 654 B** of that (measured at `a981c50`; it was 631 566 B at `e7486c3`,
after the `sessions.md`-split entry was appended — both figures are true of different commits, and
quoting them interchangeably was a defect this ADR previously carried), and its problem was not only size. It was a
reverse-chronological journal: at `a981c50` lines 5–6 740 held **219 entries** (220 once this
change appended its own), newest first, and the ten
sections that describe what the system *is* — Deployment, Read side, Write side, Authorization,
SIRENE prospection, External integrations, Ops actions, Remaining work, Production suspended,
Architecture decisions — sat at **lines 6 741–6 895 of 6 895**. A session reading top-down truncated
somewhere in early August and never reached them. **A file ordered that way answers a question only
to a reader who never gets to the answer**, which is a second, independent mechanism for the founder's
2026-08-19 question (*"do the agents ask questions the ADRs already answered?"*) alongside the one
PROP-20260819-110442 §1.5 identifies. It does not contradict that finding — that proposal showed the
re-litigated ADR was one `grep` away and volume was not its cause. Here the record genuinely was
unreadable.

Two properties made this cheap rather than a rewrite. The journal span contained **zero non-blockquote
content** — it is a pure sequence of entries, each carrying a parseable date on its own first line —
and the entries were already keyed by date, so bucketing needed no judgement about content.

This ADR records a decision the founder took from a presented option space (retention window, bucket
granularity, and whether to execute now), which is why it is an ADR and not a proposal —
CLAUDE.md's proportionality rule, middle case.

## Decision

**`docs/STATUS.md` is current state, not a journal.** It opens with the ten durable sections,
carries a *Recent changes* index at the bottom, and is **~33 KB** — a file a session reads end to
end.

1. **The journal lives in `docs/status/journal-<ISO-week>.md`**, one file per ISO week, entries
   byte-identical and in their original written order. Five files today: `2026-W30` … `2026-W34`,
   **615 608 B** in total, **220 entries** — measured at the commit that lands this ADR.
2. **`STATUS.md` keeps a one-line index for the current and preceding ISO week only** — 98 rows,
   date + headline, under a heading that links to the week file. Not the full current week: at
   70 102 B for `2026-W34` alone, keeping it inline would have restored the journal as the main body
   and defeated the objective.
3. **ISO week, not calendar month.** Week matches `.claude/loop-budget/<ISO-week>/`, adopted under
   ADR-20260812-011057 because append-only per-period files never conflict between concurrent
   sessions — the same pressure applies here, since `DECISIONS.md` and `STATUS.md` are written by
   several sessions a day. Calendar months would have produced 166 KB and 430 KB retrieval blobs,
   reproducing the present problem at a slower cadence.
4. **Writing state**: append the entry to the CURRENT week file, newest first, and add its one-line
   row to *Recent changes*. Update the durable sections in place. This is stated in `STATUS.md`'s own
   header, where a writer sees it, and in `autonomous-run.md` boot step 2.

**Two fences held, and they are part of the decision.** `docs/proposals/DECISIONS.md` (631 346 B,
88% of the remaining boot cost) was **not touched**: it is the subject of PROP-20260819-110442, which
is `Proposed` with D1–D5 open, and splitting it would pre-empt a decision the founder has not made.
No retrieval architecture was built — no QMD, no librarian agent, no ADR frontmatter — per the
founder's 2026-08-18 deferral of #643 (*"we will not apply it yet we will finish what we have started
first"*).

## Alternatives considered

- **Keep the full current week inline (70 KB).** Rejected by the founder: *"A 70 KB 'current week'
  defeats the stated boot-context objective."*
- **Keep only a pointer, no index (~0.5 KB).** Smallest, but "what changed recently" then costs a
  second file open on every boot — the one journal question a session actually has.
- **Calendar-month buckets.** Two files instead of five, but 166 KB / 430 KB each. Rejected: *"repeats
  the present problem at a slower cadence."*
- **Reorder only — move the durable sections to the top, leave the journal below.** Fixes the ordering
  defect for free and nothing else; the file stays 631 KB and step 2 stays unexecutable.
- **Do nothing until #556 lands.** `holub`'s standing position is that nothing displaces the local
  acceptance harness. Overruled for this item only, on the ground that it is docs-only, needs no
  dispatch, and every session pays the boot cost until it lands.

## Consequences

### Positive
- The boot reading order drops from **1 441 655 B → 722 124 B** (−50%), measured at the landing commit, and `STATUS.md` from
  628 654 B (`a981c50`) → **33 KB** (−94.7%). The exact post-split size is deliberately
  not stated here: this ADR is itself indexed by the file it would be measuring, so any byte figure
  written into the corpus is stale the moment the next entry lands. See the follow-up.
- A session now reads what the system IS before anything else. Authorization and Architecture
  decisions are on the first screen instead of behind 6 700 lines.
- Concurrent sessions stop contending on `STATUS.md`'s head. Entries land in the week file; only the
  one-line index row touches the shared page.
- The move was proved, not asserted: extraction is byte-identical (6 536 non-blank journal lines,
  exact multiset match), all 129 durable-section lines survive, and all 252 rewritten relative links
  resolve.

### Negative
- **`DECISIONS.md` is now 88% of the remaining boot cost.** This change makes the register the single
  dominant unread file rather than one of two, and it is fenced until D1–D5 are answered.
- Reading a full journal entry now costs a second file open.
- `2026-W33` is 271 611 B — an archive file no one can read end to end. Acceptable because it is
  fetched on purpose, never at boot, but it is not solved.
- **The eight local date inversions in the original journal were preserved, not corrected.** Bucketing
  used each entry's own date, so entries are in the right week; within a week they keep their written
  order, which is nearly but not strictly reverse-chronological.

### Follow-up actions
- **Stale `STATUS.md:NNNN` citations are a pre-existing defect, now recorded rather than silently
  repaired.** They were already broken before this change: `docs/STATUS.md:44`, cited by
  `BRIEF-20260811` for *"the window is open only while the log is empty"*, pointed at a 2026-08-19
  entry about ADR volume at `HEAD` before any file moved. Four were repointed because their quoted
  text matched exactly one week file (`BRIEF-20260811` ×3, `PROP-20260811-093000` ×1). **Left
  unrepaired, needing a human read of the original quote**: `docs/dispatch/608-…:48`
  (`STATUS.md:49-51`), `docs/legal/BRIEF-20260818-…:159` (`:2165`),
  `docs/adr/ADR-20260818-004646-…:270` (`:4686`), `docs/adr/ADR-20260818-233000-…:255,376`
  (`:70`, `:2165`), `docs/adr/ADR-20260818-210000-…:297,405` (`:70`), and
  `docs/adr/ADR-20260819-103112-…:494` (`:209`) — **eight sites, enumerated; an earlier revision of
  this ADR said eight and listed seven.** ADRs were left untouched on
  purpose — a historical record's citation is not repointed on a mapping derived from a file state it
  never referred to.
- **Two more inside the fence**, for whoever executes the register work: `DECISIONS.md:2338`
  (`STATUS.md:794-796`) and `:2596` (`STATUS.md:2187`). Not touched, per the fence.
- `PROP-20260819-110442`'s own `STATUS.md:NNNN` citations are deliberately anchored to `bfe6694` and
  are correct as of that SHA. They are not defects and must not be "fixed".
- **Two dangling ADR links found in passing, not fixed** (out of scope, and pre-existing at `HEAD`):
  `PROP-20260811-093000` links `../adr/ADR-20260807-002705-self-hosted-postgres-on-ovh-mks-with-cloudnativepg.md`
  and `../adr/ADR-20260731-160000-erasure-is-a-journey-tombstone-then-stream-deletion.md`; neither
  exists — the first is really `ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md`. This is exactly
  the `adr-citation-unresolved` class PROP-20260819-110442 slice 1 exists to catch, which is the
  argument for that slice, not for hand-fixing two links here.
- **The `sessions.md:NNNN` class this change also created, and did not first record.**
  `docs/proposals/PROP-20260818-013222-graph-engineering-for-the-team-workflow.md` carries **five**
  citations (`:104`, `:175`, `:480`, `:691`, `:772`) into a file that is now 101 lines. `:480` uses
  `sessions.md:1612` as the *evidence* for a proposed validator rule. Commit `e7486c3` repointed
  `STATUS.md`'s copy of that citation and left these five. They are left as-is on the same reasoning
  as the ADR sites — that proposal's whole subject is the staleness of those very citations, so
  repointing them would erase its evidence — **but the omission from the first residue list was a
  defect, not a decision.**
- **The hoisted durable sections were never checked for currency, and hoisting them raised the cost
  of that.** `## 🧭 Architecture decisions` still says *"latest: 20260802-200416"* while `docs/adr/`
  runs to 2026-08-19, and `## 📋 Remaining work` is captioned as a 2026-07-20 pre-migration snapshot.
  Carrying them verbatim was right for the extraction proof; a session now meets them on the first
  screen instead of behind 6 700 lines, so a stale section is read where it used to be unreachable.
  **Refreshing them is out of this chunk's scope** and is tracked as
  [#660 "STATUS.md's durable sections are stale and are now read first…"](https://github.com/TheCaptainCompany/captain-food/issues/660),
  which needs an owner who can validate the CONTENT — "which ADRs matter enough to name here" is an
  editorial judgement about the architecture, not a `max()` over filenames, so no validator can close it.
- **A self-measuring document cannot be kept consistent by discipline.** The entry counts, the byte
  figures and the index-row count in this ADR and in the W34 journal entry all describe a corpus that
  those very texts are appended to, so each was stale the instant it was written — and each was wrong
  at first commit (20 vs 21 entries, 219 vs 220, 97 vs 98 rows, five byte figures). Both review lenses
  found it independently. The executable form is the one `beck` named: assert per indexed week that
  *declared count == parsed entries == index rows*, plus *zero entry-openers in `STATUS.md`*, plus
  *every entry sits in the ISO week its date belongs to* — roughly 120 lines in a new
  `tools/codegen-rs/src/validate/status.rs`, modelled on `validate/proposals.rs`, with the planted-defect
  template already at `tools/codegen-rs/src/tests.rs:6367`. **Not built here** — the founder scoped this
  chunk to the existing gates. Filed as
  [#659 "Gate the STATUS.md journal split: a validator that derives every count…"](https://github.com/TheCaptainCompany/captain-food/issues/659)
  with five non-negotiable assertions, **NOT approved for implementation** and to be taken through the
  normal issue/proposal path; the founder has placed it **ahead of any decision-register migration**.
  Until it exists, every count on that page is unverified input.
- **Three incompatible promotion counts were published.** Commit `e7486c3` says "four" in one line and
  "22" three lines later; the review packet says "22". The measured number of `###`→`##` promotions is
  **18**, all heading strings byte-identical, none carrying a `§N`, so no `§` citation broke. Sixteen of
  the eighteen had drifted under `§17`. Antecedent: count of `^### ` in `git show a981c50:docs/claude/sessions.md`
  outside code fences, matched against `^## ` in `docs/claude/sessions/`.
- The line-number citation habit is what rots here. If it recurs, the executable form is a validator
  rule over `docs/**` requiring a citation into a dated artifact to name the date, not the line —
  cheaper than repairing them by hand a third time.
