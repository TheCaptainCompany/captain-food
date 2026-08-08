# ADR-20260808-062933 — One bin per worker; periodic workers are CronJobs

- **Status**: Accepted (product-owner decision, 2026-08-08: "Same for the workers, one app per
  worker" — companion to ADR-20260808-062432)
- **Context**: the spine families are already one bin per workload (PR #387/#389). The
  cross-cutting workers carried on
  [#385](https://github.com/TheCaptainCompany/captain-food/issues/385) — SIRENE sync, the GDPR
  retention/erasure sweeps, the mailbox journal sweep — have no bin home yet; `bam` (always-on)
  already stands alone.

## Decision

Every cross-cutting worker is its OWN deployable, holding only its own grants. **Shape follows
cadence**: periodic workers are generated Kubernetes **CronJobs** — the platform's scheduler is
the scheduler; a composed worker pod running internal cron for N jobs would rebuild the monolith
pattern — while always-on workers (bam) stay single-bin Deployments. Worker inventory + cadence
get a spec home (c4-l2 containers), the emitter derives the family, and §15/crate-graph/
determinator cover it like every family.

## Rationale

- Least privilege per workload (same structural argument as one-bin-per-adapter).
- **Auditable GDPR posture**: "the process that erases personal data" becomes one named pod with
  exactly its own grants — a legal-precondition story, not a buried sweep.
- Fate isolation: the journal sweep (mailbox) never shares a process with the erasure worker
  (customer data) or SIRENE ingestion (external API).
- Rejected: composed worker pod (internal scheduler = monolith redux; secret pile-up); leaving
  workers in the monolith past cutover (contradicts the bin topology's premise).

## Consequences

- Tracking: [#393 "Cross-cutting worker hosting: one bin per worker"](https://github.com/TheCaptainCompany/captain-food/issues/393),
  which absorbs #385's carried item; sequenced after #360's repo slice and #391.
- Monolith residence of these workers ends at cutover (#358).
