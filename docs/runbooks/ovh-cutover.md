# Runbook — cutting production over to the OVH VPS

Decision: [ADR-20260804-171030](../adr/20260804-171030-single-box-hosting-self-managed-postgres.md) ·
Plan: [PROP-20260731-061609](../proposals/PROP-20260731-061609-ovh-migration.md) ·
Host files: [`deploy/`](../../deploy/)

**Deadline: 30 Aug 2026** — the Supabase organization is over its Free-Plan egress quota and projects
are restricted from that date. Production is already down, so the window is free; the cutover is what
removes the quota problem rather than deferring it.

**Render is never resumed.** The workspace is closed, not fixed.

---

## 0. Before the window

| # | Thing | Notes |
|---|---|---|
| 0.1 | OVH **VPS-2** ordered, **Debian 13**, French region | 4 vCore / 8 GB / 75 GB NVMe |
| 0.2 | **Snapshot taken** before any provisioning | Free on the plan. This is what makes everything below reversible |
| 0.3 | SSH keypair generated for CI | `ssh-keygen -t ed25519 -C captain-food-deploy -f captain-food-deploy` |
| 0.4 | Scaleway Object Storage bucket in **Paris**, with a write-scoped API key | Different provider from the compute, on purpose |
| 0.5 | Repo secrets set | table below |
| 0.6 | `dig NS captain.food +short` run | **Find out who actually serves the zone.** ADR-0036 says Dynadot; the LWS invoice implies LWS. Guessing during the window is how a 20-minute DNS change becomes an evening |

### Repo secrets

| Secret | Value |
|---|---|
| `VPS_HOST` | the VPS IPv4 |
| `VPS_SSH_KEY` | the **private** key from 0.3 |
| `VPS_KNOWN_HOSTS` | `ssh-keyscan <ip>` output — pins the host key so the deploy cannot be intercepted |
| `DATABASE_URL` | copied from `/etc/captain-food/db.env` after step 1 |

`RENDER_DEPLOY_HOOK_URL` can be deleted once step 7 is green.

---

## 1. Provision the box

Log in once as root (SSH, or the OVH web console if you would rather not touch a terminal):

```bash
apt-get update && apt-get install -y git
git clone https://github.com/TheCaptainCompany/captain-food.git /opt/src
cd /opt/src/deploy
less provision.sh                      # read it. this is production.
DEPLOY_PUBKEY="$(cat captain-food-deploy.pub contents here)" bash provision.sh
```

It is idempotent — re-running is the normal way to apply a change to it.

Then, still on the box:

```bash
cat /etc/captain-food/db.env           # copy DATABASE_URL into the repo secret NOW, it is not reprinted
vi  /etc/captain-food/app.env          # see deploy/env.example; keys are declared in specs/configuration.yaml
vi  /etc/captain-food/backup.env       # Scaleway credentials
chmod 600 /etc/captain-food/*.env
```

**Verify:** `systemctl is-active postgresql docker` → `active`; `ufw status` → 22/80/443 only;
`sudo -u postgres psql -c 'show shared_buffers'` → `2GB`.

---

## 2. Final Supabase dump, with evidence

From your machine (or the box — the Supabase project is still reachable until step 8):

```bash
pg_dump -Fc -Z 3 -d "$SUPABASE_URL" -f supabase-final.dump

# Evidence to compare against after the restore. Do not skip this: "the restore looked fine" is
# not a check, and the event log is the business.
psql -d "$SUPABASE_URL" -tAc "
  SELECT table_name, (xpath('/row/c/text()',
    query_to_xml(format('SELECT count(*) AS c FROM %I.%I', table_schema, table_name),
    false, true, '')))[1]::text::bigint AS rows
  FROM information_schema.tables WHERE table_schema='public' ORDER BY table_name;
" > counts-before.txt
```

---

## 3. Restore onto the VPS

```bash
scp supabase-final.dump deploy@<ip>:/tmp/
ssh deploy@<ip>
sudo -u postgres pg_restore -d captain_food -e -j 2 /tmp/supabase-final.dump
```

Re-run the same counts query against `captain_food` into `counts-after.txt` and **diff it**. Any
difference stops the cutover — do not proceed to make it "probably fine".

---

## 3b. Prove the backup works — this gates step 7

```bash
sudo systemctl start captain-food-backup.service
journalctl -u captain-food-backup -n 20 --no-pager
cat /var/backups/captain-food/last-backup.json      # ok:true, and a plausible domain_events count

sudo systemctl start captain-food-restore-drill.service
journalctl -u captain-food-restore-drill -n 30 --no-pager
cat /var/backups/captain-food/last-drill.json       # ok:true
```

The drill pulls **from the remote**, restores into a scratch database and diffs `domain_events`
against live. It is testing the credentials and the upload path, not just the disk.

**If this fails, stop.** You are about to point real customers at a database whose only copy is on one
VPS. A backup that has never been restored is not a backup.

---

## 4. Migrate the schema

Dispatch **db-migrate** with `skip_deploy_check: true` — nothing is deployed yet, and its wait step
polls `api.captain.food`, which still points at Render until step 7.

