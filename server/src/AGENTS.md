<!-- Parent: ../AGENTS.md -->
# Server Source Code

## Purpose
Core implementation of the Kaiku server. Contains all business logic, API handlers, service modules, and data layer integration. Entry point is `main.rs`, which orchestrates initialization of all services.

## Key Files
- `main.rs` - Server entry point, initializes all services and starts HTTP/WebSocket server
- `lib.rs` - Module declarations, public API surface
- `config.rs` - Environment-based configuration loading

## Subdirectories
- `admin/` - System admin panel (user bans, guild suspensions, audit log) - see admin/AGENTS.md
- `api/` - HTTP REST API routes and handlers - see api/AGENTS.md
- `auth/` - Authentication and authorization (JWT, OIDC, MFA) - see auth/AGENTS.md
- `chat/` - Text chat, channels, messages, file uploads - see chat/AGENTS.md
- `db/` - Database models, queries, connection pooling - see db/AGENTS.md
- `guild/` - Guild/server management - see guild/AGENTS.md
- `permissions/` - Permission system and authorization checks - see permissions/AGENTS.md
- `ratelimit/` - Rate limiting middleware and Redis-based tracking - see ratelimit/AGENTS.md
- `social/` - Social features (friends, blocking, presence) - see social/AGENTS.md
- `voice/` - Voice service (SFU coordination, WebRTC) - see voice/AGENTS.md
- `ws/` - WebSocket real-time message routing - see ws/AGENTS.md

## For AI Agents

### Module Organization
The codebase follows clear separation of concerns:
- Each service module (`auth`, `chat`, `voice`, etc.) contains domain logic
- `api/` contains HTTP route definitions that delegate to service modules
- `ws/` handles real-time WebSocket events
- `db/` provides shared database access patterns

**Import convention:** Services depend on `db` but not on each other. Cross-service operations happen via API layer or are explicitly coordinated.

### Error Handling Conventions
```rust
// Library-style errors with thiserror
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServiceError {
    #[error("Resource not found: {0}")]
    NotFound(String),
    #[error("Permission denied")]
    Forbidden,
}

// Application-level handlers use anyhow::Result
async fn handler() -> anyhow::Result<Response> {
    // ...
}
```

- Service modules define typed errors with `thiserror`
- API handlers return `anyhow::Result` or service-specific errors
- Database errors are wrapped with context via `.context()`
- All public APIs have structured logging via `#[tracing::instrument]`

### Service Interconnection
Services are loosely coupled but coordinate via shared state:

**AppState** (`api/mod.rs`) contains:
- `db_pool: PgPool` - Database connection pool
- `redis: RedisClient` - Redis client for caching/rate limiting
- `config: Arc<Config>` - Shared configuration
- `s3: Option<S3Client>` - Optional file storage
- `sfu: Arc<SfuServer>` - Voice SFU server
- `rate_limiter: Option<RateLimiter>` - Optional rate limiting

**Initialization order** (from `main.rs`):
1. Config loaded from environment
2. Database pool created and migrations run
3. Redis client initialized
4. S3 client created (optional, graceful degradation)
5. SFU server initialized
6. Rate limiter initialized (optional)
7. AppState assembled and passed to router

**Graceful degradation:** S3 and rate limiting are optional. If initialization fails, warnings are logged and the server continues without those features.

### Security-Critical Patterns
- All authentication handlers are in `auth/` - changes need security review
- Permission checks happen in `permissions/` middleware - never bypass
- Rate limiting is applied in `api/handlers.rs` via middleware
- JWT tokens have 15-minute expiry (configurable via `JWT_ACCESS_EXPIRY`)
- All passwords use Argon2id hashing

### Performance Considerations
- Voice latency target: <50ms end-to-end
- Hot paths: `voice/` SFU coordination, `ws/` message routing
- Avoid blocking operations in async contexts
- Use `Arc` for shared state, avoid cloning large structures
- Database queries use prepared statements via SQLx compile-time checking

### Testing Strategy
- Unit tests: In-module `#[cfg(test)]` blocks
- Integration tests: `tests/` directory with `#[sqlx::test]` fixtures
- Database tests use isolated transactions that auto-rollback
- Redis tests should use unique key prefixes to avoid collisions

### Common Patterns
**Database query example:**
```rust
#[tracing::instrument(skip(pool))]
async fn get_user(pool: &PgPool, id: Uuid) -> Result<User, DbError> {
    sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", id)
        .fetch_one(pool)
        .await
        .map_err(DbError::from)
}
```

**API handler example:**
```rust
#[tracing::instrument(skip(state))]
async fn handler(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> Result<Json<Response>, ApiError> {
    // Business logic
    Ok(Json(response))
}
```

**WebSocket message routing:**
```rust
async fn handle_message(
    state: &AppState,
    user_id: Uuid,
    msg: ClientMessage,
) -> Result<ServerMessage, WsError> {
    match msg {
        ClientMessage::Subscribe { channel_id } => {
            // Check permissions, subscribe to channel
        }
        // ...
    }
}
```

### Observability
- All public functions use `#[tracing::instrument]`
- Structured logging with `tracing` crate
- JSON output in production (configured in `main.rs`)
- Filter via `RUST_LOG` environment variable
- Example: `RUST_LOG=vc_server=debug,tower_http=debug`

### Configuration Management
All configuration is loaded from environment variables via `config::Config::from_env()`. Required variables:
- `DATABASE_URL` - PostgreSQL connection string
- `JWT_SECRET` - JWT signing secret

Optional variables have sensible defaults (see `config.rs`). Tests use `Config::default_for_test()`.
