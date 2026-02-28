# Rate Limiting

This document describes the rate limiting system used by the Kaiku server to protect against abuse, brute-force attacks, and excessive API usage.

## Overview

The rate limiting system is built on Valkey (a BSD-3-Clause licensed Redis fork) and provides:

- **Category-based limits**: Different rate limits for different types of operations
- **IP-based rate limiting**: For unauthenticated endpoints (login, registration)
- **User-based rate limiting**: For authenticated endpoints (uses user ID)
- **Failed authentication tracking**: Blocks IPs after repeated failed login attempts
- **IPv6 /64 normalization**: Prevents circumvention using multiple IPv6 addresses
- **Fail-open behavior**: Optionally allows requests when Redis is unavailable

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RATE_LIMIT_ENABLED` | `true` | Enable or disable rate limiting entirely |
| `RATE_LIMIT_PREFIX` | `canis:rl` | Prefix for Valkey keys |
| `RATE_LIMIT_FAIL_OPEN` | `true` | Allow requests when Valkey is unavailable |
| `RATE_LIMIT_TRUST_PROXY` | `false` | Trust `X-Forwarded-For` and `X-Real-IP` headers |
| `RATE_LIMIT_ALLOWLIST` | (empty) | Comma-separated list of IPs to bypass rate limiting |

### Per-Category Limits

Each category can be configured using environment variables in the format `requests,window_secs`:

| Variable | Default | Description |
|----------|---------|-------------|
| `RATE_LIMIT_AUTH_LOGIN` | `3,60` | 3 requests per 60 seconds |
| `RATE_LIMIT_AUTH_REGISTER` | `5,60` | 5 requests per 60 seconds |
| `RATE_LIMIT_AUTH_PASSWORD_RESET` | `2,60` | 2 requests per 60 seconds |
| `RATE_LIMIT_AUTH_OTHER` | `20,60` | 20 requests per 60 seconds |
| `RATE_LIMIT_WRITE` | `30,60` | 30 requests per 60 seconds |
| `RATE_LIMIT_SOCIAL` | `20,60` | 20 requests per 60 seconds |
| `RATE_LIMIT_READ` | `200,60` | 200 requests per 60 seconds |
| `RATE_LIMIT_WS_CONNECT` | `10,60` | 10 connections per 60 seconds |
| `RATE_LIMIT_WS_MESSAGE` | `60,60` | 60 messages per 60 seconds |

### Failed Authentication Tracking

Configure failed auth blocking using the format `max_failures,block_duration_secs,window_secs`:

| Variable | Default | Description |
|----------|---------|-------------|
| `RATE_LIMIT_FAILED_AUTH` | `10,900,300` | 10 failures within 5 minutes triggers a 15-minute block |

## Rate Limit Categories

| Category | Purpose | Default Limit |
|----------|---------|---------------|
| `AuthLogin` | Login attempts | 3 req/60s |
| `AuthRegister` | Registration attempts | 5 req/60s |
| `AuthPasswordReset` | Password reset requests | 2 req/60s |
| `AuthOther` | Token refresh, other auth operations | 20 req/60s |
| `Write` | Create, update, delete operations | 30 req/60s |
| `Social` | Friend requests, invites | 20 req/60s |
| `Read` | Fetching data | 200 req/60s |
| `WsConnect` | WebSocket connection attempts | 10 req/60s |
| `WsMessage` | WebSocket message rate | 60 req/60s |
| `FailedAuth` | Failed login tracking | 10 failures -> 15 min block |

## Usage

### Importing the Middleware

```rust
use axum::middleware::from_fn_with_state;
use axum::middleware::from_fn;
use crate::ratelimit::{
    rate_limit_by_ip,
    rate_limit_by_user,
    with_category,
    check_ip_not_blocked,
    RateLimitCategory,
};
```

### Unauthenticated Routes (IP-based)

For login, registration, and password reset endpoints:

```rust
Router::new()
    .route("/login", post(login_handler))
    // 1. Rate limit by IP
    .layer(from_fn_with_state(state.clone(), rate_limit_by_ip))
    // 2. Set the rate limit category
    .layer(from_fn(with_category(RateLimitCategory::AuthLogin)))
    // 3. Check if IP is blocked (for login endpoints)
    .layer(from_fn_with_state(state.clone(), check_ip_not_blocked))
```

**Important**: Layers are applied in reverse order (bottom to top). The request flows:
1. `check_ip_not_blocked` - Rejects blocked IPs immediately
2. `with_category` - Sets the rate limit category for the request
3. `rate_limit_by_ip` - Checks and enforces the rate limit

### Authenticated Routes (User-based)

For API endpoints that require authentication:

```rust
Router::new()
    .route("/messages", post(create_message))
    // Rate limit by user ID
    .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
    // Set the rate limit category
    .layer(from_fn(with_category(RateLimitCategory::Write)))
