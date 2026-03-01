# Bun Migration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace npm with Bun for package management and script running, add Fedora support to setup scripts.

**Architecture:** Drop-in replacement - Bun reads the same package.json, uses bun.lockb instead of package-lock.json. Vite, Playwright, and Tauri continue working unchanged.

**Tech Stack:** Bun, Vite, Solid.js, Tauri, GitHub Actions

---

## Task 1: Install Bun and Migrate Lock File

**Files:**
- Delete: `client/package-lock.json`
- Create: `client/bun.lockb` (generated)

**Step 1: Install Bun**

Run:
```bash
curl -fsSL https://bun.sh/install | bash
source ~/.bashrc
```

**Step 2: Verify Bun installation**

Run: `bun --version`
Expected: Version number (e.g., `1.x.x`)

**Step 3: Delete npm lock file**

Run:
```bash
cd /home/detair/GIT/canis/.worktrees/bun-migration/client
rm package-lock.json
rm -rf node_modules
```

**Step 4: Install dependencies with Bun**

Run: `bun install`
Expected: Fast install, creates `bun.lockb`

**Step 5: Verify build works**

Run: `bun run build`
Expected: Build succeeds, `dist/` created

**Step 6: Verify dev server works**

Run: `bun run dev` (Ctrl+C after it starts)
Expected: Vite dev server starts on port 5173

**Step 7: Commit**

```bash
git add -A
git commit -m "chore: migrate from npm to bun

- Remove package-lock.json
- Add bun.lockb
- Verified build and dev server work"
```

---

## Task 2: Update setup-dev.sh

**Files:**
- Modify: `setup-dev.sh`

**Step 1: Read current file**

Read `setup-dev.sh` to understand current structure.

**Step 2: Update to add Fedora support and Bun**

Replace the Node.js/npm section (around lines 107-122) with:

```bash
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
```

**Step 3: Update system dependencies section to add Fedora**

Replace the system dependencies section (around lines 46-63) with:

```bash
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
```

**Step 4: Update client install section (around lines 127-141)**

Replace `npm install` with `bun install`:

```bash
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
```

**Step 5: Update final instructions (around lines 182-190)**

Replace npm references:

```bash
echo "To start the frontend:"
echo "  cd client && bun run dev"
echo ""
echo "To run E2E tests:"
echo "  cd client && bunx playwright test"
```

**Step 6: Test the changes**

Run: `bash -n setup-dev.sh`
Expected: No syntax errors

**Step 7: Commit**

```bash
git add setup-dev.sh
git commit -m "chore(setup): add Bun and Fedora support to setup-dev.sh"
```

---

## Task 3: Update scripts/dev-setup.sh

**Files:**
- Modify: `scripts/dev-setup.sh`

**Step 1: Read current file to understand structure**

**Step 2: Replace PKG_MANAGER detection (around lines 154-164) with Bun check**

```bash
# Check Bun
if check_command bun; then
    log_success "Bun $(bun --version)"
else
    MISSING_TOOLS+=("bun (curl -fsSL https://bun.sh/install | bash)")
fi

# Check Node.js (still needed for Playwright)
if check_command node; then
    NODE_VERSION=$(node --version | grep -oP '\d+' | head -1)
    if [[ "$NODE_VERSION" -ge 18 ]]; then
        log_success "Node.js $(node --version) (for Playwright)"
    else
        log_warn "Node.js $(node --version) found, but v18+ recommended for Playwright"
    fi
else
    log_warn "Node.js not found (optional, needed for Playwright tests)"
fi
```

**Step 3: Update client install section (around lines 415-428)**

```bash
if ! $NO_CLIENT; then
    log_info "Installing client dependencies..."

    cd "${PROJECT_ROOT}/client"
    bun install
    log_success "Client dependencies installed"
    echo ""
fi
```

**Step 4: Update --help text (around line 54-55)**

Change `--no-client  Skip client npm install` to:
```bash
            echo "  --no-client  Skip client bun install"
```

**Step 5: Update final instructions (around lines 450-456)**

```bash
echo "  # Terminal 2: Start the client (dev mode)"
echo "  cd client && bun run tauri dev"
```

**Step 6: Add Fedora support to system dependencies check**

After the apt-get block (around line 56), add dnf detection similar to setup-dev.sh.

**Step 7: Test syntax**

Run: `bash -n scripts/dev-setup.sh`
Expected: No syntax errors

**Step 8: Commit**

```bash
git add scripts/dev-setup.sh
git commit -m "chore(scripts): update dev-setup.sh for Bun"
```

---

## Task 4: Update scripts/update-deps.sh

**Files:**
- Modify: `scripts/update-deps.sh`

**Step 1: Update phase_npm_safe function (around lines 71-85)**

```bash
phase_npm_safe() {
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
```

**Step 2: Update run_tests function (around lines 125-146)**

```bash
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
```

**Step 3: Update show_status function (around lines 148-164)**

