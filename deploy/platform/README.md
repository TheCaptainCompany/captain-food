# deploy/platform/ -- hand-written PLATFORM SOURCE (CNPG database + restore drill)

This tree is **source, not generated** — the same status as `specs/architecture/*.yaml` (C4) and
`specs/observability.yaml`. Nothing in the specs DSL derives a database topology, an operator
version pin, or a restore rehearsal, so forcing this through `tools/codegen-rs` would manufacture a
spec for the sole purpose of emitting it back out (decided at [#360 "CNPG: operator + 3-instance
cluster, WAL archiving to Object Storage, weekly executed restore
drill"](https://github.com/TheCaptainCompany/captain-food/issues/360)). What IS enforced by the
codegen test-suite: every YAML document in this tree parses, the operator vendor file matches its
recorded upstream sha256 pin, and the safety invariants below hold (see the
`platform_*` tests in `tools/codegen-rs/src/tests.rs`).

**GATE-THEN-STABILIZE: nothing applies this tree today.** Argo CD
([#366](https://github.com/TheCaptainCompany/captain-food/issues/366)) reconciles it when steps
(6)-(7) of [ADR-20260807-183024](../../docs/adr/ADR-20260807-183024-one-decomposition-axis.md)
flip deployment, with the product owner live at the console. Until then this is the reviewed,
ready-to-apply desired state — and the secrets it references DO NOT EXIST in the repo (by name
only, sealed later per PROP-20260806-223656 §2b practice 7): a missing secret renders as a pod
that visibly fails to start (`CreateContainerConfigError`) or a CNPG cluster that reports the
missing resource, never as a placeholder value.

## Layout

| path | what | applied by |
|---|---|---|
| `cnpg-operator/` | CloudNativePG operator **1.27.4**, vendored verbatim from the upstream release (`PIN.json` records url + sha256; the vendor file is byte-identical to upstream so the pin is checkable) | console session first (CRDs before CRs), then Argo CD |
| `cnpg/` | The `captain-db` Cluster (entry shape per [ADR-20260807-114122](../../docs/adr/ADR-20260807-114122-mks-starts-at-one-node.md): `instances: 1`), `captain-db-retain` StorageClass (`reclaimPolicy: Retain`), daily ScheduledBackup, barman WAL archiving to OVH Object Storage | Argo CD (console session at cutover) |
| `cnpg/ha/` | The **LADDER** overlay — `instances: 3`, quorum-synchronous replication (ADR-20260807-114122: climb when [#242](https://github.com/TheCaptainCompany/captain-food/issues/242) lands or the first paying restaurants arrive). **Deliberately unreferenced** by any kustomization: flipping it is a separate one-line ADR, never a hand apply | nothing (gated) |
| `restore-drill/` | Weekly restore drill (§2b practice 4): CronJob restores the latest backup into the standing scratch namespace `captain-restore-drill`, verifies counts + checksums against production over the SAME event-log range, files a GitHub issue on failure; plus an hourly WAL-archiving / backup-age check | Argo CD |

## Why the database manifests are not emitted

`deploy/generated/**` is derived: bin topology = crate graph + c4-l2, env = typed configuration
keys, ingress = screens hosts. The database cluster has NO upstream spec — its shape is a set of
operational decisions (instance count, replication mode, storage class, retention) recorded in
ADRs, exactly like C4. Emitting it would mean inventing a `database-topology.yaml` spec no other
consumer reads. If a second consumer ever appears (e.g. capacity math in the validator), that is
the moment to promote it to a spec — not before.

## Safety invariants (pinned by codegen tests)

- The operator vendor file's sha256 matches `cnpg-operator/PIN.json` (a silent re-vendor is a
  supply-chain event, not a diff).
- `cluster.yaml` at `instances: 1` carries **no** `synchronous` block (quorum-sync with zero
  replicas freezes every write); the `ha/` overlay carries **both** `instances: 3` and the
  `synchronous` block (3 instances without quorum-sync can acknowledge a paid order the primary
  then loses — PROP-20260806-223656 §2b practice 3).
- `enableSuperuserAccess: false` on every Cluster in the tree.
- The drill cluster bootstrap has **no `backup` section** (a drill cluster that archives into the
  production destination path corrupts the production WAL archive) and uses the default
  delete-reclaim storage class, NOT `captain-db-retain` (a weekly drill on a Retain class leaks
  one orphaned Cinder volume per run).
- No YAML document in this tree is `kind: Secret` (values live in the sealed store, never here).

## Console-session checklist (the OUT-of-scope half of #360 — product owner at the console)

1. **Create the OVH Object Storage bucket** (S3, EU): name `captain-food-db-backups`, region
   Paris (`eu-west-par`). If the real region/endpoint differs from
   `https://s3.eu-west-par.io.cloud.ovh.net` in `cnpg/cluster.yaml`, fix the manifest in a PR
   FIRST (GitOps: the manifest is the record) — do not create the bucket to match a wrong manifest.
2. **Provision secrets** (sealed per #362; names must match exactly, all in `captain-prod` unless
   stated):
   - `cnpg-object-storage`: keys `ACCESS_KEY_ID`, `ACCESS_SECRET_KEY` (an OVH S3 user scoped to
     the bucket, read+write; the drill copies this secret into `captain-restore-drill` each run).
   - `claude-ro-credentials`: keys `username` (= `claude_ro`), `password` (CNPG `managed.roles`
     reads it; the migration grants SELECT — §2b practice 5).
   - `restore-drill-github-token` in **`captain-restore-drill`**: key `token` — a fine-grained PAT,
     `issues: write` on this repo ONLY (it files drill-failure issues).
3. **Verify the StorageClass parameters** against the cluster's stock class before applying:
   `kubectl get sc csi-cinder-high-speed -o yaml` — `cnpg/storageclass.yaml` clones its
   provisioner/parameters with `reclaimPolicy: Retain`; OVH may rename parameters between MKS
   versions.
4. **Apply order**: `kubectl apply --server-side -f deploy/platform/cnpg-operator/cnpg-1.27.4.yaml`,
   wait for `cnpg-system/cnpg-controller-manager` ready, then let Argo CD (#366) take
   `deploy/platform/` (or `kubectl apply -k deploy/platform/` once, before Argo exists).
5. **Watch the first backup**: `ScheduledBackup` fires immediately (`immediate: true`);
   `kubectl -n captain-prod get backup` until completed, then check
   `kubectl -n captain-prod get cluster captain-db -o jsonpath='{.status.firstRecoverabilityPoint}'`
   is non-empty. No recoverability point = no restore drill can ever pass.
6. **Fire the drill once, supervised** (practice 10: a runbook is trusted only after it has been
   executed): `kubectl -n captain-restore-drill create job --from=cronjob/restore-drill
   restore-drill-manual`, watch it end-to-end, record the date on #360.
7. **Re-source DATABASE_URL at cutover** (#358/#366 sequencing): the app secret's `DATABASE_URL`
   flips to the CNPG `captain-db-app` credentials; the `db-migrate` retarget gate
   (`target: cnpg-port-forward` workflow input) is exercised then — flipping its DEFAULT is a
   separate one-line ADR.
