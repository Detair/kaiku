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
WARNINGS=0
warn_pass() { # passes, but must be read
    echo "  ! WARNING: $1"
    WARNINGS=$((WARNINGS + 1))
}
fail() {
    echo "  ✗ $1" >&2
    FAILURES=$((FAILURES + 1))
}

echo "=== Kaiku upgrade preflight ($(date)) ==="

# --- 1. Core services healthy ------------------------------------------------
echo "[1/7] Service health"
for c in canis-server "$PG" canis-valkey canis-caddy; do
    state=$(docker inspect --format '{{.State.Status}}{{if .State.Health}}/{{.State.Health.Status}}{{end}}' "$c" 2>/dev/null || echo "missing")
    case "$state" in
        running | running/healthy) pass "$c: $state" ;;
        *) fail "$c: $state" ;;
    esac
done

# --- 2. Server HTTP health ----------------------------------------------------
echo "[2/7] Server /health endpoint"
if curl -sf --max-time 5 http://localhost:8080/health > /dev/null; then
    pass "server responds healthy"
else
    fail "server /health did not respond OK"
fi

# --- 3. Disk space -------------------------------------------------------------
echo "[3/7] Disk space (need >${MIN_FREE_DISK_PCT}% free)"
used_pct=$(df --output=pcent / | tail -1 | tr -dc '0-9')
free_pct=$((100 - used_pct))
if [ "$free_pct" -gt "$MIN_FREE_DISK_PCT" ]; then
    pass "${free_pct}% free on /"
else
    fail "only ${free_pct}% free on / — image pull + old image may not fit"
fi

# --- 4. Backup recency ----------------------------------------------------------
echo "[4/7] Backup recency (<${MAX_BACKUP_AGE_HOURS}h, integrity-checked)"
latest_backup=$(find "$BACKUP_DIR" -name 'kaiku-2*.sql.gz' -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-)
if [ -z "$latest_backup" ]; then
    fail "no postgres backup found in $BACKUP_DIR — run infra/scripts/backup.sh first"
else
    age_hours=$(( ($(date +%s) - $(stat -c %Y "$latest_backup")) / 3600 ))
    if [ "$age_hours" -lt "$MAX_BACKUP_AGE_HOURS" ] && gzip -t "$latest_backup" 2>/dev/null \
        && grep -q "PostgreSQL database dump complete" <<<"$(zcat "$latest_backup" | tail -n 5)"; then
        pass "latest backup ${age_hours}h old, gzip intact, dump complete ($(basename "$latest_backup"))"
    else
        fail "latest backup is ${age_hours}h old, corrupt or incomplete — take a fresh one before upgrading"
    fi
fi

# --- 5. Pending migrations (informational gate) --------------------------------
echo "[5/7] Migration delta (repo vs database)"
# Compare versions (numeric filename prefixes vs _sqlx_migrations.version),
# not counts: a renamed or missing migration must not hide behind equal totals.
repo_versions=$(find "$MIGRATIONS_DIR" -maxdepth 1 -name '*.sql' -printf '%f\n' 2>/dev/null \
    | sed -n 's/^\([0-9][0-9]*\)_.*/\1/p' | sed 's/^0*\([0-9]\)/\1/' | sort -u)
if db_versions=$(docker exec "$PG" sh -c \
    'PGPASSWORD="$POSTGRESQL_PASSWORD" psql -U "$POSTGRESQL_USERNAME" -d "$POSTGRESQL_DATABASE" -tAc "SELECT version FROM _sqlx_migrations"' 2>/dev/null); then
    db_versions=$(printf '%s\n' "$db_versions" | sed '/^$/d' | sort -u)
    unknown=$(comm -13 <(printf '%s\n' "$repo_versions") <(printf '%s\n' "$db_versions") | sed '/^$/d')
    pending=$(comm -23 <(printf '%s\n' "$repo_versions") <(printf '%s\n' "$db_versions") | sed '/^$/d')
    if [ -n "$unknown" ]; then
        fail "database has migration version(s) missing from the repo ($(echo "$unknown" | tr '\n' ' ')) — checkout is older than the running data; git pull first"
    else
        pending_count=$(printf '%s' "$pending" | grep -c . || true)
        pass "$pending_count pending migration(s) will apply on server start"
        if [ "$pending_count" -gt 0 ]; then
            echo "    NOTE: there are NO down-migrations — rollback after a migration"
            echo "    means restoring the database from backup (see the playbook)."
        fi
    fi
else
    fail "could not read _sqlx_migrations from the database"
fi

# --- 6. Compose config + rollback reference -------------------------------------
echo "[6/7] Compose config & current image (record for rollback)"
if (cd "$COMPOSE_DIR" && docker compose config -q 2>/dev/null); then
    pass "docker compose config parses"
else
    fail "docker compose config is invalid — fix before deploying"
fi
server_image=$(docker inspect -f '{{.Config.Image}}' canis-server 2>/dev/null || true)
current_image=""
if [ -n "$server_image" ]; then
    current_image=$(docker image inspect --format '{{index .RepoDigests 0}}' "$server_image" 2>/dev/null || true)
    # Locally built images have no RepoDigests: fall back to the image id.
    [ -n "$current_image" ] || current_image=$(docker image inspect --format '{{.Id}}' "$server_image" 2>/dev/null || true)
fi
[ -n "$current_image" ] || current_image="unknown"
echo "    Current server image (pin this for rollback): $current_image"

