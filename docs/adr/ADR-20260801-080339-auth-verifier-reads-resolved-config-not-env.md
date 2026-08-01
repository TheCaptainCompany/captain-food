# ADR-20260801-080339 — The auth verifier reads the JWKS URL from resolved `Config`, never `std::env`

- **Status**: Accepted
- **Date**: 2026-08-01
- **Refines**: [ADR-20260729-020000](ADR-20260729-020000-configuration-rides-the-artifact-secrets-ride-ci.md) (non-secret config is baked into the artifact; precedence env > baked > default), [ADR-0047](0047-api-auth-supabase-jwt-jwks.md) (Supabase-JWT/JWKS auth)

## Context

Production `prod-smoke` went red on 2026-08-01 (runs `30683107546`, `30683532831`, `30685054021`):
L1/L2/L3 passed, then

```
FAIL  L4: https://api.captain.food/customer/graphql returned HTTP 503 — body: auth unavailable
```

The whole authenticated GraphQL surface was down — every non-`/public` role path failed closed — while
the public surface was healthy.

Root cause: `AuthContext::from_env()` read `SUPABASE_JWKS_URL` and `SUPABASE_URL` **directly from
`std::env`**. But per [ADR-20260729-020000](ADR-20260729-020000-configuration-rides-the-artifact-secrets-ride-ci.md)
those two keys are **non-secret baked config** — their per-profile values live inside the image (the
`BAKED` table in the generated `config.rs`), and they are **absent from the Render service environment**.
So on the deployed service the env lookup returned empty, `jwks_url` was `None`, and the verifier
fail-closed with `503` on a cold cache. It never surfaced locally, where the env var is set.

This is the **second** occurrence of the same trap: `263f2a2` had just fixed the identical bug in the
prod-smoke *script* ("read `SUPABASE_URL` from baked config, not the Render env"). The server's auth
layer still carried it — a value baked into the artifact is invisible to any code that reads `env`
directly.

## Decision

Any code that needs a **non-secret baked** configuration key MUST take it from the resolved
`generated::config::Config` (which applies the `env > baked > default` precedence), never from
`std::env::var` at the call site. `AuthContext` is now constructed by
`AuthContext::from_config(jwks_url, supabase_url)`, fed the already-resolved `config.supabase_jwks_url`
/ `config.supabase_url` in `router()`. The env-override semantics are preserved for free — `resolve()`
still lets an operator win with a runtime env var during an incident.

`EXTERNAL_API_TOKENS` stays an `std::env` read inside the constructor: it is a **secret** (delivered by
CI into the service environment, ADR-20260729-020000), carries no baked value, and so is correctly
absent from `Config`.

A unit regression guard (`from_config_uses_its_arguments_not_env`) pins that the constructor takes the
JWKS URL / issuer from its arguments and fail-closes on empty. An executable *class-level* guard — a
codegen lint forbidding `std::env::var("SUPABASE_…")` / other baked keys outside the generated config —
is the stronger enforcement and is left as a follow-up (the `makefile_recipe_lines_are_ascii` model).

## Consequences

- Production auth recovers on the next deploy: `config.supabase_jwks_url` resolves to the baked
  production value (`https://…supabase.co/auth/v1/.well-known/jwks.json`), so the verifier fetches
  JWKS and authenticated paths return to `200`.
- The precedence contract of ADR-20260729-020000 now holds at the one boundary that had bypassed it.
- Reminder for future adapters: reading a baked key from `env` is a **prod-only** failure — green
  locally, `503`/misbehaviour in production. Route it through `Config`.