```bash
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
```

**Step 4: Commit**

```bash
git add scripts/update-deps.sh
git commit -m "chore(scripts): update update-deps.sh for Bun"
```

---

## Task 5: Update scripts/resume-session.sh

**Files:**
- Modify: `scripts/resume-session.sh`

**Step 1: Update npm status check (around lines 69-79)**

```bash
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
```

**Step 2: Update ready for development section (around lines 122-128)**

```bash
if [ "$NEEDS_RUST_UPDATE" = false ] && [ "$NEEDS_SYSTEM_DEPS" = false ]; then
    echo -e "${GREEN}✓ All critical issues resolved!${NC}"
    echo ""
    echo "Ready for development:"
    echo "  - Start server: cd server && cargo run --release"
    echo "  - Start frontend: cd client && bun run dev"
    echo "  - Run tests: cargo test --workspace"
fi
```

**Step 3: Add Fedora support to system dependencies check (around lines 41-54)**

Add dnf-based package check alongside apt-based check.

**Step 4: Commit**

```bash
git add scripts/resume-session.sh
git commit -m "chore(scripts): update resume-session.sh for Bun"
```

---

## Task 6: Update Makefile

**Files:**
- Modify: `Makefile`

**Step 1: Remove PKG_MANAGER detection, replace with bun (around line 15)**

Delete:
```makefile
PKG_MANAGER := $(shell command -v pnpm 2>/dev/null && echo pnpm || echo npm)
```

**Step 2: Replace all $(PKG_MANAGER) references with bun**

Around lines 64, 85, 88, 117, 121, 125, 197:
- `$(PKG_MANAGER) install` → `bun install`
- `$(PKG_MANAGER) run` → `bun run`

**Step 3: Update install target (around lines 60-64)**

```makefile
install: ## Install all dependencies (Rust + Node)
	@echo "$(CYAN)Installing Rust dependencies...$(RESET)"
	@cargo fetch
	@echo "$(CYAN)Installing Node dependencies...$(RESET)"
	@cd client && bun install
```

**Step 4: Update client targets (around lines 84-88)**

```makefile
client: ## Start client in dev mode
	cd client && bun run tauri dev

client-web: ## Start client web UI only (no Tauri)
	cd client && bun run dev
```

**Step 5: Update lint/fmt targets (around lines 116-125)**

```makefile
lint: check ## Alias for check
	@cd client && bun run lint

fmt: ## Format all code
	cargo fmt --all
	cd client && bun run format

fmt-check: ## Check code formatting
	cargo fmt --all -- --check
	cd client && bun run format -- --check
```

**Step 6: Update build-client target (around line 197)**

```makefile
build-client: ## Build client app
	cd client && bun run tauri build
```

**Step 7: Verify Makefile syntax**

Run: `make help`
Expected: Help output displays correctly

**Step 8: Commit**

```bash
git add Makefile
git commit -m "chore(make): update Makefile for Bun"
```

---

## Task 7: Update CI Workflow - ci.yml

**Files:**
- Modify: `.github/workflows/ci.yml`

**Step 1: Update frontend job (around lines 127-150)**

Replace:
```yaml
  frontend:
    name: Frontend
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: client
    steps:
      - uses: actions/checkout@v4

      - name: Setup Bun
        uses: oven-sh/setup-bun@v2
        with:
          bun-version: latest

      - name: Install dependencies
        run: bun install --frozen-lockfile

      - name: Type check
        run: bun run lint

      - name: Build
        run: bun run build
```

**Step 2: Update tauri job (around lines 178-216)**

Replace Node.js setup with Bun:
```yaml
      - name: Setup Bun
        uses: oven-sh/setup-bun@v2
        with:
          bun-version: latest

      - name: Install frontend dependencies
        working-directory: client
        run: bun install --frozen-lockfile

      - name: Build Tauri app
        working-directory: client
        run: bun run tauri build
```

**Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: update ci.yml for Bun"
```

---

## Task 8: Update CI Workflow - tauri-build.yml

**Files:**
- Modify: `.github/workflows/tauri-build.yml`

**Step 1: Replace setup-node with setup-bun (around lines 45-50)**

Replace:
```yaml
      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: "20"
          cache: "npm"
          cache-dependency-path: client/package-lock.json
```

With:
```yaml
      - name: Setup Bun
        uses: oven-sh/setup-bun@v2
        with:
          bun-version: latest
```

**Step 2: Update install and build commands (around lines 77-83)**

```yaml
      - name: Install frontend dependencies
        working-directory: client
        run: bun install --frozen-lockfile

      - name: Build Tauri app
        working-directory: client
        run: bun run tauri build -- --target ${{ matrix.target }}
