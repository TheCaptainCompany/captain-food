#!/bin/sh
# PLATFORM SOURCE, hand-written (#360) -- the WEEKLY RESTORE DRILL (PROP-20260806-223656 s2b
# practice 4): restore the latest base backup + WAL from Object Storage into the scratch
# namespace, then verify the restored event log against production OVER THE SAME RANGE -- count
# and content checksum of domain_events up to the restored high-water position, plus the
# migration chain. "A backup that has never been restored is a hope, not a backup." On ANY
# failure this files a GitHub issue and exits non-zero.
#
# Environment (set by the CronJob; the codegen test platform_drill_env_matches_cluster_backup
# pins BARMAN_* and DRILL_IMAGE to the values in ../cnpg/cluster.yaml and cnpg-operator/PIN.json):
#   BARMAN_DESTINATION_PATH  s3://... path the production cluster archives into (read here)
#   BARMAN_ENDPOINT_URL      OVH Object Storage S3 endpoint
#   DRILL_IMAGE              postgres image, same digest pin as production
#   GITHUB_REPO / GITHUB_TOKEN  issue filing (lib-issue.sh)
#   WAIT_MINUTES             recovery deadline (default 40)
#
# What the drill deliberately does NOT do:
#   - no `backup:` section on the drill cluster: archiving into the production destination
#     path would corrupt the only recovery path (serverName collision);
#   - no Retain storage class: the throwaway volume must die with the throwaway cluster;
#   - no writes anywhere near production: the comparison connects as SELECT-only claude_ro.
#
# GDPR caveat (documented, not papered over): erasure is tombstone-then-stream-deletion
# (ADR-20260731-160000), so an erasure run BETWEEN the archived point and this drill can
# legitimately shrink production's count/checksum over the compared range. The failure issue
# says to check for that before treating the mismatch as corruption.

set -eu

. /scripts/lib-issue.sh

PROD_NS="captain-prod"
DRILL_NS="captain-restore-drill"
DRILL_CLUSTER="drill-db"
PROD_SERVER_NAME="captain-db"
WAIT_MINUTES="${WAIT_MINUTES:-40}"
STEP="init"

