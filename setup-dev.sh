#!/usr/bin/env bash
#
# VoiceChat (Canis) Development Environment Setup Script
#
# This script automates the installation of dependencies for a new development machine.
# It covers System tools, Rust/Cargo, Bun, and Docker.
#
# Supports:
#   - Debian/Ubuntu
#   - Fedora (traditional)
#   - Fedora Atomic (Silverblue, Kinoite, etc.) via Distrobox
#
# Usage: ./setup-dev.sh [--distrobox] [--layer] [--help]
#
# Options:
#   --distrobox    Force Distrobox container setup (default on Atomic)
#   --layer        Use rpm-ostree layering instead of Distrobox (Atomic only)
#   --help         Show this help message
#

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Options
FORCE_DISTROBOX=false
FORCE_LAYER=false
CONTAINER_NAME="canis-dev"

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

log_section() {
    echo ""
    echo -e "${CYAN}══════════════════════════════════════════════════════════════${NC}"
    echo -e "${CYAN}  $1${NC}"
    echo -e "${CYAN}══════════════════════════════════════════════════════════════${NC}"
    echo ""
}

show_help() {
    echo "VoiceChat (Canis) Development Environment Setup"
    echo ""
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  --distrobox    Force Distrobox container setup (default on Atomic)"
    echo "  --layer        Use rpm-ostree layering instead of Distrobox (Atomic only)"
    echo "  --help         Show this help message"
    echo ""
    echo "On Fedora Atomic (Silverblue/Kinoite), Distrobox is the recommended approach."
    echo "This creates a mutable development container while keeping your base system clean."
    exit 0
}

# Parse arguments
for arg in "$@"; do
    case $arg in
        --distrobox)
            FORCE_DISTROBOX=true
            shift
            ;;
        --layer)
            FORCE_LAYER=true
            shift
            ;;
        --help|-h)
            show_help
            ;;
        *)
            log_error "Unknown option: $arg"
            echo "Use --help for usage information."
            exit 1
            ;;
    esac
done

# ==============================================================================
# System Detection
# ==============================================================================

detect_system() {
    IS_ATOMIC=false
    IS_FEDORA=false
    IS_DEBIAN=false
    DISTRO_NAME="Unknown"

    # Check for Fedora Atomic (Silverblue, Kinoite, Sericea, etc.)
    if [[ -f /run/ostree-booted ]]; then
        IS_ATOMIC=true
        IS_FEDORA=true
        if [[ -f /etc/os-release ]]; then
            DISTRO_NAME=$(grep "^VARIANT=" /etc/os-release 2>/dev/null | cut -d= -f2 | tr -d '"' || echo "Atomic")
            [[ -z "$DISTRO_NAME" ]] && DISTRO_NAME="Fedora Atomic"
        fi
    elif command -v rpm-ostree &> /dev/null; then
        IS_ATOMIC=true
        IS_FEDORA=true
        DISTRO_NAME="Fedora Atomic"
    elif command -v dnf &> /dev/null; then
        IS_FEDORA=true
        DISTRO_NAME="Fedora"
    elif command -v apt-get &> /dev/null; then
        IS_DEBIAN=true
        DISTRO_NAME="Debian/Ubuntu"
    fi
}

# ==============================================================================
# Fedora Atomic: Distrobox Setup
# ==============================================================================

