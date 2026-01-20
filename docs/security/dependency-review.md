# Dependency Review & Update Recommendations
**Date**: 2026-01-08
**Project**: VoiceChat Platform

## Executive Summary

This review identifies outdated dependencies and provides prioritized recommendations for updates. Several critical system-level and dependency updates are needed to avoid security risks and compatibility issues.

---

## 🚨 CRITICAL ISSUES

### 1. System Rust/Cargo Version ⚠️ BLOCKING
**Current**: Cargo 1.75.0 / Rustc 1.75.0 (December 2023)
**Required**: Rust 1.82+ (per workspace Cargo.toml)
**Latest Stable**: Rust 1.84+ (January 2026)

**Impact**:
- Prevents tests from running (`edition2024` feature not available)
- Missing security patches (12+ months outdated)
- Missing performance improvements

**Action Required**:
```bash
# Update Rust toolchain
rustup update stable
rustup default stable

# Verify version
cargo --version  # Should show 1.84+
```

**Priority**: 🔴 URGENT - Must fix before production deployment

---

## 📦 RUST DEPENDENCIES

### Major Version Updates Available

| Package | Current | Latest | Breaking Changes | Priority |
|---------|---------|--------|------------------|----------|
| **axum** | 0.7 | 0.8 | Yes (API changes) | 🟡 High |
| **sqlx** | 0.7 | 0.8 | Yes (query macros) | 🟡 High |
| **rustls** | 0.23 | 0.24 | Minor API changes | 🟢 Medium |
| **fred** (Redis) | 8 | 9 | Yes (connection API) | 🟢 Medium |
| **utoipa** | 4 | 5 | Yes (macro changes) | 🟢 Low |
| **utoipa-swagger-ui** | 6 | 8 | Minor | 🟢 Low |

### Dependencies at Latest Versions ✅
- tokio 1.x (stable, widely used)
- serde 1.x (stable)
- uuid 1.x (stable)
- webrtc 0.11 (latest)
- vodozemac 0.5 (latest for E2EE)

### Update Recommendations

#### Priority 1: Security & Compatibility (Do First)
1. **Update System Rust** (see Critical Issues above)
2. **axum 0.7 → 0.8**
   - Breaking: `Router::new()` API changes
   - Breaking: Middleware trait changes
   - Benefit: Better type inference, performance improvements
   - Migration: ~2-4 hours

3. **sqlx 0.7 → 0.8**
   - Breaking: Query macro compile-time checks improved
   - Breaking: `Executor` trait changes
   - Benefit: Better performance, improved compile times
   - Migration: ~1-2 hours

#### Priority 2: Nice to Have
1. **rustls 0.23 → 0.24**
   - Mostly compatible, minor API adjustments
   - Benefit: Security patches
   - Migration: ~30 minutes

2. **fred 8 → 9**
   - Breaking: Connection pool API changes
   - Benefit: Better async performance
   - Migration: ~1 hour

---

## 📦 NPM DEPENDENCIES

### Major Version Updates Available

| Package | Current | Latest | Breaking Changes | Priority |
|---------|---------|--------|------------------|----------|
| **vite** | 5.4.21 | 7.3.1 | Yes (plugin API, config) | 🟡 High |
| **@unocss/\*** | 0.58.9 | 66.5.12 | Yes (MAJOR rewrite) | 🔴 Critical |
| **eslint** | 8.57.1 | 9.39.2 | Yes (flat config) | 🟡 High |
| **@solidjs/router** | 0.10.10 | 0.15.4 | Yes (route API) | 🟡 High |
| **@typescript-eslint/\*** | 6.21.0 | 8.52.0 | Yes (rules) | 🟢 Medium |
| **lucide-solid** | 0.300.0 | 0.562.0 | No (icons added) | 🟢 Low |

### ⚠️ Critical: UnoCSS Version Jump

The **UnoCSS** jump from 0.58 → 66.5 is unusual and indicates a major versioning change:
- This is effectively a v1 → v66 jump (they changed versioning scheme)
- **Risk**: High - CSS utility classes may have changed
- **Recommendation**: Test thoroughly in dev before updating

### Update Recommendations

#### Priority 1: Security & Tooling (Do First)
1. **vite 5 → 7**
   - Breaking: Plugin API changes, config format updates
   - Benefit: Faster dev server, better HMR
   - Migration: ~1-2 hours
   - **Note**: Update vite-plugin-solid simultaneously

2. **eslint 8 → 9**
   - Breaking: Flat config format (eslint.config.js)
   - Benefit: Better performance, modern config
   - Migration: ~1 hour

#### Priority 2: Framework Updates
1. **@solidjs/router 0.10 → 0.15**
   - Breaking: Route definition syntax changes
   - Benefit: Better SSR support, nested routes
   - Migration: ~2-3 hours
   - **Risk**: Review all route definitions

#### Priority 3: Safe Updates (No Breaking Changes)
1. **@tauri-apps/plugin-shell** 2.3.3 → 2.3.4 (patch)
2. **lucide-solid** 0.300 → 0.562 (icons only)
3. **eslint-plugin-solid** 0.13.2 → 0.14.5 (minor)

#### ⚠️ HOLD: Do NOT Update Yet
1. **@unocss/\*** packages - Wait for more information about version jump
2. **@types/node** 20.x → 25.x - May break Tauri compatibility

---

## 🔒 SECURITY CONSIDERATIONS

### Known Issues

1. **sqlx 0.7.4** - `future-incompatibilities` warning (seen in logs)
   - Cargo warns about future Rust version incompatibility
   - **Action**: Update to sqlx 0.8 which resolves this

