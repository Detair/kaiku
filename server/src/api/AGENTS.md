<!-- Parent: ../../AGENTS.md -->

# API Module

Central routing configuration and shared application state for the Kaiku platform.

## Purpose

- Router setup combining all service endpoints (auth, chat, voice, guilds, social)
- Shared `AppState` providing database, Redis, S3, SFU, and rate limiter access
- Middleware layer configuration (CORS, compression, tracing, rate limiting)
- Health check endpoint

## Key Files

- `mod.rs` — Main router creation, AppState definition, middleware configuration

## For AI Agents

**Core Responsibility**: This module is the single point where all HTTP routes are assembled. When adding new endpoints:

1. Add route in appropriate service module first
2. Import service router in `mod.rs`
3. Nest under correct path and apply appropriate rate limit category
4. Protected routes must be wrapped with `auth::require_auth` middleware

**AppState Access**: All handlers get `State<AppState>` extraction. Never clone heavy resources (db pool, redis client) unnecessarily — they are already `Clone` via `Arc` internally.

**Rate Limiting Categories**:
- `AuthLogin`: 5 requests/60s (strictest, with IP blocking)
- `AuthRegister`: 3 requests/3600s
- `AuthOther`: 10 requests/60s (refresh, OIDC)
- `Write`: 30 requests/60s (API mutations)
- `Social`: 20 requests/60s (friend operations)
- `WebSocket`: 1 connection/60s per user

**Body Limit**: Default 2MB, configurable via `max_upload_size` in config (currently 50MB). Applied via `DefaultBodyLimit::max()` layer.

**Security**: All routes under `/api/*` (except `/api/messages/attachments/:id/download`) require JWT authentication. Download endpoint handles auth via query parameter for browser compatibility.

**Adding New Services**:
```rust
// In service module (e.g., src/foo/mod.rs)
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/bar", get(handler))
}

// In src/api/mod.rs
use crate::foo;

// Inside create_router():
.nest("/api/foo", foo::router())
.layer(from_fn_with_state(state.clone(), rate_limit_by_user))
.layer(from_fn(with_category(RateLimitCategory::Write)))
.layer(from_fn_with_state(state.clone(), auth::require_auth))
```

**Diagnostics**: `/health` endpoint returns `{"status": "ok", "rate_limiting": bool}`. Use this for container health checks.
