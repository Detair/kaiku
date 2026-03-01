# Bun Migration Design

**Date:** 2026-01-16
**Status:** Approved
**Scope:** Replace npm with Bun for package management and script running

## Overview

Migrate the client package management from npm to Bun for:
- Faster package installs (~10x improvement)
- Faster script execution
- Simplified tooling (one tool for package management + script running)

## What Changes

### Package Management
- **Lock file:** `package-lock.json` → `bun.lockb`
- **Install:** `npm install` → `bun install`
- **Scripts:** `npm run <script>` → `bun run <script>`
- **npx:** `npx <cmd>` → `bunx <cmd>`

### What Stays the Same
- `package.json` (unchanged, compatible with both)
- Vite (dev server, bundler)
- Playwright (E2E tests - uses Node internally)
- Tauri CLI (Rust toolchain)

## Files to Modify

### Scripts

| File | Changes |
|------|---------|
| `setup-dev.sh` | Add Bun install, Fedora support, npm → bun |
| `scripts/dev-setup.sh` | Check for bun, npm install → bun install |
| `scripts/update-deps.sh` | npm commands → bun |
| `scripts/resume-session.sh` | npm status check → bun |
| `Makefile` | Replace PKG_MANAGER logic, npm → bun |

### CI/CD Workflows

| File | Changes |
|------|---------|
| `.github/workflows/ci.yml` | setup-node → setup-bun, npm → bun |
| `.github/workflows/tauri-build.yml` | Same pattern |
| `.github/workflows/release.yml` | Same pattern |
| `.github/workflows/security.yml` | npm audit → bun audit (or keep npm) |

### Documentation

| File | Changes |
|------|---------|
| `DEVELOPMENT.md` | All npm references → bun |
| `README.md` | Update quick start commands |
| `CLAUDE.md` | Update npm packages section |
| Other docs | Grep and update npm refs |

## CI Configuration

### Before (npm)
```yaml
- uses: actions/setup-node@v4
  with:
    node-version: "20"
    cache: "npm"
    cache-dependency-path: client/package-lock.json
- run: npm ci
- run: npm run build
```

### After (Bun)
```yaml
- uses: oven-sh/setup-bun@v2
  with:
    bun-version: latest
- run: bun install --frozen-lockfile
- run: bun run build
```

## Fedora Support

Add to setup scripts alongside existing Debian/Ubuntu support:

```bash
if command -v apt-get &> /dev/null; then
    # Debian/Ubuntu
    sudo apt-get update
    sudo apt-get install -y build-essential pkg-config libssl-dev \
        libwebkit2gtk-4.1-dev libsoup-3.0-dev gtk3-devel ...
elif command -v dnf &> /dev/null; then
    # Fedora/RHEL
    sudo dnf install -y gcc gcc-c++ make pkg-config openssl-devel \
        webkit2gtk4.1-devel libsoup3-devel gtk3-devel ...
fi
```

## Migration Steps

1. **Install Bun locally**
   ```bash
   curl -fsSL https://bun.sh/install | bash
   ```

2. **Migrate lock file**
   ```bash
   cd client
   rm package-lock.json
   bun install
   ```

3. **Verify local dev works**
   ```bash
   bun run dev
   bun run build
   bunx playwright test
   ```

4. **Update scripts** (setup-dev.sh, scripts/*, Makefile)

5. **Update CI workflows**

6. **Update documentation**

7. **Commit and push** (single PR)

## Validation Criteria

| Check | Command | Expected |
|-------|---------|----------|
| Install | `bun install` | Fast install, creates bun.lockb |
| Dev server | `bun run dev` | Vite starts on :5173 |
| Build | `bun run build` | dist/ created, no errors |
| Tauri dev | `bun run tauri dev` | App window opens |
| Tauri build | `bun run tauri build` | Produces .deb/.AppImage |
| Playwright | `bunx playwright test` | Tests pass |
| Lint | `bun run lint` | ESLint runs |

## Rollback Plan

If something breaks:
1. `package.json` unchanged - works with both npm and bun
2. If CI fails: revert workflow changes, run `npm install` to regenerate `package-lock.json`
3. Contributors can temporarily use `npm install` (creates package-lock.json)

## Notes

- **Playwright:** Works with Bun as package manager. Playwright itself runs on Node.js internally - this is transparent and requires no changes.
- **Node.js:** Still needed on dev machines for Playwright. Bun and Node.js coexist fine.
- **Contributors:** Will need Bun installed (`curl -fsSL https://bun.sh/install | bash`)
