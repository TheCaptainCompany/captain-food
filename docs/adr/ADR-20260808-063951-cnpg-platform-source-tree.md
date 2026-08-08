# ADR-20260808-063951 — CNPG manifests are hand-written platform SOURCE under `deploy/platform/`, not emitted

- **Status**: Accepted (executor decision under the
  [#360 "CNPG: operator + 3-instance cluster, WAL archiving to Object Storage, weekly executed restore drill"](https://github.com/TheCaptainCompany/captain-food/issues/360)
  dispatch, which delegated exactly this judgement)
- **Context**: [ADR-20260807-002705](ADR-20260807-002705-hosting-ovh-mks-cnpg-gitops.md) D2/D5,
  amended by [ADR-20260807-114122](ADR-20260807-114122-mks-starts-at-one-node.md);
  realized by [PR #392 "CNPG platform tree"](https://github.com/TheCaptainCompany/captain-food/pull/392)

## Decision

1. **`deploy/platform/` is a SOURCE tree** — the same status as `specs/architecture/*.yaml` and
   `specs/observability.yaml` — sitting beside the emitter-owned `deploy/generated/`. D5 ("the
   manifests are generated from the specs") governs what the specs DERIVE: bin topology, env,
   ingress. The database cluster derives from nothing — its shape (instance count, replication
   mode, storage class, retention, drill cadence) is a set of operational decisions recorded in
   ADRs, exactly like C4. Emitting it would mean inventing a `database-topology.yaml` spec with a
   single consumer; the moment a second consumer appears (e.g. capacity math in the validator) is
   the moment to promote it — not before.
2. **What replaces drift-checking for hand-written manifests is a codegen test suite**
   (`platform_*` in `tools/codegen-rs/src/tests.rs`): every YAML document parses (kubeconform is
   unavailable in the container), the vendored operator matches its `PIN.json` sha256, no
   document is a `Secret`, the entry/HA replication-safety pair holds (`instances: 1` ⇒ no
   `synchronous`; the `ha/` overlay carries `instances: 3` + quorum-sync + strict durability,
   and stays unreferenced), and the drill's recovery source mirrors the production archive
   (destination, endpoint, image pin) with no `backup:` stanza and no Retain class.
3. **Third-party pins are content-addressed in-repo**: the operator release is vendored
   byte-identical with url+sha256 recorded (checkable offline); images are digest-pinned in the
   manifests themselves.

## Options considered

- **Force it through `tools/codegen-rs`** — rejected: a spec invented solely to be emitted back
  out is ceremony, not derivation; it would dilute the "specs are the source of truth" claim by
  adding a spec nothing else reads.
- **Reference the operator by remote URL (kustomize remote base / Argo helm chart)** — rejected:
  a URL is a pointer, not a pin; the repo could no longer prove WHAT it deploys offline, and the
  public-repo GitOps posture (§2b practice 1) wants the desired state reviewable in the diff.
