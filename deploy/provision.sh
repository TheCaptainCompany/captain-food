#!/usr/bin/env bash
# Provision the Captain.Food production box: OVH VPS-2, Debian 13 (trixie).
#
#   ADR-20260804-171030 -- one box runs app + PostgreSQL + Redis.
#   PostgreSQL on the HOST, app/Redis/Caddy in containers.
#
# IDEMPOTENT: re-running is always safe and is the normal way to apply a change to this file.
# Every step checks before it acts, and nothing here destroys data. The one thing it will never do
# is touch an existing PostgreSQL cluster's data directory.
#
# Usage (as root, on the box):
#   curl -fsSL https://raw.githubusercontent.com/TheCaptainCompany/captain-food/main/deploy/provision.sh -o provision.sh
#   less provision.sh          # read it before you run it -- this is production
#   DEPLOY_PUBKEY="ssh-ed25519 AAAA... captain-food-deploy" bash provision.sh
#
# Take a VPS snapshot first. It is free on this plan and makes the whole exercise reversible.

set -euo pipefail

PG_VERSION=16          # matches ci.yml's postgres:16-alpine -- do NOT take Debian 13's default 17
APP_USER=captain
APP_DIR=/opt/captain-food
CONF_DIR=/etc/captain-food
BACKUP_DIR=/var/backups/captain-food
DB_NAME=captain_food
DB_ROLE=captain_food