```

**Step 3: Commit**

```bash
git add .github/workflows/tauri-build.yml
git commit -m "ci: update tauri-build.yml for Bun"
```

---

## Task 9: Update CI Workflow - release.yml

**Files:**
- Modify: `.github/workflows/release.yml`

**Step 1: Update build-tauri job (around lines 121-136)**

Replace Node.js setup:
```yaml
      - name: Setup Bun
        uses: oven-sh/setup-bun@v2
        with:
          bun-version: latest

      - name: Install frontend dependencies
        working-directory: client
        run: bun install --frozen-lockfile

      - name: Build Tauri app
        working-directory: client
        env:
          BUILD_TARGET: ${{ matrix.target }}
        run: bun run tauri build -- --target "$BUILD_TARGET"
```

**Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: update release.yml for Bun"
```

---

## Task 10: Update CI Workflow - security.yml

**Files:**
- Modify: `.github/workflows/security.yml`

**Step 1: Update npm-audit job (around lines 68-84)**

Replace with bun audit:
```yaml
  bun-audit:
    name: Bun Security Audit
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: client
    steps:
      - uses: actions/checkout@v4

      - name: Setup Bun
        uses: oven-sh/setup-bun@v2
        with:
          bun-version: latest

      - name: Run bun audit
        run: bun pm audit
```

**Step 2: Update trigger path (around line 14)**

Change `client/package-lock.json` to `client/bun.lockb`:
```yaml
    paths:
      - 'Cargo.lock'
      - 'client/bun.lockb'
```

**Step 3: Commit**

```bash
git add .github/workflows/security.yml
git commit -m "ci: update security.yml for Bun"
```

---

## Task 11: Update Documentation - DEVELOPMENT.md

**Files:**
- Modify: `DEVELOPMENT.md`

**Step 1: Update Prerequisites section (around line 9)**

```markdown
## Prerequisites

- Docker and Docker Compose
- Rust (latest stable)
- Bun (install: `curl -fsSL https://bun.sh/install | bash`)
- Node.js 18+ (required for Playwright tests)
- sqlx-cli: `cargo install sqlx-cli --no-default-features --features postgres`
```

**Step 2: Update Start the Client section (around lines 58-65)**

```markdown
### 5. Start the Client

In a new terminal:

```bash
cd client
bun install
bun run dev
```
```

**Step 3: Update all other npm references**

Search and replace throughout the file:
- `npm install` → `bun install`
- `npm run` → `bun run`
- `npx` → `bunx`

**Step 4: Commit**

```bash
git add DEVELOPMENT.md
git commit -m "docs: update DEVELOPMENT.md for Bun"
```

---

## Task 12: Update Other Documentation

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `CONFIG.md`
- Modify: `INSTALL_DEPENDENCIES.md`
- Modify: `START_HERE.md`

**Step 1: Find and update all npm references**

Run to find files:
```bash
grep -r "npm" --include="*.md" -l . | grep -v node_modules | grep -v docs/plans
```

**Step 2: Update each file**

Replace:
- `npm install` → `bun install`
- `npm run` → `bun run`
- `npx` → `bunx`
- References to Node.js for package management → Bun

**Step 3: Update CLAUDE.md npm packages section**

Around the "Wichtige npm Packages" section, add note about Bun:
```markdown
## Package Manager
- Bun (for package management and script running)
- Node.js (still required for Playwright)
```

**Step 4: Commit**

```bash
git add README.md CLAUDE.md CONFIG.md INSTALL_DEPENDENCIES.md START_HERE.md
git commit -m "docs: update documentation for Bun migration"
```

---

## Task 13: Final Verification

**Step 1: Clean install test**

```bash
cd /home/detair/GIT/canis/.worktrees/bun-migration/client
rm -rf node_modules bun.lockb
bun install
```

**Step 2: Full build test**

```bash
bun run build
```
Expected: Build succeeds

**Step 3: Dev server test**

```bash
bun run dev
```
Expected: Vite starts on :5173 (Ctrl+C to stop)

**Step 4: Lint test**

```bash
bun run lint
```
Expected: ESLint runs

**Step 5: Verify all changes committed**

```bash
git status
```
Expected: Clean working tree

**Step 6: Create final summary commit if needed**

If any uncommitted changes remain, commit them.

---

## Task 14: Push and Create PR

**Step 1: Push branch**

```bash
git push -u origin feature/bun-migration
```

**Step 2: Create PR**

```bash
gh pr create --title "chore: migrate from npm to Bun" --body "$(cat <<'EOF'
## Summary
- Replace npm with Bun for package management and script running
- Add Fedora/RHEL support to setup scripts
- Update all CI workflows to use oven-sh/setup-bun
- Update documentation

## Changes
- Lock file: package-lock.json → bun.lockb
- Scripts: setup-dev.sh, scripts/*, Makefile updated
- CI: All workflows updated to use Bun
- Docs: All npm references updated

## Test plan
- [ ] `bun install` works
- [ ] `bun run build` succeeds
- [ ] `bun run dev` starts Vite
- [ ] CI passes on all platforms

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

**Step 3: Return PR URL**
