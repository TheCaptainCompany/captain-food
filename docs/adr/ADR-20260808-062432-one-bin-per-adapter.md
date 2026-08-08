# ADR-20260808-062432 — One bin per adapter: the composed `adapters` pod splits per partner

- **Status**: Accepted (product-owner decision, 2026-08-08: "I want an app per adapter, not one
  for all adapters")
- **Context**: [PR #389](https://github.com/TheCaptainCompany/captain-food/pull/389) wired ONE
  `adapters` bin composing all five webhook ingestors (stripe, avelo37, coopcycle, uber_direct,
  hubrise) on `hooks.captain.food`. Its review round recorded, as the top cutover precondition on
  [#385](https://github.com/TheCaptainCompany/captain-food/issues/385), that this pod — the most
  internet-exposed process in the topology — accumulates every partner's secrets by blanket
  scope grant, with `STRIPE_SECRET_KEY` even boot-required.

## Decision

The `adapters` container splits into **one bin per partner ACL**: `adapter-stripe`,
`adapter-hubrise`, `adapter-uber-direct`, `adapter-coopcycle`, `adapter-avelo37` — an emitter
FAMILY derived from the adapter-crate list (no hand-list), each with its own Deployment/Service,
image, pin, and per-partner ingress path under `hooks.captain.food`, proven by the §15 topology
validator like every other family.

## Rationale (options considered)

- **Least privilege by construction** — each pod holds ONLY its partner's secrets; a parsing bug
  in one ingestor can no longer share a process with Stripe material. Structure over filter
  (ADR-20260803-234035): this dissolves the worst instance of the recorded secret-grant
  precondition (the per-key consumer-metadata design remains for `bam`).
- **The money path is isolated** — Stripe webhook ingestion ("a paid order nobody is told about
  is the worst failure mode") no longer shares fate with four other partners' rollouts/crashes.
- **Pre-milestone partners become ABSENT, not broken** — `adapter-avelo37` is simply not
  deployed until its milestone; the recorded 503-forever route disappears.
- *Composed pod kept (status quo)* — rejected: keeps the secret pile-up and shared fate.
- *Hybrid (split Stripe only)* — rejected: captures most risk but breaks family uniformity and
  needs a hand choice the emitter can't derive; full split costs barely more (~4 extra small
  Rust pods, manifests/pins all machine-derived).

## Consequences

- c4-l2 replaces the `adapters` container with the five per-partner containers (spec change,
  approved by this decision); emitter/crate-graph/determinator/ingress follow by derivation.
- The #385 checklist item narrows to `bam` + the general per-key metadata design.
- Tracking: [#391 "One bin per adapter"](https://github.com/TheCaptainCompany/captain-food/issues/391);
  dispatched after the #360 repo-slice PR lands (single-writer tree).
