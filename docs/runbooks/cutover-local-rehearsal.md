# Runbook — rehearse the OVH cutover locally, on k3s

**Why this exists.** The founder will not pay for OVH until the team can show something that
actually deploys. This runbook is that demonstration, and it is repeatable: a single-node k3s stands
in for MKS long enough to run the **whole cutover sequence** — CNPG operator, `captain-db` Cluster,
`initdb`, the full migration chain, the monolith image, `/health`, the smoke — with no cloud account
and no money spent.

It is a **rehearsal**. [What it does not prove](#what-this-does-not-prove) is the most important
section on this page; read it before quoting any of this as evidence.

Related: [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) ·
[ADR-20260807-002705](../adr/ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md) ·
[ADR-20260811-004500](../adr/ADR-20260811-004500-role-paths-live-on-audience-hosts-api-host-is-a-webhook-address.md) ·
`deploy/platform/local-rehearsal/` · `deploy/generated/monolith/`

---

## 0. Prerequisites, and the two that will cost you an hour if you skip them

You need `dockerd` (to build an image), a `kubectl`, a `k3s` binary, and `sqlx` (prebuilt).

```bash
S=/tmp/rehearsal && mkdir -p "$S" && cd "$S"
curl -sSLo k3s     https://github.com/k3s-io/k3s/releases/download/v1.32.5+k3s1/k3s && chmod +x k3s
curl -sSLo kubectl https://dl.k8s.io/release/v1.36.3/bin/linux/amd64/kubectl        && chmod +x kubectl
# sqlx-cli, prebuilt -- the same shape CI uses (taiki-e/install-action), not a 10-minute cargo install.
curl -sSL https://github.com/cargo-bins/cargo-quickinstall/releases/download/sqlx-cli-0.8.3/sqlx-cli-0.8.3-x86_64-unknown-linux-gnu.tar.gz | tar xz
```

**`kind` and `k3d` do not work here.** Their double-nested `runc` dies with `can't get final child's
PID from pipe: EOF`. Do not spend a minute on them; run k3s directly on the host.

**Two flags are load-bearing.** Both cost an hour each the first time, and both present as a mystery.

```bash
"$S/k3s" server \
  --disable traefik --disable metrics-server --snapshotter native \
  --kubelet-arg='eviction-hard=nodefs.available<1%,imagefs.available<1%,nodefs.inodesFree<1%,imagefs.inodesFree<1%'
```

1. **`eviction-hard`** — without it the kubelet takes `DiskPressure` at ~9.6 G free and evicts every
   pod. You see a dozen `ContainerStatusUnknown` replicas and no explanation.
   (Do **not** add `eviction-minimum-reclaim=...=0%`: the kubelet refuses to start on
   `eviction percentage minimum reclaim nodefs.available must be positive: 0%`.)

   **Even with the flag, `DiskPressure` LATCHES.** Once it fires it does not clear when you free the
   disk — this kubelet's stats path is broken here (the same restriction that makes `kubectl logs`
   and `/stats/summary` return `EOF`), so the eviction manager never sees a fresh signal. Symptom:
   9 G free and `0/1 nodes are available: 1 node(s) had untolerated taint
   node.kubernetes.io/disk-pressure`. **The recovery is to restart k3s, not to wait.** And the image
   garbage collector will have deleted your locally-imported image on the way out, so re-import it
   (`k3s ctr -n k8s.io images import` — **`-n k8s.io` is required**; the default namespace is
   invisible to the kubelet). CNPG recovers on its own and its PVC survives: the schema is still
   there afterwards.

   **Therefore: do not run a workspace `cargo build`/`cargo test` while the rehearsal stack is up.**
   That is what fills the disk, and it costs a k3s restart plus an image rebuild every time.
2. **`restrict_oom_score_adj`** — if the sandbox refuses a *negative* `/proc/self/oom_score_adj`
   (check: `echo -998 > /proc/self/oom_score_adj` returns `Permission denied`), then **every pod
   sandbox** fails with the identical `can't get final child's PID from pipe: EOF` that kind/k3d
   give — so it looks like nesting, and it is not. The CRI plugin sets `-998` on each sandbox;
   clamp it:

   ```bash
   # First boot generates config.toml; copy it to the template and add the one line.
   sed 's|^  device_ownership_from_security_context = false|&\n  restrict_oom_score_adj = true|' \
     /var/lib/rancher/k3s/agent/etc/containerd/config.toml \
     > /var/lib/rancher/k3s/agent/etc/containerd/config.toml.tmpl
   # restart k3s
   ```

   The tell that this is your problem and not nesting: `k3s ctr run --rm --snapshotter native
   docker.io/library/alpine:3 probe /bin/echo hi` **succeeds** while every CRI pod fails.