setup_distrobox_container() {
    log_section "Fedora Atomic: Distrobox Development Container"

    # Check if distrobox is available
    if ! command -v distrobox &> /dev/null; then
        log_info "Installing Distrobox..."
        # Distrobox is usually pre-installed on Atomic, but just in case
        if command -v rpm-ostree &> /dev/null; then
            log_warn "Distrobox not found. Installing via rpm-ostree (requires reboot)..."
            sudo rpm-ostree install distrobox
            log_error "Please reboot and run this script again."
            exit 1
        else
            log_error "Distrobox not found and cannot be installed automatically."
            echo "Install manually: https://distrobox.it/"
            exit 1
        fi
    fi
    log_success "Distrobox is available"

    # Check if container already exists
    CONTAINER_EXISTS=false
    if distrobox list 2>/dev/null | grep -qw "${CONTAINER_NAME}"; then
        log_info "Container '${CONTAINER_NAME}' already exists."
        read -p "Do you want to recreate it? This will delete the existing container. (y/N) " -n 1 -r
        echo ""
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            log_info "Removing existing container..."
            distrobox stop "${CONTAINER_NAME}" 2>/dev/null || true
            distrobox rm "${CONTAINER_NAME}" --force
        else
            log_info "Using existing container."
            CONTAINER_EXISTS=true
        fi
    fi

    # Create the development container
    if [[ "${CONTAINER_EXISTS}" != "true" ]]; then
        log_info "Creating Distrobox container '${CONTAINER_NAME}'..."

        # Use Fedora as the base (matches host for best compatibility)
        distrobox create \
            --name "${CONTAINER_NAME}" \
            --image registry.fedoraproject.org/fedora-toolbox:41 \
            --yes

        log_success "Container created"
    fi

    # Install dependencies inside the container
    log_info "Installing development dependencies in container..."

    distrobox enter "${CONTAINER_NAME}" -- bash -c '
        set -e
        echo "Installing system packages..."
        sudo dnf install -y \
            gcc gcc-c++ make pkg-config openssl-devel curl git \
            glib2-devel gdk-pixbuf2-devel libsoup3-devel \
            webkit2gtk4.1-devel gtk3-devel \
            clang clang-devel \
            alsa-lib-devel pulseaudio-libs-devel

        echo "System packages installed."
    '

    # Install Rust inside container
    log_info "Setting up Rust toolchain in container..."
    distrobox enter "${CONTAINER_NAME}" -- bash -c '
        if command -v cargo &> /dev/null; then
            echo "Rust is already installed."
        else
            echo "Installing Rust..."
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
            source "$HOME/.cargo/env"
        fi

        # Verify Rust
        source "$HOME/.cargo/env"
        echo "Rust version: $(rustc --version)"
        echo "Cargo version: $(cargo --version)"
    '

    # Install sqlx-cli
    log_info "Installing sqlx-cli in container..."
    distrobox enter "${CONTAINER_NAME}" -- bash -c '
        source "$HOME/.cargo/env"
        if ! command -v sqlx &> /dev/null; then
            echo "Installing sqlx-cli..."
            cargo install sqlx-cli --no-default-features --features native-tls,postgres
        else
            echo "sqlx-cli is already installed."
        fi
    '

    # Install Bun inside container
    log_info "Setting up Bun in container..."
    distrobox enter "${CONTAINER_NAME}" -- bash -c '
        if command -v bun &> /dev/null; then
            echo "Bun is already installed: $(bun --version)"
        else
            echo "Installing Bun..."
            curl -fsSL https://bun.sh/install | bash
            export BUN_INSTALL="$HOME/.bun"
            export PATH="$BUN_INSTALL/bin:$PATH"
            echo "Bun installed: $(bun --version)"
        fi
    '

    # Install Node.js for Playwright
    log_info "Setting up Node.js in container (for Playwright)..."
    distrobox enter "${CONTAINER_NAME}" -- bash -c '
        if command -v node &> /dev/null; then
            echo "Node.js is already installed: $(node --version)"
        else
            echo "Installing Node.js via dnf..."
            sudo dnf install -y nodejs
            echo "Node.js installed: $(node --version)"
        fi
    '

    log_success "Development container '${CONTAINER_NAME}' is ready!"

    # Export commonly used binaries to host
    log_info "Exporting development tools to host..."

    # Create export directory if needed
    mkdir -p "$HOME/.local/bin"

    # Export cargo and related tools
    distrobox enter "${CONTAINER_NAME}" -- bash -c '
        source "$HOME/.cargo/env"
        distrobox-export --bin "$(which cargo)" --export-path "$HOME/.local/bin" 2>/dev/null || true
        distrobox-export --bin "$(which rustc)" --export-path "$HOME/.local/bin" 2>/dev/null || true
        distrobox-export --bin "$(which sqlx)" --export-path "$HOME/.local/bin" 2>/dev/null || true
    ' 2>/dev/null || log_warn "Some exports may have failed (this is often harmless)"

    # Export bun
    distrobox enter "${CONTAINER_NAME}" -- bash -c '
        export BUN_INSTALL="$HOME/.bun"
        export PATH="$BUN_INSTALL/bin:$PATH"
        distrobox-export --bin "$(which bun)" --export-path "$HOME/.local/bin" 2>/dev/null || true
    ' 2>/dev/null || log_warn "Bun export may have failed"

    log_success "Tools exported to ~/.local/bin"
    echo ""
    echo "Make sure ~/.local/bin is in your PATH:"
    echo '  export PATH="$HOME/.local/bin:$PATH"'
}

