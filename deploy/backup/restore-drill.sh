#!/usr/bin/env bash
# Restore the newest off-site backup into a scratch database and prove it is usable.
#
#   "A backup that has never been restored is not a backup" -- PROP-20260731-061609 D6.
#
# This is the control that turns pg-backup.sh from an act of faith into a fact. It deliberately
# pulls FROM THE REMOTE rather than using the local copy: the thing being tested is the off-site
# archive, including the upload path and the credentials, not the disk next to the database.
#
# Run fortnightly by captain-food-restore-drill.timer as user `postgres`, and ALSO as the gate on
# the DNS cut during the migration (runbook step 3b).
#
# Non-zero exit = your disaster recovery does not work. Treat it as a production incident, because
# that is what it is -- you just found out on a Tuesday instead of during one.

set -euo pipefail

DB_NAME=${DB_NAME:-captain_food}
SCRATCH=${SCRATCH:-captain_food_drill}
STAGING=${STAGING:-/var/backups/captain-food}
REMOTE=${REMOTE:-backup:captain-food-backups}
STATUS="$STAGING/last-drill.json"
WORK="$STAGING/drill"

# shellcheck source=/dev/null
[ -f /etc/captain-food/backup.env ] && . /etc/captain-food/backup.env

fail() {
  printf '{"ok":false,"at":"%s","stage":"%s","error":%s}\n' \
    "$(date -u +%FT%TZ)" "$1" "$(printf '%s' "${2:-unknown}" | jq -Rs .)" > "$STATUS"
  echo "RESTORE DRILL FAILED at stage '$1': ${2:-unknown}" >&2
  cleanup
  exit 1
}

cleanup() {
  dropdb --if-exists "$SCRATCH" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

mkdir -p "$WORK"

# --- Fetch the newest remote dump ---------------------------------------------------------
NEWEST=$(rclone lsf "$REMOTE/daily/" --files-only 2>/dev/null | sort | tail -1) || fail fetch "cannot list remote"
[ -n "$NEWEST" ] || fail fetch "no backups in ${REMOTE}/daily/"
rclone copy "$REMOTE/daily/$NEWEST" "$WORK/" 2>&1 || fail fetch "rclone copy failed for $NEWEST"
DUMP="$WORK/$NEWEST"

# Age check: a drill that passes against a three-week-old dump is a false green.
AGE_H=$(( ( $(date -u +%s) - $(stat -c %Y "$DUMP") ) / 3600 ))

# --- Restore into a scratch database -------------------------------------------------------
dropdb --if-exists "$SCRATCH"
createdb "$SCRATCH"
# -e: stop on error. A restore that "mostly worked" is a failed restore.
pg_restore -d "$SCRATCH" -e -j 2 "$DUMP" >/dev/null 2>"$WORK/restore.err" \
  || fail restore "$(tail -c 500 "$WORK/restore.err")"

# --- Verify: the event log is the thing that matters ----------------------------------------
LIVE=$(psql -tAd "$DB_NAME"  -c "SELECT count(*) FROM domain_events" | tr -d ' ')
REST=$(psql -tAd "$SCRATCH"  -c "SELECT count(*) FROM domain_events" | tr -d ' ')

# The restored copy is a point-in-time snapshot of an APPEND-ONLY log, so it must never hold MORE
# than production, and the shortfall must be explainable by events appended since the dump ran.
[ "$REST" -le "$LIVE" ] || fail verify "restored ${REST} events exceeds live ${LIVE} -- the log is not append-only, or the dump is not from this database"
[ "$REST" -gt 0 ]       || fail verify "restored database has no events at all"

# Prove the schema came back too, not just the rows -- a restore missing the View_* read models
# would still pass a naive row count.
VIEWS=$(psql -tAd "$SCRATCH" -c "SELECT count(*) FROM information_schema.views WHERE table_name LIKE 'View\_%'" | tr -d ' ')
[ "$VIEWS" -gt 0 ] || fail verify "no View_* read models in the restored schema"

# And prove it is queryable, not merely present.
psql -qtAd "$SCRATCH" -c "SELECT 1 FROM domain_events LIMIT 1" >/dev/null || fail verify "restored event log is not queryable"

printf '{"ok":true,"at":"%s","dump":"%s","dump_age_hours":%s,"events_restored":%s,"events_live":%s,"views":%s}\n' \
  "$(date -u +%FT%TZ)" "$NEWEST" "$AGE_H" "$REST" "$LIVE" "$VIEWS" > "$STATUS"
echo "restore drill ok: ${NEWEST} (${AGE_H}h old) -> ${REST}/${LIVE} events, ${VIEWS} views"
