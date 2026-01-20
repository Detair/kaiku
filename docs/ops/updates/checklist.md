# ✅ Dependency Update Checklist

Use this checklist to track your progress through the dependency updates.

---

## 🚨 Phase 0: Critical System Update (DO FIRST)

**Time**: 10 minutes | **Risk**: Very Low | **Priority**: 🔴 URGENT

- [ ] Backup current work
  ```bash
  git add . && git commit -m "Pre-update backup"
  git checkout -b backup-before-rust-update
  ```

- [ ] Update Rust toolchain
  ```bash
  rustup self update
  rustup update stable
  rustup default stable
  ```

- [ ] Verify Rust version
  ```bash
  rustc --version  # Should show 1.84.0 or higher
  cargo --version  # Should show 1.84.0 or higher
  ```

- [ ] Clean and rebuild project
  ```bash
  cd /home/detair/GIT/canis
  cargo clean
  cargo build --release
  ```

- [ ] Run tests (should now work!)
  ```bash
  cargo test --workspace
  ```

- [ ] Start server and verify it works
  ```bash
  cargo run --release
  # In browser: http://localhost:8080
  # Test voice chat
  ```

- [ ] Commit the lockfile changes
  ```bash
  git add Cargo.lock
  git commit -m "Update Rust toolchain to 1.84"
  git checkout main
  git merge backup-before-rust-update
  ```

---

## 📦 Phase 1: Safe npm Updates (No Breaking Changes)

**Time**: 15 minutes | **Risk**: Very Low | **Priority**: 🟡 High

- [ ] Backup current work
  ```bash
  git checkout -b update-npm-safe
  ```

- [ ] Update safe packages
  ```bash
  cd /home/detair/GIT/canis/client
  npm install @tauri-apps/plugin-shell@^2.3.4
  npm install lucide-solid@^0.562.0
  npm install eslint-plugin-solid@^0.14.5
  ```

- [ ] Test build
  ```bash
  npm run build
  npm run tauri build
  ```

- [ ] Test dev server
  ```bash
  npm run dev
  # Verify UI works in browser
  ```

- [ ] Commit changes
  ```bash
  git add package.json package-lock.json
  git commit -m "Update safe npm packages (non-breaking)"
  git checkout main
  git merge update-npm-safe
  ```

---

## 🦀 Phase 2: Rust Backend Updates (Breaking Changes)

**Time**: 4-6 hours | **Risk**: Medium | **Priority**: 🟡 High

### 2.1 Axum 0.7 → 0.8

- [ ] Create update branch
  ```bash
  git checkout -b update-axum-0.8
  ```

- [ ] Update Cargo.toml
  ```bash
  # In /home/detair/GIT/canis/Cargo.toml, change:
  axum = { version = "0.8", features = ["ws", "multipart"] }
  ```

- [ ] Rebuild and fix errors
  ```bash
  cd /home/detair/GIT/canis
  cargo clean
  cargo build --release 2>&1 | tee axum-update-errors.log
  ```

- [ ] Fix compilation errors (if any)
  - [ ] Check `server/src/main.rs`
  - [ ] Check `server/src/api/mod.rs`
  - [ ] Check `server/src/ws/mod.rs`

- [ ] Run tests
  ```bash
  cargo test --workspace
  ```

- [ ] Manual testing
  - [ ] Server starts without errors
  - [ ] HTTP endpoints work
  - [ ] WebSocket connection works
  - [ ] CORS works
  - [ ] Voice chat works

- [ ] Commit if successful
  ```bash
  git add .
  git commit -m "Update axum to 0.8"
  ```

### 2.2 SQLx 0.7 → 0.8

- [ ] Update Cargo.toml
  ```bash
  # In /home/detair/GIT/canis/Cargo.toml, change:
  sqlx = { version = "0.8", features = ["postgres", "runtime-tokio", "uuid", "chrono", "json"] }
  ```

- [ ] Reset database (optional but recommended)
  ```bash
  cd /home/detair/GIT/canis/server
  sqlx database reset
  sqlx migrate run
  ```

