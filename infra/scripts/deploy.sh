#!/usr/bin/env bash
# Deploy Kaiku to VPS by pulling pre-built images and updating the client.
#
# Usage:
#   ./infra/scripts/deploy.sh                    # pull image + rebuild client
#   ./infra/scripts/deploy.sh --server-only      # pull image only
#   ./infra/scripts/deploy.sh --client-only      # rebuild client only

set -euo pipefail

VPS="root@kaiku.pmind.de"
MODE="${1:-all}"

deploy_server() {
    echo "[+] Pulling server image on VPS..."
    ssh "$VPS" 'cd /opt/kaiku/infra/compose && docker compose pull server && docker compose --profile monitoring up -d server'
    echo "[+] Waiting for health check..."
    sleep 8
    ssh "$VPS" 'curl -sf http://localhost:8080/health && echo ""'
}

deploy_client() {
    echo "[+] Building and deploying client on VPS..."
    ssh "$VPS" 'bash -s' << 'REMOTE'
set -euo pipefail
export PATH="$HOME/.bun/bin:$PATH"
cd /opt/kaiku && git pull
cd client
echo "VITE_SERVER_URL=https://kaiku.pmind.de" > .env.production
bun install --frozen-lockfile 2>&1 | tail -1
bun run build 2>&1 | tail -2
docker cp dist/. canis-caddy:/srv/kaiku/
REMOTE
    echo "[+] Client deployed."
}

case "$MODE" in
    --server-only) deploy_server ;;
    --client-only) deploy_client ;;
    *) deploy_server; deploy_client ;;
esac

echo "[+] Deploy complete."