Verify: `kubectl get nodes` → `Ready`, and `kubectl get pods -A` shows `coredns` and
`local-path-provisioner` **Running** (not `ContainerCreating`).

---

## 1. The platform: CNPG operator + the `captain-db` Cluster

```bash
export KUBECONFIG=/etc/rancher/k3s/k3s.yaml
kubectl apply -k deploy/platform/cnpg-operator --server-side   # CRD establishment races a client apply
kubectl -n cnpg-system rollout status deploy/cnpg-controller-manager

kubectl apply -f deploy/generated/monolith/namespace.yaml
kubectl apply -k deploy/platform/local-rehearsal                # NOT deploy/platform -- see below
kubectl get cluster captain-db -n captain-prod -o jsonpath='{.status.phase}'
# -> Cluster in healthy state
```

`deploy/platform/local-rehearsal/` is the **only** substitution: it swaps `captain-db-retain` (Cinder)
for k3s's `local-path` and drops the OVH-specific `backup:`/`managed:` blocks. Everything else —
`instances: 1`, `enableSuperuserAccess: false`, the digest-pinned PostgreSQL 17.10 image, the
production resource requests, `initdb` — is applied **verbatim**. The overlay's own header lists what
each removal means the rehearsal therefore does not prove.

## 2. The schema — the same path CI uses

This mirrors `.github/workflows/db-migrate.yml`'s `cnpg-port-forward` target step for step, so what
you rehearse is the job that will run at cutover.

```bash
PGPASS=$(kubectl -n captain-prod get secret captain-db-app -o jsonpath='{.data.password}' | base64 -d)
kubectl -n captain-prod port-forward svc/captain-db-rw 15432:5432 &
DATABASE_URL="postgresql://app:${PGPASS}@127.0.0.1:15432/app" "$S/sqlx" migrate run --source migrations
```

Expect the full chain to apply against the empty database, ending at `20260812000000`. Verify:

```sql
select max(version), count(*) from _sqlx_migrations where success;   -- 20260812000000 | 46
```

