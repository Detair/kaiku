# Breaking Changes Migration Guide

This guide details the code changes needed for major version updates.

---

## 🦀 Rust Backend Updates

### Axum 0.7 → 0.8

#### Router Changes
```rust
// OLD (0.7)
use axum::Router;

let app = Router::new()
    .route("/", get(handler))
    .with_state(state);

// NEW (0.8)
use axum::Router;

let app = Router::new()
    .route("/", get(handler))
    .with_state(state);  // Same API, but internal changes

// Actually, axum 0.8 is mostly compatible!
// Main changes are internal optimizations
```

**Files to Check**:
- `server/src/main.rs` - Router setup
- `server/src/api/mod.rs` - Route definitions
- `server/src/ws/mod.rs` - WebSocket routes

**Testing**:
- All HTTP endpoints
- WebSocket connections
- Middleware (CORS, tracing)

#### Middleware Changes
```rust
// OLD (0.7)
use tower_http::cors::CorsLayer;

app.layer(CorsLayer::permissive())

// NEW (0.8)
// Same API, just rebuild and test
```

---

### SQLx 0.7 → 0.8

#### Query Macro Changes
```rust
// OLD (0.7)
let user = sqlx::query!("SELECT * FROM users WHERE id = $1", id)
    .fetch_one(pool)
    .await?;

// NEW (0.8)
// Same syntax! But with better compile-time checks
let user = sqlx::query!("SELECT * FROM users WHERE id = $1", id)
    .fetch_one(pool)
    .await?;
```

**Potential Issues**:
- Some queries may now fail at compile time if they were incorrect
- Better NULL handling detection

**Files to Check**:
- `server/src/db/queries.rs` - All query macros
- `server/src/auth/handlers.rs` - User queries
- `server/src/chat/messages.rs` - Message queries
- `server/src/voice/ws_handler_test.rs` - Test queries

**Migration Steps**:
1. Update Cargo.toml
   ```toml
   sqlx = { version = "0.8", features = ["postgres", "runtime-tokio", "uuid", "chrono", "json"] }
   ```

2. Rebuild migrations
   ```bash
   cd server
   sqlx database reset
   sqlx migrate run
   ```

3. Fix compile errors (if any)

4. Test all database operations

---

### Rustls 0.23 → 0.24

Mostly compatible, minor API adjustments:

```rust
// OLD (0.23)
use rustls::crypto::ring::default_provider;

let _ = rustls::crypto::CryptoProvider::install_default(default_provider());

// NEW (0.24)
// Same API, just updated dependencies
```

**Files to Check**:
- `server/src/main.rs` - Rustls initialization

---

### Fred (Redis) 8 → 9

#### Connection API Changes
```rust
// OLD (8)
use fred::prelude::*;

let client = RedisClient::new(config);
client.connect();
client.wait_for_connect().await?;

// NEW (9)
use fred::prelude::*;

let client = RedisClient::new(config, None, None, None);
client.init().await?;
```

**Files to Check**:
- `server/src/db/mod.rs` - Redis connection

**Migration**:
1. Update connection initialization
2. Test Redis operations (pub/sub, key operations)

---

## 📦 Frontend Updates

### Vite 5 → 7

#### Config Changes
```js
// OLD (vite.config.ts - v5)
import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

export default defineConfig({
  plugins: [solid()],
  server: {
    port: 5173
  }
});

// NEW (v7)
import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

export default defineConfig({
  plugins: [solid()],
  server: {
    port: 5173
  }
  // Mostly compatible, but check plugin compatibility
});
```

**Files to Check**:
- `client/vite.config.ts`
- `client/package.json` - Update vite-plugin-solid too

**Breaking Changes**:
- Some plugins may need updates
- Build output structure may change slightly

---

### ESLint 8 → 9