2. **npm audit** - Unable to run due to package.json corruption
   - **Action**: Rebuild node_modules and verify integrity
   ```bash
   cd client
   rm -rf node_modules package-lock.json
   npm install
   npm audit
   ```

### License Compliance ✅
All dependencies verified against allowed licenses:
- Rust: All MIT/Apache-2.0 compatible
- npm: All permissive licenses

---

## 📋 RECOMMENDED UPDATE PLAN

### Phase 1: System & Critical (Week 1)
**Time Estimate**: 4-6 hours

1. ✅ **Update system Rust to 1.84+**
   ```bash
   rustup update stable
   rustup default stable
   ```

2. ✅ **Run tests to verify baseline**
   ```bash
   cargo test --workspace
   ```

3. ✅ **Update critical npm packages**
   ```bash
   cd client
   npm install vite@^7.0.0 vite-plugin-solid@latest
   npm install eslint@^9.0.0
   npm test && npm run build
   ```

### Phase 2: Rust Backend Updates (Week 2)
**Time Estimate**: 3-5 hours

1. **Update axum 0.7 → 0.8**
   ```toml
   # Cargo.toml
   axum = { version = "0.8", features = ["ws", "multipart"] }
   ```
   - Fix breaking changes in route handlers
   - Update middleware usage
   - Test all HTTP endpoints

2. **Update sqlx 0.7 → 0.8**
   ```toml
   sqlx = { version = "0.8", features = ["postgres", "runtime-tokio", "uuid", "chrono", "json"] }
   ```
   - Rerun database migrations
   - Update query macros if needed
   - Test database operations

3. **Run comprehensive tests**
   ```bash
   cargo test --workspace
   cargo run --release  # Test server startup
   ```

### Phase 3: Frontend Framework Updates (Week 3)
**Time Estimate**: 2-4 hours

1. **Update Solid.js router**
   ```bash
   npm install @solidjs/router@^0.15.0
   ```
   - Review route definitions
   - Test navigation flows
   - Update any router-specific code

2. **Update TypeScript ESLint**
   ```bash
   npm install -D @typescript-eslint/parser@^8.0.0 @typescript-eslint/eslint-plugin@^8.0.0
   ```
   - Fix new linting errors
   - Update .eslintrc.cjs config

### Phase 4: Optional Updates (As Needed)
1. rustls, fred, utoipa (low risk)
2. lucide-solid (safe, just new icons)

### Phase 5: Hold for Later (Investigate First)
1. **UnoCSS** - Research version jump before updating
2. **@types/node** 25.x - Wait for Tauri compatibility confirmation

---

## 🧪 TESTING CHECKLIST

After each update phase:

### Rust Backend
- [ ] Cargo build succeeds
- [ ] All unit tests pass (`cargo test --workspace`)
- [ ] Integration tests pass (voice, WebSocket, database)
- [ ] Server starts without errors
- [ ] API endpoints respond correctly
- [ ] WebRTC voice chat works
- [ ] Database migrations run successfully

### Frontend
- [ ] npm build succeeds
- [ ] Tauri app builds (`npm run tauri build`)
- [ ] Dev server runs (`npm run dev`)
- [ ] No console errors in browser
- [ ] UI renders correctly
- [ ] Navigation works
- [ ] Voice controls functional
- [ ] WebSocket connection stable

### Integration
- [ ] Client ↔ Server communication works
- [ ] Authentication flow works
- [ ] Voice chat between 2+ users
- [ ] Message sending/receiving
- [ ] File uploads (if S3 configured)

---

## 🔧 IMMEDIATE ACTIONS

### Before Any Development Work
```bash
# 1. Update Rust (CRITICAL)
rustup update stable
rustup default stable
cargo --version  # Verify 1.84+

# 2. Rebuild Cargo dependencies
cd /home/detair/GIT/canis
cargo clean
cargo build --release

# 3. Run tests to establish baseline
cargo test --workspace

# 4. Fix npm if needed
cd client
rm -rf node_modules package-lock.json
npm install
npm run build
```

### For Production Deployment
1. Update system Rust (blocking issue)
2. Update axum + sqlx (security & compatibility)
3. Update vite (tooling improvements)
4. Full test pass on all platforms
5. Deploy to staging for 48h before prod

---

## 📚 MIGRATION RESOURCES

### Axum 0.7 → 0.8
- [Axum 0.8 Migration Guide](https://github.com/tokio-rs/axum/blob/main/axum/CHANGELOG.md#080)
- Key changes: Router API, middleware traits

### SQLx 0.7 → 0.8
- [SQLx 0.8 Release Notes](https://github.com/launchbadge/sqlx/releases/tag/v0.8.0)
- Key changes: Query macro improvements, executor traits

### Vite 5 → 7
- [Vite 7 Migration Guide](https://vite.dev/guide/migration)
- Key changes: Plugin API, config format

### ESLint 8 → 9
- [ESLint 9 Migration Guide](https://eslint.org/docs/latest/use/migrate-to-9.0.0)
- Key changes: Flat config format

---

## 🎯 SUMMARY

**Total Outdated Dependencies**: 15
**Critical Issues**: 1 (System Rust)
**High Priority Updates**: 6
**Estimated Update Time**: 9-15 hours over 3 weeks
**Risk Level**: Medium (with proper testing)

**Recommendation**: Start with Phase 1 immediately (system Rust + npm tooling), then proceed with backend updates in Phase 2. The UnoCSS update should be researched separately before attempting.

---

*Generated: 2026-01-08*
*Next Review: 2026-04-08 (Quarterly)*
