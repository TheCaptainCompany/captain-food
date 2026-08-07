# ADR-20260807-114122 — MKS starts at one node: the ≥3-node condition becomes a ladder

- **Status**: Accepted (product owner, 2026-08-07: the priced trio — MKS Free + 3× d2-8 + LB S =
  **€67.80/mo ex-VAT** — is over budget: *"too expensive for me, I don't have the money for that"*)
- **Amends**: [ADR-20260807-002705](ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md) — **only
  D2's node condition**. Everything else stands: MKS, CNPG, GitOps-only, generated manifests,
  `Recreate`, start clean, NS → OVH DNS.
- **Sizing detail**: [docs/runbooks/mks-bootstrap.md §2](../runbooks/mks-bootstrap.md) (real catalog
  prices) · [#358](https://github.com/TheCaptainCompany/captain-food/issues/358)

## Decision

**Entry shape: ONE d2-8 node + Load Balancer S = €26.60/mo ex-VAT.** CNPG runs `instances: 1`;
**WAL archiving/PITR to Object Storage is NON-NEGOTIABLE** — it is now the only recovery path. The
Prometheus stack is dropped; the [#364](https://github.com/TheCaptainCompany/captain-food/issues/364)
alert loop runs on **Honeycomb triggers** instead (the observability contract already lives there).
The LB S is kept at its real €6/mo catalog price — a stable entry IP in front of the Stripe webhook
path is worth more than the €6 the NodePort variant saves.

**Why this is sound and not just cheap**: [#193](https://github.com/TheCaptainCompany/captain-food/issues/193)
caps the app at ONE instance until [#242](https://github.com/TheCaptainCompany/captain-food/issues/242)
lands, so a 3-node HA database under a single-instance app was protection asymmetry — the €67.80
bought redundancy the app cannot yet match. What is accepted in exchange: a node failure means
restore-from-WAL (minutes of downtime), not failover — the same availability class the app already
has.

**The ladder up is config, not migration**: node-pool resize → `instances: 3` → the original
anti-affinity condition is satisfied unchanged. **Climb when #242 lands or the first paying
restaurants arrive, whichever comes first** — at that point the €67.80 question is re-asked with
revenue on the other side of it, per gate-then-stabilize.

## Consequences

- The restore drill ([#360](https://github.com/TheCaptainCompany/captain-food/issues/360)) becomes
  MORE important, not less — single-instance means the drill rehearses the only recovery path.
- [#364](https://github.com/TheCaptainCompany/captain-food/issues/364) re-scopes from
  kube-prometheus to Honeycomb triggers → GitHub issue webhook.
- The single-node demand budget (~5.5 Gi against ~6.3 Gi allocatable) is snug: anything new that
  wants memory on the cluster re-opens this sizing before it lands.
