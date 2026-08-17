#!/usr/bin/env bash
# Bring up the walk target: one local Postgres, the migration chain, the JWKS stub, one server
# process. No image, no cluster, logs on stdout, "rebuild" is "re-run".
#
# TARGET CHOICE (dispatch card s1). The k3s stack is not usable here: 4.7 GB free against a 9.6 GB
# DiskPressure threshold, no kubectl/k3s/sqlx, dockerd down, and its runbook forbids `cargo build`
# while it is up -- which is exactly the red-first loop this walk needs. So: a bare local process.
#
# TWO SETTINGS CARRY OVER, or the cheap target loses a real catch:
#   * APP_PROFILE=production -- development merely PRINTS the config report and continues, so a
#     misconfigured walk would look identical to a healthy one.
#   * the full deploy/generated/secret-keys.json required set -- notably SUPABASE_URL, which is
#     declared for production/staging only. A `development` walk silently gets the fail-closed
#     identity service and 503s every role path: the "auth down vs harness misconfigured"
#     ambiguity arriving by default.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WALK_STATE="${WALK_STATE:-/tmp/captain-walk}"
WALK_PORT="${WALK_PORT:-8080}"
WALK_JWKS_PORT="${WALK_JWKS_PORT:-8099}"
WALK_DB="${WALK_DB:-captain_walk}"
WALK_DATABASE_URL="${WALK_DATABASE_URL:-postgres://walk:walk@127.0.0.1:5432/${WALK_DB}}"
# The real, baked production issuer. NOT stubbed: `iss` must be the real string (see jwks.py).
WALK_SUPABASE_URL="${WALK_SUPABASE_URL:-https://zcshlzhiinwmpzujuiep.supabase.co}"

say()  { printf '%s\n' "$*" >&2; }
fail() { say "FAIL  up: $*"; exit 1; }

mkdir -p "$WALK_STATE"

# --- 1. Postgres ------------------------------------------------------------------------------
if ! pg_lsclusters -h 2>/dev/null | grep -q "^16 *main.*online"; then
  say "up: starting postgres 16/main"
  pg_ctlcluster 16 main start || fail "could not start postgres 16/main"
fi
psql "$WALK_DATABASE_URL" -tAc 'select 1' >/dev/null 2>&1 || {
  say "up: creating role+database"
  su postgres -c "psql -v ON_ERROR_STOP=1 \
    -c \"DO \\\$\\\$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='walk') THEN CREATE ROLE walk LOGIN PASSWORD 'walk' SUPERUSER; END IF; END \\\$\\\$;\" \
    -c \"SELECT 'CREATE DATABASE ${WALK_DB} OWNER walk' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname='${WALK_DB}')\\gexec\"" \
    || fail "could not provision the walk database"
}

# --- 2. Migrations ----------------------------------------------------------------------------
# ONE migration authority: sqlx-cli, the same tool CI's db-migrate.yml runs (ADR-0043). This
# harness deliberately does NOT carry its own runner. It had one briefly, and deleting it is the
# point: two implementations of "apply the chain" is how a local green and a CI red diverge, and
# the semantics are not trivial -- migration 20260730043000 is a `VACUUM FULL` carrying sqlx's
# `-- no-transaction` directive, which a naive runner wraps in a transaction and fails on.
#
# sqlx-cli is not preinstalled here. Prebuilt tarball (seconds, no compile), the shape the
# cutover-local-rehearsal runbook documents and the same version CI resolves:
#   curl -sSL https://github.com/cargo-bins/cargo-quickinstall/releases/download/sqlx-cli-0.8.3/sqlx-cli-0.8.3-x86_64-unknown-linux-gnu.tar.gz | tar xz
command -v sqlx >/dev/null || fail "sqlx-cli not found. Install the prebuilt binary:
  curl -sSL https://github.com/cargo-bins/cargo-quickinstall/releases/download/sqlx-cli-0.8.3/sqlx-cli-0.8.3-x86_64-unknown-linux-gnu.tar.gz | tar xz && install -m755 sqlx /usr/local/bin/sqlx"
say "up: applying the migration chain (sqlx-cli $(sqlx --version | awk '{print $2}'))"
sqlx migrate run --database-url "$WALK_DATABASE_URL" --source "$REPO_ROOT/migrations" >/dev/null \
  || fail "sqlx migrate run failed"

