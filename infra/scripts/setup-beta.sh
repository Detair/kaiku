#!/usr/bin/env bash
# =============================================================================
# Kaiku Beta Server Setup — kaiku.pmind.de
#
# Prepares a fresh VPS for running the Kaiku beta:
#   1. Installs Docker + Compose (if missing)
#   2. Clones the repo (if not already present)
#   3. Generates secrets and writes .env
#   4. Configures firewall (UFW)
#   5. Sets up daily database backups
#   6. Starts all services with monitoring
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/Detair/kaiku/main/infra/scripts/setup-beta.sh | bash
#   # or
#   ./infra/scripts/setup-beta.sh
#
# Prerequisites:
#   - Ubuntu/Debian-based VPS
#   - Root or sudo access
#   - DNS: kaiku.pmind.de pointing to this server
# =============================================================================

set -euo pipefail

# --- Configuration -----------------------------------------------------------
DOMAIN="kaiku.pmind.de"
ACME_EMAIL="${ACME_EMAIL:-admin@pmind.de}"
INSTALL_DIR="${INSTALL_DIR:-/opt/kaiku}"
BACKUP_DIR="/var/lib/kaiku/backups"
REPO_URL="https://github.com/Detair/kaiku.git"
BRANCH="main"

# --- Colors ------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

log()  { echo -e "${GREEN}[+]${NC} $1"; }
warn() { echo -e "${YELLOW}[!]${NC} $1"; }
err()  { echo -e "${RED}[x]${NC} $1" >&2; }

# --- Preflight ---------------------------------------------------------------
echo ""
echo -e "${CYAN}╔══════════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║   Kaiku Beta Server Setup — ${DOMAIN}  ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════════════╝${NC}"
echo ""

# Check we're running as root or with sudo
if [[ $EUID -ne 0 ]]; then
    err "This script must be run as root or with sudo."
    exit 1
fi