- [ ] Rebuild and fix errors
  ```bash
  cargo clean
  cargo build --release 2>&1 | tee sqlx-update-errors.log
  ```

- [ ] Run tests
  ```bash
  cargo test --workspace
  ```

- [ ] Manual testing
  - [ ] User login works
  - [ ] Message sending works
  - [ ] Channel creation works
  - [ ] Voice join queries work

- [ ] Commit if successful
  ```bash
  git add .
  git commit -m "Update sqlx to 0.8"
  ```

### 2.3 Rustls 0.23 → 0.24

- [ ] Update Cargo.toml
  ```bash
  # In /home/detair/GIT/canis/Cargo.toml, change:
  rustls = { version = "0.24", features = ["ring"] }
  ```

- [ ] Rebuild
  ```bash
  cargo clean
  cargo build --release
  ```

- [ ] Test TLS connections
  - [ ] HTTPS endpoints work
  - [ ] WebRTC DTLS works

- [ ] Commit if successful
  ```bash
  git add .
  git commit -m "Update rustls to 0.24"
  ```

### 2.4 Fred (Redis) 8 → 9

- [ ] Update Cargo.toml
  ```bash
  # In /home/detair/GIT/canis/Cargo.toml, change:
  fred = "9"
  ```

- [ ] Update Redis connection code
  - [ ] Fix `server/src/db/mod.rs` connection initialization

- [ ] Rebuild
  ```bash
  cargo clean
  cargo build --release
  ```

- [ ] Test Redis operations
  - [ ] Pub/sub works
  - [ ] Key operations work

- [ ] Commit if successful
  ```bash
  git add .
  git commit -m "Update fred (Redis) to 9"
  ```

### 2.5 Merge Backend Updates

- [ ] Run full test suite
  ```bash
  cargo test --workspace
  cargo run --release
  ```

- [ ] Merge to main
  ```bash
  git checkout main
  git merge update-axum-0.8
  ```

---

## 📦 Phase 3: Frontend Updates (Breaking Changes)

**Time**: 3-5 hours | **Risk**: Medium | **Priority**: 🟢 Medium

### 3.1 Vite 5 → 7

- [ ] Create update branch
  ```bash
  git checkout -b update-vite-7
  ```

- [ ] Update package.json
  ```bash
  cd /home/detair/GIT/canis/client
  npm install vite@^7.0.0 vite-plugin-solid@latest
  ```

- [ ] Test build
  ```bash
  npm run build
  ```

- [ ] Fix any errors (check vite.config.ts)

- [ ] Test dev server
  ```bash
  npm run dev
  ```

- [ ] Commit if successful
  ```bash
  git add package.json package-lock.json
  git commit -m "Update Vite to 7"
  ```

### 3.2 ESLint 8 → 9

- [ ] Update package.json
  ```bash
  npm install -D eslint@^9.0.0
  ```

- [ ] Convert to flat config
  - [ ] Rename `.eslintrc.cjs` to `eslint.config.js`
  - [ ] Convert config format (see BREAKING_CHANGES_GUIDE.md)

- [ ] Fix linting errors
  ```bash
  npm run lint
  ```

- [ ] Commit if successful
  ```bash
  git add .
  git commit -m "Update ESLint to 9 with flat config"
  ```

### 3.3 Solid Router 0.10 → 0.15

- [ ] Update package.json
  ```bash
  npm install @solidjs/router@^0.15.0
  ```

- [ ] Review route definitions in `src/App.tsx`

- [ ] Fix breaking changes (if any)

- [ ] Test navigation
  - [ ] All routes work
  - [ ] Nested routes work
  - [ ] Route params work

- [ ] Commit if successful
  ```bash
  git add package.json package-lock.json src/
  git commit -m "Update @solidjs/router to 0.15"
  ```

### 3.4 TypeScript ESLint 6 → 8

- [ ] Update package.json
  ```bash
  npm install -D @typescript-eslint/parser@^8.0.0 @typescript-eslint/eslint-plugin@^8.0.0
  ```

- [ ] Fix new linting errors

- [ ] Test build
  ```bash
  npm run build
  ```

