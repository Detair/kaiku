# 📦 Canis Dependency Audit Report

**Generated:** 2026-01-30
**Status:** Post-dependency-fix audit
**Build Status:** ✅ All tests passing (289 tests)

---

## Executive Summary

- **Total Core Dependencies:** 45+ workspace-level packages
- **Up-to-date:** 15 packages (33%)
- **Minor updates available:** 20 packages (44%)
- **Major updates available:** 10 packages (22%)
- **Critical Issues:** 1 (sqlx future incompatibility warning)

---

## 🔴 Critical Dependencies Requiring Attention

### 1. sqlx (Database ORM)
- **Current:** 0.7.2
- **Latest Stable:** 0.8.x
- **Latest (all):** 0.9.0-alpha.1
- **Issue:** Future incompatibility warning (never type fallback)
- **Impact:** Will become hard error in Rust 2024 edition
- **Migration Required:** Yes - requires adding `FromRow` derives, updating query patterns
- **Effort:** Medium (2-4 hours)
- **Blocker:** rusqlite version conflict with 0.8.x

### 2. axum (Web Framework)
- **Current:** 0.7.9
- **Latest:** 0.8.8
- **Impact:** Core web framework, breaking API changes
- **Migration Required:** Yes
- **Effort:** High (4-8 hours)
- **Recommendation:** Plan dedicated migration sprint

---

## 🟡 High Priority Updates

### Authentication & Security

| Package | Current | Latest | Notes |
|---------|---------|--------|-------|
| jsonwebtoken | 9.3.1 | 10.3.0 | JWT handling, test thoroughly |
| argon2 | 0.5.3 | 0.6.0-rc.6 | Password hashing, RC version |
| openidconnect | 3.5.0 | 4.0.1 | OAuth/OIDC, major version bump |
| rustls | 0.21.12 | 0.24.0-dev.0 | TLS library, dev version |
| vodozemac | 0.5.1 | 0.9.0 | E2EE (Olm/Megolm) |

### Infrastructure

| Package | Current | Latest | Notes |
|---------|---------|--------|-------|
| fred | 8.0.6 | 10.1.0 | Redis/Valkey client, major jump |
| webrtc | 0.11.0 | 0.14.0 | Voice/video core |
| tower | 0.4.13 / 0.5.3 | 0.5.3 | Middleware (mixed versions) |
| tower-http | 0.5.2 | 0.6.8 | HTTP middleware |
| tokio-tungstenite | 0.24.0 / 0.28.0 | 0.28.0 | WebSocket (✅ updated) |

---

## 🟢 Recently Updated (2026-01-30)

| Package | From | To | Status |
|---------|------|----|----|
| tokio-tungstenite | 0.21.0 | 0.28.0 | ✅ Done |
| reqwest | rustls-tls | rustls | ✅ Fixed deprecation |
| tower | 0.4.13 | 0.5.3 | ✅ Workspace updated |

**Breaking changes handled:**
- `Message::Text` now requires `.into()` conversion (fixed in 2 files)
- reqwest deprecated `rustls-tls` feature removed

---

## 📱 Client-Specific Dependencies

| Package | Current | Latest | Priority | Notes |
|---------|---------|--------|----------|-------|
| tauri | 2.9.5 | 2.9.5 | ✅ | Desktop framework - up to date |
| cpal | 0.15.3 | 0.17.1 | 🟡 | Audio I/O |
| rodio | 0.19.0 | 0.21.1 | 🟡 | Audio playback |
| reqwest | 0.11.27 | 0.13.1 | 🟡 | HTTP client |
| rusqlite | 0.29.0 | 0.38.0 | 🔴 | **Blocked by sqlx conflict** |
| sysinfo | 0.30.13 | 0.38.0 | 🟡 | System info (game detection) |
| keyring | 2.3.3 | 4.0.0-beta.3 | 🔴 | Credential storage (beta) |
| arboard | 3.6.1 | 3.6.1 | ✅ | Clipboard |

---

## 🛡️ License Compliance Status

All dependencies verified against project license policy (MIT OR Apache-2.0):

**Permitted Licenses:**
- MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, CC0-1.0, MPL-2.0

**Forbidden Licenses:**
- GPL-2.0, GPL-3.0, AGPL-3.0, LGPL-2/3, SSPL (✅ None detected)

