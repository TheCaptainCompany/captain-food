# deploy/ -- the GENERATED deployment (ADR-20260807-183024 step 4, #349)

Everything under `deploy/generated/` is EMITTED by `make generate` from the specs (bin topology =
crate-graph + c4-l2; env = the typed configuration keys; ingress = the screens specs' hosts and
roles) and drift-checked by `make validate` / CI -- never hand-edited. `deploy/pins/` is the ONE
exception: the deploy LEDGER, written by CI (one `{digest, source_hash}` pin-bump commit per
deploy, #363/#371), read by the emitter to bake image digests into the Deployments. Rollback of
one image = `git revert` of its pin change; `git log -- deploy/pins` is the deployment history.

TWO TREES, ON PURPOSE, while the cutover is in flight:

- `manifests/` -- the per-bin topology we are moving TO. GATE-THEN-STABILIZE: nothing applies it
  today. Argo CD (#366) reconciles it only when steps (6)-(7) of the ADR flip deployment, with the
  product owner live at the console. The bins behind these manifests are probe-serving shells
  (/health + /ping) -- their business runtime wiring (mailbox hosting, per-scope projection,
  subgraph slices, gateway composition tables) is tracked on #349 and blocks the flip.
- `monolith/` -- the workload we ACTUALLY deploy (#358): the single `server` process, serving every
  host and every role path. `kubectl apply -k deploy/generated/monolith` is the whole of deploying
  Captain.Food as it runs today, minus the sealed `captain-secrets` (checklist: secret-keys.json)
  and the schema, applied out-of-band by sqlx-cli (ADR-0043). It exists because the repo used to
  describe a future cluster in per-bin manifests and could not describe the one workload a cutover
  has to move. It is emitted from the c4-l2 `server` container's `deploy_tree: monolith`, so
  deleting that container is what retires the monolith -- and prunes this overlay with it.
  `api.captain.food` is served HERE and deliberately not in `manifests/`: it is the registered
  partner webhook address, and it outlives the cutover until those are re-registered on
  `hooks.captain.food` (ADR-20260811-004500).

Rehearse the whole sequence locally before spending anything:
docs/runbooks/cutover-local-rehearsal.md.

Bins: 58. Build one image: `docker build -f deploy/generated/Dockerfile.bin --build-arg
BIN=<bin> .` Render the manifests: `kustomize build deploy/generated/manifests`.
