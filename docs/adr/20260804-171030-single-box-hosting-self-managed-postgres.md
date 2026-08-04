# ADR-20260804-171030 — One VPS runs app, PostgreSQL and Redis; the managed database is deferred, not abandoned

## Status

Accepted (product owner, 2026-08-04). Amends ADR-20260731-061609 (destination unchanged: OVH,
France, Supabase identity-only) and revises D1/D2 of
[PROP-20260731-061609](../proposals/PROP-20260731-061609-ovh-migration.md).

## Context

ADR-20260731-061609 sent compute and the domain PostgreSQL to OVH and left the offering choice to
the proposal, which recommended a Public Cloud instance plus **managed PostgreSQL on the smallest
HA-capable plan** — about **€160/month** all-in.

Two things then changed.

**The deadline became hard.** The Supabase organization is over its Free-Plan egress quota and
projects are restricted from **30 Aug 2026**. Production is already down. The migration is now the
remedy for an imminent outage, not a planned improvement.

**The budget became the binding constraint.** The product owner stated plainly that the money is not
there, and asked whether one machine could carry app, database and Redis together. It can. The
€160/month shape was designed under an assumption — that operational safety should be bought rather
than built — which does not survive contact with a pre-revenue, pre-launch project carrying no live
traffic.

A screening pass across six providers on 2026-08-04 also produced a disqualifying property that no
price comparison surfaces: **shared hosting cannot run this system at all**, because the app is a
long-running compiled binary with in-process workers on Postgres `LISTEN/NOTIFY`. One such plan (LWS
WordPress Performance) had already been bought for a year before that was noticed.

## Decision

1. **One OVH VPS-3** (6 vCore, 12 GB RAM, 100 GB NVMe, unmetered traffic, French region) runs
   everything — €10.40/month HT. VPS-2 (€7.21) is sufficient but leaves no margin on RAM or disk,
   the two dimensions that have already caused incidents.
2. **PostgreSQL is self-hosted on that box and installed on the HOST**, not as a container. The app
   and Redis are containers. The rule is *system of record on the host, rebuildable in a container*:
   the deploy verb is `docker compose up -d` on every release, and the event log must be outside the
   blast radius of that verb and of every `docker volume prune` run under disk pressure.
3. **Backups go to a different provider**: nightly `pg_dump` plus WAL archiving to Scaleway Object
   Storage in Paris (~€1/month, S3-compatible, still France). A **restore rehearsal gates the DNS
   cut** — a backup that has never been restored is not a backup.
4. **The managed database is deferred, not abandoned.** Staying on OVH — rather than Hetzner, which
   is marginally cheaper — preserves a same-provider path back to managed PostgreSQL when revenue
   justifies it.
5. **French hosting is preserved.** No amendment to ADR-0042's strengthened French pinning was
   needed: the cheapest credible option on the board is also French.

## Alternatives considered

- **Managed HA PostgreSQL as originally proposed (~€160/month)** — the right answer post-revenue.
  Rejected now purely on cost. The reasoning that produced it ("the database is the one component
  where self-managing risks the money path") is sound and is answered by decision 3, not dismissed.
- **Hetzner CX32 (~€8.50)** — better hardware per euro, but Germany-only. It would have traded
  ADR-0042's French strengthening for a *negative* saving once OVH VPS was priced properly, and
  Hetzner offers no managed PostgreSQL, so the later escape hatch would mean changing provider
  twice.
- **Scaleway** — French and with the cheapest object storage, but instance prices exclude block
  storage at €0.0949/GB/month, landing 4x above VPS-3. Its cheap managed-PostgreSQL tiers have no HA
  either, so they buy managed operations rather than availability.
- **Bare metal (Dedibox €25–40 + install fee)** — better €/GB of RAM at scale, but recovery is a
  hardware intervention measured in hours with no snapshot to roll back a bad migration. Revisit when
  RAM is the binding constraint, not price.
- **Shared hosting (LWS, o2switch)** — structurally impossible, not merely cheap-and-limited. See
  decision context above.
- **Postgres as a compose service** — rejected: it puts the event log inside the blast radius of the
  deploy verb, and version parity with CI is obtainable from a pinned apt repo instead.

## Consequences

### Positive

- Monthly hosting drops from a proposed ~€160 to **~€11.50** (VPS-3 plus offsite backups).
- **The 30 Aug quota problem dissolves structurally**: colocating app and database turns every DB
  round-trip into loopback traffic. The ~15 GB/month idle egress baseline that exhausted Render's
  5 GB allowance simply stops existing.
- Unmetered egress changes the overage failure mode from *service suspended* (Render, Supabase) to
  *nothing happens*.
- French hosting and the "hébergé en France" positioning survive at no cost.

### Negative

- **We own a database pager**: patching, disk growth, vacuum behaviour, backup verification.
- **One failure domain** — app, log and cache die together. D6 bounds data loss, not downtime.
- **No HA.** Nothing affordable had it, so recoverability is bought instead of availability.
- Returning to a managed database later is a migration (dump, restore, re-point, re-smoke), which is
  why staying on OVH matters.

### Follow-up actions

- Name the signal that triggers buying the managed database back, before an incident forces it.
- Resolve GDPR erasure reach into offsite dumps (intersects PROP-20260726-170000) and fix a
  retention window.
- Decide whether Redis is provisioned at cutover or deferred until #267's projection targets land.
- Retitle [#271](https://github.com/TheCaptainCompany/captain-food/issues/271) to match the
  single-box shape and copy §9's unresolved questions into its checklist.
