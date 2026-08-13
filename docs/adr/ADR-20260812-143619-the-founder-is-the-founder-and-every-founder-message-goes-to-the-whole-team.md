# ADR-20260812-143619 — The founder is the founder, and every founder message goes to the whole team before any answer

- **Status**: Accepted (founder directive, 2026-08-12)
- **Date**: 2026-08-12
- **Extends**: [ADR-20260809-013142](ADR-20260809-013142-mob-programming-every-agent-is-in-the-dev.md)
  (mob programming — from dispatches to founder messages) ·
  [ADR-20260810-011500](ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
  (coordinator never authors — from the diff to the answer)
- **Relates**: [ADR-20260808-144738](ADR-20260808-144738-product-ownership-lives-in-the-team-no-pm-agent.md)
  (product ownership lives in the team) ·
  [ADR-20260810-215503](ADR-20260810-215503-backlog-prioritisation-delegated-to-the-team.md)

## Directives

Two, verbatim, 2026-08-12:

1. *"Stop calling me product owner. I'm the founder / Tech CEO."*
2. *"When I say something ask the team for answers never answer directly without asking the whole
   team."*

## Decision 1 — the title is **founder / Tech CEO**

Repo records, agent definitions, playbooks and operating docs name the human counterpart the
**founder** (or **Tech CEO** where the technical capacity is the point). "Product owner" is retired
as a form of address and as a role label for him.

**What is swept**: the LIVING operating documents — `CLAUDE.md`, `docs/PLAYBOOK.md`,
`docs/BACKLOG.md`, `docs/proposals/README.md`, `docs/claude/*.md`, and the register's owed-markers in
`docs/proposals/DECISIONS.md` (`PRODUCT-OWNER-OWED` → `FOUNDER-OWED`).

**What is NOT swept**: historical ADRs and proposals. They are records of what was decided and by
whom under the vocabulary then in use; **a verbatim quote stays verbatim**, and rewriting a record to
match today's naming is the one thing a record must never do. The single exception in this pass is
[ADR-20260812-115930](ADR-20260812-115930-each-adapter-owns-its-own-completely-isolated-database.md),
which was being materially corrected on the same day and adopts the new title in the same change.

**The term "product owner" does not disappear from the vocabulary** — it remains the name of a
*function* the team now holds (ADR-20260808-144738: product ownership lives in the team). What is
retired is using it for the human.

**Legal caveat (legal lens, recorded as given)**: *"founder / Tech CEO"* is right for repo records and
internal address, but it is **not a French corporate mandate**. Any EXTERNAL artifact — mentions
légales, partner onboarding and contracting, association filings, anything naming who binds the
entity — must state the capacity the **statutes actually confer** (président, gérant, directeur
général, membre du bureau), not the internal title. Do not propagate this rename outward.

## Decision 2 — no answer is composed before the whole team has been asked

Every founder message goes to the **whole roster** before any answer is composed and before any
record lands. This is the mob principle (ADR-20260809-013142) extended from **dispatches** to
**founder messages**, and the coordinator-never-authors principle (ADR-20260810-011500) extended from
**the diff** to **the answer**.

Mechanically, unchanged from the mob briefing shape: the message goes out to every lens in parallel,
each lens names what it sees in its own lens, and **"nothing in my lens" is a complete answer and
costs one line**. The coordinator relays, aggregates and presents; it does not author the answer any
more than it authors the diff.

Why it is worth the round-trip rather than obvious politeness: the two defects that produced this
directive were both **coordinator-authored records**, landed on `main` without a lens ever reading
them — an adapter table inventory that was wrong in the one adapter nobody re-checked, and a
recommendation whose stated rationale ran backwards (an encrypted table moved for the posture of a
plaintext one). Neither is a hard finding; both are one question to the right lens.

### The carve-outs, each attributed to the lens that asked for it

- **Relaying a recorded fact is not answering** (business lens). Anything on an **external clock** —
  a billing suspension, a token or credential expiry, a partner contract deadline, a legal opposition
  window — goes back in the **same turn**, verbatim from the register, with the mob's opinion
  following. Waiting for ten lenses before saying "the trademark opposition window closes Friday" is
  not diligence, it is a missed deadline. The test is whether the sentence *reports* a recorded fact
  or *decides* something.
- **Executing an already-recorded rollback or abort path needs no consult** (release lens). The mob's
  involvement happened when the path was written; re-convening it mid-incident spends the minutes the
  path exists to save. Going **forward** through an incident — a hotfix migration, flipping a gate to
  escape a failure, an unrecorded workaround — is a **new decision** and does get the mob. The test:
  *am I executing a recorded path, or inventing one?*
- **No lens output is legal advice or clearance, and no aggregation of lenses becomes one**
  (legal lens). Agreement between lenses never upgrades a hedged finding to a settled one; ten lenses
  concurring on a GDPR or payment-agent question produce ten opinions, not counsel. Where an answer
  needs professional clearance, the output is *"this needs counsel"*, presented as such.

## Decision 3 — a record created from a founder directive names which lenses answered

Raised independently by three lenses (testing, UX, observability) and adopted: **a lens that was
never asked is indistinguishable from a lens with nothing to say.** Silence is ambiguous — the same
defect class this repo already recorded for monitoring
([ADR-20260810-231300](ADR-20260810-231300-no-polling-only-pushing-polling-as-graceful-fallback.md):
a push-only monitor cannot tell "healthy, nothing to report" from "dead, reporting nothing").

**Rule**: an ADR or register row created from a founder directive carries a **`Consulted:` block, one
line per lens**. *"Nothing in my lens"* is a valid line — it is in fact the point, because it is the
line that distinguishes an asked lens from a skipped one. A record with no `Consulted:` block is a
record whose mob cannot be audited.

This ADR carries its own block below. **A validator rule could enforce it later** — the existing
shape is the proposal-hygiene rules in `tools/codegen-rs/src/validate/proposals.rs`, which already
gate on header blocks (`Concerns` checklists blocking `Approved`) — and per this repo's
prefer-executable-over-prose rule that is where it should end up. It is deliberately **not** written
in this change: it is code, and this change is records-only.

## Consequences

- Every founder message now costs one fan-out before an answer. That is the intended price; the
  carve-outs above are the places it is not paid.
- Records gain an auditable mob trail, and the absence of a `Consulted:` block becomes a visible
  defect rather than an invisible one.
- One term sweep lands with this ADR across the living operating docs; historical records keep their
  vocabulary, so a future reader will find both forms and this ADR explains which is which.
- External-facing artifacts are explicitly out of scope of the rename, and naming the wrong capacity
  in one of them is a legal defect, not a wording nit.

## Consulted

- **architect** — the two extended ADRs are the right lineage; the rename is vocabulary, not a change
  to who decides (ADR-20260808-144738 already moved ownership into the team).
- **holub** — nothing in my lens beyond one note: "product owner" survives as a *function name*, so
  the sweep must not blind-replace it where it denotes the role the team holds.
- **dba** — nothing in my lens.
- **farley** (release) — asked for and got the rollback/abort carve-out: executing a recorded path is
  not a new decision; going forward through an incident is.
- **beck** (testing) — a record must name who answered, or the mob is unfalsifiable; asked for the
  `Consulted:` block, and for it to be a validator rule eventually rather than a convention.
- **graphql-architect** — nothing in my lens for the rename; noted that the same "silence is
  ambiguous" argument is why the dissent in ADR-20260812-115930 is recorded in the body, not a
  footnote.
- **business-specialist** — asked for and got the external-clock carve-out: a recorded fact on
  someone else's deadline goes back in the same turn.
- **legal-specialist** — the title is fine internally and is **not** a French corporate mandate;
  external artifacts must name the statutory capacity. Also: no lens output, and no aggregation of
  lens outputs, is legal advice or clearance.
- **ux-designer** — a `Consulted:` block is the record's own affordance: it tells a reader what was
  and was not looked at, which is what makes the rest of the record trustworthy.
- **observability** — silence is ambiguous is already this repo's recorded defect class for
  monitoring; naming the lenses is the dead-man's-switch equivalent for records.