# --- 7. Rate limiter loaded on the running server -------------------------------
echo "[7/7] Rate limiter loaded at the last start (newer versions do not start without it)"
# Earlier versions logged "Rate limiter initialization failed" and ran
# without per-address limits when the limiter could not load its scripts
# into Valkey; newer versions refuse to start in that case. Only the log of
# the server's LAST start counts (from its last "Starting Kaiku Server"
# line on), and only the server's own messages (the JSON "message" field).
# A live test on the Valkey container shows whether scripts can be loaded
# now; settings changed since the last start are only seen by that test.

# Live test as the Valkey container's default user (the one the bundled
# compose file gives the server). Prints: ok | refused:<answer> | unavailable:<why>
live_script_test() {
    local out
    out=$(docker exec canis-valkey sh -c 'REDISCLI_AUTH="$VALKEY_PASSWORD" valkey-cli SCRIPT LOAD "return 1"' 2>&1)
    if grep -qE '^[0-9a-f]{40}$' <<<"$out"; then
        echo "ok"
    elif grep -qE '(^|[ (])(NOPERM|ERR|WRONGPASS|NOAUTH)' <<<"$out"; then
        echo "refused:$(head -n 1 <<<"$out")"
    else
        echo "unavailable:$(head -n 1 <<<"$out")"
    fi
}
other_user="If the server connects to Valkey as another, restricted user, check as that user: valkey-cli --user <user> SCRIPT LOAD \"return 1\" must print a hash"
see_docs="(docs: configuration/rate-limiting.md, The Server Does Not Start)"

if ! last_start_log=$(docker logs canis-server 2>&1 \
    | awk '/"message":"Starting Kaiku Server"/ { buf = ""; seen = 1 }
           { buf = buf $0 "\n" }
           END { if (seen) printf "%s", buf; else print "NO-START-LINE" }'
    exit "${PIPESTATUS[0]}"); then
    fail "could not read the canis-server logs"
elif [ "$last_start_log" = "NO-START-LINE" ]; then
    # The log no longer reaches back to the last start (rotated or emptied):
    # it proves nothing either way. Only the live test can tell.
    live=$(live_script_test)
    case "$live" in
        ok)
            warn_pass "the server's log no longer shows its last start, so it could not confirm that the rate limiter loaded; a live test on canis-valkey with its default user accepted SCRIPT LOAD. $other_user" ;;
        refused:*)
            fail "cannot tell from the server's log whether the rate limiter loaded (its last start is no longer in the log), and a live test on canis-valkey with its default user did not accept SCRIPT LOAD: ${live#refused:}. $other_user" ;;
        *)
            fail "cannot tell from the server's log whether the rate limiter loaded (its last start is no longer in the log), and the live test on canis-valkey could not run: ${live#unavailable:}. Test as the server's Valkey user: valkey-cli SCRIPT LOAD \"return 1\" must print a hash" ;;
    esac
else
    outcome=$(grep -oE '"message":"(the server does not start: rate limiter not loaded"|Rate limiter initialization failed|Rate limiter initialized"|Rate limiting disabled by configuration")' <<<"$last_start_log" \
        | tail -n 1 | sed -e 's/^"message":"//' -e 's/"$//')
    case "$outcome" in
        "the server does not start: rate limiter not loaded")
            fail "the server's last start was refused: its rate limiter could not load its scripts into Valkey — allow SCRIPT LOAD for the server's Valkey user first $see_docs" ;;
        "Rate limiter initialization failed")
            fail "the server's last start logged 'Rate limiter initialization failed': it runs without per-address limits and the new version will not start — allow SCRIPT LOAD for the server's Valkey user first $see_docs" ;;
        "Rate limiting disabled by configuration")
            pass "rate limiting is switched off (RATE_LIMIT_ENABLED=false); nothing to load" ;;
        "Rate limiter initialized")
            live=$(live_script_test)
            case "$live" in
                ok)
                    pass "the rate limiter loaded at the server's last start, and a live test on canis-valkey with its default user accepts SCRIPT LOAD now" ;;
                refused:*)
                    # The logged success proves the server's own Valkey user
                    # could load scripts; the test ran as the default user,
                    # which may not be the server's.
                    warn_pass "the rate limiter loaded at the server's last start, but the live test was refused for canis-valkey's default user (${live#refused:}): if the server uses that user, the new version will not start — allow SCRIPT LOAD for it first $see_docs. $other_user" ;;
                *)
                    warn_pass "the rate limiter loaded at the server's last start; the live test on canis-valkey could not run (${live#unavailable:}), so Valkey settings changed since that start are not checked. $other_user" ;;
            esac ;;
        *)
            # The last start is in the log but says nothing about the limiter
            # (for example still starting): only the live test can tell.
            live=$(live_script_test)
            case "$live" in
                ok)
                    warn_pass "the server's last start logged nothing about the rate limiter; a live test on canis-valkey with its default user accepted SCRIPT LOAD. $other_user" ;;
                *)
                    fail "cannot tell from the server's last start whether the rate limiter loaded, and the live test on canis-valkey did not succeed: ${live#*:}. $other_user" ;;
            esac ;;
    esac
fi

echo
if [ "$FAILURES" -gt 0 ]; then
    echo "PREFLIGHT FAILED: $FAILURES check(s) — do not deploy."
    exit 1
fi
if [ "$WARNINGS" -gt 0 ]; then
    echo "Preflight OK with $WARNINGS warning(s) — read them before deploying."
else
    echo "Preflight OK — safe to deploy."
fi
