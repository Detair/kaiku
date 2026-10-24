#!/usr/bin/env bash
# Tests for infra/scripts/*.sh using the fake-docker shim (no Docker needed).
# Usage: bash infra/scripts/tests/run.sh
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPTS="$(dirname "$HERE")"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/bin"
cp "$HERE/fake-docker" "$WORK/bin/docker"
export PATH="$WORK/bin:$PATH"
export FAKE_DOCKER_LOG="$WORK/docker.log"

FAILED=0
check() { # check <description> <command...>
    local desc="$1"
    shift
    if "$@"; then
        echo "ok   - $desc"
    else
        echo "FAIL - $desc"
        FAILED=$((FAILED + 1))
    fi
}

# --- syntax -------------------------------------------------------------------
check "bash -n backup.sh" bash -n "$SCRIPTS/backup.sh"
check "bash -n upgrade-preflight.sh" bash -n "$SCRIPTS/upgrade-preflight.sh"

# --- backup: ok ---------------------------------------------------------------
OK_DIR="$WORK/ok"
mkdir -p "$OK_DIR"
BACKUP_DIR="$OK_DIR" BACKUP_RUSTFS=false "$SCRIPTS/backup.sh" >"$WORK/ok.out" 2>&1
check "backup ok: exit 0" test $? -eq 0
shopt -s nullglob
files=("$OK_DIR"/kaiku-*.sql.gz)
check "backup ok: exactly one dump" test "${#files[@]}" -eq 1
check "backup ok: dump complete marker" bash -c 'zcat "$1" | grep -q "dump complete"' _ "${files[0]:-/nonexistent}"

# --- backup: failing dump -----------------------------------------------------
FAIL_DIR="$WORK/fail"
mkdir -p "$FAIL_DIR"
FAKE_DOCKER_MODE=fail BACKUP_DIR="$FAIL_DIR" BACKUP_RUSTFS=false "$SCRIPTS/backup.sh" >"$WORK/fail.out" 2>&1
rc=$?
check "backup fail: exit non-zero" test "$rc" -ne 0
left=("$FAIL_DIR"/*.sql.gz)
check "backup fail: no .sql.gz left behind" test "${#left[@]}" -eq 0

check "backup: exec used in-container POSTGRESQL_PASSWORD" grep -q 'POSTGRESQL_PASSWORD' "$FAKE_DOCKER_LOG"

# --- backup: truncated dump (docker exits 0, no trailer) -----------------------
TR_DIR="$WORK/trunc"
mkdir -p "$TR_DIR"
FAKE_DOCKER_MODE=truncated BACKUP_DIR="$TR_DIR" BACKUP_RUSTFS=false "$SCRIPTS/backup.sh" >"$WORK/trunc.out" 2>&1
rc=$?
check "backup truncated: exit non-zero" test "$rc" -ne 0
left=("$TR_DIR"/*.sql.gz)
check "backup truncated: no .sql.gz left behind" test "${#left[@]}" -eq 0

# --- preflight check 4: truncated latest dump is rejected ----------------------
mkdir -p "$WORK/pfbackups"
printf -- '-- PostgreSQL database dump\nSELECT 1;\n' | gzip > "$WORK/pfbackups/kaiku-2024-01-01_000000.sql.gz"

# --- preflight: migration version comparison (check 5) -------------------------
MIG="$WORK/migrations"
mkdir -p "$MIG" "$WORK/pfbackups"
touch "$MIG/20240101000001_a.sql" "$MIG/20240101000002_b.sql"
pf() { MIGRATIONS_DIR="$MIG" BACKUP_DIR="$WORK/pfbackups" "$SCRIPTS/upgrade-preflight.sh" 2>&1; }
out=$(pf)
check "preflight: truncated backup fails check 4" grep -q "corrupt or incomplete" <<<"$out"
rm -f "$WORK"/pfbackups/*.sql.gz
out=$(FAKE_DOCKER_MIGRATIONS="20240101000001" pf)
check "preflight: 1 pending migration reported" grep -q "1 pending migration" <<<"$out"
out=$(FAKE_DOCKER_MIGRATIONS="20240101000001 20240101000002 20240101000009" pf)
check "preflight: DB version unknown to repo fails" grep -q "missing from the repo" <<<"$out"

# --- dump-complete check under pipefail ---------------------------------------
# A `tail` shim that writes its lines slowly: `... | tail -n 5 | grep -q` lets
# grep exit at the marker, the next write raises SIGPIPE in tail, and pipefail
# turns a complete dump into a failure. The check must not depend on that race.
REAL_TAIL="$(command -v tail)"
mkdir -p "$WORK/slowtail"
cat >"$WORK/slowtail/tail" <<SHIM
#!/usr/bin/env bash
[ "\$*" = "-n 5" ] || exec "$REAL_TAIL" "\$@"
"$REAL_TAIL" -n 5 | while IFS= read -r line; do
    printf '%s\n' "\$line" || exit 141
    sleep 0.2
done
exit "\${PIPESTATUS[1]}"
SHIM
chmod +x "$WORK/slowtail/tail"

SP_DIR="$WORK/sigpipe"
mkdir -p "$SP_DIR"
PATH="$WORK/slowtail:$PATH" BACKUP_DIR="$SP_DIR" BACKUP_RUSTFS=false "$SCRIPTS/backup.sh" >"$WORK/sigpipe.out" 2>&1
check "backup sigpipe: complete dump accepted" test $? -eq 0
kept=("$SP_DIR"/kaiku-*.sql.gz)
check "backup sigpipe: dump kept" test "${#kept[@]}" -eq 1

rm -f "$WORK"/pfbackups/*.sql.gz
printf -- '-- PostgreSQL database dump\nSELECT 1;\n--\n-- PostgreSQL database dump complete\n--\n\n' \
    | gzip > "$WORK/pfbackups/kaiku-2024-01-02_000000.sql.gz"
out=$(PATH="$WORK/slowtail:$PATH" pf)
check "preflight sigpipe: complete backup passes check 4" grep -q "dump complete (" <<<"$out"

if [ "$FAILED" -gt 0 ]; then
    echo "$FAILED check(s) failed"
    exit 1
fi
echo "All ops-script tests passed"
