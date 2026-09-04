# ADR-20260904-013834 — The team decides option spaces and spec diffs; external, legal and admin-gated actions stay with the founder

<!-- Filename: docs/adr/ADR-20260904-013834-the-team-decides-option-spaces-and-spec-diffs-external-legal-and-admin-gated-actions-stay-with-the-founder.md -->

## Status

Accepted (founder directive 2026-09-03, scope answer 2026-09-04). Records a founder directive: the
`Consulted:` block below is required
([ADR-20260812-143619](ADR-20260812-143619-the-founder-is-the-founder-and-every-founder-message-goes-to-the-whole-team.md)).

**Register row**: [TEAM-DECIDES-OPTION-SPACES](../decisions/TEAM-DECIDES-OPTION-SPACES.yaml)
(decided, this record). **Amends**
[ADR-20260810-215503](ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md)
§"What is NOT delegated" items 1 and 3, and
[ADR-20260808-144738](ADR-20260808-144738-product-ownership-lives-in-the-team-no-pm-agent.md)
decision 3's last sentence (*"When in doubt, it goes to the customer"*) — both carry a banner
pointing here. Items 2, 4 and 5 of ADR-20260810-215503 (external/legal/admin-gated matters, the
binding method, the founder's override) are **untouched**.

## Enforced by

n/a — no behavioral guarantee. Operating-model rule. Its executable half is unchanged: the
`Concerns` mechanics on proposals, the `Consulted:` requirement, and the register-check hook.

## Context

On 2026-09-03 the founder said, verbatim:

> **"Don't need ask me authorization all the time / I authorize you to do everything"**

The register check found it amends a recorded decision.
[ADR-20260810-215503](ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md)
delegated backlog ranking to the team and listed what was NOT delegated: (1) genuine option spaces,
(2) external, legal and admin-gated matters, (3) `specs/**` approval, (4) the method, (5) the
founder's override. The directive as spoken reaches all five; taken literally it would also reach
sending a filing or a partner mail in the company's name. So the boundary was put to the founder
as question 1 of the 2026-09-04 form, with three options, and he chose:

> **"A — team decides option spaces and spec diffs; external, legal and admin-gated actions still
> come to me"**

Holub's consult surfaced the second record this touches: ADR-20260808-144738 decision 3 resolves
a split among lenses with *"When in doubt, it goes to the customer"*, and
[ADR-20260808-155656](ADR-20260808-155656-first-consent-based-ensemble-decisions.md) applied it (a
decision contested between lenses went to the customer). Removing the founder as the default
destination for option spaces removes that tie-break, and an unstated tie-break lands by default on
the one role ADR-20260808-144738 decision 4 says must never decide alone — the coordinator, the PM
re-emerging. This record therefore names the deciding body and what a split means.

## Decision

1. **Genuine option spaces are the team's to decide.** Item 1 of ADR-20260810-215503 §"What is
   NOT delegated" is withdrawn. A proposal's forks and Concerns, a Concern's option space, the
   build-order interpretation questions — these are decided in the mob and recorded, and the founder
   reads the record. He is no longer asked "which option?"; he is told which was taken and why.
2. **`specs/**` approval is the team's.** Item 3 is withdrawn. The freeze was already lifted
   ([ADR-20260810-221840](ADR-20260810-221840-specs-are-the-teams-work-the-freeze-is-lifted.md));
   what item 3 still reserved was the *approval* of an AMBER diff. That approval now comes from the
   mob briefing under the ordinary gates, recorded in the dispatch card. The three questions of
   CLAUDE.md §"Non-negotiable rules" (reversal? migration? otherwise the team's) and the SPEC-LOG
   sentence are unchanged and are what replaces the approval.
3. **External, legal and admin-gated actions stay with the founder** (item 2, unchanged): entity
   and brand naming, engaging counsel, filings, partner onboarding and partner mail, money posture,
   consumer-mediator registration, provisioning that needs a console or a credential he holds. An
   external artifact must name the capacity the statutes actually confer, and no agent holds one.
4. **The deciding body is the mob, by consent** (holub; the mechanics are this repo's,
   ADR-20260808-155656): a decision stands when no lens names a concrete harm, never when a majority
   likes it. The coordinator relays the verdict and never casts one.
5. **A split among lenses is evidence that the question is not evidence-settled**, and the response
   is the repo's own machinery, not escalation: take the **reversible** option behind a gate
   (gate-then-stabilize), name the observation that would settle it, and let the next release
   answer. On a **legal surface** the safer option is taken and the split is written into the
   `Consulted:` block. The founder's override (item 5) and his asynchronous veto window
   (ADR-20260808-144738 decision 3) are untouched.
6. **A split on a `HOLD: human`-axis subject that is ALSO external, legal or admin-gated** is not
   an option space the team holds; it is item 2. The `HOLD: human` class itself (stored shapes,
   money movement, legal surfaces, Tours-facing) is still decided by the team — `HOLD: human`
   names the team's reviewer pass, never a founder wait
   ([ADR-20260815-134655](ADR-20260815-134655-the-team-merges-its-own-work-no-pr-waits-on-founder-review.md)).
   A lens brief is never legal advice or clearance.
7. **Every founder message still goes to the whole roster before an answer**
   (ADR-20260812-143619) and **records created from a directive still carry `Consulted:`**. This
   record changes who *decides*, not who is *asked*.
8. **The question form survives for one purpose**: item 3 matters, and the founder's own
   election to be asked (as on 2026-09-04: *"ask me your questions with form to copy here"*). It is
   never used to ask "shall I proceed?".

## Alternatives considered

- **B — everything, external and legal actions included.** Rejected by the founder. The one class
  a record cannot undo; the coordinator holds no capacity under the statutes and would have stopped
  before sending anything outward regardless.
- **C — only stop asking "shall I proceed"; real option spaces still come to the founder on a
  form.** Rejected by the founder: the fan-out and the form would stay for every fork, and a fork
  waits on him rather than on the team.
- **Leave the directive in the journal only.** Rejected: it contradicts two recorded decisions,
  and an unrecorded reversal is the landmine `/decision` exists to stop
  ([ADR-20260821-103403](ADR-20260821-103403-decision-ask-unregistered-and-the-citation-ratchet.md)).

## Consequences

### Positive
- The decision queue shrinks to what only the founder can do. Forks such as the
  `one-subject-one-role` Concern and the step-4 counsel collision are decided by the team, the
  safer way, and recorded — the four decisions of 2026-09-04 were the last taken by form under the
  old rule.
- The tie-break is explicit, so "the team decides" cannot degrade into "the coordinator decides".

### Negative
- The founder sees decisions after they land. His correction is the override and the veto window,
  which cost a reversal record each time they are used.
- More `Consulted:` blocks and more journal lines: the record is now the only place he sees the
  option space.

### Follow-up actions
- [x] CLAUDE.md §"Respect the prioritised backlog" — the "NOT delegated" clause rewritten in this
      change to name only external/legal/admin-gated matters and the method.
- [x] Banners on ADR-20260810-215503 and ADR-20260808-144738, this change.
- [ ] `.claude/skills/whatsup`, `/direct-question` and `/decision` prose that says "decision-queue
      rows … awaiting the founder" is re-read against this record on their next edit — no
      content there contradicts it today (the queue still exists for item 3).

## Consulted (ADR-20260812-143619 — one line per lens)

Consulted for the completeness of the record, never to relitigate; **no lens output is legal
advice or clearance**.

- **holub** — briefing + `Consulted:` + safer-option is NOT enough because the tie-break was being
  deleted, not amended: named the deciding body (the mob, by consent), the meaning of a split
  (not evidence-settled → reversible option behind a gate), and the `HOLD: human` boundary
  (§Decision 4–6); cited ADR-20260808-144738 d.3 and ADR-20260808-155656 as the records that
  needed the banner.
- **legal-specialist** — asked on the same day's Q2/Q3 (recorded in their own ADRs); on this
  record, the capacity clause of §Decision 3 restates its standing position that no lens output
  and no agent holds a capacity the statutes confer.
- **beck, farley** — asked on the same day's model-tier decision; nothing in their lens on this.
- **architect, business-specialist, dba, evans, graphql-architect, observability-agent,
  ux-designer, vernon, young** — not asked: the subject is the operating model, in no domain
  lens; recorded so a lens never asked is distinguishable from one with nothing to say.
