#!/usr/bin/env bash
# Apply the migration chain to a LOCAL database, without sqlx-cli.
#
# WHY this exists rather than `sqlx migrate run`: sqlx-cli is not installed in the walk
# environment and `cargo install sqlx-cli` costs a multi-GB build on a disk that has ~4 GB free.
# The app never reads the migration CHECKSUMS -- `/health` reads exactly
# `SELECT max(version) FROM _sqlx_migrations WHERE success` (crates/server/src/lib.rs:1521) and
# compares it to REQUIRED_SCHEMA_VERSION. So the honest minimum is: apply every file in version
# order, and record each in `_sqlx_migrations` with the checksum sqlx itself would have written
# (SHA-384 of the file bytes), so a later real `sqlx migrate run` against the same database is a
# no-op rather than a checksum-mismatch failure.
#
# This is a LOCAL-ONLY tool. It refuses any target that is not loopback: applying a hand-rolled
# migration chain to a shared database is how a schema diverges from the one CI believes it has.
set -euo pipefail

WALK_DATABASE_URL="${WALK_DATABASE_URL:?WALK_DATABASE_URL must be set}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MIGRATIONS_DIR="$REPO_ROOT/migrations"

say() { printf '%s\n' "$*" >&2; }
fail() { say "FAIL  migrate: $*"; exit 1; }

# Fence: loopback only. A migration chain applied by anything other than sqlx-cli must never touch
# a database another process believes CI owns.
case "$WALK_DATABASE_URL" in
  *@127.0.0.1:*|*@localhost:*) : ;;
  *) fail "refusing a non-loopback target ($WALK_DATABASE_URL). This tool is local-only." ;;
esac

command -v psql >/dev/null || fail "psql not found"
[ -d "$MIGRATIONS_DIR" ] || fail "no migrations directory at $MIGRATIONS_DIR"

psql_do() { psql "$WALK_DATABASE_URL" -v ON_ERROR_STOP=1 "$@"; }

# The sqlx bookkeeping table, verbatim in shape from sqlx's own migrator.
psql_do -q -c "
CREATE TABLE IF NOT EXISTS _sqlx_migrations (
  version BIGINT PRIMARY KEY,
  description TEXT NOT NULL,
  installed_on TIMESTAMPTZ NOT NULL DEFAULT now(),
  success BOOLEAN NOT NULL,
  checksum BYTEA NOT NULL,
  execution_time BIGINT NOT NULL
);"

applied=0
skipped=0
for f in $(ls "$MIGRATIONS_DIR"/*.sql | sort); do
  base="$(basename "$f")"
  version="${base%%_*}"
  description="${base#*_}"
  description="${description%.sql}"
  description="${description//_/ }"

  already=$(psql_do -tAc "SELECT count(*) FROM _sqlx_migrations WHERE version = $version AND success;")
  if [ "$already" != "0" ]; then
    skipped=$((skipped + 1))
    continue
  fi

  checksum="\\x$(python3 -c "
import hashlib, sys
print(hashlib.sha384(open(sys.argv[1],'rb').read()).hexdigest())
" "$f")"

  say "  applying $base"
  start=$(date +%s%N)
  # Honour sqlx's own `-- no-transaction` directive (first line). VACUUM FULL and CREATE INDEX
  # CONCURRENTLY cannot run inside a transaction block, and a runner that wraps them anyway fails
  # on a statement sqlx itself would have executed happily -- a harness artifact masquerading as a
  # broken migration.
  txn=(--single-transaction)
  if [ "$(head -1 "$f")" = "-- no-transaction" ]; then
    txn=()
  fi
  # Each migration with its bookkeeping row: a migration that half-applies and still records
  # success is the failure mode that makes a schema unexplainable afterwards. (A no-transaction
  # migration cannot have that guarantee -- that is inherent to the directive, not to this runner.)
  if ! psql_do -q "${txn[@]}" \
        -c "$(cat "$f")" \
        -c "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
            VALUES ($version, '$description', true, '$checksum'::bytea, 0)
            ON CONFLICT (version) DO UPDATE SET success = true;"; then
    fail "migration $base failed -- schema left at the last successful version"
  fi
  end=$(date +%s%N)
  psql_do -q -c "UPDATE _sqlx_migrations SET execution_time = $(( (end - start) )) WHERE version = $version;"
  applied=$((applied + 1))
done

max=$(psql_do -tAc "SELECT coalesce(max(version), 0) FROM _sqlx_migrations WHERE success;")
say "migrate: applied=$applied skipped=$skipped max_version=$max"
printf '%s\n' "$max"