# ==============================================================================
# Fedora Atomic: rpm-ostree Layering (Alternative)
# ==============================================================================

setup_atomic_layered() {
    log_section "Fedora Atomic: rpm-ostree Layering"

    log_warn "Layering packages on Atomic systems requires reboots and is not recommended."
    log_warn "Consider using --distrobox instead for a cleaner development experience."
    echo ""
    read -p "Continue with rpm-ostree layering? (y/N) " -n 1 -r
    echo ""

    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        log_info "Aborting. Re-run with --distrobox for the recommended approach."
        exit 0
    fi

    log_info "Installing base development packages via rpm-ostree..."

    # Layer essential development packages
    sudo rpm-ostree install --idempotent \
        gcc gcc-c++ make pkg-config openssl-devel curl git \
        glib2-devel gdk-pixbuf2-devel libsoup3-devel \
        webkit2gtk4.1-devel gtk3-devel

    log_warn "A reboot is required for changes to take effect."
    log_warn "After rebooting, run this script again to complete setup."

    # Check if changes are pending
    if rpm-ostree status | grep -q "pending"; then
        read -p "Reboot now? (y/N) " -n 1 -r
        echo ""
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            sudo systemctl reboot
        fi
    fi

    # Continue with user-space tools (don't require reboot)
    setup_rust
    setup_bun
    setup_node
}

# ==============================================================================
# Traditional System Setup Functions
# ==============================================================================

setup_system_deps_debian() {
    log_info "Installing Debian/Ubuntu system dependencies..."

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
}

setup_system_deps_fedora() {
    log_info "Installing Fedora system dependencies..."

    if [ "$EUID" -eq 0 ] || command -v sudo &> /dev/null; then
        CMD_PREFIX=""
        if [ "$EUID" -ne 0 ]; then CMD_PREFIX="sudo"; fi

        $CMD_PREFIX dnf install -y gcc gcc-c++ make pkg-config openssl-devel curl git \
            glib2-devel gdk-pixbuf2-devel libsoup3-devel webkit2gtk4.1-devel gtk3-devel
        log_success "System dependencies installed."
    else
        log_warn "Skipping system package installation (no sudo)."
    fi
}

setup_docker() {
    log_info "Checking Docker..."

    if command -v docker &> /dev/null; then
        log_success "Docker is installed."
        if docker compose version &> /dev/null; then
            log_success "Docker Compose is available."
        else
            log_warn "Docker Compose plugin not found. Please install docker-compose-plugin."
        fi
    elif command -v podman &> /dev/null; then
        log_success "Podman is installed (Docker-compatible)."
        if command -v podman-compose &> /dev/null || command -v docker-compose &> /dev/null; then
            log_success "Compose tool is available."
        else
            log_warn "No compose tool found. Install podman-compose or docker-compose."
        fi
    else
        log_error "Neither Docker nor Podman is installed. One is required for the database and valkey."
        echo "Please install Docker Desktop or Engine: https://docs.docker.com/engine/install/"

        if $IS_ATOMIC; then
            echo ""
            echo "On Fedora Atomic, Podman is recommended:"
            echo "  Podman is usually pre-installed."
            echo "  Install compose: sudo rpm-ostree install podman-compose"
            echo ""
            echo "Or use Docker in a Distrobox container."
        fi
        exit 1
    fi
}

setup_rust() {
    log_info "Checking Rust toolchain..."

    if command -v cargo &> /dev/null; then
        log_success "Rust is installed ($(rustc --version))."
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
}

setup_bun() {
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
}

setup_node() {
    log_info "Checking Node.js (required for Playwright)..."

    if command -v node &> /dev/null; then
        log_success "Node.js is installed ($(node --version))."
    else
        log_warn "Node.js not found. It is required for Playwright tests."
        log_info "Install via: https://nodejs.org or use nvm"
    fi
}

setup_project_deps() {
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
}