log()  { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
ok()   { printf '    \033[32mok\033[0m %s\n' "$*"; }
warn() { printf '    \033[33m!!\033[0m %s\n' "$*"; }
die()  { printf '\n\033[31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------------------------------
# 0. Preflight -- fail loudly rather than half-provision a box that is not what we think
# ---------------------------------------------------------------------------------------
log "Preflight"
[ "$(id -u)" -eq 0 ] || die "run as root"

. /etc/os-release
[ "${ID:-}" = "debian" ] || warn "expected Debian, found '${ID:-unknown}' -- paths below assume the Debian PostgreSQL layout"
[ "${VERSION_ID:-}" = "13" ] || warn "expected Debian 13 (trixie), found '${VERSION_ID:-unknown}'"
ok "$PRETTY_NAME"

MEM_MB=$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)
DISK_GB=$(df -BG --output=size / | tail -1 | tr -dc '0-9')
ok "${MEM_MB} MB RAM, ${DISK_GB} GB disk"
[ "$MEM_MB" -ge 7000 ] || warn "under 8 GB RAM -- postgresql/captain-food.conf is tuned for 8 GB and will over-commit"
[ "$DISK_GB" -ge 70 ]  || warn "under 75 GB disk -- watch pg_dump headroom in backup/pg-backup.sh"

# ---------------------------------------------------------------------------------------
# 1. Base system
# ---------------------------------------------------------------------------------------
log "Base packages"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq \
  ca-certificates curl gnupg lsb-release \
  ufw unattended-upgrades apt-listchanges \
  postgresql-common \
  jq zstd rsync htop rclone
ok "installed"

log "Unattended security upgrades"
# Security patches apply themselves. This is one of the reasons PostgreSQL is on the host and not
# in a container: `apt` patches it, nobody has to remember to repull an image.
cat > /etc/apt/apt.conf.d/20auto-upgrades <<'EOF'
APT::Periodic::Update-Package-Lists "1";
APT::Periodic::Unattended-Upgrade "1";
EOF
systemctl enable --now unattended-upgrades >/dev/null 2>&1 || true
ok "enabled"

# ---------------------------------------------------------------------------------------
# 2. Users
# ---------------------------------------------------------------------------------------
log "Service and deploy users"
id -u "$APP_USER" >/dev/null 2>&1 || adduser --system --group --home "$APP_DIR" --shell /usr/sbin/nologin "$APP_USER"
ok "service user '$APP_USER' (no shell -- nothing logs in as the app)"

id -u deploy >/dev/null 2>&1 || adduser --disabled-password --gecos "CI deploy" deploy
ok "deploy user"

if [ -n "${DEPLOY_PUBKEY:-}" ]; then
  install -d -m 700 -o deploy -g deploy /home/deploy/.ssh
  touch /home/deploy/.ssh/authorized_keys
  grep -qxF "$DEPLOY_PUBKEY" /home/deploy/.ssh/authorized_keys || echo "$DEPLOY_PUBKEY" >> /home/deploy/.ssh/authorized_keys
  chown deploy:deploy /home/deploy/.ssh/authorized_keys
  chmod 600 /home/deploy/.ssh/authorized_keys
  ok "deploy public key installed"
else
  warn "DEPLOY_PUBKEY not set -- CI cannot reach this box until a key is in /home/deploy/.ssh/authorized_keys"
fi

# The deploy user may drive containers, and nothing else. It is NOT in sudo.
# NOTE: docker group membership is root-equivalent in practice (a container can mount /). That is an
# accepted, documented trade for a one-box V0; the mitigation is that the key is CI-only and the
# workflow that uses it is reviewed. Revisit if this box ever hosts a second tenant of trust.
getent group docker >/dev/null 2>&1 && adduser deploy docker >/dev/null 2>&1 || true

# ---------------------------------------------------------------------------------------
# 3. SSH hardening
# ---------------------------------------------------------------------------------------
log "SSH hardening"
# Only applied once a key is actually present -- locking password auth off before that would strand
# you outside your own server with only the OVH web console to get back in.
if [ -s /home/deploy/.ssh/authorized_keys ] || [ -s /root/.ssh/authorized_keys ]; then
  cat > /etc/ssh/sshd_config.d/10-captain-food.conf <<'EOF'
PasswordAuthentication no
PermitRootLogin prohibit-password
KbdInteractiveAuthentication no
X11Forwarding no
EOF
  # Debian 13 ships socket-activated sshd; reload whichever unit is actually in use.
  systemctl reload ssh 2>/dev/null || systemctl restart ssh 2>/dev/null || true
  ok "password auth disabled, root login key-only"
else
  warn "no authorized_keys found -- SSH left as-is on purpose, so you cannot lock yourself out"
fi

# ---------------------------------------------------------------------------------------
# 4. Firewall
# ---------------------------------------------------------------------------------------
log "Firewall"
ufw --force reset >/dev/null
ufw default deny incoming >/dev/null
ufw default allow outgoing >/dev/null
ufw allow 22/tcp  comment 'ssh'   >/dev/null
ufw allow 80/tcp  comment 'http'  >/dev/null
ufw allow 443/tcp comment 'https' >/dev/null
ufw --force enable >/dev/null
ok "22/80/443 in, everything else denied -- PostgreSQL and Redis are loopback only"

# ---------------------------------------------------------------------------------------
# 5. PostgreSQL 16 from PGDG
# ---------------------------------------------------------------------------------------
log "PostgreSQL ${PG_VERSION}"
if [ ! -f /etc/apt/sources.list.d/pgdg.list ]; then
  install -d /usr/share/postgresql-common/pgdg
  curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc \
    -o /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc
  echo "deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc] https://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" \
    > /etc/apt/sources.list.d/pgdg.list
  apt-get update -qq
fi
# postgresql-16 EXPLICITLY -- `postgresql` would pull Debian 13's default 17 and diverge from CI.
apt-get install -y -qq "postgresql-${PG_VERSION}" "postgresql-client-${PG_VERSION}"
ok "installed"

log "PostgreSQL tuning"
SRC="$(dirname "$(readlink -f "$0")")/postgresql/captain-food.conf"
DST="/etc/postgresql/${PG_VERSION}/main/conf.d/captain-food.conf"
install -d "$(dirname "$DST")"
if [ -f "$SRC" ]; then
  install -m 644 "$SRC" "$DST"
else
  curl -fsSL "https://raw.githubusercontent.com/TheCaptainCompany/captain-food/main/deploy/postgresql/captain-food.conf" -o "$DST"
fi
ok "conf.d drop-in written (distro postgresql.conf untouched)"

# shared_preload_libraries needs a restart, not a reload.
systemctl restart "postgresql@${PG_VERSION}-main"
systemctl enable postgresql >/dev/null 2>&1 || true
ok "cluster restarted"

log "Database and role"
if ! su - postgres -c "psql -tAc \"SELECT 1 FROM pg_roles WHERE rolname='${DB_ROLE}'\"" | grep -q 1; then
  DB_PASSWORD="$(openssl rand -base64 33 | tr -d '/+=' | cut -c1-32)"
  su - postgres -c "psql -q -c \"CREATE ROLE ${DB_ROLE} LOGIN PASSWORD '${DB_PASSWORD}'\""
  install -d -m 750 "$CONF_DIR"
  umask 077
  printf 'DATABASE_URL=postgres://%s:%s@127.0.0.1:5432/%s\n' "$DB_ROLE" "$DB_PASSWORD" "$DB_NAME" \
    > "$CONF_DIR/db.env"
  chmod 600 "$CONF_DIR/db.env"
  ok "role created, DATABASE_URL written to ${CONF_DIR}/db.env (mode 600)"
  warn "copy that URL into the DATABASE_URL repo secret -- it is not printed again"
else
  ok "role '${DB_ROLE}' already exists (password untouched)"
fi

if ! su - postgres -c "psql -tAc \"SELECT 1 FROM pg_database WHERE datname='${DB_NAME}'\"" | grep -q 1; then
  su - postgres -c "createdb -O ${DB_ROLE} ${DB_NAME}"
  ok "database '${DB_NAME}' created"
else
  ok "database '${DB_NAME}' already exists"
fi
su - postgres -c "psql -q -d ${DB_NAME} -c 'CREATE EXTENSION IF NOT EXISTS pg_stat_statements'" >/dev/null
ok "pg_stat_statements ready"

# ---------------------------------------------------------------------------------------
# 6. Docker
# ---------------------------------------------------------------------------------------
log "Docker"
if ! command -v docker >/dev/null 2>&1; then
  install -m 0755 -d /etc/apt/keyrings
  curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc
  chmod a+r /etc/apt/keyrings/docker.asc
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian $(lsb_release -cs) stable" \
    > /etc/apt/sources.list.d/docker.list
  apt-get update -qq
  apt-get install -y -qq docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
fi
# Bound log growth. Without this, container logs fill 75 GB eventually and take the database with them.
cat > /etc/docker/daemon.json <<'EOF'
{
  "log-driver": "json-file",
  "log-opts": { "max-size": "50m", "max-file": "3" }
}
EOF
systemctl enable --now docker >/dev/null
systemctl restart docker
adduser deploy docker >/dev/null 2>&1 || true
ok "docker ready, log rotation bounded"

# ---------------------------------------------------------------------------------------
# 7. Layout
# ---------------------------------------------------------------------------------------
log "Directories"
install -d -m 755 "$APP_DIR"
install -d -m 750 "$CONF_DIR"
install -d -m 750 "$BACKUP_DIR"
install -d -m 700 -o postgres -g postgres /var/lib/postgresql/wal-archive   # phase 2, unused for now
chown -R deploy:deploy "$APP_DIR"
ok "$APP_DIR (compose), $CONF_DIR (secrets), $BACKUP_DIR (staging)"

if [ ! -f "$CONF_DIR/app.env" ]; then
  warn "no ${CONF_DIR}/app.env yet -- the app will not boot until it exists (see deploy/env.example)"
fi

# ---------------------------------------------------------------------------------------
# 8. Runtime files: compose, Caddy, backup scripts, timers
# ---------------------------------------------------------------------------------------
SRC_ROOT="$(dirname "$(readlink -f "$0")")"

log "Runtime files"
if [ -d "$SRC_ROOT/systemd" ]; then
  install -m 755 -o deploy -g deploy "$SRC_ROOT/docker-compose.yml" "$APP_DIR/docker-compose.yml"
  ok "compose file -> $APP_DIR"

  # Caddyfile is mounted read-only into the container. Overwritten on every run: it is derived from
  # the repo, so hand edits on the box are drift and are meant to be lost.
  install -m 644 "$SRC_ROOT/Caddyfile" "$CONF_DIR/Caddyfile"
  install -d -m 755 /var/log/caddy
  ok "Caddyfile -> $CONF_DIR"

  install -m 755 "$SRC_ROOT/backup/pg-backup.sh"     /usr/local/bin/captain-food-pg-backup
  install -m 755 "$SRC_ROOT/backup/restore-drill.sh" /usr/local/bin/captain-food-restore-drill
  chown -R postgres:postgres "$BACKUP_DIR"
  ok "backup + restore-drill scripts -> /usr/local/bin"

  install -m 644 "$SRC_ROOT/systemd"/*.service "$SRC_ROOT/systemd"/*.timer /etc/systemd/system/
  systemctl daemon-reload
  systemctl enable --now captain-food-backup.timer captain-food-restore-drill.timer >/dev/null
  ok "nightly backup + fortnightly restore drill timers enabled"

  if [ ! -f "$CONF_DIR/backup.env" ]; then
    warn "no ${CONF_DIR}/backup.env -- backups WILL FAIL until the Scaleway credentials are in place"
  fi
else
  warn "this script was run standalone (no deploy/ tree next to it)"
  warn "clone the repo on the box and re-run to install compose, Caddy, backups and timers"
fi

# ---------------------------------------------------------------------------------------
log "Done"
cat <<EOF

  PostgreSQL   $(su - postgres -c 'psql -tAc "SHOW server_version"' | tr -d ' ')  on 127.0.0.1:5432
  Docker       $(docker --version | cut -d, -f1)
  Firewall     $(ufw status | head -1)

  Next:
    1. Put the runtime environment in ${CONF_DIR}/app.env   (see deploy/env.example)
    2. Copy DATABASE_URL from ${CONF_DIR}/db.env into the repo secret
    3. Put the S3 backup credentials in ${CONF_DIR}/backup.env
    4. Dispatch the 'deploy' workflow -- it brings up compose and Caddy

  Runbook: docs/runbooks/ovh-cutover.md
EOF
