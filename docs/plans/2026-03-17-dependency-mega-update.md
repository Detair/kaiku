# Dependency Mega-Update Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Update all Rust crates and NPM packages to their latest compatible versions, documenting new features unlocked by each upgrade.

**Architecture:** Sequential phase-based upgrades respecting cross-dependency constraints. Each phase is independently testable.

**Tech Stack:** Rust (workspace Cargo.toml), TypeScript (client/package.json), Vite, UnoCSS, ESLint, Tauri

---

## Phase 1: Semver-Compatible Patches

### Task 1: Cargo lockfile update

**Files:**
- Modify: `Cargo.lock`

**Steps:**
1. Run `cargo update` to pull all semver-compatible patches (192 updates)
2. Run `SQLX_OFFLINE=true cargo clippy -- -D warnings` to verify build
3. Run `cargo test -p vc-server` to verify server tests
4. Commit: `chore(infra): update Cargo.lock with semver-compatible patches`

### Task 2: NPM patch updates

**Files:**
- Modify: `client/package.json`, `client/bun.lock`

**Steps:**
1. `cd client && bun update` for semver-compatible patches
2. `bun run test:run` to verify client tests
3. Commit: `chore(client): update npm packages with semver-compatible patches`

---

## Phase 2: Rust Independent Upgrades

### Task 3: Bump MSRV to 1.93

**Files:**
- Modify: `Cargo.toml` (workspace) — change `rust-version = "1.82"` to `"1.93"`

**Steps:**
1. Update rust-version field
2. Build to confirm: `SQLX_OFFLINE=true cargo check`
3. Commit: `chore(infra): bump MSRV to 1.93 (matches installed rustc)`

### Task 4: axum-extra 0.10 → 0.12

**Files:**
- Modify: `Cargo.toml` (workspace) — axum-extra version

