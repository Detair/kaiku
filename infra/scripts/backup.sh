#!/usr/bin/env bash
# Daily backup for Kaiku: PostgreSQL dump + (optional) RustFS object data.
#
# Usage:
#   ./infra/scripts/backup.sh
#
# Crontab (daily at 03:00):
#   0 3 * * * /opt/kaiku/infra/scripts/backup.sh >> /var/log/kaiku-backup.log 2>&1
#
# Restore procedure + drill checklist: docs/ops/backup-restore-runbook.md
#
# Environment variables (all optional, defaults shown):
#   BACKUP_DIR=/var/lib/kaiku/backups
#   POSTGRES_CONTAINER=canis-postgres
#   POSTGRES_USER=voicechat
#   POSTGRES_DB=voicechat
#   RETENTION_DAYS=7
#   BACKUP_RUSTFS=true            # also archive the RustFS data volume
#   RUSTFS_VOLUME=compose_rustfs_data   # docker volume holding object data
#                                       # (verify: docker volume ls | grep rustfs)
#   RUSTFS_RETENTION_DAYS=7

set -euo pipefail

BACKUP_DIR="${BACKUP_DIR:-/var/lib/kaiku/backups}"
CONTAINER="${POSTGRES_CONTAINER:-canis-postgres}"
DB_USER="${POSTGRES_USER:-voicechat}"
DB_NAME="${POSTGRES_DB:-voicechat}"
RETENTION_DAYS="${RETENTION_DAYS:-7}"
BACKUP_RUSTFS="${BACKUP_RUSTFS:-true}"
RUSTFS_VOLUME="${RUSTFS_VOLUME:-compose_rustfs_data}"
RUSTFS_RETENTION_DAYS="${RUSTFS_RETENTION_DAYS:-7}"
TIMESTAMP=$(date +%Y-%m-%d_%H%M%S)
BACKUP_FILE="${BACKUP_DIR}/kaiku-${TIMESTAMP}.sql.gz"
RUSTFS_FILE="${BACKUP_DIR}/kaiku-rustfs-${TIMESTAMP}.tar.gz"

mkdir -p "$BACKUP_DIR"

echo "[$(date)] Starting backup..."

# --- PostgreSQL ------------------------------------------------------------

docker exec "$CONTAINER" pg_dump -U "$DB_USER" "$DB_NAME" | gzip > "$BACKUP_FILE"

# Verify: non-empty AND the gzip stream is intact (a truncated dump from a
# mid-backup failure would otherwise look like a valid backup until restore
# day).
if [ -s "$BACKUP_FILE" ] && gzip -t "$BACKUP_FILE" 2>/dev/null; then
    SIZE=$(du -h "$BACKUP_FILE" | cut -f1)
    echo "[$(date)] Postgres backup complete: $BACKUP_FILE ($SIZE)"
else
    echo "[$(date)] ERROR: Postgres backup missing or corrupt!" >&2
    rm -f "$BACKUP_FILE"
    exit 1
fi

# --- RustFS object data (uploads, avatars, emoji) ---------------------------
# Archives the docker volume contents. RustFS keeps running during the tar;
# for the beta's traffic level a live snapshot is acceptable (worst case: a
# file uploaded mid-backup lands in the next day's archive).

if [ "$BACKUP_RUSTFS" = "true" ]; then
    if docker volume inspect "$RUSTFS_VOLUME" >/dev/null 2>&1; then
        docker run --rm \
            -v "${RUSTFS_VOLUME}:/data:ro" \
            -v "${BACKUP_DIR}:/backup" \
            alpine tar czf "/backup/$(basename "$RUSTFS_FILE")" -C /data .
        if [ -s "$RUSTFS_FILE" ] && gzip -t "$RUSTFS_FILE" 2>/dev/null; then
            SIZE=$(du -h "$RUSTFS_FILE" | cut -f1)
            echo "[$(date)] RustFS backup complete: $RUSTFS_FILE ($SIZE)"
        else
            echo "[$(date)] ERROR: RustFS backup missing or corrupt!" >&2
            rm -f "$RUSTFS_FILE"
            exit 1
        fi
    else
        echo "[$(date)] WARN: RustFS volume '$RUSTFS_VOLUME' not found, skipping object backup" >&2
    fi
fi

# --- Retention ---------------------------------------------------------------

find "$BACKUP_DIR" -name "kaiku-2*.sql.gz" -mtime +"$RETENTION_DAYS" -delete
find "$BACKUP_DIR" -name "kaiku-rustfs-*.tar.gz" -mtime +"$RUSTFS_RETENTION_DAYS" -delete
REMAINING=$(find "$BACKUP_DIR" -name "kaiku-*.gz" | wc -l)
echo "[$(date)] Retained $REMAINING backup file(s)"
