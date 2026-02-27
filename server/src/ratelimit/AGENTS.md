<!-- Parent: ../../AGENTS.md -->

# Rate Limit Module

**SECURITY CRITICAL** — Redis-based rate limiting to protect against abuse (brute-force, spam, DoS).

## Purpose

- Token bucket rate limiting per category (login, register, API calls, WebSocket)
- IP-based blocking after repeated failures
- Per-user and per-IP rate limiting
- Middleware integration for automatic enforcement
- Configurable limits and windows

## Key Files

- `mod.rs` — Re-exports for all rate limit types and middleware
- `limiter.rs` — Core `RateLimiter` implementation using Redis
- `middleware.rs` — Axum middleware (`rate_limit_by_ip`, `rate_limit_by_user`, `check_ip_not_blocked`, `with_category`)
- `types.rs` — `RateLimitCategory` enum and rate limit result types
- `config.rs` — `RateLimitConfig` struct for tuning limits per category
- `constants.rs` — Default rate limit values (requests/window/block duration)
- `error.rs` — `RateLimitError` type
- `ip.rs` — IP extraction from request headers (X-Forwarded-For, X-Real-IP)

## For AI Agents

**SECURITY CRITICAL MODULE**: Rate limiting is the first line of defense against automated attacks. Never disable rate limits in production without careful consideration. All limits must be tuned based on legitimate traffic patterns.

### Rate Limit Categories

**Defined in `types.rs`**:
```rust
pub enum RateLimitCategory {
    AuthLogin,      // 5 req/60s per IP + blocking after 10 failures
    AuthRegister,   // 3 req/3600s per IP (prevent mass account creation)
    AuthOther,      // 10 req/60s per IP (token refresh, OIDC)
    Write,          // 30 req/60s per user (API mutations)
    Social,         // 20 req/60s per user (friend requests)
    WebSocket,      // 1 connection/60s per user (prevent connection spam)
}
```

**Configurable Limits** (`config.rs`):
```rust
pub struct RateLimitConfig {
    pub login_limit: u32,           // Default: 5
    pub login_window_secs: u32,     // Default: 60
    pub login_block_threshold: u32, // Default: 10 failures
    pub login_block_duration_secs: u32, // Default: 3600 (1 hour)

    pub register_limit: u32,        // Default: 3
    pub register_window_secs: u32,  // Default: 3600 (1 hour)

    // ... similar for other categories
}
```

**Strictness Ranking** (most to least strict):
1. `AuthRegister` — 3 req/hour (prevent spam accounts)
2. `AuthLogin` — 5 req/min + blocking (prevent brute-force)
3. `AuthOther` — 10 req/min (balance security and UX for token refresh)
4. `Social` — 20 req/min (prevent friend spam)
5. `Write` — 30 req/min (general API mutation protection)
6. `WebSocket` — 1 conn/min (prevent connection flooding)

### RateLimiter Implementation

**Algorithm**: Token bucket with Redis backend.

**Redis Keys**:
- `ratelimit:{category}:{identifier}` — Counter (requests in current window)
- `ratelimit:block:{ip}` — Blocked IP flag (set with TTL)
- `ratelimit:login_failures:{ip}` — Login failure counter

**Token Bucket Logic**:
```rust
pub async fn check_rate_limit(
    &self,
    category: RateLimitCategory,
    identifier: &str,  // IP or user_id
) -> Result<RateLimitResult, RateLimitError> {
    let key = format!("ratelimit:{}:{}", category.as_str(), identifier);
    let limit = self.config.get_limit(category);
    let window = self.config.get_window(category);

    // Increment counter
    let count: i64 = self.redis.incr(&key).await?;

    if count == 1 {
        // First request in window, set expiry
        self.redis.expire(&key, window).await?;
    }

    if count > limit {
        return Ok(RateLimitResult::Limited {
            retry_after: window,
        });
    }

    Ok(RateLimitResult::Allowed {
        remaining: limit - count,
    })
}
```