**Latest Check:** Run `cargo deny check licenses` before adding new dependencies.

---

## 📊 Stability Analysis

### Production-Ready (Stable APIs)
- tokio 1.49.0 ✅
- serde 1.0.228 ✅
- chrono 0.4.43 ✅
- uuid 1.20.0 ✅
- anyhow 1.0.100 ✅

### Requires Monitoring
- webrtc 0.11.0 (voice stability critical)
- vodozemac 0.5.1 (E2EE foundation)
- sqlx 0.7.2 (deprecation warning)

---

## 🚀 Recommended Update Strategy

### Phase 1: Foundation Updates (Low Risk)
**Estimated Time:** 2-3 hours
1. Update utility crates (sysinfo, cpal, rodio)
2. Update AWS SDK packages (minor version bumps)
3. Test audio and system detection features

### Phase 2: Security Updates (Medium Risk)
**Estimated Time:** 3-4 hours
1. jsonwebtoken 9 → 10 (review API changes)
2. argon2 0.5 → 0.6 (if stable, not RC)
3. Run full auth test suite
4. Test E2EE key operations

### Phase 3: Infrastructure Overhaul (High Risk)
**Estimated Time:** 8-12 hours
1. **axum 0.7 → 0.8** migration
   - Review [axum 0.8 CHANGELOG](https://github.com/tokio-rs/axum/blob/main/axum/CHANGELOG.md)
   - Update all API handlers
   - Test all endpoints
2. **sqlx 0.7 → 0.8** migration
   - Add `FromRow` derives to all database models
   - Update query patterns
   - Resolve rusqlite version conflict
3. **fred 8 → 10** (Redis client)
   - Check for API changes
   - Test pub/sub, rate limiting
4. **webrtc 0.11 → 0.14**
   - Critical for voice - test extensively
   - Check for breaking changes in SFU logic

### Phase 4: Future-Proofing (Post-MVP)
1. Evaluate rustls 0.24 (when stable)
2. Consider reqwest 0.13
3. Monitor vodozemac updates

---

## ⚠️ Known Blockers

### 1. libsqlite3-sys Version Conflict
**Problem:** sqlx and rusqlite both link to native sqlite3 library
**Current State:**
- sqlx 0.7.x uses libsqlite3-sys 0.26.0
- sqlx 0.8.x uses libsqlite3-sys 0.30.1
- rusqlite 0.29 uses libsqlite3-sys 0.26.0 (✅ compatible)
- rusqlite 0.31+ uses libsqlite3-sys 0.28.0+ (❌ conflicts)

**Solutions:**
1. Keep rusqlite at 0.29 until sqlx 0.8+ migration complete
2. Migrate sqlx to 0.8.x with `default-features = false` to exclude sqlite
3. Consider alternative local storage for client (sled, redb)

### 2. Multiple Transitive Dependency Versions
Some crates appear multiple times in Cargo.lock due to different version requirements:
- `tower` 0.4.13 and 0.5.3 (webrtc still uses 0.4.x)
- `tokio-tungstenite` 0.24.0 and 0.28.0 (transitive deps)

**Action:** Monitor and minimize with `cargo tree --duplicates`

---

## 📋 Testing Checklist

Before each major update:

- [ ] `cargo test --workspace` (all 289 tests pass)
- [ ] `cargo clippy -- -D warnings` (clean)
- [ ] `cargo deny check licenses` (compliance)
- [ ] Manual smoke tests:
  - [ ] User registration/login
  - [ ] Voice channel join/leave
  - [ ] Message send/receive
  - [ ] File upload/download
  - [ ] E2EE message encryption
  - [ ] WebSocket reconnection
  - [ ] Audio device selection

---

## 🔗 References

- [Keep a Changelog](https://keepachangelog.com/) - Changelog format
- [Semantic Versioning](https://semver.org/) - Version numbering
- [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) - License checking
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) - Best practices

---

## Next Steps

1. ✅ ~~Fix critical dependency deprecations~~ (completed 2026-01-30)
2. 📝 Address TODO-REVIEW-FIXES (7 critical blockers)
3. 🔄 Complete Phase 4 roadmap items (4 remaining)
4. 🚀 Plan Phase 3 update strategy (axum + sqlx migration)

---

*Last Updated: 2026-01-30*
*Maintainer: VoiceChat Development Team*