# Check DNS
log "Checking DNS for ${DOMAIN}..."
RESOLVED_IP=$(dig +short "${DOMAIN}" 2>/dev/null | tail -1)
SERVER_IP=$(curl -sf https://ifconfig.me 2>/dev/null || curl -sf https://api.ipify.org 2>/dev/null || echo "unknown")

if [[ -z "$RESOLVED_IP" ]]; then
    warn "DNS for ${DOMAIN} does not resolve. Make sure it points to this server (${SERVER_IP})."
    read -rp "Continue anyway? [y/N] " yn
    [[ "$yn" =~ ^[Yy]$ ]] || exit 1
elif [[ "$RESOLVED_IP" != "$SERVER_IP" ]]; then
    warn "DNS resolves to ${RESOLVED_IP} but this server is ${SERVER_IP}."
    read -rp "Continue anyway? [y/N] " yn
    [[ "$yn" =~ ^[Yy]$ ]] || exit 1
else
    log "DNS OK: ${DOMAIN} -> ${RESOLVED_IP}"
fi

# --- Step 1: Install Docker --------------------------------------------------
if command -v docker &>/dev/null; then
    log "Docker already installed: $(docker --version)"
else
    log "Installing Docker..."
    apt-get update -qq
    apt-get install -y -qq ca-certificates curl gnupg
    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL https://download.docker.com/linux/ubuntu/gpg | gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    chmod a+r /etc/apt/keyrings/docker.gpg
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu $(. /etc/os-release && echo "$VERSION_CODENAME") stable" \
        > /etc/apt/sources.list.d/docker.list
    apt-get update -qq
    apt-get install -y -qq docker-ce docker-ce-cli containerd.io docker-compose-plugin
    systemctl enable --now docker
    log "Docker installed: $(docker --version)"
fi

# Verify compose
if ! docker compose version &>/dev/null; then
    err "Docker Compose plugin not found. Install docker-compose-plugin."
    exit 1
fi
log "Docker Compose: $(docker compose version --short)"

# --- Step 2: Clone Repo ------------------------------------------------------
if [[ -d "${INSTALL_DIR}/.git" ]]; then
    log "Repository exists at ${INSTALL_DIR}, pulling latest..."
    cd "$INSTALL_DIR"
    git pull origin "$BRANCH"
else
    log "Cloning repository to ${INSTALL_DIR}..."
    git clone --branch "$BRANCH" "$REPO_URL" "$INSTALL_DIR"
    cd "$INSTALL_DIR"
fi

# --- Step 3: Generate Secrets & Write .env ------------------------------------
ENV_FILE="${INSTALL_DIR}/infra/compose/.env"

if [[ -f "$ENV_FILE" ]]; then
    warn ".env already exists at ${ENV_FILE}"
    read -rp "Overwrite with fresh secrets? [y/N] " yn
    [[ "$yn" =~ ^[Yy]$ ]] || { log "Keeping existing .env"; SKIP_ENV=1; }
fi

if [[ "${SKIP_ENV:-0}" != "1" ]]; then
    log "Generating secrets..."
    POSTGRES_PASSWORD=$(openssl rand -base64 24)

    # Generate Ed25519 key pair for JWT signing (EdDSA)
    JWT_PEM_FILE=$(mktemp)
    openssl genpkey -algorithm Ed25519 -out "$JWT_PEM_FILE" 2>/dev/null
    JWT_PRIVATE_KEY=$(base64 -w0 < "$JWT_PEM_FILE")
    JWT_PUBLIC_KEY=$(openssl pkey -in "$JWT_PEM_FILE" -pubout 2>/dev/null | base64 -w0)
    rm -f "$JWT_PEM_FILE"

    MFA_ENCRYPTION_KEY=$(openssl rand -hex 32)
    VALKEY_PASSWORD=$(openssl rand -base64 16)
    GRAFANA_ADMIN_PASSWORD=$(openssl rand -base64 16)
    TURN_SHARED_SECRET=$(openssl rand -hex 32)

    log "Writing ${ENV_FILE}..."
    cp "${INSTALL_DIR}/infra/compose/.env.beta" "$ENV_FILE"

    # Replace CHANGEME placeholders with generated secrets
    sed -i "s|ACME_EMAIL=CHANGEME@pmind.de|ACME_EMAIL=${ACME_EMAIL}|" "$ENV_FILE"
    sed -i "s|POSTGRES_PASSWORD=CHANGEME|POSTGRES_PASSWORD=${POSTGRES_PASSWORD}|" "$ENV_FILE"
    sed -i "s|JWT_PRIVATE_KEY=CHANGEME|JWT_PRIVATE_KEY=${JWT_PRIVATE_KEY}|" "$ENV_FILE"
    sed -i "s|JWT_PUBLIC_KEY=CHANGEME|JWT_PUBLIC_KEY=${JWT_PUBLIC_KEY}|" "$ENV_FILE"
    sed -i "s|MFA_ENCRYPTION_KEY=CHANGEME|MFA_ENCRYPTION_KEY=${MFA_ENCRYPTION_KEY}|" "$ENV_FILE"
    sed -i "s|VALKEY_PASSWORD=CHANGEME|VALKEY_PASSWORD=${VALKEY_PASSWORD}|" "$ENV_FILE"
    sed -i "s|GRAFANA_ADMIN_PASSWORD=CHANGEME|GRAFANA_ADMIN_PASSWORD=${GRAFANA_ADMIN_PASSWORD}|" "$ENV_FILE"
    sed -i "s|TURN_SHARED_SECRET=CHANGEME_TURN_SECRET|TURN_SHARED_SECRET=${TURN_SHARED_SECRET}|" "$ENV_FILE"

    # Point STUN at the self-hosted coturn (serves STUN on the same 3478
    # port) so client IP discovery does not leak to Google's public server.
    sed -i "s|STUN_SERVER=stun:stun.l.google.com:19302|STUN_SERVER=stun:${DOMAIN}:3478|" "$ENV_FILE"

    # Detect public IP for WebRTC and TURN
    if [[ -n "$SERVER_IP" && "$SERVER_IP" != "unknown" ]]; then
        sed -i "s|PUBLIC_IP=CHANGEME|PUBLIC_IP=${SERVER_IP}|" "$ENV_FILE"
        sed -i "s|TURN_SERVER=CHANGEME_TURN|TURN_SERVER=turn:${DOMAIN}:3478|" "$ENV_FILE"
        log "Set PUBLIC_IP=${SERVER_IP}, STUN/TURN to self-hosted coturn (${DOMAIN}:3478)"
    else
        sed -i "s|TURN_SERVER=CHANGEME_TURN|TURN_SERVER=turn:${DOMAIN}:3478|" "$ENV_FILE"
        warn "PUBLIC_IP could not be detected. Set it manually in ${ENV_FILE} for TURN to work."
    fi

    chmod 600 "$ENV_FILE"
    log "Secrets generated and written to ${ENV_FILE}"

    echo ""
    echo -e "${CYAN}--- Generated Credentials (save these!) ---${NC}"
    echo "  Grafana:  admin / ${GRAFANA_ADMIN_PASSWORD}"
    echo "  Postgres: voicechat / ${POSTGRES_PASSWORD}"
    echo -e "${CYAN}--------------------------------------------${NC}"
    echo ""
fi

# --- Step 4: Firewall --------------------------------------------------------
if command -v ufw &>/dev/null; then
    log "Configuring UFW firewall..."
    ufw allow 22/tcp comment "SSH" 2>/dev/null || true
    ufw allow 80/tcp comment "HTTP (Caddy)" 2>/dev/null || true
    ufw allow 443/tcp comment "HTTPS (Caddy)" 2>/dev/null || true
    ufw allow 3478/tcp comment "TURN (coturn)" 2>/dev/null || true
    ufw allow 3478/udp comment "TURN (coturn)" 2>/dev/null || true
    ufw allow 10000:10100/udp comment "WebRTC RTP" 2>/dev/null || true
    ufw allow 49152:49200/udp comment "TURN relay (coturn)" 2>/dev/null || true
    ufw --force enable 2>/dev/null || true
    log "UFW configured: SSH, HTTP, HTTPS, TURN, WebRTC"
else
    warn "UFW not installed. Ensure ports 80, 443 (TCP), 3478 (TCP+UDP), 10000-10100 (UDP), and 49152-49200 (UDP) are open."
fi

# --- Step 5: Backup Cron -----------------------------------------------------
mkdir -p "$BACKUP_DIR"
BACKUP_SCRIPT="${INSTALL_DIR}/infra/scripts/backup.sh"
CRON_LINE="0 3 * * * ${BACKUP_SCRIPT} >> /var/log/kaiku-backup.log 2>&1"

if crontab -l 2>/dev/null | grep -qF "$BACKUP_SCRIPT"; then
    log "Backup cron already configured"
else
    log "Setting up daily backup cron (03:00)..."
    (crontab -l 2>/dev/null || true; echo "$CRON_LINE") | crontab -
    log "Backup cron installed: ${CRON_LINE}"
fi

# --- Step 6: Build & Start ---------------------------------------------------
log "Building Kaiku server image..."
cd "${INSTALL_DIR}/infra/compose"
docker compose build server

log "Starting all services with monitoring..."
docker compose --profile monitoring up -d

# Wait for health
log "Waiting for services to become healthy..."
sleep 10

# Check health
if curl -sf http://localhost:8080/health &>/dev/null; then
    log "Server is healthy!"
else
    warn "Server health check failed. Check: docker compose logs server"
fi

# --- Done! -------------------------------------------------------------------
echo ""
echo -e "${GREEN}╔══════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║          Kaiku Beta Setup Complete!          ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════════╝${NC}"
echo ""
echo "  App:      https://${DOMAIN}"
echo "  Grafana:  http://localhost:3000 (via Netbird)"
echo "  Backups:  ${BACKUP_DIR} (daily at 03:00, 7-day retention)"
echo ""
echo "  Useful commands:"
echo "    cd ${INSTALL_DIR}/infra/compose"
echo "    docker compose logs -f server       # Server logs"
echo "    docker compose logs -f caddy        # TLS/proxy logs"
echo "    docker compose --profile monitoring ps  # All service status"
echo ""
echo "  Update:"
echo "    cd ${INSTALL_DIR} && git pull"
echo "    cd infra/compose && docker compose build --no-cache server"
echo "    docker compose --profile monitoring up -d"
echo ""