**Result Types**:
```rust
pub enum RateLimitResult {
    Allowed { remaining: i64 },
    Limited { retry_after: u32 },
}
```

### Middleware Usage

**IP-Based Rate Limiting** (for public routes):
```rust
use axum::middleware::from_fn_with_state;
use crate::ratelimit::{rate_limit_by_ip, with_category, RateLimitCategory};

Router::new()
    .route("/login", post(handlers::login))
    .layer(from_fn_with_state(state.clone(), rate_limit_by_ip))
    .layer(from_fn(with_category(RateLimitCategory::AuthLogin)))
```

**User-Based Rate Limiting** (for protected routes):
```rust
Router::new()
    .route("/api/messages", post(handlers::create_message))
    .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
    .layer(from_fn(with_category(RateLimitCategory::Write)))
    .layer(from_fn_with_state(state.clone(), auth::require_auth))
```

**Middleware Order** (bottom to top):
1. `with_category` — Sets category in request extensions
2. `rate_limit_by_{ip|user}` — Checks rate limit using category
3. `auth::require_auth` — (if user-based) Extracts user from JWT

**IP Blocking Middleware**:
```rust
Router::new()
    .route("/login", post(handlers::login))
    .layer(from_fn_with_state(state.clone(), check_ip_not_blocked))
```

### IP Blocking

**Login Failure Tracking**:
```rust
pub async fn record_login_failure(&self, ip: &str) -> Result<(), RateLimitError> {
    let key = format!("ratelimit:login_failures:{}", ip);
    let failures: i64 = self.redis.incr(&key).await?;

    // Set 1-hour expiry on first failure
    if failures == 1 {
        self.redis.expire(&key, 3600).await?;
    }

    // Block after threshold (default 10)
    if failures >= self.config.login_block_threshold {
        self.block_ip(ip, self.config.login_block_duration_secs).await?;
    }

    Ok(())
}

pub async fn block_ip(&self, ip: &str, duration_secs: u32) -> Result<(), RateLimitError> {
    let key = format!("ratelimit:block:{}", ip);
    self.redis.set(&key, "1", Some(Expiration::EX(duration_secs)), None, false).await?;
    Ok(())
}
```

**Check Blocked**:
```rust
pub async fn is_ip_blocked(&self, ip: &str) -> Result<bool, RateLimitError> {
    let key = format!("ratelimit:block:{}", ip);
    let exists: bool = self.redis.exists(&key).await?;
    Ok(exists)
}
```

**Clear Block** (manual unblock for false positives):
```rust
pub async fn unblock_ip(&self, ip: &str) -> Result<(), RateLimitError> {
    let key = format!("ratelimit:block:{}", ip);
    self.redis.del(&key).await?;
    Ok(())
}
```

### IP Extraction

**Challenge**: Clients behind proxies/load balancers have proxy IP, not real client IP.

**Header Priority** (in `ip.rs`):
1. `X-Forwarded-For` — Comma-separated list, take last (rightmost) IP (appended by trusted proxy)
2. `X-Real-IP` — Single IP from proxy
3. `Forwarded` — RFC 7239 standard header (parse `for=` parameter)
4. Connection remote address (fallback, may be proxy)

**Extraction Logic**:
```rust
pub fn extract_client_ip(headers: &HeaderMap) -> Option<String> {
    // Try X-Forwarded-For (take last/rightmost IP — appended by closest trusted proxy)
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(value) = xff.to_str() {
            if let Some(last_ip) = value.rsplit(',').next() {
                return Some(last_ip.trim().to_string());
            }
        }
    }

    // Try X-Real-IP
    if let Some(xri) = headers.get("x-real-ip") {
        if let Ok(value) = xri.to_str() {
            return Some(value.to_string());
        }
    }

    // No headers found
    None
}
```

**Security Warning**: If running behind untrusted proxies, validate `X-Forwarded-For` to prevent IP spoofing. Use proxy/load balancer configuration to set trusted headers.

