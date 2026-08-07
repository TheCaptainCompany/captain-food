# Runbook — MKS bootstrap (OVH Managed Kubernetes)

- **Realizes**: [#358 "MKS bootstrap: OVH auth, cluster + vRack, ≥3-node pool, kubeconfig into CI"](https://github.com/TheCaptainCompany/captain-food/issues/358),
  first slice of [ADR-20260807-002705](../adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md)
  under [#271](https://github.com/TheCaptainCompany/captain-food/issues/271).
- **Last executed**: _(not yet — a runbook is trusted only after it has been executed once,
  PROP-20260806-223656 §2b practice 10)_

Sections are filled in as the bootstrap proceeds; nothing here is aspirational — each step is
recorded as actually performed, with the values chosen.

## 1. OVH API auth shape

Established 2026-08-07 from OVH's own documentation and price catalog, before naming any
credential (sessions.md §5). Three distinct credential planes exist; do not conflate them:

1. **The OVHcloud API** (`https://api.ovh.com` / `eu.api.ovh.com`, region `ovh-eu`) — manages
   MKS (`/cloud/project/{serviceName}/kube/*`), Public Cloud private networks
   (`/cloud/project/{serviceName}/network/private`), the vRack (`/vrack/*`) and DNS zones
   (`/domain/zone/*`). Two credential kinds authenticate against it:
   - **OAuth2 service account (IAM)** — `client_id`/`client_secret`, client-credentials flow
     against `/auth/oauth2/token`, scoped by IAM policies attached in the OVHcloud manager
     (Identity & Access Management → service accounts). The modern, resource-scopable kind.
   - **Legacy application triplet** — application key + application secret + consumer key with
     per-path access rules and request signing. This is the shape the SMS hook already uses
     (`OVH_APPLICATION_KEY`/`OVH_APPLICATION_SECRET`/`OVH_CONSUMER_KEY` in
     `specs/configuration.yaml`); it stays untouched.
2. **The OpenStack APIs** (Keystone/Nova/Neutron) underneath Public Cloud — a separate user
   base with its own credentials. **Not needed by this slice** (console + OVHcloud API cover
   everything); only relevant if OpenTofu's OpenStack provider is adopted later.
3. **The kubeconfig** MKS emits — a cluster-local credential, produced by the control plane,
   independent of both API planes. It is this slice's only credential output (§5 below).

Direction and tenancy (sessions.md §5 items 3–4): everything here is **outbound** (we call
OVH); nothing is per-tenant — all infra credentials are per-deployment.

Consequences for the follow-up slices, so keys are named in the provider's vocabulary at
creation time and not before:

- **#358 (this slice) needs NO OVH API credential at all** — the console does vRack, cluster
  and node pool; the kubeconfig is downloaded from the console.
- **#361/#362 (OVH DNS + cert-manager DNS-01)**: the community solver
  ([aureq/cert-manager-webhook-ovh](https://github.com/aureq/cert-manager-webhook-ovh))
  supports **both** kinds; prefer an **OAuth2 service account scoped by an IAM policy to the
  `captain.food` zone only** — smallest blast radius the provider allows (D4's "zone-scoped
  and sealed"). Name the keys from the IAM screens when the account is created, not earlier.
- **OpenTofu later**: the `ovh/ovh` provider takes either kind; same recommendation.

## 2. Node pool sizing

Priced 2026-08-07 from the public order catalog
(`GET https://api.ovh.com/1.0/order/catalog/public/cloud?ovhSubsidiary=FR`, prices in
10⁻⁸ EUR, ex-VAT). Full option table and the recorded answer:
[PROP-20260806-223656 §6.2](../proposals/PROP-20260806-223656-kubernetes-as-the-deployment-substrate.md).

Facts that shaped it:

- **MKS has two plans since 2025**: **Free** (€0 control plane, 99.5% SLO, single-zone control
  plane, **shared etcd capped at 400 MB**, ≤100 nodes) and **Standard** (€0.09/h ≈ €65.70/mo,
  99.9% SLA 1-AZ / 99.99% 3-AZ, dedicated etcd 8 GB). The ADR's "free control plane" premise
  is the Free plan.
- Worker flavors (monthly ex-VAT): d2-4 (2 vCPU/4 GB) **€11.44** · d2-8 (4 vCPU/8 GB)
  **€20.60** · b2-7 (2 vCPU/7 GB) €25.17 · b3-8 (2 vCPU/8 GB, hourly-only €0.0512/h) ≈ €37.38.
- Public Cloud Load Balancer (Octavia) **S: €6.00/mo**; Gateway S (€2.00/mo) is **not needed**
  in the default vRack shape — nodes keep their public `eth0` as default route, `eth1` carries
  only private traffic.
- Memory demand at V0 (requests, rounded): CNPG 3×1 Gi + Argo CD ~1.5 Gi + ingress-nginx
  ~0.5 Gi + cert-manager ~0.2 Gi + OTel collector ~0.3 Gi + api ~0.5 Gi + per-node system pods
  ~1.5 Gi total ≈ **8.5 Gi** — a d2-4 trio's ~9 Gi allocatable leaves no headroom for the
  database; a d2-8 trio (~19 Gi allocatable) is comfortable.

## 3. vRack private network

## 4. Cluster creation

## 5. Kubeconfig into CI

## 6. Follow-ups

- Read-only RBAC per-session token (PROP-20260806-223656 §2b practice 5) — not part of this slice.
