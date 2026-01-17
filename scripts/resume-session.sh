#!/bin/bash
# Quick Session Resume Script
# Run this when you return to quickly check project status

set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Detect project root dynamically
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo -e "${BLUE}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║              VoiceChat Session Resume                     ║${NC}"
echo -e "${BLUE}╔════════════════════════════════════════════════════════════╗${NC}"
echo ""

cd "$PROJECT_ROOT"

# Check Git Status
echo -e "${BLUE}📋 Git Status${NC}"
echo "Current branch: $(git branch --show-current)"
echo "Last commit: $(git log -1 --oneline)"
git status --short
echo ""

# Check Rust Version
echo -e "${BLUE}🦀 Rust Version${NC}"
RUST_VERSION=$(rustc --version | grep -oP '\d+\.\d+' | head -1)
if (( $(echo "$RUST_VERSION < 1.82" | bc -l) )); then
    echo -e "${RED}✗ Rust $RUST_VERSION is outdated (need 1.82+)${NC}"
    echo -e "${YELLOW}  Run: rustup update stable && rustup default stable${NC}"
else
    echo -e "${GREEN}✓ Rust $RUST_VERSION is up to date${NC}"
fi
echo ""

# Check System Dependencies
echo -e "${BLUE}📦 System Dependencies${NC}"
if pkg-config --modversion glib-2.0 >/dev/null 2>&1; then
    echo -e "${GREEN}✓ GLib installed ($(pkg-config --modversion glib-2.0))${NC}"
else
    echo -e "${RED}✗ GLib not found${NC}"
    echo -e "${YELLOW}  Run: sudo apt-get install -y libglib2.0-dev${NC}"
fi

if pkg-config --modversion webkit2gtk-4.0 >/dev/null 2>&1; then
    echo -e "${GREEN}✓ WebKit2GTK installed${NC}"
else
    echo -e "${RED}✗ WebKit2GTK not found${NC}"
    echo -e "${YELLOW}  Run: sudo apt-get install -y libwebkit2gtk-4.0-dev${NC}"
fi
echo ""

# Check if Server Builds
echo -e "${BLUE}🔨 Server Build Status${NC}"
cd "$PROJECT_ROOT/server"
if cargo check --quiet 2>&1 | grep -q "error"; then
    echo -e "${RED}✗ Server has build errors${NC}"
    echo -e "${YELLOW}  Run: cargo build --release${NC}"
else
    echo -e "${GREEN}✓ Server compiles successfully${NC}"
fi
cd "$PROJECT_ROOT"
echo ""

# Check Bun Status
echo -e "${BLUE}📦 Bun Status${NC}"
cd "$PROJECT_ROOT/client"
if command -v bun &> /dev/null; then
    echo -e "${GREEN}✓ Bun installed ($(bun --version))${NC}"
    if [ -d "node_modules" ]; then
        echo -e "${GREEN}✓ node_modules exists${NC}"
    else
        echo -e "${YELLOW}⚠ node_modules missing${NC}"
        echo -e "${YELLOW}  Run: bun install${NC}"
    fi
else
    echo -e "${YELLOW}⚠ Bun not installed${NC}"
    echo -e "${YELLOW}  Run: curl -fsSL https://bun.sh/install | bash${NC}"
fi
cd "$PROJECT_ROOT"
echo ""

# Show Session State
echo -e "${BLUE}📄 Last Session Summary${NC}"
if [ -f "$PROJECT_ROOT/SESSION_STATE.md" ]; then
    echo -e "${GREEN}✓ SESSION_STATE.md found${NC}"
    echo ""
    echo "Read full state: cat SESSION_STATE.md"
    echo ""
    echo "Quick summary:"
    grep -A 5 "## ✅ What Was Accomplished" "$PROJECT_ROOT/SESSION_STATE.md" | tail -5
else
    echo -e "${RED}✗ SESSION_STATE.md not found${NC}"
fi
echo ""

# Next Actions
echo -e "${BLUE}🎯 Recommended Next Actions${NC}"
echo ""

NEEDS_RUST_UPDATE=false
NEEDS_SYSTEM_DEPS=false

if (( $(echo "$RUST_VERSION < 1.82" | bc -l) )); then
    NEEDS_RUST_UPDATE=true
fi

if ! pkg-config --modversion glib-2.0 >/dev/null 2>&1; then
    NEEDS_SYSTEM_DEPS=true
fi

if [ "$NEEDS_RUST_UPDATE" = true ]; then
    echo -e "${RED}1. Update Rust (CRITICAL)${NC}"
    echo "   rustup update stable && rustup default stable"
    echo ""
fi

if [ "$NEEDS_SYSTEM_DEPS" = true ]; then
    echo -e "${YELLOW}2. Install System Dependencies${NC}"
    echo "   sudo apt-get install -y libwebkit2gtk-4.0-dev build-essential libssl-dev libgtk-3-dev pkg-config"
    echo ""
fi

if [ "$NEEDS_RUST_UPDATE" = false ] && [ "$NEEDS_SYSTEM_DEPS" = false ]; then
    echo -e "${GREEN}✓ All critical issues resolved!${NC}"
    echo ""
    echo "Ready for development:"
    echo "  - Start server: cd server && cargo run --release"
    echo "  - Start frontend: cd client && bun run dev"
    echo "  - Run tests: cargo test --workspace"
fi

echo ""
echo -e "${BLUE}📚 Helpful Commands${NC}"
echo "  ./scripts/update-deps.sh status     - Check dependency status"
echo "  cat UPDATE_NOW.md                   - Critical fixes guide"
echo "  cat DEPENDENCY_REVIEW.md            - Full dependency analysis"
echo "  cat SESSION_STATE.md                - Detailed session state"
echo ""

echo -e "${BLUE}╚════════════════════════════════════════════════════════════╝${NC}"
