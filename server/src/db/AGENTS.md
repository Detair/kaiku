<!-- Parent: ../../AGENTS.md -->

# Database Module

PostgreSQL connection pooling and Redis client setup. Data models and query functions.

## Purpose

- PostgreSQL connection pool creation and management
- Redis client initialization for pub/sub and caching
- Database migration runner
- Centralized data models (structs matching DB schema)
- Query functions for all database operations

## Key Files

- `mod.rs` — Pool creation (`create_pool`), migration runner (`run_migrations`), Redis client setup
- `models.rs` — Rust structs representing database tables (User, Guild, Channel, Message, etc.)
- `queries.rs` — SQL query functions using sqlx (CRUD operations for all entities)
- `tests.rs` — Database integration tests (uses `#[sqlx::test]` for isolated test DB)

## For AI Agents

### Database Stack

**PostgreSQL**: Primary data store for all persistent data.
- **Version**: 14+ (uses `gen_random_uuid()`, JSONB, full-text search)
- **Pool Size**: Default 20 connections (configurable via `PgPoolOptions`)
- **Connection String**: `postgres://user:pass@host:port/dbname`

**Redis**: Pub/sub for WebSocket events, rate limiting, session storage.
- **Client**: `fred` crate (async Redis client)
- **Connection**: `RedisConfig::from_url()` with auto-reconnect

### Schema Overview

**Tables** (see `migrations/*.sql` for full DDL):
- `users` — User accounts (id, username, email, password_hash, mfa_secret, status)
- `user_identities` — Linked external OIDC identities (id, user_id, provider_slug, subject, email, last_used_at); authoritative `(provider_slug, subject)` login lookup
- `sessions` — Refresh tokens (id, user_id, token_hash, expires_at)
- `guilds` — Servers/workspaces (id, name, owner_id)
- `guild_members` — Guild membership (guild_id, user_id, joined_at)
- `roles` — Guild roles for RBAC (id, guild_id, name, permissions_bitfield, position)
- `member_roles` — Role assignments (guild_id, user_id, role_id)
- `channels` — Text/voice channels (id, guild_id, name, type)
- `channel_members` — Private channel membership (channel_id, user_id)
- `messages` — Chat messages (id, channel_id, author_id, content, created_at, deleted_at)
- `message_attachments` — File uploads (id, message_id, filename, s3_key, size)
- `dm_channels` — DM channels (id, created_at)
- `dm_participants` — DM membership (channel_id, user_id, last_read_message_id)
- `friendships` — Friend relationships (user_id, friend_id, status: pending/accepted/blocked)

**ID Type**: UUIDv7 (time-ordered UUIDs, generated via `gen_random_uuid()` in Postgres 14+)

### Query Patterns

**sqlx Compile-Time Verification**:
- All queries checked at compile time against database schema
- Requires `DATABASE_URL` env var during build
- Use `cargo sqlx prepare` to generate `.sqlx/` query metadata for CI

**Query Functions** (in `queries.rs`):
- Naming: `find_*`, `create_*`, `update_*`, `delete_*`
- Return types: `Result<Option<T>>` for single entity, `Result<Vec<T>>` for lists
- Use `query_as!` macro for type-safe row mapping

**Example**:
```rust
pub async fn find_user_by_username(
    pool: &PgPool,
    username: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as!(
        User,
        "SELECT * FROM users WHERE username = $1",
        username
    )
    .fetch_optional(pool)
    .await
}
```

**Transactions**: Use `pool.begin()` for multi-step operations:
```rust
let mut tx = pool.begin().await?;
// ... multiple queries ...
tx.commit().await?;
```

### Migrations

**Location**: `server/migrations/*.sql` (numbered files, e.g., `001_initial_schema.sql`)

**Running Migrations**:
- Automatically on server start: `db::run_migrations(&pool).await?`
- Manually via CLI: `sqlx migrate run`

**Creating New Migrations**:
```bash
cd server
sqlx migrate add <name>  # Creates timestamped .sql file
```

**Rollback**: Not supported by sqlx (design philosophy: always forward). For rollback, create new migration.

