# ADR-20260730-233749 — Hosting moves to an OVH VPS (app + Postgres colocated); Supabase stays for identity only

## Status

Accepted (product-owner directive, 2026-07-30). Realization (provisioning, migration, cutover) is
follow-up work; until it lands, production remains Render (paused) + Supabase, with the marketplace
host dark (ADR-20260730-135741).

## Context

The current split — app on Render's free tier, database on Supabase's free tier — failed on both
sides of the same meter in the same week: Render is paused (100 GB outbound bandwidth exhausted,
ADR-20260730-051500) and Supabase's 5 GB/month database egress cap is being approached. The
structural cause is the topology, not the traffic: app and database live in different clouds, so
every SQLx read crosses a metered public boundary — the marketplace SSR burned both meters at once
(ADR-20260730-135741). Metered egress is a tax on split hosting.

A hosting survey (2026-07-30, verified against provider pages) established:

- **Free managed Postgres with >5 GB egress essentially means Aiven** (1 GB storage, unmetered
  egress, EU, real Postgres). Supabase/Neon both cap at 5 GB; Prisma Postgres meters operations;
  Xata/Render/Koyeb free tiers are gone, expiring, or closed; CockroachDB is not real Postgres.
- **OVH VPS traffic is officially unlimited and unmetered in EU datacenters** (the only limiter is
  port speed); Public Cloud egress is likewise free in EU regions. OVH managed Postgres is also
  unmetered but starts ~€54/mo.
- **Scaleway** is cheap-ish (managed PG ~€13–14/mo) but its entry instances cost more than OVH for
  half the RAM, its serverless Postgres cold-starts exactly at the Friday-peak worst moment, and —
  decisive — **the product owner had a serious prior security incident there ("hacked from the
  inside") and excludes Scaleway as a provider**. Recorded here so no future session re-proposes it.
- Azure offers no always-free Postgres (12-month trial only); LWS (already paid for the year) is
  available but the product owner prefers not to build on it.

## Decision

1. **Target topology: one OVH VPS in an EU French datacenter (Gravelines or Strasbourg) running the
   Axum binary and PostgreSQL side by side.** Postgres binds to localhost/unix socket only — DB
   traffic stops being egress entirely and the database is not network-reachable, which also matches
   the post-incident security posture (smallest attack surface, fewest third parties holding data).
2. **Start on VPS-1** (2 vCPU, 4 GB RAM, 40 GB local NVMe, 500 Mbit/s unmetered, €4.57 TTC/mo on
   12-month commitment) **and upgrade on evidence** — the upgrade to VPS-2+ is in-place (data and IP
   kept), a downgrade is not possible, so starting small is the correct order.
3. **Supabase is kept for identity only** (Supabase Auth behind our GraphQL, ADR-0015 unchanged);
   auth traffic is far below the free tier's egress cap once the database moves off it.
4. **Backup posture** (the local NVMe's RAID is undocumented — treat the disk as a single failure
   domain): the €1.10 HT/mo Premium backup option (7 rolling days) plus a nightly encrypted
   `pg_dump` shipped off-box to a different provider. The included backup alone (24 h retention) is
   insufficient for a database holding paid orders.
5. **Scaleway is excluded** as a hosting provider (product-owner directive; prior security
   incident).

## Alternatives considered

- **Aiven free Postgres (Frankfurt) + app elsewhere** — the only genuinely free fix ($0, unmetered
  egress, real Postgres). Rejected as the target: 1 GB storage ceiling with no headroom, single
  node, inactivity power-off, and it adds a third party holding order data — but it remains the
  fallback if the VPS path stalls, and was seriously weighed.
- **Supabase Pro ($25/mo)** — lifts the cap (250 GB egress), no pausing; boring and safe but 5×
  the VPS price and keeps the split-cloud egress tax.
- **Scaleway** — excluded (see Context).
- **Hetzner CX22 (€3.79/mo, 20 TB metered)** — marginally cheaper, more RAM per euro; German/Finnish
  rather than French, and its traffic is metered (harmless in practice). Kept as runner-up.
- **LWS (already paid)** — available but the product owner does not want to build on it.
- **OVH managed Postgres (~€54/mo)** — unmetered egress but defeats the budget at V0 scale.

## Consequences

### Positive
- Outbound bandwidth and DB egress stop being failure modes: EU OVH VPS traffic is unmetered by
  policy, and app↔DB traffic never leaves the box.
- ~€5–6/mo total (VPS-1 + Premium backup) replaces both the paused Render service and the
  egress-capped Supabase database; French provider, EU data residency (GDPR/ADR-0042 posture kept).
- 4 GB RAM / local NVMe (~30k random IOPS measured by third parties) comfortably fits the Axum
  binary + PostgreSQL 16 + the in-process projection worker at Tours-scale peak.

### Negative
- One box is one failure domain (99.9% SLA ≈ 43 min/month allowed downtime); "a paid order nobody
  is told about" must be covered by external uptime monitoring and Stripe-webhook replay, not by
  hardware redundancy.
- We take on database operations (updates, tuning, restore drills) that Supabase did for us.
- Deploys/infra automation (currently Render deploy hooks, ADR-20260730-051500) must be rebuilt for
  the VPS (image pull or binary rollout — to be specified in the migration work).

### Follow-up
- Provision the VPS, define the rollout mechanism, migrate the database (sequence AFTER the pending
  enum-text release lands, ADR-20260730-051500), cut DNS over, then remove the marketplace 204
  mitigation (ADR-20260730-135741) once bandwidth is no longer metered.