fail() {
  echo "RESTORE DRILL FAILED at step: ${STEP}" >&2
  echo "detail: $1" >&2
  recent=$(kubectl -n "${DRILL_NS}" get events --sort-by=.lastTimestamp 2>/dev/null | tail -15 || true)
  file_issue "[restore-drill] weekly restore drill FAILED at: ${STEP}" \
"The weekly restore drill (deploy/platform/restore-drill/, #360, s2b practice 4) failed.

- step: \`${STEP}\`
- detail: ${1}
- namespace: \`${DRILL_NS}\`, cluster: \`${DRILL_CLUSTER}\`

Recent namespace events:
\`\`\`
${recent}
\`\`\`

Checklist before assuming archive corruption:
1. A GDPR erasure run between the archived point and now legitimately changes production's count/checksum over the compared range (tombstone-then-stream-deletion, ADR-20260731-160000).
2. \`kubectl -n ${DRILL_NS} describe cluster ${DRILL_CLUSTER}\` for the recovery status.
3. The WAL-archive-age check's issues, if any -- a stale archive fails the drill too.

The drill is the only rehearsal of the ONLY recovery path (ADR-20260807-114122): treat as urgent, not housekeeping. Runbook: deploy/platform/README.md."
  exit 1
}

# ---- 1. wipe any leftovers from a previous run (resource-level; the namespace stands) -------
STEP="cleanup-previous"
kubectl -n "${DRILL_NS}" delete cluster "${DRILL_CLUSTER}" --ignore-not-found --wait=true --timeout=10m \
  || fail "could not delete leftover drill cluster from a previous run"
kubectl -n "${DRILL_NS}" delete secret cnpg-object-storage --ignore-not-found \
  || fail "could not delete leftover copied secret"

# ---- 2. copy the object-storage credentials into the scratch namespace ----------------------
STEP="copy-credentials"
kubectl -n "${PROD_NS}" get secret cnpg-object-storage -o json \
  | jq 'del(.metadata.namespace, .metadata.uid, .metadata.resourceVersion,
            .metadata.creationTimestamp, .metadata.ownerReferences, .metadata.managedFields)' \
  | kubectl -n "${DRILL_NS}" apply -f - \
  || fail "cnpg-object-storage secret missing or unreadable in ${PROD_NS} -- unprovisioned? (README checklist item 2)"
# The copied credential is read+write to the ONLY backup of the event log; bound its lifetime
# to THIS RUN whatever happens next — without the trap, a failure between here and teardown
# leaves it in the scratch namespace until next Monday's cleanup-previous (7-day exposure).
trap 'kubectl -n "${DRILL_NS}" delete secret cnpg-object-storage --ignore-not-found' EXIT

# ---- 3. recovery cluster: latest base backup + all archived WAL ----------------------------
STEP="create-recovery-cluster"
cat <<EOF | kubectl -n "${DRILL_NS}" apply -f - || fail "could not create the recovery cluster"
apiVersion: postgresql.cnpg.io/v1
kind: Cluster
metadata:
  name: ${DRILL_CLUSTER}
  labels:
    app.kubernetes.io/part-of: captain-food
spec:
  instances: 1
  imageName: ${DRILL_IMAGE}
  enableSuperuserAccess: false
  # Throwaway: DEFAULT storage class (delete-reclaim), never captain-db-retain -- a weekly
  # drill on Retain leaks one Cinder volume per run. And NO backup section: this cluster must
  # never archive into the production destination path.
  storage:
    size: 20Gi
  resources:
    requests:
      cpu: 100m
      memory: 512Mi
    limits:
      memory: 1Gi
  bootstrap:
    recovery:
      source: prod-archive
  externalClusters:
    - name: prod-archive
      barmanObjectStore:
        destinationPath: ${BARMAN_DESTINATION_PATH}
        endpointURL: ${BARMAN_ENDPOINT_URL}
        serverName: ${PROD_SERVER_NAME}
        s3Credentials:
          accessKeyId:
            name: cnpg-object-storage
            key: ACCESS_KEY_ID
          secretAccessKey:
            name: cnpg-object-storage
            key: ACCESS_SECRET_KEY
        wal:
          compression: gzip
EOF

# ---- 4. wait for the recovery to reach a ready primary --------------------------------------
STEP="wait-for-recovery"
tries=$(( WAIT_MINUTES * 3 ))   # poll every 20s
i=0
while :; do
  ready=$(kubectl -n "${DRILL_NS}" get cluster "${DRILL_CLUSTER}" \
            -o jsonpath='{.status.readyInstances}' 2>/dev/null || echo "")
  [ "${ready}" = "1" ] && break
  i=$(( i + 1 ))
  if [ "${i}" -ge "${tries}" ]; then
    phase=$(kubectl -n "${DRILL_NS}" get cluster "${DRILL_CLUSTER}" -o jsonpath='{.status.phase}' 2>/dev/null || echo "?")
    fail "recovery not ready after ${WAIT_MINUTES} minutes (phase: ${phase})"
  fi
  sleep 20
done
PRIMARY=$(kubectl -n "${DRILL_NS}" get cluster "${DRILL_CLUSTER}" -o jsonpath='{.status.currentPrimary}')
[ -n "${PRIMARY}" ] || fail "recovered cluster reports no current primary"

# psql inside the restored pod (peer auth on the local socket -- the drill cluster is ours).
drill_sql() {
  kubectl -n "${DRILL_NS}" exec "${PRIMARY}" -c postgres -- psql -U postgres -d app -tA -c "$1"
}

# ---- 5. verify: the restored database is a real, migrated schema ----------------------------
STEP="verify-schema"
mig_count=$(drill_sql "SELECT count(*) FROM _sqlx_migrations" ) \
  || fail "restored database has no _sqlx_migrations table -- restore produced no schema"
[ "${mig_count}" -gt 0 ] || fail "restored migration chain is empty"

# ---- 6. verify: counts + checksums vs production over the SAME event-log range --------------
# The event log is append-only ordered by position, so production restricted to the restored
# high-water position must match the restored copy EXACTLY (modulo the GDPR caveat above).
STEP="verify-event-log"
restored_max=$(drill_sql "SELECT coalesce(max(position), 0) FROM domain_events") \
  || fail "restored database has no domain_events table"
events_q="SELECT count(*) || '|' || coalesce(md5(string_agg(id::text, ',' ORDER BY position)), 'empty') FROM domain_events WHERE position <= ${restored_max}"
migrations_q="SELECT count(*) || '|' || coalesce(md5(string_agg(version::text, ',' ORDER BY version)), 'empty') FROM (SELECT version FROM _sqlx_migrations ORDER BY version LIMIT ${mig_count}) m"
restored_events=$(drill_sql "${events_q}") || fail "checksum query failed on the restored copy"
restored_migrations=$(drill_sql "${migrations_q}") || fail "checksum query failed on the restored copy (migrations)"

STEP="verify-against-production"
CLAUDE_RO_PASSWORD=$(kubectl -n "${PROD_NS}" get secret claude-ro-credentials \
  -o jsonpath='{.data.password}' 2>/dev/null | base64 -d) \
  || fail "claude-ro-credentials secret missing in ${PROD_NS} -- unprovisioned? (README checklist item 2)"
[ -n "${CLAUDE_RO_PASSWORD}" ] || fail "claude-ro-credentials has an empty password"
prod_psql() {
  kubectl -n "${DRILL_NS}" exec "${PRIMARY}" -c postgres -- \
    env PGPASSWORD="${CLAUDE_RO_PASSWORD}" \
    psql "host=captain-db-rw.${PROD_NS}.svc user=claude_ro dbname=app sslmode=require" -tA -c "$1"
}
prod_events=$(prod_psql "${events_q}") \
  || fail "could not query production as claude_ro (role unprovisioned, grants missing, or captain-db-rw unreachable)"
prod_migrations=$(prod_psql "${migrations_q}") || fail "could not query production migrations as claude_ro"

[ "${restored_events}" = "${prod_events}" ] \
  || fail "event-log mismatch over positions <= ${restored_max}: restored='${restored_events}' production='${prod_events}' (count|md5)"
[ "${restored_migrations}" = "${prod_migrations}" ] \
  || fail "migration-chain mismatch over the first ${mig_count} migrations: restored='${restored_migrations}' production='${prod_migrations}'"

# ---- 7. success: report and tear down -------------------------------------------------------
STEP="teardown"
echo "RESTORE DRILL PASSED: ${restored_events%%|*} events (positions <= ${restored_max}), ${mig_count} migrations, checksums match production."
kubectl -n "${DRILL_NS}" delete cluster "${DRILL_CLUSTER}" --wait=true --timeout=10m \
  || fail "verification PASSED but teardown failed -- the drill cluster is still consuming node memory"
kubectl -n "${DRILL_NS}" delete secret cnpg-object-storage --ignore-not-found
echo "drill complete: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
