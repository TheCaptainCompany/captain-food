# ADR-20260729-010500 — Configuration is declared in the DSL, and a missing required key stops the app

- **Status**: Accepted
- **Date**: 2026-07-29
- **Issue**: [#246 "Declare the app's configuration in specs/, validate it at startup, and refuse to boot when a required key is missing"](https://github.com/TheCaptainCompany/captain-food/issues/246)
- **Proposal**: [PROP-20260729-004500](../proposals/PROP-20260729-004500-configuration-is-declared-and-validated-at-startup.md) (approved in-session, product owner: *"Fail-fast: approved"*)
- **Refines**: [ADR-0043](0043-db-migration-release-strategy.md) (readiness posture), [ADR-20260728-224500](ADR-20260728-224500-every-background-loop-publishes-readiness.md) (lenient `RUN_*` parsing, now generated)

## Context

Configuration was the one part of this system with **no source of truth**. The DSL governed the domain,
the API, the screens and the observability contracts; the ~21 environment variables the app actually
reads existed only as inline `std::env::var` calls scattered through the composition root, plus a
partial and stale mirror in a `render.yaml` that is not applied.

Measured over one evening (2026-07-28):

- `RUN_SIRENE_WORKER` gates the SIRENE drain, defaults **OFF**, and was written down **nowhere** — not
  in a file, not in the manifest, not in the dashboard. 6,649 department-37 rows sat `PENDING` for four
  hours; answering *"is the worker running?"* took a code read, three `curl`s and an elimination
  argument ([#238](https://github.com/TheCaptainCompany/captain-food/issues/238)).
- `API_SECRET` was configured on the production service and **read by nothing** — never in git history,
  not a Render platform variable.
- `render.yaml` documented **9 of ~21** variables, and one of the nine (`HUBRISE_ACCESS_TOKEN`) is
  retired in code.
- Several keys **degrade silently**: no `AUTH_SESSION_KEY` → login quietly becomes anonymous-only; no
  `STRIPE_WEBHOOK_SECRET` → webhook verification fails closed, so a **captured payment never reaches
  the domain** — the "paid order nobody is told about" failure `CLAUDE.md` names as the worst there is.

## Decision

**1. `specs/configuration.yaml` is the single declaration.** Each key carries its type
(`bool`/`string`/`int`/`enum`), per-profile `required`, `default`, `secret`, `consumer`, and **`gates`**
— one sentence on what breaks without it. `gates` is not a comment: it is *printed next to the key* when
startup fails, so the validator rejects a key without one. A key nobody can explain is not declared.

**2. The reader is generated.** `Config::resolve()` returns the config **and** every missing required
key. `from_env()` is the strict wrapper. Lenient `bool` parsing (ADR-20260728-224500) moves into the
generated reader, so every toggle gets it uniformly. An empty or whitespace-only value counts as
**absent** — a dashboard field someone cleared must not satisfy a requirement.

**3. Every value is TYPED by a scalar, and validated against its `pattern` at startup.** *Present is
not usable*: a live Stripe key pasted into the test slot, a webhook secret carrying a trailing newline
from a dashboard copy-paste, a 31-byte `AUTH_SESSION_KEY` — each boots happily without this and fails
later, at the worst moment, looking like a code bug. Nine configuration scalars live in `scalars.yaml`
alongside every other type in the system; splitting `StripeSecretKeyTest` from `StripeSecretKeyLive` is
what makes "a LIVE key in the test slot" a startup failure rather than a way for a job whose whole
premise is that it cannot move real money to do exactly that.

**4. Missing OR malformed keys stop the app**, reporting **all** of them at once — grouped `MISSING`
(absent) and `INVALID` (present but malformed), because those are different problems with different
fixes — then exiting `78` (`EX_CONFIG`). Not the first one: an operator who learns of one bad variable
per deploy cycle fixes a three-key outage in three deploys. **A secret's value is never printed**; the
report names the key and the expected shape only.

**5. Enforcement follows the PROFILE, not a second toggle** (product-owner directive, 2026-07-29:
*"fast fail to avoid misconfiguration on production"*). Production and staging stop; development
reports and continues. An earlier draft gated this on a `CONFIG_ENFORCE` flag for a warn-only rollout;
that flag is gone, because the risk it hedged against does not exist — see below — and a second knob is
a second thing to get wrong.

**6. A boot report states what resolved** — key, value for non-secrets, `set`/`unset` for secrets, and
for `STRIPE_SECRET_KEY` the **mode** derived from its `sk_test_`/`sk_live_` prefix. Never a secret value.

**7. `consumer` scopes a key to its binary.** The CI `sirene_ingest` job's keys are declared — so the
drift gate and the docs cover them — without being injected into the server, which never reads them.

**8. A drift test pins the declaration to reality.** Every `env::var` / `env_flag` call site in
`crates/**` must correspond to a declared key, or the build fails. This is the load-bearing part: any
hand-maintained inventory drifts, and this one already had.

### Why this does not contradict ADR-0043

ADR-0043 has the app **start and report `503`** when the DB schema is behind, which looks like the
opposite of refusing to boot. The rule that reconciles them:

> **Missing configuration cannot self-heal → refuse to start.
> An unavailable dependency can → start, report `503`, keep probing.**

A schema-behind DB resolves the moment CI applies the migration, and the app must be alive to notice.
An absent `AUTH_SESSION_KEY` will never appear on its own; staying up only serves degraded traffic.

**On Render this is strictly safer than what we had.** A container that exits **fails the deploy**, and
the previous version keeps serving. Today a misconfigured deploy replaces a working one and degrades
quietly; with this, it cannot take over at all.

## Consequences

- **The warn-only rollout (PROP D5) was dropped, deliberately.** It hedged against "the first enforced
  deploy fails because production is missing a key" — but a container that exits **fails the deploy and
  the previous version keeps serving**. The hedge protected against nothing: the failure mode it feared
  is a deploy that does not take over, which is precisely the desired outcome. Enforcing immediately is
  therefore both simpler and safer than phasing it in.
- **`APP_PROFILE` is declared, never inferred** (default `development`). Inferring it from the host or
  from a key prefix is wrong exactly when it matters most — during a test→live switch-over.
- **A new env var is now a spec change.** Adding one to the code without declaring it fails the build.
  That is the intended cost.
- **Still to come, deliberately out of this change**: injecting `Config` into `router()` so the
  composition root stops calling `env::var` at all (the drift gate already makes every such call
  *declared*, just not yet *injected*), and the presence-only `/config` endpoint (PROP D4, deferred).
- **The derived artifacts** — the `render.yaml` manifest, the CI-sync list, the documented inventory —
  now have one upstream. `API_SECRET` becomes visibly undeclared rather than invisibly pointless.
