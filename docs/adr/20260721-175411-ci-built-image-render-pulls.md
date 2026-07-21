# ADR-20260721-175411 — Build the server image in CI (GHCR); Render only pulls it

<!-- Filename: docs/adr/20260721-175411-ci-built-image-render-pulls.md — UTC date-time id. -->

## Status

Accepted (amends ADR-0042 — the *build/deploy mechanism* only; hosting decision unchanged)

## Context

ADR-0042 hosts the Axum BFF on **Render** and builds it there from the repo via the cargo-chef
`Dockerfile` (`runtime: docker`, `autoDeployTrigger: checksPass`). Render has since started metering
**build-pipeline minutes** as a separately-billed resource with its own spend cap. Our account's cap sits
at **$0.00**, so once the free included minutes are exhausted Render **kills the build and the deploy
fails** ("your most recent build failed … reached its custom build pipeline minute spend limit of $0.00").

Two forces made this bite hard:

- **Rust builds are long.** Even with the cargo-chef dependency-layer cache, a cold or dependency-changing
  build of the workspace is many minutes on Render's build host.
- **High merge frequency.** The operating model (CLAUDE.md: claim ⇒ draft PR ⇒ supervised auto-merge)
  merges to `main` often, and *every* merge triggered a full Render build — including the many merges that
  only touch `specs/**`, `docs/**`, ADRs, or the codegen tool and do **not** change the deployed binary.

So we were paying (in failed deploys) to recompile Rust on Render on a cadence Render's free build pipeline
was never going to sustain. Meanwhile **GitHub Actions is free and unlimited on standard runners for public
repositories** — and this repo is public.

## Decision

**Move the build off Render into CI, and make Render a pure image-runner.**