**Migration Rules**:
- Always use `IF NOT EXISTS` for idempotency
- Never drop columns with data (add new column, migrate data, then drop old)
- Add indices for foreign keys and frequently queried columns
- Test migrations on copy of production data

### Models

**Conventions**:
- Struct fields match database column names (snake_case)
- Use `#[derive(sqlx::FromRow)]` for query result mapping
- Serialize/Deserialize for JSON responses
- Optional fields for nullable columns

**Example Model**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub display_name: Option<String>,
    pub status: UserStatus,  // Custom enum
    pub created_at: chrono::DateTime<chrono::Utc>,
}
```

**Custom Types** (PostgreSQL enums):
```sql
CREATE TYPE user_status AS ENUM ('online', 'away', 'busy', 'offline');
```
```rust
#[derive(Debug, Clone, sqlx::Type, Serialize, Deserialize)]
#[sqlx(type_name = "user_status", rename_all = "lowercase")]
pub enum UserStatus {
    Online,
    Away,
    Busy,
    Offline,
}
```

### Redis Usage

**Key Patterns**:
- `session:{user_id}` — Session metadata (optional cache)
- `ratelimit:{category}:{ip|user_id}` — Rate limit counters
- `ratelimit:block:{ip}` — Blocked IPs
- `oidc:state:{uuid}` — OIDC state parameters (10min TTL)

**Pub/Sub Channels**:
- `channel:{channel_id}` — WebSocket events for specific channel
- `presence:{user_id}` — User status updates (future)
- `global` — System-wide events (future)

**Redis Commands** (via `fred`):
```rust
// Set with expiry
redis.set("key", "value", Some(Expiration::EX(60)), None, false).await?;

// Increment counter
let count: i64 = redis.incr("counter").await?;

// Publish message
redis.publish("channel:123", "message").await?;
```

### Testing

**Test Database Setup**:
```rust
#[sqlx::test]
async fn test_create_user(pool: PgPool) {
    // Pool is pre-migrated, isolated test database
    let user = create_user(&pool, "testuser", "test@example.com").await.unwrap();
    assert_eq!(user.username, "testuser");
}
```

**Fixtures**: Use `sqlx::test(fixtures(...))` to load initial data:
```rust
#[sqlx::test(fixtures("users", "guilds"))]
async fn test_guild_members(pool: PgPool) {
    // Database pre-populated with users and guilds from fixtures/*.sql
}
```

**Required Tests**:
- [ ] All query functions with valid/invalid input
- [ ] Foreign key constraints (cascading deletes)
- [ ] Unique constraints (duplicate username, email)
- [ ] Enum type conversions (UserStatus, ChannelType)
- [ ] Soft deletes (verify `deleted_at` filtering)

### Performance Optimization

**Indices** (see migrations):
- Primary keys: `id` (UUID)
- Foreign keys: `user_id`, `guild_id`, `channel_id` (B-tree indices)
- Lookups: `username`, `email` (unique indices)
- Time-range queries: `created_at`, `updated_at` (B-tree indices)
- Composite: `(channel_id, created_at)` for message pagination

**Query Optimization**:
- Use `LIMIT` for pagination (avoid full table scans)
- Fetch only required columns (`SELECT id, username` vs `SELECT *`)
- Use joins instead of N+1 queries
- Profile slow queries with `EXPLAIN ANALYZE`

**Connection Pooling**:
- Pool size: `CPU cores * 2 + 1` (default 20 for 8-core)
- Connection timeout: 30s default
- Idle timeout: 10min (connections closed if unused)

### Common Pitfalls

**DO NOT**:
- Use raw SQL strings (use `query!` or `query_as!` macros)
- Forget to bind parameters (SQL injection risk)
- Ignore `sqlx::Error` variants (distinguish NotFound vs DatabaseError)
- Use blocking I/O in async context (always `await` queries)

**DO**:
- Handle `Option<T>` for `fetch_optional()` results
- Use transactions for related operations
- Add database indices for new query patterns
- Verify schema changes with `cargo sqlx prepare`
- Use `#[sqlx::test]` for integration tests