```

If the user is not authenticated, `rate_limit_by_user` falls back to IP-based rate limiting.

### Recording Failed Authentication

When a login attempt fails, record it to track repeated failures:

```rust
if let Some(ref rate_limiter) = state.rate_limiter {
    let _ = rate_limiter.record_failed_auth(&normalized_ip).await;
}
```

### Clearing Failed Auth on Success

When a user successfully authenticates, clear their failure counter:

```rust
if let Some(ref rate_limiter) = state.rate_limiter {
    let _ = rate_limiter.clear_failed_auth(&normalized_ip).await;
}
```

## HTTP Response Headers

When rate limited, the server returns:

- **Status**: `429 Too Many Requests`
- **Header**: `Retry-After: <seconds>`
- **Body**:
```json
{
  "error": "rate_limited",
  "message": "Too many requests. Wait 45 seconds.",
  "retry_after": 45,
  "limit": 3,
  "remaining": 0
}
```

For blocked IPs:
```json
{
  "error": "ip_blocked",
  "message": "IP blocked. Wait 900 seconds.",
  "retry_after": 900,
  "limit": 0,
  "remaining": 0
}
```

## IP Address Handling

### Proxy Trust

When `RATE_LIMIT_TRUST_PROXY=true`, the system checks headers in this order:
1. `X-Forwarded-For` (first IP in the list)
2. `X-Real-IP`
3. Direct connection IP

**Security Warning**: Only enable `RATE_LIMIT_TRUST_PROXY` when running behind a trusted reverse proxy. Otherwise, clients can spoof their IP address.

### IPv6 Normalization

IPv6 addresses are normalized to their /64 prefix to prevent rate limit circumvention. For example:
- `2001:db8:85a3:1234::1` becomes `2001:db8:85a3:1234::/64`

This ensures that clients cannot bypass rate limits by using multiple addresses within the same /64 allocation.

## Troubleshooting

### Rate Limiting Not Working

1. **Check if enabled**: Verify `RATE_LIMIT_ENABLED` is not set to `false`
2. **Check Valkey connection**: Ensure Valkey is running and accessible
3. **Check fail-open**: If `RATE_LIMIT_FAIL_OPEN=true`, requests pass through when Valkey is unavailable

### Too Many False Positives

1. **Increase limits**: Adjust the relevant `RATE_LIMIT_*` environment variable
2. **Check IPv6 normalization**: Multiple users behind the same IPv6 /64 share limits
3. **Add allowlist**: Add trusted IPs to `RATE_LIMIT_ALLOWLIST`

### Users Getting Blocked Unexpectedly

1. **Check failed auth settings**: The `RATE_LIMIT_FAILED_AUTH` may be too aggressive
2. **Review block duration**: Default is 15 minutes (900 seconds)
3. **Check logs**: Look for `"IP blocked due to repeated auth failures"` messages

### Debugging Rate Limits

Enable debug logging to see rate limit checks:

```bash
RUST_LOG=canis::ratelimit=debug cargo run
```

This will show:
- Rate limit checks with category and identifier
- Allowlist bypasses
- Rate limit exceeded events
- IP blocks

## Valkey Keys

The rate limiter uses these Valkey key patterns:

| Pattern | Purpose | TTL |
|---------|---------|-----|
| `{prefix}:{category}:{identifier}` | Rate limit counter | Window duration |
| `{prefix}:failed_auth:{ip}` | Failed auth counter | 5 minutes (default) |
| `{prefix}:blocked:{ip}` | IP block flag | 15 minutes (default) |

Default prefix is `canis:rl`, so a typical key might be:
- `canis:rl:auth_login:192.168.1.100`
- `canis:rl:blocked:192.168.1.100`

## Adding Rate Limiting to New Routes

1. **Choose a category**: Select an existing `RateLimitCategory` or add a new one
2. **Decide IP vs User**: Use `rate_limit_by_ip` for unauthenticated, `rate_limit_by_user` for authenticated
3. **Apply layers in correct order**: Category layer first (applied last), rate limit layer last (applied first)
4. **Add IP block check**: For authentication endpoints, add `check_ip_not_blocked`

Example for a new social endpoint:

```rust
// In social/mod.rs
Router::new()
    .route("/friend-request", post(send_friend_request))
    .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
    .layer(from_fn(with_category(RateLimitCategory::Social)))
```

## Architecture

```
Request
    |
    v
+-------------------+
| check_ip_blocked  | <-- Rejects if IP in block list
+-------------------+
    |
    v
+-------------------+
| with_category     | <-- Sets RateLimitCategory in extensions
+-------------------+
    |
    v
+-------------------+
| rate_limit_by_ip  | <-- Checks limit, increments counter
| or rate_limit_by_ |
| user              |
+-------------------+
    |
    v
+-------------------+
| Handler           | <-- Actual route handler
+-------------------+
```
