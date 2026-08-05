# ADR-20260805-070138 — render-status reflects service suspension, not just the last deploy status

- **Status**: Accepted
- **Date**: 2026-08-05
- **Context**: unattended CI/deploy watchdog run

## Context

On 2026-08-05 the production web service `captain-food.onrender.com` was returning **HTTP 404 —
the site was offline** — while every signal the repo publishes said "green": CI on `main` was
all-success, and the `render/deploy` badge/commit status read `live @ 320c215`.

Root cause of the *offline*: the Render service (`srv-d9ctcpgk1i2s73cj6820`) was **suspended for
billing** (`suspended: "suspended"`, `suspenders: ["billing"]`). That is an account action, not a
code fault, and is out of scope for an autonomous fix — it was surfaced to the product owner.

Root cause of the *false green*: `render-status.yml` classified deploy health from the latest
**deploy's** `.status` alone. A billing/account suspension takes the whole service offline while
`GET /deploys` keeps reporting the last deploy as `live`. The workflow never looked at the
service-level `suspended` flag, so its whole purpose — "keep the badge honest" — silently failed
exactly when it mattered.

## Decision

`render-status` additionally fetches `GET /v1/services/{id}` and, when `suspended == "suspended"`,
overrides the effective status to `suspended`. `suspended` is folded into the existing failure
mapping: `render/deploy` commit status → `failure`, badge → `red`, message → `suspended @ <sha>`,
and a workflow `::error::` naming the suspenders. A live latest deploy no longer implies a live
service.

Options considered: (a) probe the public URL for a non-2xx — rejected: conflates spin-down cold
starts and transient 5xx with a real outage, and adds a flaky external dependency; (b) read the
authoritative `suspended` field from the API we already call — chosen: one extra GET, deterministic,
no new failure modes.

## Consequences

- The badge and the per-commit `render/deploy` status now go **red** whenever the service is
  suspended, so the next watchdog/human sees prod-down instead of a false green.
- No effect on `deploy.yml` (deploys remain gated on `checksPass`); this is monitoring only.
- Verified against the live suspended service: raw deploy status `live` → effective `suspended` →
  `state=failure`, `color=red`.
- Does **not** fix the billing suspension itself — that requires a Render dashboard/billing action
  by the owner.
