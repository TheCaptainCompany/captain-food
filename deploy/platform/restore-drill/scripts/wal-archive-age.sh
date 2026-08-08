#!/bin/sh
# PLATFORM SOURCE, hand-written (#360) -- the WAL-ARCHIVE-AGE check (PROP-20260806-223656 s2b
# practice 4's alert half). Hourly: reads the production Cluster STATUS (a resourceNames-scoped
# get -- the check never touches the database itself) and files a GitHub issue when the only
# recovery path is degrading:
#   1. ContinuousArchiving condition != True  -- WAL archiving is failing RIGHT NOW; every
#      unarchived segment widens the data-loss window of a node failure (instances: 1 -- there
#      is no replica to fail over to, ADR-20260807-114122).
#   2. firstRecoverabilityPoint absent        -- no base backup has EVER completed: nothing is
#      restorable at all (day-one misconfiguration, or the bucket/secret is gone).
#   3. lastSuccessfulBackup older than BACKUP_MAX_AGE_HOURS (default 28) -- the daily
#      ScheduledBackup (02:30 UTC) has missed a cycle.
# Filing is deduplicated by exact title (lib-issue.sh) so a day-long incident is one issue,
# not 24. Finer-grained lag metrics (seconds of unarchived WAL) belong to the Honeycomb
# trigger loop (#364), not to this coarse hourly safety net.

set -eu

. /scripts/lib-issue.sh

PROD_NS="captain-prod"
CLUSTER="captain-db"
BACKUP_MAX_AGE_HOURS="${BACKUP_MAX_AGE_HOURS:-28}"

status=$(kubectl -n "${PROD_NS}" get cluster "${CLUSTER}" -o json 2>&1) || {
  file_issue "[wal-archive] cannot read cluster ${CLUSTER} status" \
"The hourly WAL-archive-age check (deploy/platform/restore-drill/, #360) could not read the production Cluster resource at all:

\`\`\`
${status}
\`\`\`

Either the cluster is gone (worst case) or the drill RBAC drifted. Check \`kubectl -n ${PROD_NS} get cluster\` from a read-path session immediately."
  exit 1
}

problems=""

archiving=$(printf '%s' "${status}" | jq -r '.status.conditions[]? | select(.type == "ContinuousArchiving") | .status' 2>/dev/null || echo "")
archiving_msg=$(printf '%s' "${status}" | jq -r '.status.conditions[]? | select(.type == "ContinuousArchiving") | "\(.reason // "?"): \(.message // "?") (since \(.lastTransitionTime // "?"))"' 2>/dev/null || echo "?")
if [ "${archiving}" != "True" ]; then
  problems="${problems}
- ContinuousArchiving is '${archiving:-absent}' -- WAL archiving is NOT healthy: ${archiving_msg}"
fi

first_point=$(printf '%s' "${status}" | jq -r '.status.firstRecoverabilityPoint // empty')
if [ -z "${first_point}" ]; then
  problems="${problems}
- firstRecoverabilityPoint is absent -- NO base backup has ever completed; nothing is restorable (README checklist item 5)."
fi

last_backup=$(printf '%s' "${status}" | jq -r '.status.lastSuccessfulBackup // empty')
if [ -n "${last_backup}" ]; then
  last_epoch=$(date -u -d "${last_backup}" +%s 2>/dev/null || echo 0)
  now_epoch=$(date -u +%s)
  age_hours=$(( (now_epoch - last_epoch) / 3600 ))
  if [ "${last_epoch}" = "0" ]; then
    problems="${problems}
- lastSuccessfulBackup '${last_backup}' could not be parsed."
  elif [ "${age_hours}" -gt "${BACKUP_MAX_AGE_HOURS}" ]; then
    problems="${problems}
- lastSuccessfulBackup is ${age_hours}h old (max ${BACKUP_MAX_AGE_HOURS}h) -- the daily ScheduledBackup is missing cycles."
  fi
elif [ -n "${first_point}" ]; then
  problems="${problems}
- lastSuccessfulBackup is absent while a recoverability point exists -- backup status is inconsistent; inspect \`kubectl -n ${PROD_NS} get backup\`."
fi

if [ -n "${problems}" ]; then
  echo "WAL-archive check FAILED:${problems}" >&2
  file_issue "[wal-archive] WAL archiving / backup recency degraded on ${CLUSTER}" \
"The hourly WAL-archive-age check (deploy/platform/restore-drill/, #360, s2b practice 4) found the recovery path degraded:
${problems}

At \`instances: 1\` (ADR-20260807-114122) the archive IS the recovery path: a node failure during this condition loses everything since the last archived WAL segment. Peak exposure is Fri/Sat 19:00-21:30 Europe/Paris. Diagnose via the read path (D7): \`kubectl -n ${PROD_NS} describe cluster ${CLUSTER}\`, then fix by PR; this issue auto-deduplicates hourly while the condition persists."
  exit 1
fi

echo "WAL-archive check OK: archiving healthy, first recoverability point ${first_point}, last backup ${last_backup:-n/a}."
