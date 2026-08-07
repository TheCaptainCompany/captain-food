# Runbook — MKS bootstrap (OVH Managed Kubernetes)

- **Realizes**: [#358 "MKS bootstrap: OVH auth, cluster + vRack, ≥3-node pool, kubeconfig into CI"](https://github.com/TheCaptainCompany/captain-food/issues/358),
  first slice of [ADR-20260807-002705](../adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md)
  under [#271](https://github.com/TheCaptainCompany/captain-food/issues/271).
- **Last executed**: _(not yet — a runbook is trusted only after it has been executed once,
  PROP-20260806-223656 §2b practice 10)_

Sections are filled in as the bootstrap proceeds; nothing here is aspirational — each step is
recorded as actually performed, with the values chosen.

## 1. OVH API auth shape

_(established before any credential is named — sessions.md §5)_

## 2. Node pool sizing

_(the answer is recorded in PROP-20260806-223656 §6; this section carries the flavor chosen)_

## 3. vRack private network

## 4. Cluster creation

## 5. Kubeconfig into CI

## 6. Follow-ups

- Read-only RBAC per-session token (PROP-20260806-223656 §2b practice 5) — not part of this slice.
