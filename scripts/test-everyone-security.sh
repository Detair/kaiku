#!/usr/bin/env bash
# Run the ignored @everyone dangerous-permissions integration test in Docker.
#
# Prerequisites:
# - Docker running
# - Development Postgres compose service available (docker-compose.dev.yml)
#
# Usage:
#   ./scripts/test-everyone-security.sh
#
# Optional env overrides:
#   RUST_IMAGE=rust:1.88-bookworm
#   SQLX_OFFLINE=true

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

RUST_IMAGE="${RUST_IMAGE:-rust:1.88-bookworm}"
SQLX_OFFLINE="${SQLX_OFFLINE:-true}"

POSTGRES_CONTAINER="canis-dev-postgres"
DB_URL="postgres://voicechat:voicechat_dev@${POSTGRES_CONTAINER}:5432/voicechat"

echo "[everyone-security] Ensuring postgres container is running..."
docker compose -f "$PROJECT_ROOT/docker-compose.dev.yml" up -d postgres >/dev/null

NETWORK_NAME="$(docker inspect -f '{{range $k,$v := .NetworkSettings.Networks}}{{$k}}{{end}}' "$POSTGRES_CONTAINER")"
if [ -z "$NETWORK_NAME" ]; then
  echo "[everyone-security] Could not determine docker network for $POSTGRES_CONTAINER" >&2
  exit 1
fi

echo "[everyone-security] Running ignored integration test in $RUST_IMAGE on network $NETWORK_NAME"
docker run --rm \
  --network "$NETWORK_NAME" \
  -e SQLX_OFFLINE="$SQLX_OFFLINE" \
  -e DATABASE_URL="$DB_URL" \
  -v "$PROJECT_ROOT:/work" \
  -w /work \
  "$RUST_IMAGE" \
  bash -lc 'export PATH=/usr/local/cargo/bin:$PATH && apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && cargo test -p vc-server test_cannot_grant_dangerous_permissions_to_everyone -- --ignored --nocapture'
