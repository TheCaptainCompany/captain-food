# ADR-20260807-220528 — The deploy emitter: pins are generator INPUT, env is scope-derived, bins are probe-serving shells

- **Status**: Accepted (realization detail of ADR-20260807-183024 step (4), executed under the
  coordinator dispatch for [#349 "Derive deployment artifacts from the existing specs"](https://github.com/TheCaptainCompany/captain-food/issues/349);
  PR [#384 "Codegen emits the deployment"](https://github.com/TheCaptainCompany/captain-food/pull/384))
- **Extends**: [ADR-20260807-183024](ADR-20260807-183024-one-decomposition-axis.md) (step 4:
  "emitter — manifests, Dockerfile targets, `{digest, source_hash}` pins") and
  [PROP-20260806-223656](../proposals/PROP-20260806-223656-kubernetes-as-the-deployment-substrate.md)
  D5 + its addendum (generated manifests, pin ledger, per-bin images).

## Decision

Step (4) is realized with four micro-decisions that were not pinned by the approved design, each
recorded here so the next session inherits them as choices, not accidents:

1. **The pin ledger is emitter INPUT; digests are baked into the generated Deployments.**
   `deploy/pins/{bin}.json` (`{digest, source_hash}`) is CI-owned state (product-owner directive
   2026-08-07: git is the deploy ledger): the emitter READS it and writes the digest into the
   Deployment's `image:`, seeds missing pins with nulls, and never overwrites or prunes one. A
   null pin renders the deliberately-undeployable `:unpinned` tag. Consequence: a pin bump must
   run `make generate` and commit pins + regenerated manifest together — the pin-bump commit IS
   the manifest change Argo syncs, `git revert` of it is per-image rollback, and the generated
   tree stays deterministic from (specs + pins) so `check-drift` still holds. The alternative —
   kustomize `images:` transformers over digest-free manifests — was rejected because it splits
   the desired state across two mechanisms and makes the generated tree lie about what runs.
2. **Env blocks carry only secret-sourced keys, routed by scope membership — with ONE
   family-based exception.** Non-secret keys are baked per profile (PROP-20260729-014500 D5), so
   a pod's env is: `APP_PROFILE` + `PORT` + every production `deploy.from_secret` key whose
   configuration fragment scope is in {bin's linked scopes + its owning scope + common}, as
   `secretKeyRef` into the sealed `captain-secrets` (contract emitted as `secret-keys.json`).
   `DATABASE_URL` is additionally withheld from the `gateway`/`surface` families — D8 makes "no
   DB access" part of those families' definition, so the pod never holds the credential it must
   not use — UNLESS the bin has a declared c4-l2 relationship to `event-store`/`read-models`
   (the rule that routes `adapters`, whose ACLs record inbound facts through the mailbox; found
   and fixed in the architect review pass). Everything finer (e.g. OVH SMS keys reaching bins
   that never send SMS) waits on the per-bin generated `Config` reader —
   [#374](https://github.com/TheCaptainCompany/captain-food/issues/374) unresolved question 4 —
   and is deliberately NOT hand-curated here.
3. **Ingress hosts and role paths are derived from the screens specs**: each surface's host is
   its screens file's `base_url`, its `/{role}/graphql` paths the union of its screens' `roles`
   (role = path, ADR-0006); `fo-storefront` serves the tenant wildcard `*.captain.food`. Two
   recorded gaps rather than guesses: the BARE `captain.food` host is unrouted (the two
   front-office screens files disagree on its owner), and the integration paths
   (`/webhooks`, `/adapters`, `/services`, `/external/graphql`) ride the marketplace host until a
   spec names a dedicated integration host.
4. **The 49 bins are PROBE-SERVING SHELLS, not business runtimes.** Each binds `$PORT`, serves
   the `/health` + `/ping` its generated Deployment probes (reporting `wired:false` honestly),
   and drains on SIGTERM — so the emitted probe contract is real and an applied manifest would
   not crash-loop. The business wiring (actor bins hosting their mailbox worker via
   `actor_runtime`, per-scope projection filtering, subgraph schema slices, gateway composition
   tables) is NOT part of step (4): the monolith's composition root cannot be extracted without
   touching what currently runs, which gate-then-stabilize forbids in the same change. It is
   recorded on #349 as the remainder and BLOCKS the steps (6)–(7) flip.

## Consequences

- `deploy/generated/**` joins the never-hand-edited generated surface (pruned + drift-checked);
  `deploy/pins/` is the one CI-owned directory under `deploy/`, guarded by the codegen
  completeness test (bin ↔ image ↔ pin ↔ manifest, both ways; stale pins fail the build).
- Nothing applies the tree yet: no Argo CD ([#366](https://github.com/TheCaptainCompany/captain-food/issues/366)),
  no CI apply step; the monolith `server` deployment remains the runtime.
- [#363](https://github.com/TheCaptainCompany/captain-food/issues/363)'s build matrix consumes
  `images.json` + `Dockerfile.bin` and writes the pins;
  [#358](https://github.com/TheCaptainCompany/captain-food/issues/358) seals `captain-secrets`
  from `secret-keys.json`.
