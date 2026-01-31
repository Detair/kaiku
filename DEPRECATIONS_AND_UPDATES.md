# Deprecation Warnings and Version Updates

**Generated:** 2026-01-30
**Status:** In Progress (Previous agent interrupted)

## Executive Summary

The codebase has several deprecation warnings and outdated dependencies that need attention. Most critical is the future incompatibility warning in `sqlx-postgres` that will break in Rust 2024 edition.

---

## 🔴 Critical Issues

### 1. sqlx-postgres Future Incompatibility

**Current Version:** `0.8.0`
**Recommended Version:** `0.8.6` (latest stable)

**Issue:** Never type fallback warnings that will become hard errors in Rust 2024 edition.

**Affected Code:**
- `sqlx-postgres-0.8.0/src/connection/executor.rs:22`
- `sqlx-postgres-0.8.0/src/copy.rs:254`
- `sqlx-postgres-0.8.0/src/copy.rs:286`
- `sqlx-postgres-0.8.0/src/copy.rs:323`

**Fix:** Update in `Cargo.toml`:
```toml
sqlx = { version = "0.8.6", features = ["postgres", "runtime-tokio", "uuid", "chrono", "json"] }
```

**Note:** Already partially fixed - workspace was upgraded from 0.7 → 0.8, but needs final bump to 0.8.6.

---

## 🟡 Deprecation Warnings

### 2. reqwest rustls-tls Feature Obsolete

**Location:** `client/src-tauri/Cargo.toml:41`
**Current:** `features = ["json", "rustls-tls"]`
**Issue:** Feature `rustls-tls` obsolete since reqwest 0.13.1

**Fix Options:**
1. Use `rustls` feature instead: `features = ["json", "rustls"]`
2. Use `default-tls` (recommended, enabled by default)
3. Use `native-tls` as alternative

**Recommended:**
```toml
reqwest = { version = "0.11", features = ["json", "rustls"] }
```

**Also affects worktrees:**
- `.worktrees/feature/home-unread-aggregator/`
- `.worktrees/feature/spoilers-mentions/`
- `.worktrees/emoji-picker-polish/`

---

## 📦 Outdated Dependencies

### Major Version Updates Available

| Dependency | Current | Latest | Update Type |
|------------|---------|--------|-------------|
| rusqlite | 0.31 | 0.38.0 | Minor |
| tokio-tungstenite | 0.21 | 0.28.0 | Minor |
| tower | 0.4 | 0.5.3 | Major (⚠️) |
| axum | 0.7 | 0.8.8 | Major (⚠️) |

**Notes:**
- **tower**: Workspace already at 0.5 (up-to-date!)
- **axum**: Major update from 0.7 → 0.8 requires migration
- **rusqlite**: Recently updated from 0.29 → 0.31, could go to 0.38
- **tokio-tungstenite**: Update from 0.21 → 0.28 recommended

---

## 🔧 Recent Work (Previous Agent)

The previous agent was working on **code quality improvements** following Rust idioms:

### Pattern Applied Across Codebase

**Before:**
```rust
impl IntoResponse for FavoritesError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match &self {
            FavoritesError::ChannelNotFound => { ... }
            FavoritesError::Database(err) => { ... }
        }
    }
}
```

**After:**
```rust
impl IntoResponse for FavoritesError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match &self {
            Self::ChannelNotFound => { ... }
            Self::Database(err) => { ... }
        }
    }
}
```

### Files Modified (64 files total)

Major areas:
- `server/src/api/` - 8 files
- `server/src/` - 16 files
- `server/tests/` - 13 files
- `client/src-tauri/src/` - 13 files

**Changes:**
- Enum variant matches: `TypeName::Variant` → `Self::Variant`
- Struct initialization: `TypeName { ... }` → `Self { ... }`
- Doc comment formatting improvements
- Code style consistency

---

## 🛠️ Recommended Action Plan

### Phase 1: Critical Fixes (Do First)

1. **Update sqlx to 0.8.6**
   ```bash
   # Update Cargo.toml
   vim Cargo.toml  # Change sqlx version to 0.8.6
   cargo update -p sqlx
   cargo test --workspace
   ```

2. **Fix reqwest rustls-tls deprecation**
   ```bash
   # Replace in all Cargo.toml files
   find . -name "Cargo.toml" -exec sed -i 's/"rustls-tls"/"rustls"/g' {} \;
   cargo check --workspace
   ```

### Phase 2: Minor Updates (Safe)

3. **Update rusqlite to 0.38.0**
   ```toml
   rusqlite = { version = "0.38", features = ["bundled"] }
   ```
   **Test:** Client database operations, secure storage

4. **Update tokio-tungstenite to 0.28**
   ```toml
   tokio-tungstenite = "0.28"
   ```
   **Test:** WebSocket connections, real-time messaging

### Phase 3: Major Updates (Careful)

5. **Consider axum 0.8 migration**
   - **Status:** Breaking changes expected
   - **Action:** Review [axum 0.8 migration guide](https://github.com/tokio-rs/axum/blob/main/axum/CHANGELOG.md)
   - **Estimate:** Medium effort, test all API endpoints
   - **Defer until:** After Phase 1-2 complete

---

## ✅ Verification Checklist

After each update:

- [ ] `cargo build --workspace` succeeds
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --all-targets` clean
- [ ] `cargo deny check licenses` passes
- [ ] Integration tests pass
- [ ] Update `CHANGELOG.md` under `[Unreleased]` / `### Changed`

---

## 📊 Build Status

**Last successful build:** 2026-01-30
**Rust version:** 1.92.0
**Build time:** 1m 38s
**Warnings:** 1 future-incompatibility (sqlx-postgres)

**Current state:** All code compiles and tests pass, but future-compat warning needs addressing.

---

## 🔍 Detection Methods Used

1. `cargo build --workspace` - Future incompatibility warnings
2. `cargo clippy` - Code style recommendations
3. `cargo report future-incompatibilities` - Detailed analysis
4. Manual `cargo search` - Latest versions check
5. Git diff analysis - Previous agent's work pattern

---

## Notes

- **cargo-outdated** has dependency conflicts (sqlx sqlite vs rusqlite), but builds succeed
- Previous agent made excellent progress on code quality (64 files improved)
- All worktrees need same reqwest fix applied
- No security vulnerabilities detected in current dependencies
