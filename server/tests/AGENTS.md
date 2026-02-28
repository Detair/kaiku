<!-- Parent: ../AGENTS.md -->
# Server Integration Tests

## Purpose

Integration tests for the Kaiku server. Tests API endpoints, WebSocket connections, and cross-service interactions with real database and Redis instances.

## Key Files

| File | Purpose |
|------|---------|
| `ratelimit_test.rs` | Comprehensive rate limiting tests (login, WebSocket, API endpoints) |
| `websocket_integration_test.rs` | WebSocket connection lifecycle and message routing tests |

## For AI Agents

### Test Environment Setup

Tests require running infrastructure:

```bash
# Start test dependencies
cd infra/compose && docker compose up -d

# Run integration tests
cargo test -p vc-server --test '*'

# Run specific test file
cargo test -p vc-server --test ratelimit_test
```

### Test Database

Integration tests use SQLx test fixtures:
- Each test gets isolated database transaction
- Rolled back after test completion
- Defined via `#[sqlx::test]` attribute

### Key Test Categories

**Rate Limiting (`ratelimit_test.rs`):**
- Login attempt limits
- WebSocket connection limits
- API endpoint rate limits
- Sliding window behavior
- IP-based and user-based limits

**WebSocket (`websocket_integration_test.rs`):**
- Connection establishment
- Authentication handshake
- Message routing
- Reconnection handling
- Error scenarios

### Writing New Integration Tests

```rust
use sqlx::PgPool;

#[sqlx::test(fixtures("users", "channels"))]
async fn test_channel_access(pool: PgPool) {
    // Test with seeded data
    let app = create_test_app(pool).await;

    // Make requests against app
    let response = app.get("/api/channels").await;
    assert_eq!(response.status(), 200);
}
```

### Test Fixtures Location

SQL fixtures are in `server/seeds/` directory. Reference by name without extension.

### Performance Considerations

- Integration tests are slower than unit tests
- Run in parallel where possible
- Use connection pooling
- Clean up test data via transactions

## Dependencies

- Test server infrastructure (PostgreSQL, Redis)
- SQLx test framework
- Server binary with test features enabled
