#!/usr/bin/env bash
#
# VoiceChat (Canis) Development Environment Setup Script
#
# This script automates the installation of dependencies for a new development machine.
# It covers System tools, Rust/Cargo, Bun, and Docker.
#
# Usage: ./setup-dev.sh
#

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check for sudo/root
if [ "$EUID" -eq 0 ]; then
    log_warn "Running as root is not recommended for some steps (like rustup)."
fi

log_info "Starting VoiceChat Development Environment Setup..."

# ==============================================================================
# 1. System Dependencies (Linux/Debian-based assumption, adaptable)
# ==============================================================================
log_info "Checking system dependencies..."

if command -v apt-get &> /dev/null; then
    log_info "Detected Debian/Ubuntu system. Updating and installing build essentials..."
    if [ "$EUID" -eq 0 ] || command -v sudo &> /dev/null; then
        CMD_PREFIX=""
        if [ "$EUID" -ne 0 ]; then CMD_PREFIX="sudo"; fi

        $CMD_PREFIX apt-get update
        $CMD_PREFIX apt-get install -y build-essential pkg-config libssl-dev curl git \
            libglib2.0-dev libgdk-pixbuf2.0-dev libsoup-3.0-dev libwebkit2gtk-4.1-dev
        log_success "System dependencies installed."
    else
        log_warn "Skipping system package installation (no sudo)."
    fi
elif command -v dnf &> /dev/null; then
    log_info "Detected Fedora/RHEL system. Installing build essentials..."
    if [ "$EUID" -eq 0 ] || command -v sudo &> /dev/null; then
        CMD_PREFIX=""
        if [ "$EUID" -ne 0 ]; then CMD_PREFIX="sudo"; fi

        $CMD_PREFIX dnf install -y gcc gcc-c++ make pkg-config openssl-devel curl git \
            glib2-devel gdk-pixbuf2-devel libsoup3-devel webkit2gtk4.1-devel gtk3-devel
        log_success "System dependencies installed."
    else
        log_warn "Skipping system package installation (no sudo)."
    fi
else
    log_warn "Unsupported system. Please manually install: gcc, pkg-config, openssl-dev."
fi

# ==============================================================================
# 2. Docker & Docker Compose
# ==============================================================================
log_info "Checking Docker..."

if command -v docker &> /dev/null; then
    log_success "Docker is installed."
    if docker compose version &> /dev/null; then
        log_success "Docker Compose is available."
    else
        log_warn "Docker Compose plugin not found. Please install docker-compose-plugin."
    fi
else
    log_error "Docker is NOT installed. It is required for the database and redis."
    echo "Please install Docker Desktop or Engine: https://docs.docker.com/engine/install/"
    exit 1
fi

# ==============================================================================
# 3. Rust & Cargo
# ==============================================================================
log_info "Checking Rust toolchain..."

if command -v cargo &> /dev/null; then
    log_success "Rust is installed."
else
    log_info "Installing Rust (rustup)..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    log_success "Rust installed."
fi

# Install sqlx-cli for migrations
if ! command -v sqlx &> /dev/null; then
    log_info "Installing sqlx-cli (this may take a minute)..."
    cargo install sqlx-cli --no-default-features --features native-tls,postgres
    log_success "sqlx-cli installed."
else
    log_success "sqlx-cli is already installed."
fi

# ==============================================================================
# 4. Bun (replaces Node.js for package management)
# ==============================================================================
log_info "Checking Bun..."

if command -v bun &> /dev/null; then
    log_success "Bun is installed ($(bun --version))."
else
    log_info "Installing Bun..."
    curl -fsSL https://bun.sh/install | bash
    export BUN_INSTALL="$HOME/.bun"
    export PATH="$BUN_INSTALL/bin:$PATH"
    log_success "Bun installed ($(bun --version))."
fi

# Node.js is still needed for Playwright
log_info "Checking Node.js (required for Playwright)..."

if command -v node &> /dev/null; then
    log_success "Node.js is installed ($(node --version))."
else
    log_warn "Node.js not found. It is required for Playwright tests."
    log_info "Install via: https://nodejs.org or use nvm"
fi

# ==============================================================================
# 5. Project Dependencies
# ==============================================================================
log_info "Installing Frontend dependencies..."

if [ -d "client" ]; then
    cd client
    bun install

    log_info "Installing Playwright browsers..."
    bunx playwright install --with-deps

    cd ..
    log_success "Frontend dependencies installed."
else
    log_error "Directory 'client' not found. Are you in the project root?"
    exit 1
fi

# ==============================================================================
# 6. Database Setup (Optional)
# ==============================================================================
echo ""
read -p "Do you want to start the database via Docker now? (y/n) " -n 1 -r
echo ""
if [[ $REPLY =~ ^[Yy]$ ]]; then
    log_info "Starting infrastructure..."
    docker compose -f docker-compose.dev.yml up -d
    
    log_info "Waiting for database to be ready..."
    sleep 5
    
    log_info "Running migrations..."
    # Ensure DATABASE_URL is set (from .env.example if .env missing)
    if [ ! -f .env ]; then
        cp .env.example .env
        log_info "Created .env from .env.example"
    fi
    
    # Run migrations using sqlx
    # We need to source the .env to get DATABASE_URL for sqlx
    set -a
    source .env
    set +a
    sqlx database create
    sqlx migrate run --source server/migrations
    
    log_success "Database setup complete."
fi

# ==============================================================================
# 7. Final Instructions
# ==============================================================================
echo ""
echo "----------------------------------------------------------------"
log_success "Development Environment Setup Complete!"
echo "----------------------------------------------------------------"
echo ""
echo "To start the backend:"
echo "  cd server && cargo run"
echo ""
echo "To start the frontend:"
echo "  cd client && bun run dev"
echo ""
echo "To run E2E tests:"
echo "  cd client && bunx playwright test"
echo ""
echo "Happy Coding!"
