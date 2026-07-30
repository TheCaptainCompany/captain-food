# ADR-20260730-051500 — Isolate build, deploy and migrate; the schema only moves after a deploy

## Status

Accepted (product-owner directive, 2026-07-30).

## Context

The delivery pipeline ran three production-touching actions off one trigger: a green `ci` on `main`
started `db-migrate` (schema → prod Supabase) and `build-image` (GHCR push **and** Render deploy
trigger) in parallel, with the app's `/health` schema-version gate reconciling the ordering — a new
binary waits (503) until pending migrations land.

That self-correction only covers one direction. The day the enum-text conversion
(ADR-20260728-170000) shipped, **Render was paused — outbound bandwidth exhausted** — so no deploy
could arrive, while `db-migrate` would still have fired automatically on the next green merge.
The schema would have converted underneath the OLD binary, breaking production with no rollout able
to fix it. The `/health` gate cannot hold back a migration; it can only hold back a binary.

## Decision

Split the pipeline into three isolated stages; only the first is automatic:

1. **`build-image`** (automatic, after green `ci` on `main`) — builds and pushes the image to GHCR
   only. Publishing an image is always safe; it runs nothing.
2. **`deploy`** (NEW, manual `workflow_dispatch`, the only workflow that touches Render) — resolves
   a published tag (default: `sha-<short>` of the dispatched commit; any published tag for
   rollback) to its immutable digest and triggers the Render deploy hook with it.
3. **`db-migrate`** (automatic **after `deploy` completes successfully**, plus manual dispatch) —
   the schema only ever moves once a new binary has actually been sent to Render. The `/health`
   gate makes the rollout converge whichever of deploy/migrate finishes first.

## Alternatives considered

- **Keep migrate-on-ci, gate on Render health** — probing "is Render able to deploy?" from the
  migrate job is fragile (paused workspaces still answer APIs) and still a race; the pause taught us
  the probe would have to predict the future.
- **Fully manual everything** — build minutes are free and images are inert; making the build manual
  adds a human step with no risk removed.
- **Expand/contract migrations only** (schema always compatible with both binaries) — the right
  discipline for post-launch scale, but heavyweight for V0; revisit when there is real traffic to
  protect (the enum-text change would have needed a 3-phase rollout).

## Consequences

### Positive
- A paused/broken Render can no longer strand the database ahead of the binary; nothing automatic
  touches production infrastructure except an image registry push.
- Deploys become deliberate and pinned: a human dispatch, digest-resolved from an already-green,
  already-published image — and rollback is the same dispatch with an older tag.
- Migrations always follow a real deploy, and stay idempotent/manual-dispatchable for recovery.

### Negative
- Going live now requires one manual action per release (`Actions → deploy → Run workflow`); a merge
  alone no longer reaches production.
- `prod-smoke` timing is on the operator: nothing automatic verifies the deploy (the daily schedule
  still runs).

### Follow-up actions
- Once Render's bandwidth/workspace is restored: dispatch `deploy` for the enum-text release, let
  `db-migrate` apply the split `20260730043000`–`0436` set, then run `prod-smoke`.
