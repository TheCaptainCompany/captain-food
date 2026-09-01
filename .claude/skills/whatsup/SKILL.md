---
name: whatsup
description: >
  The founder wants to know where things stand. READ-ONLY: report what is running, what is blocked
  and on what, and what is open awaiting him -- BY DEFAULT as a published status page carrying an
  answer form for the founder-owned decision rows, plus a short prose summary in-session. Invoked
  ONLY by the founder as `/whatsup` -- never selected by the model. Named `/whatsup` by the
  founder; never `/status`, because Claude Code ships a built-in `/status` and a colliding skill
  shadows it silently. No register check, no record written, no fan-out, no dispatch. A `/whatsup`
  never becomes work.
disable-model-invocation: true
---

# `/whatsup` — read-only, and it stays read-only

**What the founder is doing.** Asking where things stand. Nothing else.

**Why `/whatsup` and not `/status`.** Claude Code ships a **built-in `/status`** — the panel you
reach for when something looks wrong with the session itself (version, model, account, API
connectivity, tool stats). Skills resolve **before** built-ins on a first-match-wins scan, and dedup
is by **file path, never by name**, so a skill called `status` would shadow that panel **silently**:
no collision detection, no warning, and the outcome resting on array order inside a vendor bundle
that can reorder on upgrade. Losing the built-in inside this repo is worse than picking another
name, and this command's job is *"where are we"* — so it is not `/status`. Founder decision,
2026-08-31, which first landed the name **`/where`**; the founder then chose **`/whatsup`** on
2026-09-01 (*"Instead of /where use /whatsup"*). Both names were verified free against the running
binary — `readlink -f "$(which claude)"`, then the built-in names in that artifact — and that check
is the reusable part: **check a proposed command name against the built-ins before writing the
skill**, re-deriving it rather than trusting a version or a list written down earlier.

## The one rule

> **Never turn a `/whatsup` into work.**

Not "I noticed X was broken so I fixed it". Not "while checking I re-ran the gates". Not a claim,
not a branch, not a push, not a re-ranking. If the report surfaces something that needs doing, the
report **says so** and stops. He has `/work` and he will use it.

The reason is that `/whatsup` is the one command with no confirmation step. Every other command in
this set ends in something he can see and correct — an answer, a record, a PR. A `/whatsup` that
quietly did something leaves him with a report that is **not a description of the state he asked
about**, because the act of reporting changed it.

## Report exactly three things

**1. What is running.** Open PRs you own, with number, title, and their *actual* state — draft /
ready / checks running / green / conflicted. Read `mergeable_state` rather than assuming: a
serialized merge queue makes its own conflicts, GitHub sends **no webhook** for the
mergeable→dirty transition, and a "waiting" PR may have been conflicted for hours. In-flight agent
runs, with what they were dispatched to do.

**2. What is blocked, and on what.** Name the blocker concretely — a red check with its failure, a
conflict, an AMBER item needing a `specs/**` approval, a dependency on another PR. *"Blocked"* with
no object is not a status.

**3. What is open awaiting him.** Decision-queue rows, with the option space and the recommendation
already attached. Apply the test before listing anything here: **what am I waiting for, and who
sends it?** If the answer names the founder and no decision-queue row is open, **nothing is coming**
— that item is not awaiting him, it is awaiting you, and it belongs in section 1 or 2. A PR sitting
at ready-for-review with a PASS review and green checks is **not** an item for this section: `HOLD:
human` names the team's reviewer pass, never a founder wait, and the coordinator merges.

## The default output is a published page, and the report still gets spoken

Founder directive, 2026-09-01, verbatim: *"Show me a status page with form to answer your questions
and copy/paste them<== make the /whatsup do that by default"*. **By default** — he should never have
to ask for it twice.

So a `/whatsup` ends in a **published HTML artifact**, and **the terminal report does not go away**:
the same turn carries a short prose summary *and* the link. A link with nothing said around it is a
worse `/whatsup` than the plain report was — he asked where things stand, and the answer to that
question is sentences. The page is where he *acts*; the summary is where he *learns*.

**The page is a RENDERING of the report above, not a new report.** The three sections are exactly
the three sections, held to exactly the same content contract. Nothing is added to what is reported
because it now has a layout, and nothing is dropped because it did not fit.

### What the page must do

These seven are what make it useful rather than decorative. They are a contract, not a template:
build the page fresh from the state you just read, because a stored template carries last week's
rows and this command's whole value is that it does not.

1. **The three sections, unchanged** — what is running · what is blocked and on what · what is open
   awaiting him.
