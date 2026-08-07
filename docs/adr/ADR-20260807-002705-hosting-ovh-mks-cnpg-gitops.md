# ADR-20260807-002705 — Hosting: OVH Managed Kubernetes, CNPG in-cluster, GitOps-only operations

- **Status**: Accepted (product owner, 2026-08-06/07, in-session — the full decision set of
  PROP-20260806-223656, answered across the session and closed with *"D3 and D5 yes, start clean,
  move the NS to OVH"*)
- **Supersedes**: [ADR-20260806-151122](ADR-20260806-151122-hosting-destination-is-clever-cloud-not-ovh.md)
  (Clever Cloud — REOPENED 2026-08-06, now closed by this record). Points 2–4 of
  [ADR-20260731-061609](ADR-20260731-061609-hosting-migrates-to-ovh-supabase-identity-only.md) remain
  in force unchanged: Supabase identity-only, the build side does not move (GitHub Actions + GHCR,
  digest-pinned), the cutover uses the existing outage.
- **Proposal**: [PROP-20260806-223656 "Kubernetes as the deployment substrate"](../proposals/PROP-20260806-223656-kubernetes-as-the-deployment-substrate.md)
  (Approved by this ADR) — the option space, trade-offs, diagrams and the §2b operating practices.
- **Tracking issue**: [#271](https://github.com/TheCaptainCompany/captain-food/issues/271)

## Context

The destination question ran three rounds in two days: OVH instances (ADR-20260731-061609), Clever
Cloud PaaS (ADR-20260806-151122), and — after the operator-capacity premise behind the PaaS choice
proved wrong (the product owner has run Kubernetes professionally) — the reopening that produced
PROP-20260806-223656. The proposal holds the full rationale; this ADR records what was decided.

## Decision

1. **D1 — OVH Managed Kubernetes (MKS)**, Paris/EU region: GA (vs CKE's public beta), free control
   plane, free egress including object storage, same provider as the SMS hook and the vRack.
2. **D2 — PostgreSQL runs in-cluster via CloudNativePG**, with the operability conditions as part of
   the decision, not extras: **≥3 worker nodes, required pod anti-affinity, quorum-synchronous
   replication, continuous WAL archiving to OVH Object Storage, and a scheduled restore drill that
   actually executes**.
3. **D3 — `strategy: Recreate` until [#242](https://github.com/TheCaptainCompany/captain-food/issues/242)
   lands**: a RollingUpdate runs two write-path instances simultaneously, which
   [#193](https://github.com/TheCaptainCompany/captain-food/issues/193) forbids until the mailbox
   leases and fencing exist. Flipped only by a spec change once #242 is done.
4. **D4 — ingress-nginx + cert-manager, DNS-01 wildcard for `*.captain.food`** — and the **DNS zone
   HOSTING moves from Dynadot to OVH DNS** (nameserver change only; **Dynadot remains the
   registrar**), because no Dynadot cert-manager solver exists and the community OVH solver does.
   The DNS credential is zone-scoped and sealed.
5. **D5 — the Kubernetes manifests are GENERATED from the specs** (`configuration.yaml`,
   `observability.yaml`, `services.yaml`, C4) by `tools/codegen-rs`, and reconciled by a GitOps
   controller with self-heal — the cluster cannot drift from git, git cannot drift from the specs.
6. **D6 — build the cluster now and cut over once** (against the restore-first recommendation,
   recorded as such): the Render/Supabase deployment *"was a crash test"*; the outage window is
   accepted. **The data decision is explicit: production STARTS CLEAN** — empty schema, all
   migrations applied fresh, **no dump restore**. The crash-test data is discarded; the Supabase
   database is emptied at decommission (the GDPR posture of ADR-20260731-061609 §consequences,
   now with no copy made first).
7. **D7 — GitOps is the only change path.** The agent (assistant) operates production through
   repository changes reconciled by the controller; holds per-session, audited **read-only** cluster
   access and a SELECT-only `claude_ro` Postgres role for diagnosis; break-glass is a time-boxed,
   per-incident grant that always ends in a backfill PR. Deletion of PVCs, StatefulSets and
   namespaces sits outside every standing role. The ten operating practices are the proposal's §2b.

## Consequences

- **Realization backlog** (issues to be created under [#271](https://github.com/TheCaptainCompany/captain-food/issues/271)):
  MKS cluster + node pool (≥3) via the console or OpenTofu; GitOps controller install; the
  **manifests emitter** in `tools/codegen-rs` (D5 — the piece that is genuinely ours); CNPG cluster +
  WAL archiving + the weekly restore drill; ingress + cert-manager + the OVH webhook solver; sealed
  secrets; the NS migration (low TTLs first); `deploy.yml` retargeted to commit digest bumps to the
  GitOps path.
- **No dump/restore/checksum workstream** — deleted by the start-clean decision. The pending
  enum-text migrations simply exist in the fresh chain.
- [#242](https://github.com/TheCaptainCompany/captain-food/issues/242) slice 3's prod-gate becomes
  **"MKS cutover complete"**; #242 is also what unlocks RollingUpdate (D3) and multi-instance.
- Future Redis ([#267](https://github.com/TheCaptainCompany/captain-food/issues/267)) has two
  candidate homes — in-cluster or OVH managed — decided at realization, not here.
- Telemetry unchanged (Honeycomb EU); the OTel collector's placement (cluster vs in-process) is an
  open realization question in the proposal's §6.
- ADR-20260806-151122's factual findings (Clever Cloud prices, 10 TB egress, the Docker-runtime
  correction) remain valid reference; the PaaS stays the documented fallback if MKS disappoints.
