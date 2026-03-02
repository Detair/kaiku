#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

SPECS=("e2e/gates.spec.ts")
CUSTOM_SPECS=false
PLAYWRIGHT_PROJECT="chromium"
PLAYWRIGHT_WORKERS="${PLAYWRIGHT_WORKERS:-1}"
KEEP_STACK=false
EXTRA_ARGS=()

usage() {
  cat <<'EOF'
Usage: ./scripts/run-e2e-real.sh [options] [-- <extra-playwright-args>]

Options:
  --spec <path>       Playwright spec path relative to client/ (repeatable)
  --project <name>    Playwright project/browser (default: chromium)
  --keep-stack        Keep backend and containers running after test
  -h, --help          Show help

Environment overrides:
  E2E_COMPOSE_EXTRA   Extra compose file (absolute or repo-relative)
  E2E_DB_PORT         Database host port override
  DATABASE_URL        Full database URL override
  PLAYWRIGHT_WORKERS  Playwright worker count (default: 1)
  CONTAINERS_STORAGE_CONF  Podman storage config (auto-detected if absent)

Examples:
  ./scripts/run-e2e-real.sh
  ./scripts/run-e2e-real.sh --spec e2e/onboarding.spec.ts
  ./scripts/run-e2e-real.sh --spec e2e/gates.spec.ts --spec e2e/status-presence.spec.ts
  ./scripts/run-e2e-real.sh --project firefox -- --headed
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --spec)
      if [[ "${CUSTOM_SPECS}" = false ]]; then
        SPECS=()
        CUSTOM_SPECS=true
      fi
      SPECS+=("$2")
      shift 2
      ;;
    --project)
      PLAYWRIGHT_PROJECT="$2"
      shift 2
      ;;
    --keep-stack)
      KEEP_STACK=true
      shift
      ;;
    --)
      shift
      EXTRA_ARGS=("$@")
      break
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

log() {
  printf '[e2e-real] %s\n' "$1"
}

fail() {
  printf '[e2e-real] ERROR: %s\n' "$1" >&2
  exit 1
}

