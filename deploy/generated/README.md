# deploy/ -- the GENERATED deployment (ADR-20260807-183024 step 4, #349)

Everything under `deploy/generated/` is EMITTED by `make generate` from the specs (bin topology =
crate-graph + c4-l2; env = the typed configuration keys; ingress = the screens specs' hosts and
roles) and drift-checked by `make validate` / CI -- never hand-edited. `deploy/pins/` is the ONE
exception: the deploy LEDGER, written by CI (one `{digest, source_hash}` pin-bump commit per
deploy, #363/#371), read by the emitter to bake image digests into the Deployments. Rollback of
one image = `git revert` of its pin change; `git log -- deploy/pins` is the deployment history.

GATE-THEN-STABILIZE: nothing applies this tree today. Argo CD (#366) will reconcile
`deploy/generated/manifests/` only when steps (6)-(7) of the ADR flip deployment, with the
product owner live at the console; until then the monolith `server` deployment remains the
runtime. The bins behind these manifests are probe-serving shells (/health + /ping) -- their
business runtime wiring (mailbox hosting, per-scope projection, subgraph slices, gateway
composition tables) is tracked on #349 and blocks the flip.

Bins: 56. Build one image: `docker build -f deploy/generated/Dockerfile.bin --build-arg
BIN=<bin> .` Render the manifests: `kustomize build deploy/generated/manifests`.