- **Build in GitHub Actions** (`.github/workflows/build-image.yml`): build the same cargo-chef `Dockerfile`
  with buildx, using a GitHub Actions layer cache (`type=gha`) so the dependency layer persists across runs.
  Push to **GHCR** (`ghcr.io/captain-food/captain-food`) with two tags: an immutable `sha-<commit>` (the
  artifact each deploy pins) and a moving `latest` (the blueprint's bootstrap default).
- **Gate it like migrations** (ADR-0043): the workflow runs on `workflow_run` after the `ci` workflow
  concludes **success on `main`**, so no image is ever published for a commit that fails build, tests, or
  spec-validation.
- **Render pulls, never builds** (`render.yaml`): `runtime: image` with `image.url` pointing at GHCR, and
  `autoDeploy: false`. Deploys are triggered by the workflow calling a **Render deploy hook** with
  `imgURL=…@sha256:<digest>` so Render runs the exact image CI just built. Render spends **zero
  build-pipeline minutes**.
- **Deploy by immutable digest, never a moving tag** (best practice). Production never resolves `latest`:
  the deploy hook pins the **content-addressed digest** (`@sha256:…`, from `build-push-action`'s output),
  which is cryptographically immutable and cannot be repointed — strictly stronger than a tag (tags can be
  moved). Images also carry a human-readable immutable `sha-<commit>` tag for git correlation. `render.yaml`
  keeps a `:latest` reference ONLY as the blueprint bootstrap seed (the first pull when a blueprint is
  created); with `autoDeploy: false`, no running deploy ever resolves it. The version-of-record is Render's
  deploy/event history (it names the exact digest even for a container that never starts). Rollback = hit
  the deploy hook with a prior `sha-<commit>`/digest — no rebuild. (A full-GitOps variant — CI writes the
  deployed digest back into `render.yaml` so the repo is the source of truth — was rejected for now: it
  needs CI push access to the protected `main` and adds a chore commit per deploy at our high merge cadence.)

The ordering property from ADR-0043 is preserved unchanged: db-migrate and build-image both fire off the
same green `ci` run; if a deploy races ahead of its migration, the app's `/health` schema-version gate
holds it at 503 until the migration lands.

**Precise build identity (diagnostics).** The **short (7-char) commit SHA** is the human-readable version
(e.g. `829f4ad`, matching what GitHub displays); the image **digest** remains the exact machine identity.
It flows end-to-end: the workflow tags the image `sha-<short>` and passes the short SHA as the
`CAPTAIN_BUILD_VERSION` Docker build-arg, which the `Dockerfile` bakes into the runtime image as an env
var + OCI labels (`org.opencontainers.image.revision`); the server reports it as `version` at `/health`.
The build-arg is declared only in the Dockerfile's final stage, so a new SHA changes only trailing metadata
layers and never invalidates the cached cargo-chef build. (Deploys still pin the immutable **digest**, not
this tag — the short SHA is a label, so its 7-char abbreviation costs no precision.)

Because a health endpoint is useless when the instance never starts, the version is discoverable at **three
layers**, each covering the failure mode the one above it cannot:
1. **`X-VERSION` response header + `/health` `version` field** — the running build's short SHA is stamped on
   **every** HTTP response (the `response_timing` middleware) and echoed in the `/health` body in every state
   (incl. `degraded`/`down`), so any client reading any endpoint knows which deploy served it, without a
   dedicated call. (Previously `/health` reported only the DB schema version, not the app build.)
2. **Startup log line** — `main()` prints `captain-food server starting — version <sha>` as its *first*
   statement, before any fallible startup (router build, port bind, DB probe), so a process that panics or
   never binds still names its version in the Render logs.
3. **The pinned image digest** — deploys pin the immutable `@sha256:<digest>` via the deploy hook, so Render's
   deploy/event record names the exact image even for a container that never execs at all (bad image, exec
   error). This is the platform-side source of truth, independent of the app running.

## Rollback

Because every deploy runs an immutable, content-addressed image (never a moving tag), rolling back is
just **redeploying a previous image** — no rebuild, no revert commit, no CI run:

1. Find the target build's reference — Render's **deploy/event history** (each entry names its digest), or
   the `render commit` badge / `render-status` workflow (`<status> @ <sha>`), or the GHCR package tags
   (`sha-<commit>`).
2. Re-hit the Render deploy hook pinning that reference:

   ```bash
   # by the human-readable immutable tag
   curl -fsS -X POST "$RENDER_DEPLOY_HOOK_URL&imgURL=ghcr.io/captain-food/captain-food:sha-<previous-commit>"
   # strongest form — pin the exact digest (cannot be repointed)
   curl -fsS -X POST "$RENDER_DEPLOY_HOOK_URL&imgURL=ghcr.io/captain-food/captain-food@sha256:<digest>"
   ```

3. Confirm: read `version` from `/health` (`https://live.captain.food/health`) — it must report the
   rolled-back commit. If the container never comes up, the deployed digest still shows in Render's events.

Notes: a failed deploy leaves the previous instance serving (Render cuts over only after the new image
passes the health check), so a *forward* deploy that never goes healthy is a self-rollback. A rollback
across a **schema migration** is safe by design — the `/health` gate is `>=` (ADR-0043), so an older app
still runs against a newer DB; migrations are expand/contract and never destructive in the same release.

## Alternatives considered

- **Raise Render's build-pipeline spend limit.** Simplest, but pays per-minute to recompile Rust on
  *every* deploy forever, on a build host we don't control the cache ergonomics of. Rejected: recurring
  cost for the slowest possible build, and merge-frequency makes it worse.
- **Keep building on Render but add a `buildFilter`** so only `crates/**` / root build inputs trigger a
  build (spec/doc/tooling merges skip it). This genuinely removes the *wasteful* builds and is a valid
  lighter-touch fix; it was prototyped on a branch. Rejected as the primary approach because it still runs
  slow Rust builds *on Render's metered pipeline* for every real code change — it narrows the bleeding
  rather than stopping it. The CI-built-image path makes build cost structurally $0 on Render regardless of
  merge cadence, and gives us control over the build cache. (The buildFilter remains a useful fallback if
  we ever revert to `runtime: docker`.)
- **GitHub Actions → deploy to Render via the Render API instead of a deploy hook.** Equivalent; the deploy
  hook is a single secret URL with no extra token scopes, so it is the smaller surface. The `RENDER_API_KEY`
  we already hold (prod-smoke) could drive this later if we need richer deploy control.
- **Push to Docker Hub / a Render-native registry** instead of GHCR. GHCR is free for public images, needs
  no extra account, and authenticates in-workflow with the built-in `GITHUB_TOKEN` — least friction.

## Consequences

### Positive

- **Render build cost is structurally $0** — it only pulls an image; merge frequency no longer matters.
- **Faster, cache-controlled builds** in CI (buildx `type=gha` + cargo-chef), free/unlimited on this public
  repo's standard runners.
- **Traceable, rollback-friendly deploys**: every deploy pins an immutable `sha-<commit>` image; rolling
  back is redeploying a previous tag (no rebuild).
- **Same safety gate**: image publish is gated on green `ci`, mirroring db-migrate — no drift, no untested
  image shipped.
- **No wasted deploys on docs-only merges**: the workflow diffs the merge and skips build+deploy when only
  specs/docs/ADRs/tooling changed (fail-safe: builds if the diff is unavailable), so the many non-code
  merges don't churn an identical image. The live `X-VERSION`/`version` then tracks the last *code* commit,
  which is correct — the running binary didn't change.

### Negative

- **The Rust image is compiled twice per green `main`**: once as the CI `cargo build`/tests, once as the
  release Docker image. Both are on free CI; acceptable. (Could be unified later by building the image
  inside `ci` and gating differently, at the cost of a more complex single workflow.)
- **New moving parts / secrets to operate**: a GHCR package whose visibility must stay **public** (or carry
  a Render `registryCredential`), and a `RENDER_DEPLOY_HOOK_URL` repo secret. If the hook secret is missing
  the workflow fails loudly (by design) rather than silently not deploying.
- **The Render service must be switched from `runtime: docker` to `runtime: image`** — a one-time blueprint
  re-sync / service reconfigure (see Follow-up).

### Follow-up actions

- [ ] In Render, either sync the blueprint or set the `captain-food` service to **Deploy an existing image**
      = `ghcr.io/captain-food/captain-food:latest`, and turn **Auto-Deploy off**.
- [ ] Create the service's **Deploy Hook** and store it as the GitHub repo secret `RENDER_DEPLOY_HOOK_URL`.
- [ ] Set the GHCR package `captain-food/captain-food` visibility to **Public** (or add a `registryCredential`).
- [ ] Verify end-to-end: merge to `main` → `ci` green → `build-image` pushes `sha-<commit>` → Render deploys
      it → `/health` returns `db:up` with the schema gate satisfied.
- [ ] Update ADR-0042's operational note (done in this change) and, once verified live, record the first
      image-pull deploy commit here.