resolve_path() {
  local candidate="$1"
  if [[ "${candidate}" = /* ]]; then
    printf '%s' "${candidate}"
  else
    printf '%s' "${PROJECT_ROOT}/${candidate}"
  fi
}

command -v bun >/dev/null 2>&1 || fail "bun is required"
command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v sqlx >/dev/null 2>&1 || fail "sqlx is required"
command -v cargo >/dev/null 2>&1 || fail "cargo is required"

COMPOSE_CMD=()
RUNTIME_CMD=()
ENGINE=""

if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
  COMPOSE_CMD=(docker compose)
  RUNTIME_CMD=(docker)
  ENGINE="docker"
elif command -v podman-compose >/dev/null 2>&1; then
  COMPOSE_CMD=(podman-compose)
  RUNTIME_CMD=(podman)
  ENGINE="podman"
elif command -v docker-compose >/dev/null 2>&1 && command -v docker >/dev/null 2>&1; then
  COMPOSE_CMD=(docker-compose)
  RUNTIME_CMD=(docker)
  ENGINE="docker"
else
  fail "docker compose, docker-compose, or podman-compose is required"
fi

if [[ "${ENGINE}" = "podman" && -z "${CONTAINERS_STORAGE_CONF:-}" && -f "${PROJECT_ROOT}/.tmp/podman-storage.conf" ]]; then
  export CONTAINERS_STORAGE_CONF="${PROJECT_ROOT}/.tmp/podman-storage.conf"
  log "Using CONTAINERS_STORAGE_CONF=${CONTAINERS_STORAGE_CONF}"
fi

COMPOSE_FILES=(-f "${PROJECT_ROOT}/docker-compose.dev.yml")
USE_HOSTNET=false

EXTRA_FILE=""
if [[ -n "${E2E_COMPOSE_EXTRA:-}" ]]; then
  EXTRA_FILE="$(resolve_path "${E2E_COMPOSE_EXTRA}")"
elif [[ "${ENGINE}" = "podman" && -f "${PROJECT_ROOT}/.tmp/docker-compose.hostnet.yml" ]]; then
  EXTRA_FILE="${PROJECT_ROOT}/.tmp/docker-compose.hostnet.yml"
fi

if [[ -n "${EXTRA_FILE}" ]]; then
  [[ -f "${EXTRA_FILE}" ]] || fail "Compose override not found: ${EXTRA_FILE}"
  COMPOSE_FILES+=( -f "${EXTRA_FILE}" )
  if grep -q 'network_mode:[[:space:]]*host' "${EXTRA_FILE}"; then
    USE_HOSTNET=true
  fi
fi

if [[ -n "${E2E_DB_PORT:-}" ]]; then
  DB_PORT="${E2E_DB_PORT}"
elif [[ "${USE_HOSTNET}" = true ]]; then
  DB_PORT="5432"
else
  DB_PORT="5433"
fi

DATABASE_URL="${DATABASE_URL:-postgres://voicechat:voicechat_dev@localhost:${DB_PORT}/voicechat}"
REDIS_URL="${REDIS_URL:-redis://localhost:6379}"
S3_ENDPOINT="${S3_ENDPOINT:-http://localhost:9000}"
S3_BUCKET="${S3_BUCKET:-voicechat}"
AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-rustfsdev}"
AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-rustfsdev_secret}"
JWT_SECRET="${JWT_SECRET:-dev-secret-change-in-production}"
JWT_PRIVATE_KEY="${JWT_PRIVATE_KEY:-LS0tLS1CRUdJTiBQUklWQVRFIEtFWS0tLS0tCk1DNENBUUF3QlFZREsyVndCQ0lFSUZuUDFodDNNcjlkOGJyYW4zV2IyTGFxSStqd2NnY0V4YXp2V0pQNWUrSG8KLS0tLS1FTkQgUFJJVkFURSBLRVktLS0tLQo=}"
JWT_PUBLIC_KEY="${JWT_PUBLIC_KEY:-LS0tLS1CRUdJTiBQVUJMSUMgS0VZLS0tLS0KTUNvd0JRWURLMlZ3QXlFQW80TlJjVnQ2ajF3OHRCWUtxUEJzS0krNUZVREkwVGtJaHF4WWlud05TRlU9Ci0tLS0tRU5EIFBVQkxJQyBLRVktLS0tLQo=}"

mkdir -p "${PROJECT_ROOT}/.tmp"
SERVER_LOG="${PROJECT_ROOT}/.tmp/vc-server.log"
SERVER_PID_FILE="${PROJECT_ROOT}/.tmp/vc-server.pid"
SERVER_PID=""
CLEANUP_DONE=false
COMPOSE_UP_LOG="${PROJECT_ROOT}/.tmp/e2e-compose-up.log"

POSTGRES_CONTAINER="${POSTGRES_CONTAINER:-canis-dev-postgres}"
VALKEY_CONTAINER="${VALKEY_CONTAINER:-canis-dev-valkey}"
RUSTFS_CONTAINER="${RUSTFS_CONTAINER:-canis-dev-rustfs}"

compose() {
  "${COMPOSE_CMD[@]}" "${COMPOSE_FILES[@]}" "$@"
}

stop_backend() {
  local pid="${SERVER_PID}"

  if [[ -z "${pid}" && -f "${SERVER_PID_FILE}" ]]; then
    pid="$(tr -d '[:space:]' < "${SERVER_PID_FILE}")"
  fi

  if [[ -n "${pid}" ]] && kill -0 "${pid}" >/dev/null 2>&1; then
    kill "${pid}" >/dev/null 2>&1 || true
    for _ in $(seq 1 20); do
      if ! kill -0 "${pid}" >/dev/null 2>&1; then
        break
      fi
      sleep 0.2
    done

    if kill -0 "${pid}" >/dev/null 2>&1; then
      kill -9 "${pid}" >/dev/null 2>&1 || true
    fi
  fi

  SERVER_PID=""
  rm -f "${SERVER_PID_FILE}"
}

wait_for_postgres() {
  for _ in $(seq 1 60); do
    if "${RUNTIME_CMD[@]}" exec "${POSTGRES_CONTAINER}" pg_isready -U voicechat -d voicechat >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  compose ps >&2 || true
  fail "PostgreSQL did not become ready"
}

wait_for_valkey() {
  for _ in $(seq 1 60); do
    if "${RUNTIME_CMD[@]}" exec "${VALKEY_CONTAINER}" valkey-cli ping >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  compose ps >&2 || true
  fail "Valkey did not become ready"
}

wait_for_rustfs() {
  for _ in $(seq 1 60); do
    if curl -sf "${S3_ENDPOINT}/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  fail "RustFS did not become ready"
}

wait_for_backend() {
  for _ in $(seq 1 60); do
    if curl -sf "http://localhost:8080/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  tail -n 80 "${SERVER_LOG}" >&2 || true
  fail "Backend did not become ready"
}

apply_migrations() {
  for _ in $(seq 1 20); do
    if DATABASE_URL="${DATABASE_URL}" sqlx migrate run --source "${PROJECT_ROOT}/server/migrations"; then
      return 0
    fi
    sleep 1
  done
  fail "Failed to apply migrations"
}

init_bucket() {
  for _ in $(seq 1 20); do
    if "${RUNTIME_CMD[@]}" run --rm --network "container:${RUSTFS_CONTAINER}" --entrypoint sh docker.io/minio/mc -c "mc alias set local ${S3_ENDPOINT} ${AWS_ACCESS_KEY_ID} ${AWS_SECRET_ACCESS_KEY} && mc mb --ignore-existing local/${S3_BUCKET} && mc anonymous set none local/${S3_BUCKET}"; then
      return 0
    fi
    sleep 1
  done
  fail "Failed to initialize RustFS bucket"
}

cleanup() {
  if [[ "${CLEANUP_DONE}" = true ]]; then
    return
  fi
  CLEANUP_DONE=true

  if [[ "${KEEP_STACK}" = true ]]; then
    log "Keeping backend and containers running"
    return
  fi
  log "Cleaning up backend and containers"
  stop_backend
  compose down -v >/dev/null 2>&1 || true
}

trap cleanup EXIT INT TERM

log "Resetting containers and volumes"
stop_backend
compose down -v >/dev/null 2>&1 || true

log "Starting infrastructure"
if compose --profile storage up -d >"${COMPOSE_UP_LOG}" 2>&1; then
  :
else
  log "Compose profile start failed, retrying without profile"
  if compose up -d >>"${COMPOSE_UP_LOG}" 2>&1; then
    compose up -d rustfs >>"${COMPOSE_UP_LOG}" 2>&1 || true
  else
    cat "${COMPOSE_UP_LOG}" >&2 || true
    fail "Infrastructure failed to start"
  fi
fi

log "Waiting for services"
wait_for_postgres
wait_for_valkey
wait_for_rustfs

log "Applying migrations"
apply_migrations

log "Ensuring RustFS bucket"
init_bucket

log "Starting backend"
pushd "${PROJECT_ROOT}" >/dev/null
DATABASE_URL="${DATABASE_URL}" \
REDIS_URL="${REDIS_URL}" \
JWT_SECRET="${JWT_SECRET}" \
JWT_PRIVATE_KEY="${JWT_PRIVATE_KEY}" \
JWT_PUBLIC_KEY="${JWT_PUBLIC_KEY}" \
S3_ENDPOINT="${S3_ENDPOINT}" \
S3_BUCKET="${S3_BUCKET}" \
AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID}" \
AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY}" \
cargo run -p vc-server > "${SERVER_LOG}" 2>&1 &
SERVER_PID=$!
popd >/dev/null
printf '%s\n' "${SERVER_PID}" > "${SERVER_PID_FILE}"
wait_for_backend

log "Running Playwright: ${SPECS[*]} (${PLAYWRIGHT_PROJECT}, workers=${PLAYWRIGHT_WORKERS})"
(
  cd "${PROJECT_ROOT}/client"
  bunx playwright test "${SPECS[@]}" --project "${PLAYWRIGHT_PROJECT}" --workers "${PLAYWRIGHT_WORKERS}" "${EXTRA_ARGS[@]}"
)

log "Playwright run completed successfully"
