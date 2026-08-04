#!/usr/bin/env bash
# Nightly logical backup of the domain database to off-site object storage.
#
#   ADR-20260804-171030 / PROP-20260731-061609 D6.
#
# WHY off-site AND off-provider: the backup must survive losing the OVH account or region, not just
# losing the disk. It goes to Scaleway Object Storage in Paris -- a different provider, still France.
# A backup stored beside the thing it protects is not a backup.
#
# WHY logical (pg_dump) rather than snapshots: the VPS's included daily snapshot is crash-consistent
# at best and cannot restore a single table or a single mistake. This one can.
#
# RPO is 24 hours. That is a deliberate phase-1 choice: WAL archiving (PITR) is phase 2 and is not
# claimed until its shipper and pruning are proven. See deploy/README.md.
#
# Run by captain-food-backup.timer as user `postgres`. Exits non-zero on any failure so systemd
# records it -- and backup-check.yml notices a stale last-backup.json.

set -euo pipefail

DB_NAME=${DB_NAME:-captain_food}
STAGING=${STAGING:-/var/backups/captain-food}
REMOTE=${REMOTE:-backup:captain-food-backups}
KEEP_LOCAL=${KEEP_LOCAL:-2}          # local copies are a convenience, not the archive
KEEP_REMOTE_DAYS=${KEEP_REMOTE_DAYS:-30}
STATUS="$STAGING/last-backup.json"

# Credentials + rclone remote definition. Never committed; written by hand once (see env.example).
# shellcheck source=/dev/null
[ -f /etc/captain-food/backup.env ] && . /etc/captain-food/backup.env

STAMP=$(date -u +%Y%m%dT%H%M%SZ)
DUMP="$STAGING/${DB_NAME}-${STAMP}.dump"

fail() {
  printf '{"ok":false,"at":"%s","stage":"%s","error":%s}\n' \
    "$(date -u +%FT%TZ)" "$1" "$(printf '%s' "${2:-unknown}" | jq -Rs .)" > "$STATUS"
  echo "BACKUP FAILED at stage '$1': ${2:-unknown}" >&2
  exit 1
}

# --- Preflight: never start a dump that cannot fit -------------------------------------
AVAIL_MB=$(df -BM --output=avail "$STAGING" | tail -1 | tr -dc '0-9')
DB_MB=$(psql -tAd "$DB_NAME" -c "SELECT pg_database_size('$DB_NAME')/1024/1024" 2>/dev/null | tr -d ' ') || fail preflight "cannot query database size"
# A compressed custom-format dump is far smaller than the database, but leave real headroom:
# filling the disk here would take PostgreSQL down with it.
[ "$AVAIL_MB" -gt "$DB_MB" ] || fail preflight "only ${AVAIL_MB}MB free for a ${DB_MB}MB database"

# --- Dump -------------------------------------------------------------------------------
# -Fc: custom format -- selectively restorable (a single table, a single schema), unlike plain SQL.
pg_dump -Fc -Z 3 -d "$DB_NAME" -f "$DUMP" || fail dump "pg_dump returned $?"
SIZE=$(stat -c %s "$DUMP")
[ "$SIZE" -gt 1024 ] || fail dump "dump is suspiciously small (${SIZE} bytes)"

# --- Record what is IN it, so a restore can be verified rather than assumed --------------
EVENTS=$(psql -tAd "$DB_NAME" -c "SELECT count(*) FROM domain_events" 2>/dev/null | tr -d ' ' || echo "0")
SHA=$(sha256sum "$DUMP" | cut -d' ' -f1)

# --- Ship off-site -----------------------------------------------------------------------
command -v rclone >/dev/null 2>&1 || fail upload "rclone is not installed"
rclone copy "$DUMP" "$REMOTE/daily/" --s3-no-check-bucket 2>&1 || fail upload "rclone copy failed"

# Verify it actually landed at the expected size, rather than trusting a zero exit code.
REMOTE_SIZE=$(rclone size "$REMOTE/daily/$(basename "$DUMP")" --json 2>/dev/null | jq -r '.bytes' || echo 0)
[ "$REMOTE_SIZE" = "$SIZE" ] || fail verify "remote size ${REMOTE_SIZE} != local ${SIZE}"

# --- Prune -------------------------------------------------------------------------------
rclone delete "$REMOTE/daily/" --min-age "${KEEP_REMOTE_DAYS}d" 2>/dev/null || true
# GDPR note: these dumps contain customer personal data. The retention window above is what bounds
# an erasure request's reach into backups -- see PROP-20260731-061609 section 9, still unresolved.
ls -1t "$STAGING"/${DB_NAME}-*.dump 2>/dev/null | tail -n +$((KEEP_LOCAL + 1)) | xargs -r rm -f

printf '{"ok":true,"at":"%s","file":"%s","bytes":%s,"sha256":"%s","domain_events":%s}\n' \
  "$(date -u +%FT%TZ)" "$(basename "$DUMP")" "$SIZE" "$SHA" "$EVENTS" > "$STATUS"
echo "backup ok: $(basename "$DUMP") (${SIZE} bytes, ${EVENTS} events) -> ${REMOTE}/daily/"
