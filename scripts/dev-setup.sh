#!/usr/bin/env bash
#
# VoiceChat Development Environment Setup
#
# This script sets up the complete development environment:
# - Checks for required tools
# - Creates .env file with secure defaults
# - Starts Docker services (PostgreSQL, Redis, MinIO, MailHog)
# - Runs database migrations
# - Installs frontend dependencies
#
# Usage: ./scripts/dev-setup.sh [--clean] [--no-docker] [--no-client]
#

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Options
CLEAN=false
NO_DOCKER=false
NO_CLIENT=false

# Parse arguments
for arg in "$@"; do
    case $arg in
        --clean)
            CLEAN=true
            shift
            ;;
        --no-docker)
            NO_DOCKER=true
            shift
            ;;
        --no-client)
            NO_CLIENT=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --clean      Remove existing .env and Docker volumes before setup"
            echo "  --no-docker  Skip Docker services setup"
            echo "  --no-client  Skip client npm install"
            echo "  --help, -h   Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $arg"
            exit 1
            ;;
    esac
done

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if a command exists
check_command() {
    if command -v "$1" &> /dev/null; then
        return 0
    else
        return 1
    fi
}

# Check required version
check_version() {
    local cmd=$1
    local min_version=$2
    local current_version=$3

    if [[ "$(printf '%s\n' "$min_version" "$current_version" | sort -V | head -n1)" == "$min_version" ]]; then
        return 0
    else
        return 1
    fi
}

# Generate secure random string
generate_secret() {
    openssl rand -base64 32 2>/dev/null || head -c 32 /dev/urandom | base64
}

echo ""
echo "======================================"
echo "  VoiceChat Development Setup"
echo "======================================"
echo ""

# =============================================================================
# Step 1: Check Required Tools
# =============================================================================
log_info "Checking required tools..."

MISSING_TOOLS=()

# Check Rust
if check_command rustc; then
    RUST_VERSION=$(rustc --version | grep -oP '\d+\.\d+\.\d+' | head -1)
    if check_version rustc "1.82.0" "$RUST_VERSION"; then
        log_success "Rust $RUST_VERSION"
    else
        log_error "Rust $RUST_VERSION found, but 1.82+ required"
        echo "  Update with: rustup update stable"
        MISSING_TOOLS+=("Rust 1.82+ (current: $RUST_VERSION)")
    fi
else
    MISSING_TOOLS+=("rustc (https://rustup.rs)")
fi

# Check Cargo
if check_command cargo; then
    log_success "Cargo $(cargo --version | grep -oP '\d+\.\d+\.\d+' | head -1)"
else
    MISSING_TOOLS+=("cargo (https://rustup.rs)")
fi

# Check Node.js
if check_command node; then
    NODE_VERSION=$(node --version | grep -oP '\d+' | head -1)
    if [[ "$NODE_VERSION" -ge 18 ]]; then
        log_success "Node.js $(node --version)"
    else
        log_warn "Node.js $(node --version) found, but v18+ recommended"
    fi
else
    MISSING_TOOLS+=("node (https://nodejs.org)")
fi

# Check npm/pnpm
if check_command pnpm; then
    PKG_MANAGER="pnpm"
    log_success "pnpm $(pnpm --version)"
elif check_command npm; then
    PKG_MANAGER="npm"
    log_success "npm $(npm --version)"
else
    MISSING_TOOLS+=("npm or pnpm")
fi

# Check Docker
if ! $NO_DOCKER; then
    if check_command docker; then
        log_success "Docker $(docker --version | grep -oP '\d+\.\d+\.\d+' | head -1)"
    else
        MISSING_TOOLS+=("docker (https://docs.docker.com/get-docker/)")
    fi

    # Check Docker Compose
    if docker compose version &> /dev/null; then
        log_success "Docker Compose $(docker compose version | grep -oP '\d+\.\d+\.\d+' | head -1)"
    elif check_command docker-compose; then
        log_success "Docker Compose (standalone)"
        DOCKER_COMPOSE="docker-compose"
    else
        MISSING_TOOLS+=("docker compose")
    fi
fi