# --- 3. JWKS stub -----------------------------------------------------------------------------
WWW="$(python3 "$REPO_ROOT/tools/walk/jwks.py" init --dir "$WALK_STATE")"
if ! curl -sf "http://127.0.0.1:${WALK_JWKS_PORT}/auth/v1/.well-known/jwks.json" >/dev/null 2>&1; then
  say "up: starting JWKS stub on :${WALK_JWKS_PORT}"
  # setsid + </dev/null: a background child that inherits this script's stdout keeps the write end
  # of a caller's pipe open, so `up.sh | tail` never sees EOF and the caller appears to hang with
  # no output at all. Detaching it fully is the difference between a readable run and a mystery.
  ( cd "$WWW" && setsid python3 -m http.server "$WALK_JWKS_PORT" --bind 127.0.0.1 \
      </dev/null >"$WALK_STATE/jwks-stub.log" 2>&1 & echo $! >"$WALK_STATE/jwks-stub.pid" )
  for _ in $(seq 1 40); do
    curl -sf "http://127.0.0.1:${WALK_JWKS_PORT}/auth/v1/.well-known/jwks.json" >/dev/null 2>&1 && break
    sleep 0.25
  done
fi
curl -sf "http://127.0.0.1:${WALK_JWKS_PORT}/auth/v1/.well-known/jwks.json" >/dev/null \
  || fail "JWKS stub did not come up on :${WALK_JWKS_PORT}"

# --- 4. /etc/hosts ------------------------------------------------------------------------------
# Host classification hard-codes the apex, so the literal names must resolve to loopback.
HOSTS_LINE="127.0.0.1 captain.food live.captain.food system.captain.food smoke-test.captain.food walk-test.captain.food hooks.captain.food api.captain.food"
grep -qF "walk-test.captain.food" /etc/hosts || {
  say "up: mapping captain.food names to 127.0.0.1"
  printf '%s\n' "$HOSTS_LINE" >>/etc/hosts
}

# --- 5. The server ------------------------------------------------------------------------------
# The required production key set. Values that must be REAL are real; the two that cannot be are
# named here rather than hidden:
#   SUPABASE_JWKS_URL -> loopback stub (Supabase unreachable: proxy 502 on CONNECT).
#   STRIPE_SECRET_KEY -> see the walk transcript; no test key is available in this environment.
export APP_PROFILE="${APP_PROFILE:-production}"
export DATABASE_URL="$WALK_DATABASE_URL"
export SUPABASE_URL="$WALK_SUPABASE_URL"
export SUPABASE_JWKS_URL="http://127.0.0.1:${WALK_JWKS_PORT}/auth/v1/.well-known/jwks.json"
export SUPABASE_PUBLISHABLE_KEY="${SUPABASE_PUBLISHABLE_KEY:-sb_publishable_iJnNRwbG123-7bNcFEtz7Q_IvhhfwBE}"
export AUTH_SESSION_KEY="${AUTH_SESSION_KEY:-$(printf '%064d' 0 | tr '0' 'a')}"
export STRIPE_SECRET_KEY="${STRIPE_SECRET_KEY:-}"
export STRIPE_WEBHOOK_SECRET="${STRIPE_WEBHOOK_SECRET:-}"
export PORT="$WALK_PORT"
export WEB_ASSETS_DIR="${WEB_ASSETS_DIR:-$REPO_ROOT/crates/web/assets}"
# Off: it reaches an external API the walk does not exercise and does not need.
export RUN_SIRENE_WORKER=false
# Left at their defaults ON PURPOSE (card s7): all three default OFF and the chain still
# completes, so the walk certifies the pre-flip configuration.
#   ROUTE_ORDER_BIRTH_THROUGH_LANE=false  ENFORCE_ACCEPTANCE_TIMEOUT=false
#   ENFORCE_SERVICE_HOURS_GUARD=false

# The binary may live in the MAIN checkout's target even when the walk runs from a worktree: a
# worktree gets no target/ of its own, and building one costs ~20 GB on a disk with ~4 GB free.
# The walk changes no Rust, so the main checkout's binary IS this branch's binary.
BIN="${WALK_SERVER_BIN:-}"
if [ -z "$BIN" ]; then
  for candidate in \
      "$REPO_ROOT/target/debug/server" \
      "$(git -C "$REPO_ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)/../target/debug/server"; do
    [ -x "$candidate" ] && { BIN="$candidate"; break; }
  done
fi
[ -n "$BIN" ] && [ -x "$BIN" ] \
  || fail "no server binary found -- run: cargo build -p server --bin server (or set WALK_SERVER_BIN)"

if curl -sf "http://127.0.0.1:${WALK_PORT}/ping" >/dev/null 2>&1; then
  say "up: a server is already listening on :${WALK_PORT}"
else
  say "up: starting server on :${WALK_PORT} (profile=$APP_PROFILE)"
  nohup "$BIN" >"$WALK_STATE/server.log" 2>&1 &
  echo $! >"$WALK_STATE/server.pid"
  for _ in $(seq 1 120); do
    curl -sf "http://127.0.0.1:${WALK_PORT}/ping" >/dev/null 2>&1 && break
    sleep 0.5
  done
fi

curl -sf "http://127.0.0.1:${WALK_PORT}/ping" >/dev/null \
  || fail "server did not answer /ping -- see $WALK_STATE/server.log"
say "up: ready"