#### Flat Config Migration
```js
// OLD (.eslintrc.cjs - v8)
module.exports = {
  extends: [
    'eslint:recommended',
    'plugin:@typescript-eslint/recommended'
  ],
  parser: '@typescript-eslint/parser',
  plugins: ['@typescript-eslint', 'solid'],
  rules: {
    // rules
  }
};

// NEW (eslint.config.js - v9)
import eslint from '@eslint/js';
import tseslint from '@typescript-eslint/eslint-plugin';
import solid from 'eslint-plugin-solid';

export default [
  eslint.configs.recommended,
  {
    plugins: {
      '@typescript-eslint': tseslint,
      'solid': solid
    },
    rules: {
      // rules
    }
  }
];
```

**Migration Steps**:
1. Rename `.eslintrc.cjs` → `eslint.config.js`
2. Convert to flat config format
3. Update npm scripts if needed
4. Fix new linting errors

---

### @solidjs/router 0.10 → 0.15

#### Route Definition Changes
```tsx
// OLD (0.10)
import { Router, Routes, Route } from '@solidjs/router';

<Router>
  <Routes>
    <Route path="/" component={Home} />
    <Route path="/chat/:id" component={Chat} />
  </Routes>
</Router>

// NEW (0.15)
import { Router, Route } from '@solidjs/router';

<Router>
  <Route path="/" component={Home} />
  <Route path="/chat/:id" component={Chat} />
</Router>
// Routes wrapper may be optional now
```

**Files to Check**:
- `client/src/App.tsx` - Main router setup
- All route definitions

**Breaking Changes**:
- Route API may have changed
- Nested route syntax may be different
- Check route data loading patterns

---

## 🧪 Testing Strategy

### After Each Update

#### Rust Backend Tests
```bash
cd server
cargo clean
cargo build --release
cargo test --workspace
cargo run --release
```

**Manual Tests**:
- [ ] Server starts without errors
- [ ] API health check works (`curl http://localhost:8080/health`)
- [ ] WebSocket connection works
- [ ] Database queries work
- [ ] Redis pub/sub works
- [ ] Voice WebRTC works

#### Frontend Tests
```bash
cd client
rm -rf node_modules package-lock.json
npm install
npm run build
npm run tauri build
```

**Manual Tests**:
- [ ] Dev server starts (`npm run dev`)
- [ ] Build succeeds
- [ ] Tauri app builds
- [ ] UI renders correctly
- [ ] No console errors
- [ ] Navigation works
- [ ] Voice controls work

#### Integration Tests
- [ ] Login flow
- [ ] Message sending
- [ ] Voice join/leave
- [ ] Multi-user voice chat
- [ ] File uploads (if S3 configured)

---

## 🆘 Rollback Procedures

### If Rust Update Breaks

```bash
# In Cargo.toml, revert versions
# Then:
cd server
cargo clean
cargo update
cargo build --release
```

### If npm Update Breaks

```bash
# In package.json, revert versions
cd client
rm -rf node_modules package-lock.json
npm install
npm run build
```

---

## 📋 Update Checklist Template

Copy this for each major update:

```markdown
## Update: [Package Name] [Old Version] → [New Version]

### Pre-Update
- [ ] Commit current working state
- [ ] Create backup branch: `git checkout -b backup-before-[package]`
- [ ] Document current behavior
- [ ] Run tests to establish baseline

### Update
- [ ] Update version in Cargo.toml / package.json
- [ ] Run cargo/npm install
- [ ] Fix compilation errors
- [ ] Fix linting errors
- [ ] Update code for breaking changes

### Testing
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Manual testing checklist completed
- [ ] Performance check (no regressions)

### Post-Update
- [ ] Commit changes
- [ ] Update CHANGELOG.md
- [ ] Update this guide if needed
- [ ] Deploy to staging
- [ ] Monitor for 24-48 hours
- [ ] Deploy to production
```

---

## 🔗 Official Migration Guides

- [Axum CHANGELOG](https://github.com/tokio-rs/axum/blob/main/axum/CHANGELOG.md)
- [SQLx Releases](https://github.com/launchbadge/sqlx/releases)
- [Vite Migration](https://vite.dev/guide/migration)
- [ESLint v9 Migration](https://eslint.org/docs/latest/use/migrate-to-9.0.0)
- [Solid Router Docs](https://docs.solidjs.com/solid-router)

---

*Last Updated: 2026-01-08*