2. **The form covers only rows that are genuinely his move**: `status: open` **and** `owner:
   founder`, read from `docs/decisions/<KEY>.yaml`. Never from the prose in `DECISIONS.md`, which is
   history — see the authority order below. `owner: counsel` and `owner: team` rows are excluded
   **by construction, not by judgement**: the register's own rule is that the index must not invite
   him to push on those. A footer line saying how many were excluded and why is honest; listing
   them is the invitation the rule forbids.
3. **A row he has already answered renders as answered, not asked again.** Same failure mode as
   reading the prose rows, and the more insulting one: asking a second time for an answer he gave.
   Where a row stays open *because* his direction does not settle it yet, say that — "nothing needed
   from you here" is a complete cell.
4. **Quick-fill buttons come from the row's own enumerated options** — the option space its
   `question:` or `evidence:` field actually states, verbatim. Never invented, never a paraphrase,
   never the team's recommendation dressed up as one of the choices. A row with no enumerated option
   space gets a free-text box and nothing else.
5. **Copy-back uses the register's own envelope**: `Decision row: <KEY>` / `Q:` / `A:`, with the
   assembled block prefixed `/decision`, so a paste lands with its trail already attached and he
   never restates what he is answering. Per-row copy **and** copy-all. The clipboard API is not
   always available in a sandboxed frame, so always ship the select-the-text fallback — a copy
   button that silently does nothing is CLAUDE.md's *"a control that renders but does nothing"*, on
   the one surface whose entire job is getting his answer back out.
6. **Drafts persist in `localStorage`**, every access wrapped in `try`/`catch`, and the page renders
   correctly with nothing stored. He will close the tab mid-answer, and storage can be blocked.
7. **Every figure carries its antecedent** (ADR-20260817-105845). A number on a page is consumed by
   its reader as established fact, and a page has more room to state a number than a paragraph
   does — which makes the discipline tighter here, not looser. Mark a bare input `UNVERIFIED input`.

### Why publishing a page is not "work"

The one rule above stands at exactly its original strength, and so does *No record written* below.
The next reader will ask how a published artifact squares with them, and should, so: publishing
**writes no repo record, mutates no branch, claims no issue, pushes nothing, and changes nothing
about the state he asked after**. It is a rendering of what was read. The act of reporting still
does not change the thing reported — which is the entire reason the rule exists.

What is still work, and still stops at being reported: fixing something the page surfaced,
re-running a gate to fill a cell, re-ranking a row, filing the issue for the staleness you noticed
while reading.
If a section would need an investigation to fill, the page **says that** — an empty cell with an
honest reason beats a full one bought with an errand he did not ask for.

## Where to read it from

[`docs/STATUS.md`](../../../docs/STATUS.md) is the durable state; the current
`docs/status/journal-YYYY-Www.md` (`date +%G-W%V`) is the dated history, newest first. Live PR and
check state comes from the API. Prefer both over the conversation — a long session's memory of what
shipped is exactly the projection that goes stale.

**Section 3 has its own source, and the authority order matters.** Decision-queue rows come from
`docs/decisions/<KEY>.yaml`, which is authoritative for **current status**; the prose row in
[`docs/proposals/DECISIONS.md`](../../../docs/proposals/DECISIONS.md) is its **history**. Read the
YAML. A status built from the prose rows will report a row as founder-owed that the YAML records as
`decided` — the exact failure of telling him he owes an answer he already gave.

Two cheap accuracy rules, because a wrong `/whatsup` is worse than a slow one:

- **A record that pins a fact to "in flight" expires and nothing detects it.** Date the claim, or
  re-read it.
- **Read the state, do not infer it from what you last did.** The worktree is shared; "already on
  `main`" has a shelf life of one tool call.

## Limits

- **No register check** — this command asserts nothing about what was decided. If the report needs
  to say *"X is decided"*, that is an assertion and it needs a citation like any other; prefer
  pointing at the record.
- **No record written.** No journal entry, no `STATUS.md` edit, no ADR. `/whatsup` reads state; it
  does not create it. The published page is not an exception to this — see *Why publishing a page is
  not "work"* above: it is a rendering, and it lands in no record the repo keeps.
- **No fan-out.** No lens is consulted to describe what is running.
- **Uncertainty is reported, not resolved.** *"#820's checks were green 40 minutes ago; I have not
  re-read them"* is a better answer than a fresh investigation the founder did not ask for. If
  re-reading is cheap and material, read; if it is a research task, say what you would need to do
  and let him choose.
