# ADR-20260731-061609 — Hosting migrates to OVH; Supabase is retained for identity only

- **Status**: **Superseded IN PART by [ADR-20260806-151122](ADR-20260806-151122-hosting-destination-is-clever-cloud-not-ovh.md)**
  (product owner, 2026-08-06: *"Instead of OVH"*) — **only point 1, the destination**: app compute and
  the domain PostgreSQL go to **Clever Cloud** (French PaaS, Paris), not OVH, because owning a host OS
  generated a tail of undifferentiated work (WireGuard, block volumes, WAL archiving) that a managed
  platform removes. **Points 2, 3 and 4 below remain in force verbatim**, as does the whole Context —
  the reasons for leaving Render/Supabase are unchanged and are NOT what was revisited. OVH also
  remains the SMS provider (ADR-20260722-174500).
  Originally Accepted (product owner, 2026-07-31: *"We are going to migrate to OVH. Render +
  Supabase does not match our need — the limitations are exhausted and too expensive for the
  project. We will keep Supabase for the identity."*)
- **Tracking issue**: [#271 "Migrate hosting to OVH…"](https://github.com/TheCaptainCompany/captain-food/issues/271)
- **Migration plan**: [PROP-20260731-061609 "OVH migration"](../proposals/PROP-20260731-061609-ovh-migration.md)
- **Amends**: ADR-20260721-175411 (CI-built images — the build half SURVIVES; the Render deploy
  half is superseded), ADR-20260730-051500 (build/deploy/migrate isolation — the SHAPE survives,
  the deploy target changes), ADR-0042 (EU pinning — STRENGTHENED: French hosting).

## Context

The limitations were not hypothetical — each one cost an incident recorded in this repo: Render's
build-minute cap (ADR-20260721-175411), Render's outbound-bandwidth exhaustion (prod paused, still
down), Render's production disk (#264 split a routine migration to fit it), Supabase's Disk-IO
budget (#220's investigation), Supabase's storage economics (#231: one mirror at 77% of the
database). The pattern: platform ceilings sized for hobby projects, hit by a system that is still
pre-launch. Costs at the next tier up do not match the project.

## Decision

1. **App compute and the domain PostgreSQL move to OVH** (offering choices carried as D1/D2 in the
   proposal). DNS + wildcard TLS for `captain.food` / `*.captain.food` move with them.
2. **Supabase is retained for IDENTITY ONLY** — phone OTP, email magic links, JWKS. The boundary
   is already clean: auth is wrapped behind our GraphQL (ADR-0015), and `auth_ref` is an opaque
   subject string — nothing in the domain, the read models, or the #235 identity bridge changes.
3. **The build side does not move**: GitHub Actions builds, GHCR hosts images (free for this
   public repo), the isolated build → manual deploy → migrate pipeline keeps its shape with an
   OVH target.
4. **Cutover uses the outage**: prod is already down, so the migration window is free; the pending
   enum-text migrations apply on the OVH restore target, and Render is never resumed.

## Consequences

- `deploy.yml` retargets OVH; `render-config-sync.yml` and its generated manifest are superseded;
  `specs/configuration.yaml` `deploy:` blocks re-declare who supplies each value per profile.
- #242 slice 3's prod-gate becomes "OVH cutover complete" (was "Render restored + enum-text
  applied").
- Future Redis (D7 placement cache, #267 `ScopeMembership` target) has a first-party home:
  OVH managed Redis — one provider, one EU jurisdiction.
- Supabase's database is emptied after the final verified dump (GDPR posture: no orphaned
  customer-data copy on a de-commissioned tier).
- Telemetry unchanged (Honeycomb EU, ADR-20260729-183000).