setup_database() {
    echo ""
    read -p "Do you want to start the database via Docker/Podman now? (y/n) " -n 1 -r
    echo ""
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        log_info "Starting infrastructure..."

        # Detect compose command
        if docker compose version &> /dev/null 2>&1; then
            COMPOSE_CMD="docker compose"
        elif command -v podman-compose &> /dev/null; then
            COMPOSE_CMD="podman-compose"
        elif command -v docker-compose &> /dev/null; then
            COMPOSE_CMD="docker-compose"
        else
            log_error "No compose command found."
            return 1
        fi

        $COMPOSE_CMD -f docker-compose.dev.yml up -d

        log_info "Waiting for database to be ready..."
        sleep 5

        log_info "Running migrations..."
        # Ensure DATABASE_URL is set (from .env.example if .env missing)
        if [ ! -f .env ]; then
            cp .env.example .env
            log_info "Created .env from .env.example"
        fi

        # Run migrations using sqlx
        set -a
        source .env
        set +a
        sqlx database create
        sqlx migrate run --source server/migrations

        log_success "Database setup complete."
    fi
}

print_final_instructions() {
    echo ""
    echo "----------------------------------------------------------------"
    log_success "Development Environment Setup Complete!"
    echo "----------------------------------------------------------------"
    echo ""

    if $USE_DISTROBOX; then
        echo "Your development container '${CONTAINER_NAME}' is ready."
        echo ""
        echo "To enter the development container:"
        echo "  distrobox enter ${CONTAINER_NAME}"
        echo ""
        echo "Inside the container, you can run:"
        echo "  cd $(pwd)"
        echo "  cargo run -p vc-server    # Start the backend"
        echo "  cd client && bun run dev  # Start the frontend"
        echo ""
        echo "Exported tools in ~/.local/bin can be used directly from the host."
        echo ""
    else
        echo "To start the backend:"
        echo "  cd server && cargo run"
        echo ""
        echo "To start the frontend:"
        echo "  cd client && bun run dev"
        echo ""
    fi

    echo "To run E2E tests:"
    echo "  cd client && bunx playwright test"
    echo ""
    echo "Happy Coding!"
}

# ==============================================================================
# Main Execution
# ==============================================================================

# Check for sudo/root warning
if [ "$EUID" -eq 0 ]; then
    log_warn "Running as root is not recommended for some steps (like rustup)."
fi

log_section "VoiceChat Development Environment Setup"

# Detect system type
detect_system
log_info "Detected system: ${DISTRO_NAME}"

if $IS_ATOMIC; then
    log_info "This is a Fedora Atomic system (immutable base OS)"
fi

# Determine setup method
USE_DISTROBOX=false

if $IS_ATOMIC; then
    if $FORCE_LAYER; then
        log_info "Using rpm-ostree layering (--layer flag)"
        setup_atomic_layered
    elif $FORCE_DISTROBOX || [[ "$FORCE_LAYER" != "true" ]]; then
        # Default to Distrobox on Atomic
        USE_DISTROBOX=true
        setup_distrobox_container
    fi
else
    if $FORCE_DISTROBOX; then
        log_info "Using Distrobox (--distrobox flag)"
        USE_DISTROBOX=true
        setup_distrobox_container
    else
        # Traditional setup
        log_section "System Dependencies"

        if $IS_DEBIAN; then
            setup_system_deps_debian
        elif $IS_FEDORA; then
            setup_system_deps_fedora
        else
            log_warn "Unsupported system. Please manually install: gcc, pkg-config, openssl-dev."
        fi

        log_section "Docker"
        setup_docker

        log_section "Rust Toolchain"
        setup_rust

        log_section "JavaScript Tools"
        setup_bun
        setup_node
    fi
fi

# Project-specific setup (works the same for all methods)
if ! $USE_DISTROBOX; then
    log_section "Project Dependencies"
    setup_project_deps

    log_section "Database Setup"
    setup_database
else
    # For Distrobox, project deps are installed inside container
    log_section "Project Dependencies (in container)"

    distrobox enter "${CONTAINER_NAME}" -- bash -c "
        cd '${SCRIPT_DIR}'
        if [ -d 'client' ]; then
            cd client
            export BUN_INSTALL=\"\$HOME/.bun\"
            export PATH=\"\$BUN_INSTALL/bin:\$PATH\"
            bun install
            echo 'Frontend dependencies installed.'
        fi
    "

    log_section "Database Setup"
    # Docker/Podman should be accessible from host
    setup_docker
    setup_database
fi

print_final_instructions