Applies `20260730043000`–`0436` (the enum-text split set from [#264](https://github.com/TheCaptainCompany/captain-food/pull/264))
plus everything merged since. This is the first run with adequate disk — the constraint that forced
the split no longer exists.

---

## 5. Deploy

Dispatch **deploy** from `main`. It resolves the tag to an immutable digest, SSHes to the box, writes
`APP_IMAGE`, pulls, `compose up -d`, and **waits** until `/health` reports the deployed SHA.

Unlike the Render era this is no longer fire-and-forget ([#281](https://github.com/TheCaptainCompany/captain-food/issues/281)) —
the job fails if the container never serves the expected build.

`db-migrate` follows automatically and is idempotent, so the second run is a no-op.

---

## 6. Smoke before DNS

The box is not yet in DNS, so test it by forcing the Host header:

```bash
ssh deploy@<ip> 'curl -sS -H "Host: api.captain.food" http://127.0.0.1:8080/health'
```

Expect `200` with `version` = the deployed SHA. A `503 schema_behind` means step 4 did not land.

Check the config fail-fast report (ADR-20260729-010500) — a missing OVH-era value stops the
container rather than degrading silently, so a running container is itself evidence.

---

## 7. Cut DNS

**One record changes.** Everything else stays exactly as it is:

```
*.captain.food.   CNAME captain-food.onrender.com   ->   A   <VPS IPv4>
```

| Record | Action |
|---|---|
| `captain.food` (apex) | **leave alone** — 301-forwards to `join`, marketing |
| `www` | **leave alone** — 301 to `join` |
| `join` | **leave alone** — CNAME to GitHub Pages, marketing |
| `_acme-challenge.captain.food` | **DELETE** — it delegates to Render's ACME verifier and will break your own issuance |
| `*.captain.food` | **A → VPS IP** |

Marketing is untouched by the cutover, so the acquisition surface cannot break in the window.

Once DNS propagates, Caddy issues certificates over HTTP-01 for `api`, `live`, `restos`, `riders`,
`system` automatically. Watch it happen:

```bash
ssh deploy@<ip> 'docker compose -f /opt/captain-food/docker-compose.yml logs -f caddy'
```

Then run **prod-smoke** and re-verify Host-header tenant routing over real TLS.

---

## 8. Empty the Supabase database

Only after 7 is green and verified. Auth configuration is **untouched** — OTP templates, the OVH SMS
hook, the JWKS URL. Supabase is identity-only from here (ADR-20260731-061609), and `auth_ref` stays
an opaque subject string, so nothing in the domain or the read models changes.

Dropping the data matters for GDPR posture: no orphaned copy of customer data on a decommissioned
tier.

---

## Rollback

| Failed at | Do |
|---|---|
| 1–3 | Restore the 0.2 snapshot. Nothing has moved yet |
| 4 | Do not deploy. Restore the snapshot and re-restore the dump — the schema moved but no binary is serving |
| 5 | Re-dispatch `deploy` with `tag:` set to a previous `sha-XXXXXXX`. Rollback is redeploying an older digest |
| 7 | Point `*.captain.food` back at the CNAME. DNS TTL is the recovery time — **lower the TTL 24h before the window** |

---

## Before tenant onboarding — wildcard TLS

Phase 1 covers only the named reserved hosts. Real restaurant tenants at `{slug}.captain.food` need a
**wildcard certificate**, and that changes two things:

1. Let's Encrypt will not issue a wildcard over HTTP-01 — it requires **DNS-01**, so the ACME client
   must write TXT records through a DNS API.
2. Per-tenant certificates are not a workaround: Let's Encrypt caps issuance at **50 certificates per
   registered domain per week**, globally across accounts. That ceiling arrives at 50 new restaurants
   in a week, i.e. exactly when onboarding is going well.

The recommended shape (PROP-20260731-061609) is **delegation, not migration**: leave the zone where it
is and CNAME `_acme-challenge.captain.food` to a zone you can automate. Moving the whole zone to OVH
would mean rebuilding the apex 301 forward, which is a *registrar* feature rather than a DNS record —
unnecessary risk to the marketing entry point.

Caddy then needs a DNS-01 module, which means a custom build (`xcaddy` with `caddy-dns/ovh`) pushed
to GHCR like the app image.

**One credential trap**: do not reuse the OVH API keys from the SMS hook. An OVH consumer key is bound
to specific authorised routes, and the SMS credentials have no `/domain/zone/*` rights. Create a
separate application and consumer key for DNS — establish the API surface before naming the
credential (ADR-20260730-032306).

---

## After the window

- Watch egress for a week against the ~15 GB/month pre-cutover baseline. It should collapse to
  customer-facing traffic only. If it does not, the colocation premise is wrong and you want to know
  in week one rather than at the next quota.
- Confirm the nightly backup timer fired: `systemctl list-timers captain-food-\*`.
- Retire `render.yaml`, `render-config-sync.yml`, `render-status.yml` and the
  `RENDER_DEPLOY_HOOK_URL` secret.
- Unblock [#242](https://github.com/TheCaptainCompany/captain-food/issues/242) slice 3 — its prod-gate
  was "OVH cutover complete".
