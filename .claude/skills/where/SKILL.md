---
name: where
description: >
  The founder wants to know where things stand. READ-ONLY: report what is running, what is blocked
  and on what, and what is open awaiting him. Invoked ONLY by the founder as `/where` -- never
  selected by the model. Named `/where`, not `/status`, because Claude Code ships a built-in
  `/status` and a colliding skill shadows it silently. No register check, no record written, no
  fan-out, no dispatch. A `/where` never becomes work.
disable-model-invocation: true
---

# `/where` — read-only, and it stays read-only

**What the founder is doing.** Asking where things stand. Nothing else.

**Why `/where` and not `/status`.** Claude Code ships a **built-in `/status`** — the panel you reach
for when something looks wrong with the session itself (version, model, account, API connectivity,
tool stats). Skills resolve **before** built-ins on a first-match-wins scan, and dedup is by **file
path, never by name**, so a skill called `status` would shadow that panel **silently**: no collision
detection, no warning, and the outcome resting on array order inside a vendor bundle that can
reorder on upgrade. Losing the built-in inside this repo is worse than picking another name, and
this command's job is *"where are we"* — so `/where`, which is free. Founder decision, 2026-08-31.

## The one rule

> **Never turn a `/where` into work.**

Not "I noticed X was broken so I fixed it". Not "while checking I re-ran the gates". Not a claim,
not a branch, not a push, not a re-ranking. If the report surfaces something that needs doing, the
report **says so** and stops. He has `/work` and he will use it.

The reason is that `/where` is the one command with no confirmation step. Every other command in
this set ends in something he can see and correct — an answer, a record, a PR. A `/where` that
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

Two cheap accuracy rules, because a wrong `/where` is worse than a slow one:

- **A record that pins a fact to "in flight" expires and nothing detects it.** Date the claim, or
  re-read it.
- **Read the state, do not infer it from what you last did.** The worktree is shared; "already on
  `main`" has a shelf life of one tool call.

## Limits

- **No register check** — this command asserts nothing about what was decided. If the report needs
  to say *"X is decided"*, that is an assertion and it needs a citation like any other; prefer
  pointing at the record.
- **No record written.** No journal entry, no `STATUS.md` edit, no ADR. `/where` reads state; it
  does not create it.
- **No fan-out.** No lens is consulted to describe what is running.
- **Uncertainty is reported, not resolved.** *"#820's checks were green 40 minutes ago; I have not
  re-read them"* is a better answer than a fresh investigation the founder did not ask for. If
  re-reading is cheap and material, read; if it is a research task, say what you would need to do
  and let him choose.
