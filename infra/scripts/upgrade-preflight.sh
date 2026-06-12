#!/usr/bin/env bash
# Pre-upgrade safety checks for Kaiku. Run ON THE VPS before any deploy.
#
# Usage (on the VPS):
#   /opt/kaiku/infra/scripts/upgrade-preflight.sh
#
# Exits non-zero if any check fails — do not deploy until all pass.
# Companion playbook: docs/ops/upgrade-playbook.md
#
# Environment variables (defaults shown):
#   COMPOSE_DIR=/opt/kaiku/infra/compose
#   BACKUP_DIR=/var/lib/kaiku/backups
#   MIGRATIONS_DIR=/opt/kaiku/server/migrations
#   POSTGRES_CONTAINER=canis-postgres
#   MIN_FREE_DISK_PCT=20
#   MAX_BACKUP_AGE_HOURS=26

set -uo pipefail

COMPOSE_DIR="${COMPOSE_DIR:-/opt/kaiku/infra/compose}"
BACKUP_DIR="${BACKUP_DIR:-/var/lib/kaiku/backups}"
MIGRATIONS_DIR="${MIGRATIONS_DIR:-/opt/kaiku/server/migrations}"
PG="${POSTGRES_CONTAINER:-canis-postgres}"
MIN_FREE_DISK_PCT="${MIN_FREE_DISK_PCT:-20}"
MAX_BACKUP_AGE_HOURS="${MAX_BACKUP_AGE_HOURS:-26}"

FAILURES=0

pass() { echo "  ✓ $1"; }
fail() {
    echo "  ✗ $1" >&2
    FAILURES=$((FAILURES + 1))
}

echo "=== Kaiku upgrade preflight ($(date)) ==="

# --- 1. Core services healthy ------------------------------------------------
echo "[1/6] Service health"
for c in canis-server "$PG" canis-valkey canis-caddy; do
    state=$(docker inspect --format '{{.State.Status}}{{if .State.Health}}/{{.State.Health.Status}}{{end}}' "$c" 2>/dev/null || echo "missing")
    case "$state" in
        running | running/healthy) pass "$c: $state" ;;
        *) fail "$c: $state" ;;
    esac
done

# --- 2. Server HTTP health ----------------------------------------------------
echo "[2/6] Server /health endpoint"
if curl -sf --max-time 5 http://localhost:8080/health > /dev/null; then
    pass "server responds healthy"
else
    fail "server /health did not respond OK"
fi

# --- 3. Disk space -------------------------------------------------------------
echo "[3/6] Disk space (need >${MIN_FREE_DISK_PCT}% free)"
used_pct=$(df --output=pcent / | tail -1 | tr -dc '0-9')
free_pct=$((100 - used_pct))
if [ "$free_pct" -gt "$MIN_FREE_DISK_PCT" ]; then
    pass "${free_pct}% free on /"
else
    fail "only ${free_pct}% free on / — image pull + old image may not fit"
fi

# --- 4. Backup recency ----------------------------------------------------------
echo "[4/6] Backup recency (<${MAX_BACKUP_AGE_HOURS}h, integrity-checked)"
latest_backup=$(find "$BACKUP_DIR" -name 'kaiku-2*.sql.gz' -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-)
if [ -z "$latest_backup" ]; then
    fail "no postgres backup found in $BACKUP_DIR — run infra/scripts/backup.sh first"
else
    age_hours=$(( ($(date +%s) - $(stat -c %Y "$latest_backup")) / 3600 ))
    if [ "$age_hours" -lt "$MAX_BACKUP_AGE_HOURS" ] && gzip -t "$latest_backup" 2>/dev/null; then
        pass "latest backup ${age_hours}h old, gzip intact ($(basename "$latest_backup"))"
    else
        fail "latest backup is ${age_hours}h old or corrupt — take a fresh one before upgrading"
    fi
fi

# --- 5. Pending migrations (informational gate) --------------------------------
echo "[5/6] Migration delta (repo vs database)"
repo_migrations=$(find "$MIGRATIONS_DIR" -name '*.sql' 2>/dev/null | wc -l)
db_migrations=$(docker exec "$PG" psql -U voicechat -d voicechat -tAc \
    "SELECT count(*) FROM _sqlx_migrations" 2>/dev/null || echo "?")
if [ "$db_migrations" = "?" ]; then
    fail "could not read _sqlx_migrations from the database"
elif [ "$repo_migrations" -lt "$db_migrations" ]; then
    fail "repo has FEWER migrations ($repo_migrations) than the DB ($db_migrations) — checkout is older than the running data; git pull first"
else
    pending=$((repo_migrations - db_migrations))
    pass "$pending pending migration(s) will apply on server start ($db_migrations applied, $repo_migrations in repo)"
    if [ "$pending" -gt 0 ]; then
        echo "    NOTE: there are NO down-migrations — rollback after a migration"
        echo "    means restoring the database from backup (see the playbook)."
    fi
fi

# --- 6. Compose config + rollback reference -------------------------------------
echo "[6/6] Compose config & current image (record for rollback)"
if (cd "$COMPOSE_DIR" && docker compose config -q 2>/dev/null); then
    pass "docker compose config parses"
else
    fail "docker compose config is invalid — fix before deploying"
fi
current_image=$(docker inspect --format '{{index .RepoDigests 0}}' canis-server 2>/dev/null || echo "unknown")
echo "    Current server image (pin this for rollback): $current_image"

echo
if [ "$FAILURES" -gt 0 ]; then
    echo "PREFLIGHT FAILED: $FAILURES check(s) — do not deploy."
    exit 1
fi
echo "Preflight OK — safe to deploy."