# Check sqlx-cli (optional but recommended)
if check_command sqlx; then
    log_success "sqlx-cli $(sqlx --version | grep -oP '\d+\.\d+\.\d+' | head -1)"
    HAS_SQLX=true
else
    log_warn "sqlx-cli not found (optional, install with: cargo install sqlx-cli)"
    HAS_SQLX=false
fi

# Check openssl (for secret generation)
if check_command openssl; then
    log_success "OpenSSL $(openssl version | grep -oP '\d+\.\d+\.\d+' | head -1)"
else
    log_warn "OpenSSL not found, using fallback for secret generation"
fi

# Exit if required tools are missing
if [[ ${#MISSING_TOOLS[@]} -gt 0 ]]; then
    echo ""
    log_error "Missing required tools:"
    for tool in "${MISSING_TOOLS[@]}"; do
        echo "  - $tool"
    done
    echo ""
    echo "Please install the missing tools and run this script again."
    exit 1
fi

echo ""

# =============================================================================
# Step 2: Clean (if requested)
# =============================================================================
if $CLEAN; then
    log_info "Cleaning previous setup..."

    # Remove .env
    if [[ -f "${PROJECT_ROOT}/.env" ]]; then
        rm "${PROJECT_ROOT}/.env"
        log_success "Removed .env"
    fi

    # Stop and remove Docker volumes
    if ! $NO_DOCKER; then
        cd "${PROJECT_ROOT}/infra/compose"
        docker compose -f docker-compose.dev.yml down -v 2>/dev/null || true
        log_success "Removed Docker volumes"
    fi

    echo ""
fi

# =============================================================================
# Step 3: Create .env file
# =============================================================================
log_info "Setting up environment configuration..."

ENV_FILE="${PROJECT_ROOT}/.env"

if [[ -f "$ENV_FILE" ]] && ! $CLEAN; then
    log_warn ".env file already exists, skipping (use --clean to regenerate)"
else
    # Generate secure JWT secret
    JWT_SECRET=$(generate_secret)

    cat > "$ENV_FILE" << EOF
# VoiceChat Development Environment
# Generated by dev-setup.sh on $(date -Iseconds)
#
# DO NOT COMMIT THIS FILE

# =============================================================================
# Database & Services
# =============================================================================

DATABASE_URL=postgres://voicechat:devpassword@localhost:5432/voicechat
REDIS_URL=redis://localhost:6379

# =============================================================================
# Authentication
# =============================================================================

# JWT secret (auto-generated, keep secure!)
JWT_SECRET=${JWT_SECRET}

# Token expiry (seconds)
JWT_ACCESS_EXPIRY=900
JWT_REFRESH_EXPIRY=604800

# =============================================================================
# Server
# =============================================================================

BIND_ADDRESS=0.0.0.0:8080
RUST_LOG=vc_server=debug,tower_http=debug,sqlx=warn

# =============================================================================
# S3 Storage (MinIO for development)
# =============================================================================

S3_ENDPOINT=http://localhost:9000
S3_BUCKET=voicechat
AWS_ACCESS_KEY_ID=minioadmin
AWS_SECRET_ACCESS_KEY=minioadmin

# =============================================================================
# WebRTC
# =============================================================================

STUN_SERVER=stun:stun.l.google.com:19302
# TURN_SERVER=
# TURN_USERNAME=
# TURN_CREDENTIAL=

# =============================================================================
# OIDC (Optional - leave empty to disable)
# =============================================================================

# OIDC_ISSUER_URL=
# OIDC_CLIENT_ID=
# OIDC_CLIENT_SECRET=

# =============================================================================
# Email (MailHog for development)
# =============================================================================

SMTP_HOST=localhost
SMTP_PORT=1025
EOF

    log_success "Created .env with secure defaults"
fi

echo ""

# =============================================================================
# Step 4: Start Docker Services
# =============================================================================
if ! $NO_DOCKER; then
    log_info "Starting Docker services..."

    cd "${PROJECT_ROOT}/infra/compose"

    # Use docker compose (v2) or docker-compose
    if docker compose version &> /dev/null; then
        DOCKER_COMPOSE="docker compose"
    else
        DOCKER_COMPOSE="docker-compose"
    fi

    $DOCKER_COMPOSE -f docker-compose.dev.yml up -d

    log_info "Waiting for services to be healthy..."

    # Wait for PostgreSQL
    echo -n "  PostgreSQL: "
    for i in {1..30}; do
        if docker exec voicechat-dev-postgres pg_isready -U voicechat &> /dev/null; then
            echo -e "${GREEN}ready${NC}"
            break
        fi
        if [[ $i -eq 30 ]]; then
            echo -e "${RED}timeout${NC}"
            log_error "PostgreSQL failed to start"
            exit 1
        fi
        sleep 1
        echo -n "."
    done

    # Wait for Redis
    echo -n "  Redis: "
    for i in {1..30}; do
        if docker exec voicechat-dev-redis redis-cli ping &> /dev/null; then
            echo -e "${GREEN}ready${NC}"
            break
        fi
        if [[ $i -eq 30 ]]; then
            echo -e "${RED}timeout${NC}"
            log_error "Redis failed to start"
            exit 1
        fi
        sleep 1
        echo -n "."
    done

    log_success "All Docker services are running"
    echo ""
    echo "  Services:"
    echo "    - PostgreSQL: localhost:5432 (user: voicechat, pass: devpassword)"
    echo "    - Redis:      localhost:6379"
    echo "    - MinIO:      localhost:9000 (console: localhost:9001)"
    echo "    - MailHog:    localhost:8025 (SMTP: localhost:1025)"
    echo ""
fi

# =============================================================================
# Step 5: Run Database Migrations
# =============================================================================
log_info "Running database migrations..."

cd "${PROJECT_ROOT}"

# Load .env for DATABASE_URL
export $(grep -v '^#' .env | xargs)

if $HAS_SQLX; then
    # Use sqlx-cli if available
    sqlx database create 2>/dev/null || true
    sqlx migrate run --source server/migrations
    log_success "Migrations completed (sqlx-cli)"
else
    # Fallback: Use psql directly
    if check_command psql; then
        PGPASSWORD=devpassword psql -h localhost -U voicechat -d voicechat -f server/migrations/20240101000000_initial_schema.sql 2>/dev/null || {
            log_warn "Migration may have already been applied"
        }
        log_success "Migrations completed (psql)"
    else
        log_warn "Neither sqlx-cli nor psql available. Please run migrations manually:"
        echo "  cargo install sqlx-cli"
        echo "  sqlx migrate run --source server/migrations"
    fi
fi

echo ""

# =============================================================================
# Step 6: Install Client Dependencies
# =============================================================================
if ! $NO_CLIENT; then
    log_info "Installing client dependencies..."

    cd "${PROJECT_ROOT}/client"

    if [[ "$PKG_MANAGER" == "pnpm" ]]; then
        pnpm install
    else
        npm install
    fi

    log_success "Client dependencies installed"
    echo ""
fi

# =============================================================================
# Step 7: Build Check
# =============================================================================
log_info "Running cargo check (this may take a while on first run)..."

cd "${PROJECT_ROOT}"
cargo check 2>&1 | head -20 || {
    log_warn "Cargo check showed some warnings/errors (see output above)"
}

echo ""

# =============================================================================
# Done!
# =============================================================================
echo "======================================"
echo -e "  ${GREEN}Setup Complete!${NC}"
echo "======================================"
echo ""
echo "Quick Start:"
echo ""
echo "  # Terminal 1: Start the server"
echo "  cargo run -p vc-server"
echo ""
echo "  # Terminal 2: Start the client (dev mode)"
echo "  cd client && ${PKG_MANAGER} run tauri dev"
echo ""
echo "Useful commands:"
echo ""
echo "  make dev          # Start server in watch mode"
echo "  make client       # Start client in dev mode"
echo "  make test         # Run all tests"
echo "  make check        # Run cargo check + clippy"
echo "  make db-reset     # Reset database"
echo "  make docker-logs  # View Docker logs"
echo ""
echo "Environment:"
echo ""
echo "  Server:    http://localhost:8080"
echo "  MinIO:     http://localhost:9001 (admin/minioadmin)"
echo "  MailHog:   http://localhost:8025"
echo ""