> **These two numbers go stale every time a migration lands** — they did once already, between the
> first rehearsal (45 / `20260810113000`) and this merge, because
> [#500](https://github.com/TheCaptainCompany/captain-food/pull/500) added
> `20260812000000_drop_command_journal.sql`. Do not trust them; derive them:
> `ls migrations/*.sql | wc -l` and the last entry of `ls migrations/*.sql | sort`, which must equal
> `REQUIRED_SCHEMA_VERSION` in `crates/server/src/lib.rs` (a codegen test asserts that equality).

> `kubectl port-forward` is flaky here: it drops after the first connection with
> `an error occurred forwarding ... connection reset by peer`. Re-establish it per command rather
> than holding one open. The app itself never needs it — it reaches
> `captain-db-rw.captain-prod.svc:5432` in-cluster, exactly as in production.

## 3. The secret — and why the pod will refuse to start

`deploy/generated/secret-keys.json` **is** the checklist; build `captain-secrets` from it, never from
memory:

```bash
kubectl -n captain-prod create secret generic captain-secrets \
  --from-literal=DATABASE_URL="postgresql://app:${PGPASS}@captain-db-rw.captain-prod.svc:5432/app" \
  --from-literal=... # one --from-literal per key in secret-keys.json
```

Two things this step teaches, both of which are cutover-day facts:

- **Every key must be present.** The generated Deployment references each with a plain
  `secretKeyRef` (no `optional: true`), so one missing key means the pod never starts.
- **Every value must be well-formed.** The `production` profile's typed config gate refuses a
  malformed value at boot and starts *nothing*, printing the expected pattern for each. Placeholders
  are fine for a rehearsal, but they must have the right SHAPE:
  `AUTH_SESSION_KEY` = 64 hex chars, `HONEYCOMB_API_KEY` = `[A-Za-z0-9_]{20,120}`,
  `STRIPE_WEBHOOK_SECRET` = `whsec_...`.

## 4. The workload — the generated monolith overlay

```bash
kubectl apply -k deploy/generated/monolith
```

The manifest renders `image: ghcr.io/thecaptaincompany/captain-food:unpinned`, which is
**deliberately undeployable** — CI's pin-bump supplies the digest. Locally, build an image and
override that one field:

```bash
cargo build -p server                       # debug; the release build does not fit a small disk
mkdir -p "$S/img" && cp target/debug/server "$S/img/"
cat > "$S/img/Dockerfile" <<'EOF'
FROM ubuntu:24.04
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*
COPY server /usr/local/bin/server
ENV CAPTAIN_BUILD_VERSION=rehearsal
CMD ["server"]
EOF
docker build -t captain-food-rehearsal:local "$S/img"
docker save captain-food-rehearsal:local -o "$S/r.tar" && "$S/k3s" ctr images import "$S/r.tar"

kubectl -n captain-prod set image deploy/server server=docker.io/library/captain-food-rehearsal:local
kubectl -n captain-prod patch deploy/server --type=json \
  -p='[{"op":"replace","path":"/spec/template/spec/containers/0/imagePullPolicy","value":"Never"}]'
```

`ubuntu:24.04`, not the real image's `debian:bookworm-slim`, because the host-built binary links the
host glibc. Verify:

```bash
IP=$(kubectl -n captain-prod get pod -l app.kubernetes.io/name=server -o jsonpath='{.items[0].status.podIP}')
curl -s "http://$IP:8080/ping"     # pong
curl -s "http://$IP:8080/health"   # {"status":"ok","db":"up",...,"requiredSchemaVersion":20260812000000}
```

`kubectl logs` does **not** work here (the kubelet's `:10250` endpoint returns `EOF`). Read
`/var/log/pods/<ns>_<pod>*/<container>/*.log` on the host instead — that is how the config-gate
failure above is diagnosed.

## 5. The smoke

Nothing serves the Ingress locally (no ingress-nginx —
[#362](https://github.com/TheCaptainCompany/captain-food/issues/362)), so point the real hostnames at
the pod. `classify_host` hard-codes the `captain.food` apex, so the literal names are required:

```bash
echo "$IP live.captain.food system.captain.food smoke-test.captain.food api.captain.food captain.food" >> /etc/hosts
SMOKE_BASE_DOMAIN="captain.food:8080" SMOKE_SCHEME=http bash tools/smoke/prod-smoke.sh
```

**Undo the `/etc/hosts` line when you are done** — it shadows production for every tool on the box.

Layers L1 (`/ping`, `/health`, schema version) and L2 (public GraphQL introspection on the tenant
host) pass against a fresh, empty database. **L3 onward need `SUPABASE_SECRET_KEY`** to mint role
JWTs; without it the run stops at L3 with that named reason. No seed data is needed — L3 creates its
own fixture through the ADMIN GraphQL and waits on the projection, so an empty database is correct.

---

## What this does **not** prove

An overclaimed rehearsal is worse than none, because a spending decision rests on it. This exercise
says nothing about:

- **OVH itself** — MKS node behaviour, the vRack, Cinder volume provisioning, `Retain` semantics,
  volume expansion, or OVH's stock StorageClass parameter names.
- **Backup and restore.** `barmanObjectStore` is removed by the overlay, so WAL archiving, base
  backups and the restore drill are entirely untested — and at `instances: 1` that is the *only*
  recovery path. This is the largest gap.
- **DNS, TLS and the LoadBalancer.** No ingress-nginx, no cert-manager, no certificate issuance, no
  external IP. The Ingress object is applied and parsed; nothing serves it. Reaching the app by pod
  IP proves the workload, not the edge.
- **High availability.** One instance, one node, no failover, no PDB behaviour under drain.
- **The production image.** The rehearsal runs a debug binary in an ad-hoc image. The real
  cargo-chef release build (with the wasm hydrate bundle) is exercised by
  `.github/workflows/build-image.yml`, not here.
- **The money path.** L3/L4 did not run. Stripe webhook delivery, the place-order saga and the
  capture→projection path are untested by this rehearsal.
- **Performance, at peak or otherwise.** One pod, one user, no load.