### Response Headers

**Standard Headers** (future implementation):
```http
X-RateLimit-Limit: 30
X-RateLimit-Remaining: 27
X-RateLimit-Reset: 1672531200
Retry-After: 42
```

**Setting Headers in Middleware**:
```rust
// After rate limit check
response.headers_mut().insert(
    "X-RateLimit-Limit",
    limit.to_string().parse().unwrap(),
);
response.headers_mut().insert(
    "X-RateLimit-Remaining",
    remaining.to_string().parse().unwrap(),
);
```

### Error Responses

**Rate Limited Response**:
```json
{
    "error": "Rate limit exceeded",
    "retry_after": 42
}
```
**HTTP Status**: `429 Too Many Requests`

**Blocked IP Response**:
```json
{
    "error": "IP address blocked due to suspicious activity",
    "unblock_at": "2024-01-20T15:30:00Z"
}
```
**HTTP Status**: `403 Forbidden`

### Testing

**Required Tests**:
- [ ] Rate limit enforced (exceed limit, get 429)
- [ ] Rate limit resets after window expires
- [ ] Multiple requests within limit succeed
- [ ] IP blocking triggers after threshold failures
- [ ] Blocked IP returns 403 on all requests
- [ ] Different categories have independent counters
- [ ] User-based vs IP-based rate limits work correctly

**Test Utilities**:
```rust
#[cfg(test)]
mod tests {
    async fn reset_rate_limit(redis: &RedisClient, category: &str, id: &str) {
        let key = format!("ratelimit:{}:{}", category, id);
        let _: () = redis.del(&key).await.unwrap();
    }
}
```

### Configuration Tuning

**Metrics to Monitor**:
- Rate limit hit rate per category (how many requests get 429?)
- False positive rate (legitimate users hitting limits)
- Attack success rate (blocked IPs, rate-limited requests)
- p95/p99 latency for rate-limited endpoints

**Tuning Guidelines**:
- **Too Strict**: High false positive rate, user complaints
- **Too Lenient**: Successful attacks, server overload
- **Ideal**: <1% legitimate requests rate-limited, >99% attacks blocked

**Recommended Starting Points**:
- `AuthLogin`: 5 req/min, block at 10 failures
- `AuthRegister`: 3 req/hour (tight to prevent spam)
- `Write`: 30 req/min (allows batch operations)
- `WebSocket`: 1 conn/min (prevents reconnection storms)

**Adjust Based On**:
- User behavior (power users may need higher limits)
- Attack patterns (if seeing 100 req/s attacks, lower limits)
- Server capacity (higher capacity → can allow higher limits)

### Common Pitfalls

**DO NOT**:
- Use local in-memory rate limiting (breaks with multiple servers)
- Rate limit health checks or metrics endpoints (breaks monitoring)
- Block all traffic on Redis failure (fail open, log errors)
- Use user_id for unauthenticated routes (always IP-based pre-auth)
- Forget to set TTL on Redis keys (memory leak)

**DO**:
- Use Redis for distributed rate limiting
- Log rate limit violations (detect attack patterns)
- Provide clear error messages with `retry_after` values
- Allow manual IP unblocking (for false positives)
- Test rate limits with realistic traffic patterns
- Consider per-endpoint limits in addition to categories

### Future Enhancements

**Adaptive Rate Limiting**:
- Increase limits for verified users (email verified, MFA enabled)
- Decrease limits during detected attacks (automatic tightening)
- Whitelist trusted IPs (API partners, internal tools)

**Rate Limit Bypass**:
- API key tier system (free/paid with different limits)
- CAPTCHA challenges after soft limit (before hard block)
- Temporary limit increases (user requests, automated approval)

**Advanced Blocking**:
- Fingerprint-based blocking (beyond IP, use User-Agent, TLS fingerprint)
- Gradual throttling (slow down requests instead of hard block)
- Distributed ban list (share blocked IPs across instances)
