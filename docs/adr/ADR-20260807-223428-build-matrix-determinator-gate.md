# ADR-20260807-223428 — The build matrix + determinator gate: tool shape, workflow split, cook-cache fix

- **Status**: Accepted (realization detail of ADR-20260807-183024 step (5), executed under the
  coordinator dispatch for [#363 "deploy.yml targets the GitOps path" — realized as build matrix + determinator](https://github.com/TheCaptainCompany/captain-food/issues/363);
  PR [#386 "Build matrix + determinator"](https://github.com/TheCaptainCompany/captain-food/pull/386))
- **Extends**: [ADR-20260807-183024](ADR-20260807-183024-one-decomposition-axis.md) (step 5:
  "build matrix + determinator gate"), [ADR-20260807-220528](ADR-20260807-220528-deploy-emitter-pins-are-input.md)
  (pins are emitter input; #363 writes them), and the settled detection protocol in
  [PROP-20260806-223656](../proposals/PROP-20260806-223656-kubernetes-as-the-deployment-substrate.md)'s
  D5 addendum (fail open to rebuilding; `determinator` library; `{digest, source_hash}`
  repo-vs-repo compare; two-level skip).

## Decision

The protocol itself was settled with the product owner on #363; this ADR pins the realization
choices the protocol left open:

1. **The gate is a second binary of `tools/codegen-rs`** (`[[bin]] determinator`,
   `src/determinator_gate/`), not a separate crate or a workflow script. It shares the repo's
   one tooling package (same lints, same CI cache) but deliberately NOT the spec model: its
   inputs are `cargo metadata` (via `guppy`), the git tree, and the generated
   `deploy/generated/images.json` — the materialized contract, so the tool cannot disagree with
   what the emitter shipped. `default-run = "generate"` keeps bare `cargo run` (Makefile, hooks,
   ci.yml) meaning the generator. All decision logic lives in the tool where it is unit-tested
   (16 property tests assert the fail-open bias: unknown file → all bins, pin bump → nothing,
   domain scope → its linked bins only); the workflows only wire git plumbing to it and treat a
   non-zero exit as "everything affected".
2. **Two answer shapes for two questions, different biases toward the same safety.**
   `affected` (PR gate) is deliberately BROAD: the `determinator` library's affected set plus
   repo path rules where `specs/**` and `tools/**` mark everything (the dispatch rule: codegen
   touches everything) and anything unmatched fails open. `hash` (publish/deploy gate) is
   deliberately PRECISE: per-bin sha256 over the git blob shas of the bin's workspace-crate
   closure + the global inputs (`Cargo.toml`, `Cargo.lock` wholesale, `rust-toolchain.toml`,
   `.dockerignore`, `Dockerfile.bin`, `build-bins.yml`) + the bin's image name, `v1:`-prefixed
   so a recipe change rebuilds everything once. Precision is safe there because the compare is
   against recorded state: a seeded/missing/unreadable pin counts as CHANGED, and a closure dir
   with no tracked files is an ERROR, never an empty contribution.
3. **The workflows are new files, not edits** (`build-bins.yml`, `deploy-bins.yml`) —
   gate-then-stabilize: `build-image.yml` and `deploy.yml` (monolith, Render-targeting) stay
   byte-identical and authoritative; nothing applies the manifests until Argo (#366). The
   pin-bump path preserves ADR-20260730-051500's posture one level up: publishing per-bin
   images is automatic after green ci, WRITING THE LEDGER is a manual dispatch. deploy-bins
   refuses to pin when the tag is missing or the image's `food.captain.source-hash` label
   mismatches the computed hash — staleness can block loudly, never ship silently.
4. **`ARG BIN` moved AFTER the chef cook in `Dockerfile.bin`** (emitter fix): Docker keys a
   RUN's layer cache on every ARG declared earlier in the stage, so the previous placement gave
   every bin its OWN dependency cook — 49 cold cooks instead of the one shared cook the whole
   matrix design assumes. The publish loop builds changed bins sequentially in one job over one
   buildx GHA cache (`scope=bins`, separate from the monolith's) rather than fanning out matrix
   jobs that would each pay a cold cook; parallel fan-out over a pre-warmed cook is the
   recorded optimization path if the sequential worst case (a `Cargo.lock` ripple rebuilding
   all 49) ever hurts.
5. **Scope reconciliation recorded on the issue**: #363's original body (deploy.yml GitOps
   retarget, Render workflow retirement, prod-smoke at the LB) predates the decomposition ADR;
   Render retirement and smoke retargeting move with the cutover steps (6)–(7) (#358/#366).
   The wasm hydrate bundle is not yet a surface-image input (recorded on #349); when it lands,
   the `web` closure joins the surface bins' hash inputs — until then surface closures are the
   bin crate alone, which matches what the images actually contain.

## Consequences

- A docs/spec-only merge to `main` builds and publishes nothing (hash unchanged); a
  single-scope change builds that scope's bins only; `Cargo.lock`/kernel/codegen changes
  legitimately rebuild everything. PRs build/test exactly the affected set, full matrix on any
  doubt.
- The first real `publish` run builds all 49 bins (every pin is seeded null) — expected, and
  the cook is shared.
- New-bin completeness holds end to end: the emitter's bin↔image↔pin↔manifest test plus the
  tool's images↔workspace-package refusal and hash-totality check make an unmapped or
  unhashable bin a build failure, not a workload that silently never deploys.
- The new workflows are additive and non-required until proven on real runs (a workflow cannot
  be fully tested locally); the PR that lands them is the first live proof of the `affected`
  path.
