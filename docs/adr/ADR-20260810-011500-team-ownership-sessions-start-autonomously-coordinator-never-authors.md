# ADR-20260810-011500 — Team ownership: sessions start autonomously, and the coordinator never authors the diff

**Status**: Accepted (product-owner directive, 2026-08-10, in-session)
**Extends**: [ADR-20260809-013142 "Mob programming — every agent is in the dev"](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md)

## The directive, verbatim

Three product-owner statements from the 2026-08-09/10 session, in order:

1. *"I'm worried that you are not working as a loop with executor and team agents in mob approach but you are doing the work yourself."*
2. *"Never do the job yourself only the team agents have the ownership of the product you are playing the role of assistant for us."*
3. *"I want that every next session start to work by itself without asking permission because the team has the ownership of the product. I don't want to repeat myself that the way session should work — I'm talking about the way you are supervising and the usage of the executor agent and the team agents involvement."*

## Decision

Every session operates as follows, without being told and without asking:

1. **Autonomous start.** A session begins working by itself: read CLAUDE.md → docs/STATUS.md → the prioritised backlog, have the **architect agent** name the next chunk (skipping the top needs a stated reason), then claim and run it. No "shall I proceed?" — the team has ownership. The claim protocol, gates and records discipline in CLAUDE.md are unchanged and still bind.
2. **The session lead is a COORDINATOR, never an author.** The coordinator writes dispatch briefs, runs the mob loop, reads agent output critically, relays state, and handles GitHub mechanics (claims, PR bodies, comments, ready + auto-merge, supervision to MERGED). The coordinator **never writes the product diff** — not code, not specs, not the records that ride the branch. The **executor agent** (or the general team agent when the chunk includes product-owner-approved spec edits the `executor` charter forbids) implements every phase.
3. **The full mob loop applies to every chunk** (ADR-20260809-013142, now with the executor split made explicit): full-roster briefing before any code → phased execution → mob checkpoints where named lenses read the ACTUAL diff with stop authority → independent full-diff reviewer as the third look → ready + auto-merge as one indivisible step → supervised to MERGED.
4. **Escalation is a decision queue, not a permission request.** The only things surfaced to the product owner are genuine product-owner decisions (real option spaces the team cannot arbitrate, external/legal actions, credential provisioning needing admin rights). Everything else the team decides and records. The queue is presented with options, trade-offs and a recommendation — never as "may I continue?".

## Why

The 2026-08-09/10 session ran three chunks end-to-end this way — #433 (claims-only ReadScope), #435 (member rename), #437 (verifyPhone claim stamp) — and the loop caught, mid-flight and before review, defects the single-author mode had been missing: a wrong-role idempotency hole (checkpoint b of #437), a transport premise that would have shipped dead bearer plumbing (checkpoint c, deviation ratified), and a rename re-key trap neutralised by a pinned test the briefing demanded. The product owner named this mode as the reference: *"I like the way you are working — it's not the way the other session is doing it."*

## Enforcement

CLAUDE.md carries this as a non-negotiable rule (same change as this ADR), which every session loads before working. It is a prose gate: no compiler can check who authored a diff. The observable signature of compliance is in the artifacts — mob briefing findings and checkpoint verdicts in the PR body, executor-authored commits, and a coordinator whose own pushes touch only claim commits and GitHub surfaces. A session whose PR body carries no mob evidence is out of process, and the independent reviewer should say so.
