# ADR-20260729-020000 — Non-secret configuration rides the artifact; secrets ride CI; the dashboard owns nothing

- **Status**: Accepted
- **Date**: 2026-07-29
- **Issue**: [#248 "CI owns the Render service configuration"](https://github.com/TheCaptainCompany/captain-food/issues/248)
- **Proposal**: [PROP-20260729-014500](../proposals/PROP-20260729-014500-ci-owns-the-render-service-configuration.md) — **all five decisions approved** in-session, 2026-07-29 (*"Yes to all recommendations, implement it"*)
- **Refines**: [ADR-20260729-010500](ADR-20260729-010500-configuration-is-declared-and-fails-fast.md) (the declaration this makes authoritative), [ADR-0042](0042-render-hosting.md) (hosting), [ADR-20260721-175411](20260721-175411-prebuilt-image-and-digest-pinned-deploys.md) (digest-pinned deploys)

## Context

[ADR-20260729-010500](ADR-20260729-010500-configuration-is-declared-and-fails-fast.md) gave configuration
a **declaration**. It did not give it an **owner**: values were still typed by hand into the Render
dashboard, invisible from the repository.

The production boot log of 2026-07-28 is the evidence:

```
sirene sync worker: PAUSED (issue #220) — poll loop not started; set RUN_SIRENE_WORKER=true to resume
```

`RUN_SIRENE_WORKER` was never set. 6,649 SIRENE rows sat `PENDING`, their payloads unreleased, and
establishing why consumed an evening. In the same dashboard sits `API_SECRET`, read by nothing.

The product owner then asked the sharper question: *"Is it possible to configure the deployment, not the
Render service?"* — and it reframed the problem.

## Decision

**Configuration is split by secrecy, because the platform and the threat model force it.**

Render offers **no per-deploy environment override**: its deploy API accepts only `clearCache`,
`commitId`, `imageUrl` and `deployMode`, and a deploy always runs with the *service's* stored variables.
Attaching configuration to the deployment therefore means putting it **inside the artifact**.

| category | where it lives | why |
|---|---|---|
| **Non-secret** (`RUN_*`, `PORT`, `WEB_ASSETS_DIR`) | **baked into the binary** by the codegen, per profile | the digest determines behaviour |
| **Secret** (DB, Stripe, Supabase, tokens) | **service env, pushed by CI** from GitHub repo secrets | the GHCR package is PUBLIC — a baked `ENV` is world-readable |
| **`APP_PROFILE`** | service env, necessarily | one image is promoted across environments by digest, so the thing that *distinguishes* them cannot live inside it — and baking it would be circular, since it selects the baked table |

**Precedence: environment variable > baked profile value > `default`.** The env var wins so an operator
keeps a seconds-fast override during an incident; the reviewed, recorded value runs the rest of the time.

### Why baking, and not simply syncing everything

Deploys here are digest-pinned *precisely* so production never runs an ambiguous artifact. Leaving
toggles in mutable service state re-opens that ambiguity through the back door: the same
`sha-abc123`, deployed today and redeployed next month, can behave differently with nothing recording
why. Baking closes it — the digest determines behaviour completely, and **a rollback restores the
configuration that shipped with that build**. Secrets are the residue that cannot be treated this way;
they can enable behaviour but never change it.

### The CI sync, and what it deliberately refuses to do

- **Upsert only** (D1). `PUT /v1/services/{id}/env-vars/{key}` cannot delete, so a bad manifest can
  never wipe production config. An undeclared key on the service is **reported**, never removed — the
  ordering [#238](https://github.com/TheCaptainCompany/captain-food/issues/238) taught: never remove
  what CI does not yet write.
- **Dry-run by default** (D2). The workflow cannot be tested anywhere but CI — there is no local
  `RENDER_API_KEY` — so its first real execution would otherwise be an untested write against live
  production. Dry-run prints the exact diff and writes nothing.
- **Non-secrets first** (D3): they are baked, so they need no sync at all. Secrets follow once
  bootstrapped into GitHub.
- **Reuses the existing `RENDER_API_KEY`** (D4): account-scoped and already present for read-only use,
  so the exposure exists today; Render issues no narrower token. CI gaining *write* access to production
  config is a real escalation, mitigated by manual dispatch and the post-deploy `/health` + `prod-smoke`
  gates.
- **A missing repo secret is skipped and reported**, never written as empty. A key that looks configured
  but is not is worse than one that is plainly missing.

## Consequences

- **Pausing a pipeline is now a PR.** `RUN_SIRENE_WORKER=false` for production means editing
  `specs/configuration.yaml`, review, build, deploy (~minutes) rather than a dashboard edit (seconds).
  For a flag whose job is stopping a production pipeline, reviewed-and-recorded is the feature — and the
  runtime env override survives for the case where minutes are too slow.
- **A secret must never be baked** — validator-enforced (`config-secret-baked`), because the image is
  public. The converse (a non-secret sourced from a repo secret) is only a **warning**: it is a
  preference, not a safety rule, and is legitimate for values that are not secret yet are
  environment-specific and better left out of the repo (a Supabase project URL, an OAuth client id).
- **The dashboard becomes derived state.** Its remaining job is `APP_PROFILE` and whatever CI has
  pushed. `API_SECRET` now shows up in the sync report as `UNDECLARED` on every run.
- **Still manual, deliberately**: the first `apply: true` run, and setting `APP_PROFILE=production`
  (which arms fail-fast). Both are one-time acts that should be done with the dry-run report in hand.
