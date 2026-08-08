# ADR-20260808-144738 — Product ownership lives in the team; no product-manager agent, ever

## Status

Accepted (product-owner decision, 2026-08-08, session
https://claude.ai/code/session_01AKgDqRbCcCxtUePWPRfxtp)

## Context

The founder holds Allen Holub's view of agility: product ownership belongs to the TEAM, not to a
role; a single product-manager position is a proxy interposed between the team and the customer,
filtering information in both directions and becoming a bottleneck and a single point of judgment.
Building this system with AI agents created a felt conflict — the agent-organization patterns
common in the industry reproduce exactly the role-shaped hierarchy those values reject, and the
repo's own "product-owner directive" vocabulary reads that way.

The mechanical reality is different from the vocabulary, and this ADR records it so future
sessions do not drift into "fixing" it: the human in the loop is not a product manager. They are
the FOUNDER-CUSTOMER — the one entity in the system with continuity of purpose, legal and
financial exposure, and contact with the real market (Tours). Holub's objection is to proxies
between team and customer, not to customers deciding what they want. Meanwhile ownership already
lives in team-shared artifacts, not in a role: specs are the source of truth, decisions are
versioned ADRs/proposals with recorded rationale, gates are executable rather than managerial, and
any session — human or agent — can act from that shared context. That is more team-ownership than
most human organizations achieve.

The honest limit, also recorded: Holub's team-ownership presumes members with continuity and skin
in the game. Today's agents have neither — each is ephemeral, carries no accountability, and
ensemble consensus among models can be correlatedly wrong in ways a diverse human team is not. So
some decisions genuinely cannot be delegated to the team-of-agents yet; that is a property of what
agents currently are, not of bad organization design.

## Decision

1. **No product-manager or product-owner AGENT is ever created.** A synthetic proxy between the
   team and the customer is the Holub anti-pattern realized in software: the team would optimize
   for the proxy's model of the customer instead of the customer. Standing SPECIALIST lenses
   (architect, dba, graphql-architect, ux-designer, reviewer, …) are welcome; a standing MANAGER
   is not. If a future session finds a "pm"/"product-owner" agent under `.claude/agents/`,
   deleting it is the correct move and this ADR is the authority.
2. **The human decides as CUSTOMER, not as manager.** Questions routed to them are value, taste,
   legal, and money-path arbitration — never work-routing, never status ceremony. The existing
   "product-owner directive" phrasing is kept for continuity but reads as "customer decision of
   record".
3. **Decisions are classified by reversibility, not by rank.** A decision that is reversible,
   evidence-settled, and gated (gate-then-stabilize provides the machinery) may be made by the
   agent ensemble and recorded as an ADR, with the customer holding an asynchronous veto window —
   decision by consent, not by approval queue. Irreversible, value-laden, legal, or money-path
   decisions go to the customer. When in doubt, it goes to the customer.
4. **The coordinator role is per-session and disposable.** The repo carries the state
   (proposals, ADRs, checklists, sessions.md), so any future session can coordinate. No standing
   coordinator agent is created either — that is where a PM would quietly re-emerge.
5. **Closer-to-reality duty (the long game): evidence displaces proxy judgment.** Wherever
   production evidence CAN settle a question — telemetry under the observability contracts,
   ratings, reclamation categories and resolution outcomes, conversation content, dispatch and
   payment failure rates — the team gathers it BEFORE queueing the decision to the customer, and
   cites it in audits and proposals once the system is live. The customer is the team's sensor of
   the market only until the team can sense it directly; every feedback loop added shrinks the
   judgment bottleneck without ever needing a PM. Corollary: when an advisory agent needs a
   production signal that does not exist, the missing observability contract is itself a finding
   (specs/observability.yaml is the source DSL for those).

## Alternatives considered

- **A product-manager agent** — rejected: institutionalizes the proxy anti-pattern; the team
  optimizes for a simulation of the customer; the simulation drifts.
- **Full team autonomy (no customer gate at all)** — rejected for now: agents lack continuity,
  accountability, and market contact; ensemble consensus can be correlatedly wrong. Revisit as
  evidence loops (decision 5) mature.
- **Renaming every "product-owner" reference in the repo** — rejected as churn: the mechanics are
  what matter; this ADR pins the reading instead.

## Consequences

### Positive

- Future sessions cannot "helpfully" invent a PM agent; the stance is a recorded decision with an
  ADR to cite, not a preference living in one person's head.
- The decision queue to the customer shrinks over time along two axes: reversible decisions move
  to the ensemble (decision 3), and evidence-answerable questions stop being questions
  (decision 5).
- `docs/proposals/DECISIONS.md` remains exactly what it is: the queue of CUSTOMER decisions, not
  a manager's backlog.

### Negative / accepted costs

- Consent-based ensemble decisions can be wrong in correlated ways; the veto window and
  gate-then-stabilize reversibility are the mitigations, and a wrong ensemble decision that
  passes both is an accepted cost of not funneling everything through one human.
- The customer remains a genuine bottleneck for value/legal/money decisions until real-market
  feedback loops exist — accepted knowingly rather than papered over with a proxy.

## Refs

`.claude/agents/ux-designer.md` (dispatch protocol — supervised pipeline, PO directive
2026-08-08) · ADR-20260803-234035 (compiler first — decision rules over decision queues) ·
ADR-20260801-020000 (living documents) · `docs/proposals/DECISIONS.md` ·
`specs/observability.yaml` (the contracts decision 5 builds on)
