#!/usr/bin/env bash
# Deploy Kaiku to VPS by pulling the pre-built server and web images.
#
# Usage:
#   ./infra/scripts/deploy.sh                    # pull server + web images
#   ./infra/scripts/deploy.sh --server-only      # pull server image only
#   ./infra/scripts/deploy.sh --client-only      # pull web client image only

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
    echo "[+] Pulling web client image on VPS..."
    ssh "$VPS" 'cd /opt/kaiku/infra/compose && docker compose pull web && docker compose up -d web'
    echo "[+] Client deployed."
}

case "$MODE" in
    --server-only) deploy_server ;;
    --client-only) deploy_client ;;
    *) deploy_server; deploy_client ;;
esac

echo "[+] Deploy complete."
