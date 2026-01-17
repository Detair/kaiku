#!/bin/bash
# Dependency Update Helper Script
# Usage: ./scripts/update-deps.sh [phase]

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Detect project root dynamically
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

print_header() {
    echo -e "\n${BLUE}========================================${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}========================================${NC}\n"
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
}

check_rust_version() {
    print_header "Checking Rust Version"

    RUST_VERSION=$(rustc --version | grep -oP '\d+\.\d+' | head -1)
    CARGO_VERSION=$(cargo --version | grep -oP '\d+\.\d+' | head -1)

    echo "Current Rust: $RUST_VERSION"
    echo "Current Cargo: $CARGO_VERSION"

    if (( $(echo "$RUST_VERSION < 1.82" | bc -l) )); then
        print_error "Rust $RUST_VERSION is too old (need 1.82+)"
        echo "Run: rustup update stable && rustup default stable"
        return 1
    fi

    print_success "Rust version OK"
}

phase_system() {
    print_header "Phase 0: System Update (CRITICAL)"

    if ! check_rust_version; then
        print_warning "Updating Rust toolchain..."
        rustup self update
        rustup update stable
        rustup default stable

        print_success "Rust updated to $(rustc --version)"
    fi

    print_header "Rebuilding project with new Rust version"
    cd "$PROJECT_ROOT"
    cargo clean
    cargo build --release

    print_success "System update complete!"
}

phase_bun_safe() {
    print_header "Phase 1: Safe Frontend Updates (Non-Breaking)"

    cd "$PROJECT_ROOT/client"

    print_warning "Updating safe packages..."
    bun install @tauri-apps/plugin-shell@^2.3.4
    bun install lucide-solid@^0.562.0
    bun install eslint-plugin-solid@^0.14.5

    print_warning "Testing build..."
    bun run build

    print_success "Safe frontend updates complete!"
}

phase_rust_backend() {
    print_header "Phase 2: Rust Backend Updates (Breaking)"

    print_error "This phase requires manual code changes!"
    echo ""
    echo "Updates needed:"
    echo "  1. axum 0.7 → 0.8"
    echo "  2. sqlx 0.7 → 0.8"
    echo "  3. rustls 0.23 → 0.24"
    echo "  4. fred 8 → 9"
    echo ""
    echo "Estimated time: 3-5 hours"
    echo "See DEPENDENCY_REVIEW.md for migration guides"
    echo ""
    read -p "Continue? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        print_warning "Skipping backend updates"
        return 0
    fi

    print_error "Manual update required - this script cannot automate breaking changes"
}

phase_frontend() {
    print_header "Phase 3: Frontend Updates (Breaking)"

    print_error "This phase requires manual code changes!"
    echo ""
    echo "Updates needed:"
    echo "  1. vite 5 → 7"
    echo "  2. eslint 8 → 9"
    echo "  3. @solidjs/router 0.10 → 0.15"
    echo ""
    echo "Estimated time: 3-5 hours"
    echo "See DEPENDENCY_REVIEW.md for migration guides"
}

run_tests() {
    print_header "Running Test Suite"

    cd "$PROJECT_ROOT"

    print_warning "Running Rust tests..."
    if cargo test --workspace; then
        print_success "Rust tests passed"
    else
        print_error "Rust tests failed"
        return 1
    fi

    print_warning "Running frontend build..."
    cd client
    if bun run build; then
        print_success "Frontend build succeeded"
    else
        print_error "Frontend build failed"
        return 1
    fi
}

show_status() {
    print_header "Dependency Status"

    cd "$PROJECT_ROOT"

    echo "Rust Version:"
    rustc --version
    cargo --version
    echo ""

    echo "Bun outdated packages:"
    cd client
    bun outdated || true
    echo ""

    print_success "Status check complete"
}

show_help() {
    echo "Dependency Update Helper"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  system     - Update Rust toolchain (REQUIRED FIRST)"
    echo "  bun-safe   - Update safe frontend packages (no breaking changes)"
    echo "  rust       - Update Rust backend deps (breaking - manual)"
    echo "  frontend   - Update frontend deps (breaking - manual)"
    echo "  test       - Run full test suite"
    echo "  status     - Show current dependency status"
    echo "  all        - Run all safe updates (system + bun-safe + test)"
    echo "  help       - Show this help"
    echo ""
    echo "Recommended order:"
    echo "  1. ./scripts/update-deps.sh system"
    echo "  2. ./scripts/update-deps.sh bun-safe"
    echo "  3. ./scripts/update-deps.sh test"
    echo "  4. Then schedule manual updates later"
}

# Main
case "${1:-help}" in
    system)
        phase_system
        ;;
    bun-safe)
        phase_bun_safe
        ;;
    rust)
        phase_rust_backend
        ;;
    frontend)
        phase_frontend
        ;;
    test)
        run_tests
        ;;
    status)
        show_status
        ;;
    all)
        phase_system
        phase_bun_safe
        run_tests
        print_success "All safe updates complete!"
        ;;
    help|*)
        show_help
        ;;
esac