**Steps:**
1. Change `axum-extra = { version = "0.10" ...}` to `"0.12"`
2. `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
3. `cargo test -p vc-server`
4. Commit: `chore(api): upgrade axum-extra 0.10 → 0.12`

### Task 5: rusqlite 0.32 → 0.39

**Files:**
- Modify: `client/src-tauri/Cargo.toml` — rusqlite version

**Steps:**
1. Change `rusqlite = { version = "0.32" ...}` to `"0.39"`
2. `SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings`
3. Commit: `chore(client): upgrade rusqlite 0.32 → 0.39`

### Task 6: rodio 0.21 → 0.22

**Files:**
- Modify: `client/src-tauri/Cargo.toml` — rodio version
- Modify: `client/src-tauri/src/commands/sound.rs` — Sink → Player rename

**Steps:**
1. Change `rodio = "0.21"` to `"0.22"`
2. Fix API: `Sink` → check if renamed. In rodio 0.22: `Sink` is still available but `Player` is the new name
3. `SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings` — fix any breaking API changes
4. Commit: `chore(client): upgrade rodio 0.21 → 0.22`

### Task 7: sysinfo 0.34 → 0.38

**Files:**
- Modify: `client/src-tauri/Cargo.toml` — sysinfo version
- Possibly modify: process scanning code

**Steps:**
1. Change `sysinfo = "0.34"` to `"0.38"`
2. `SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings` — fix any API changes
3. Commit: `chore(client): upgrade sysinfo 0.34 → 0.38`

### Task 8: jsonwebtoken 9 → 10

**Files:**
- Modify: `Cargo.toml` (workspace) — jsonwebtoken version + features
- Possibly modify: `server/src/auth/jwt.rs` — API changes
- Possibly modify: `server/src/auth/error.rs` — error type changes

**Steps:**
1. Change `jsonwebtoken = "9"` to `jsonwebtoken = { version = "10", features = ["rust_crypto"] }`
2. Check if EdDSA support requires additional feature flag
3. `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings` — fix API changes
4. `cargo test -p vc-server` — verify JWT tests pass
5. Commit: `chore(auth): upgrade jsonwebtoken 9 → 10 with rust_crypto backend`

### Task 9: sentry 0.36 → 0.47 (Rust)

**Files:**
- Modify: `client/src-tauri/Cargo.toml` — sentry + sentry-tracing versions

**Steps:**
1. Change `sentry = { version = "0.36" ...}` to `"0.47"` and `sentry-tracing = "0.36"` to `"0.47"`
2. `SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings` — fix breaking changes
3. Commit: `chore(client): upgrade sentry 0.36 → 0.47`

---

## Phase 3: NPM Patch + Minor Upgrades

### Task 10: Safe NPM minor updates

**Files:**
- Modify: `client/package.json`

**Steps:**
1. Update in package.json:
   - `mermaid` → `^11.13.0` (fixes lodash CVE)
   - `@floating-ui/dom` → `^1.7.6`
   - `@tanstack/solid-virtual` → `^3.13.23`
   - `marked` → `^17.0.4`
   - `dompurify` → `^3.3.3`
   - `@playwright/test` → `^1.58.2`
   - `@rollup/plugin-commonjs` → `^29.0.2`
   - `prettier` → `^3.8.1`
2. `bun install && bun run test:run`
3. Commit: `chore(client): update npm minor/patch dependencies`

---

## Phase 4: Tauri 2.10 (JS + Rust coordinated)

### Task 11: Tauri ecosystem upgrade

**Files:**
- Modify: `client/package.json` — @tauri-apps/* packages
- Modify: `Cargo.lock` (via cargo update)

**Steps:**
1. Update in package.json:
   - `@tauri-apps/api` → `^2.10.1`
   - `@tauri-apps/cli` → `^2.10.1`
   - `@tauri-apps/plugin-shell` → `^2.3.5`
   - `@tauri-apps/plugin-notification` → `^2.3.3` (already latest)
   - `@tauri-apps/plugin-global-shortcut` → `^2.3.1` (already latest)
2. `bun install`
3. `cargo update -p tauri -p tauri-build -p tauri-plugin-shell -p tauri-plugin-notification -p tauri-plugin-global-shortcut`
4. `SQLX_OFFLINE=true cargo clippy -p vc-client -- -D warnings`
5. `bun run test:run`
6. Commit: `chore(client): upgrade Tauri ecosystem to 2.10`

---

## Phase 5: Vite 8 Ecosystem

### Task 12: Vite 8 + plugins

**Files:**
- Modify: `client/package.json`
- Modify: `client/vite.config.ts` — migrate esbuild options

**Steps:**
1. Update package.json:
   - `vite` → `^8.0.0`
   - `vite-plugin-solid` → `^2.11.11`
   - `@vitejs/plugin-basic-ssl` → `^2.2.0`
   - `vitest` → `^4.1.0`
2. `bun install`
3. Migrate vite.config.ts:
   - `esbuild` config key → Vite 8 uses Rolldown. The `esbuild.drop` and `esbuild.pure` options may need to move to `rolldownOptions` or equivalent. Check Vite 8 migration guide.
   - `build.commonjsOptions` → check if still valid under Rolldown
4. `bun run test:run`
5. `bun run build` — verify production build works
6. Commit: `chore(client): upgrade Vite 5 → 8 with Rolldown`

---

## Phase 6: UnoCSS 66

### Task 13: UnoCSS ecosystem upgrade

**Files:**
- Modify: `client/package.json`
- Possibly modify: `client/uno.config.ts`

**Steps:**
1. Update package.json:
   - `unocss` → `^66.6.6`
   - `@unocss/preset-icons` → `^66.6.6`
   - `@unocss/preset-uno` → `^66.6.6`
   - `@unocss/reset` → `^66.6.6`
2. `bun install`
3. Check uno.config.ts: the engine is now fully async. Update imports if needed:
   - `import { defineConfig, presetUno, presetIcons } from "unocss"` should still work
4. `bun run build` — verify CSS generation works
5. `bun run test:run`
6. Commit: `chore(client): upgrade UnoCSS 0.58 → 66`

---

## Phase 7: openidconnect 4 + reqwest 0.13 (server)

### Task 14: openidconnect + reqwest upgrade

**Files:**
- Modify: `Cargo.toml` (workspace) — openidconnect version
- Modify: `server/Cargo.toml` — reqwest version (0.11 → 0.13)
- Modify: `server/src/auth/oidc.rs` — async_http_client → reqwest client

**Steps:**
1. Change workspace: `openidconnect = "3"` → `"4"`
2. Change server: `reqwest = { version = "0.11" ...}` → `"0.13"` (both deps and dev-deps)
3. Major API change: `async_http_client` is removed in openidconnect 4.
   - Replace `CoreProviderMetadata::discover_async(issuer, async_http_client)` with new client-based API
   - Replace `.request_async(async_http_client)` calls similarly
4. `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings`
5. `cargo test -p vc-server`
6. Commit: `chore(auth): upgrade openidconnect 3 → 4 + reqwest 0.11 → 0.13`

---

## Phase 8: OpenTelemetry Stack

### Task 15: OpenTelemetry coordinated upgrade

**Files:**
- Modify: `Cargo.toml` (workspace) — all 8 otel crates

**Steps:**
1. Update all at once in workspace Cargo.toml:
   - `opentelemetry = "0.29"` → `"0.31"`
   - `opentelemetry_sdk = "0.29"` → `"0.31"`
   - `opentelemetry-otlp = "0.29"` → `"0.31"`
   - `opentelemetry-appender-tracing = "0.29"` → `"0.31"`
   - `tracing-opentelemetry = "0.30"` → `"0.32"`
   - `axum-tracing-opentelemetry = "0.28"` → `"0.32"`
   - `init-tracing-opentelemetry = "0.28"` → `"0.36"`
   - `tracing-opentelemetry-instrumentation-sdk = "0.28"` → `"0.32"`
2. `cargo update`
3. `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings` — fix API changes
4. `cargo test -p vc-server`
5. Commit: `chore(infra): upgrade OpenTelemetry stack 0.29 → 0.31`

---

## Phase 9: ESLint 9 + Flat Config

### Task 16: ESLint 9 migration

**Files:**
- Modify: `client/package.json`
- Create: `client/eslint.config.mjs` (flat config)
- Modify: `client/package.json` scripts — update lint command

**Steps:**
1. Update package.json:
   - `eslint` → `^9.0.0`
   - `@typescript-eslint/eslint-plugin` → `^8.57.1`
   - `@typescript-eslint/parser` → `^8.57.1`
   - `eslint-plugin-solid` → `^0.14.5`
2. Create `eslint.config.mjs` (flat config format)
3. Update lint script: `eslint src --ext .ts,.tsx` → `eslint src`
4. `bun install && bun run lint`
5. Commit: `chore(client): upgrade ESLint 8 → 9 with flat config migration`

---

## Phase 10: @solidjs/router 0.15

### Task 17: SolidJS router upgrade

**Files:**
- Modify: `client/package.json`
- Possibly modify: router usage in source files

**Steps:**
1. Change `@solidjs/router` → `^0.15.4`
2. `bun install`
3. Check for breaking changes (no `cache()` usage found — main concern eliminated)
4. Verify Router, Route, A, useNavigate, useParams, useSearchParams, useLocation still work
5. `bun run test:run && bun run build`
6. Commit: `chore(client): upgrade @solidjs/router 0.10 → 0.15`

---

## Phase 11: @sentry/browser 10

### Task 18: Sentry JS upgrade (8 → 10)

**Files:**
- Modify: `client/package.json`
- Possibly modify: `client/src/lib/sentry.ts`

**Steps:**
1. Change `@sentry/browser` → `^10.44.0`
2. `bun install`
3. Review sentry.ts — current code uses simple `Sentry.init()` with no deprecated APIs
4. `bun run test:run && bun run build`
5. Commit: `chore(client): upgrade @sentry/browser 8 → 10`

---

## Phase 12: lucide-solid update

### Task 19: lucide-solid icon update

**Files:**
- Modify: `client/package.json`

**Steps:**
1. Change `lucide-solid` → `^0.577.0`
2. `bun install && bun run build` (verify no removed icon names)
3. Commit: `chore(client): upgrade lucide-solid 0.300 → 0.577`

---

## Phase 13: Feature Update List + Final Verification

### Task 20: Create feature update document + full test run

**Steps:**
1. Create `docs/plans/2026-03-17-dependency-update-features.md` documenting new capabilities
2. Full build: `SQLX_OFFLINE=true cargo clippy -- -D warnings`
3. Full test: `cargo test -p vc-server && cd client && bun run test:run`
4. Commit: `docs: add dependency update feature list`