- [ ] Commit if successful
  ```bash
  git add .
  git commit -m "Update TypeScript ESLint to 8"
  ```

### 3.5 Merge Frontend Updates

- [ ] Run full build
  ```bash
  npm run build
  npm run tauri build
  ```

- [ ] Test Tauri app manually

- [ ] Merge to main
  ```bash
  git checkout main
  git merge update-vite-7
  ```

---

## 🔬 Phase 4: Integration Testing

**Time**: 1-2 hours | **Risk**: Low | **Priority**: 🔴 CRITICAL

- [ ] Clean build of everything
  ```bash
  cd /home/detair/GIT/canis
  cargo clean
  cargo build --release

  cd client
  rm -rf node_modules package-lock.json
  npm install
  npm run build
  ```

- [ ] Server tests
  - [ ] Cargo tests pass: `cargo test --workspace`
  - [ ] Server starts: `cargo run --release`
  - [ ] Health check: `curl http://localhost:8080/health`

- [ ] Client tests
  - [ ] Dev server runs: `npm run dev`
  - [ ] Build succeeds: `npm run build`
  - [ ] Tauri builds: `npm run tauri build`

- [ ] Manual end-to-end testing
  - [ ] User registration
  - [ ] User login
  - [ ] Channel navigation
  - [ ] Send messages
  - [ ] Join voice channel (2+ users)
  - [ ] Mute/unmute voice
  - [ ] Leave voice channel
  - [ ] Upload file (if S3 configured)

- [ ] Performance check
  - [ ] Server startup time (<5s)
  - [ ] Voice latency (<100ms)
  - [ ] UI responsiveness (no lag)
  - [ ] Memory usage (<150MB browser, <80MB Tauri)

---

## 🚫 Phase 5: Do NOT Update (Yet)

These packages need further investigation:

- [ ] ❌ **UnoCSS** 0.58 → 66.5
  - Reason: Massive version jump, may break CSS
  - Action: Research breaking changes first

- [ ] ❌ **@types/node** 20.x → 25.x
  - Reason: May break Tauri compatibility
  - Action: Wait for Tauri team confirmation

---

## 📝 Final Steps

- [ ] Update CHANGELOG.md with all changes

- [ ] Tag release
  ```bash
  git tag v0.2.0-updated-deps
  git push origin v0.2.0-updated-deps
  ```

- [ ] Deploy to staging environment

- [ ] Monitor for 48 hours

- [ ] Deploy to production

- [ ] Set calendar reminder for next review (April 2026)

- [ ] Delete backup branches
  ```bash
  git branch -D backup-before-rust-update
  git branch -D update-npm-safe
  git branch -D update-axum-0.8
  git branch -D update-vite-7
  ```

---

## 📊 Progress Tracking

| Phase | Status | Date Completed | Notes |
|-------|--------|----------------|-------|
| Phase 0: Rust Update | ⬜ | | Critical - do first |
| Phase 1: Safe npm | ⬜ | | Low risk |
| Phase 2.1: Axum | ⬜ | | Breaking changes |
| Phase 2.2: SQLx | ⬜ | | Breaking changes |
| Phase 2.3: Rustls | ⬜ | | Minor changes |
| Phase 2.4: Fred | ⬜ | | Breaking changes |
| Phase 3.1: Vite | ⬜ | | Breaking changes |
| Phase 3.2: ESLint | ⬜ | | Breaking changes |
| Phase 3.3: Router | ⬜ | | Breaking changes |
| Phase 3.4: TS ESLint | ⬜ | | Breaking changes |
| Phase 4: Integration | ⬜ | | Final validation |

**Legend**: ⬜ Not Started | 🟡 In Progress | ✅ Completed | ❌ Failed

---

## 🆘 Emergency Rollback

If anything goes wrong:

```bash
# Rollback to before all updates
git checkout main
git reset --hard [commit-before-updates]

# Rebuild
cd /home/detair/GIT/canis
cargo clean && cargo build --release
cd client && rm -rf node_modules && npm install
```

---

*Use this checklist to track your progress. Check off items as you complete them.*
*Estimated total time: 9-15 hours over 3 weeks*
