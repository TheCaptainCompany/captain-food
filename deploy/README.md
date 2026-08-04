# `deploy/` — the OVH VPS host configuration

Everything needed to stand up and run the production box, as reviewable files rather than commands
typed into a terminal. Decision record: [ADR-20260804-171030](../docs/adr/20260804-171030-single-box-hosting-self-managed-postgres.md),
option space: [PROP-20260731-061609](../docs/proposals/PROP-20260731-061609-ovh-migration.md).

**The cutover itself is [docs/runbooks/ovh-cutover.md](../docs/runbooks/ovh-cutover.md).** Start there.

## The box

| | |
|---|---|
| Host | OVH **VPS-2** — 4 vCore, 8 GB RAM, 75 GB NVMe, unmetered traffic |
| Region | France (GRA or SBG) |
| OS | **Debian 13 (trixie)**, plain image — no control panel |
| PostgreSQL | **16**, from `apt.postgresql.org`, **on the host** (matches `ci.yml`'s `postgres:16-alpine`) |
| App + Redis + Caddy | containers, `docker compose` |

The split is deliberate (ADR-20260804-171030): **system of record on the host, rebuildable in a
container.** The deploy verb is `docker compose up -d` on every release, so the event log must sit
outside its blast radius — `docker compose down -v` and `docker volume prune` cannot reach a
host-installed PostgreSQL.

## Files

| Path | What it is |
|---|---|
| `provision.sh` | Idempotent host setup — users, SSH, firewall, PostgreSQL 16 + tuning, Docker, directories, systemd units. Safe to re-run |
| `postgresql/captain-food.conf` | PostgreSQL tuning for 8 GB / 4 vCore / NVMe. Dropped into `conf.d`, never edits the distro's `postgresql.conf` |
| `docker-compose.yml` | `app` (host network, port 8080) + `redis` + `caddy` |
| `Caddyfile` | TLS termination and reverse proxy to the app |
| `backup/pg-backup.sh` | Nightly `pg_dump` to S3-compatible object storage, with retention |
| `backup/restore-drill.sh` | Restores the newest backup into a scratch database and diffs row counts. **This is what makes the backup real** |
| `systemd/` | Timers for the two scripts above |
| `env.example` | The runtime environment file's shape. The real `/etc/captain-food/app.env` is never committed |

## Two deliberate omissions

**Wildcard TLS is phase 2.** Caddy issues certificates over HTTP-01 for the explicitly named reserved
hosts (`api`, `live`, `restos`, `riders`, `system`), which needs no DNS API and works the moment DNS
points at the box. A wildcard `*.captain.food` certificate — required before real tenant onboarding,
because Let's Encrypt caps issuance at 50 certificates per registered domain per week — needs DNS-01
and therefore an automatable zone. See the runbook's "Before tenant onboarding" section.

**WAL archiving is phase 2.** Phase 1 is nightly `pg_dump`, giving a 24-hour RPO, plus the VPS's own
daily snapshot. `wal_level = replica` is set from the start so enabling `archive_mode` later needs no
restart, but PITR is not claimed until the archive shipper and its pruning are proven. Gate, then
stabilize — do not tell yourself you have PITR because the config mentions WAL.

## Running anything here

You should not need to SSH in for routine work. `provision.sh` runs once at build-out;
after that `.github/workflows/deploy.yml` reaches the box over SSH as the restricted `deploy`
user, and maintenance runs through manually dispatched workflows. Every change to this
directory is reviewed in a PR before it touches production — on this repo that review **is** the
security boundary between a bad diff and the box.
