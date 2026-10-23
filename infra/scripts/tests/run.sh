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

if [ "$FAILED" -gt 0 ]; then
    echo "$FAILED check(s) failed"
    exit 1
fi
echo "All ops-script tests passed"
