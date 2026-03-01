# Rate Limiting Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement Redis-based rate limiting to protect against brute-force attacks, spam, and resource abuse.

**Architecture:** Custom Axum middleware using Redis for distributed state. Fixed window algorithm with Lua script for atomic check-and-increment. Hybrid identification (IP for unauth, user ID for auth endpoints).

**Tech Stack:** Rust, Axum, Redis (fred crate), Lua scripting, metrics crate for Prometheus

**Design Doc:** `docs/plans/2026-01-17-rate-limiting-design.md`

---

## Task 1: Constants and Types

**Files:**
- Create: `server/src/ratelimit/mod.rs`
- Create: `server/src/ratelimit/constants.rs`
- Create: `server/src/ratelimit/types.rs`
- Modify: `server/src/lib.rs` (add module)

**Step 1: Create ratelimit module structure**

```rust
// server/src/ratelimit/mod.rs
pub mod constants;
pub mod types;

pub use constants::*;
pub use types::*;
```

**Step 2: Create constants**

```rust
// server/src/ratelimit/constants.rs

/// Redis key pre-allocation size
pub const REDIS_KEY_CAPACITY: usize = 64;

/// IPv6 prefix segments for rate limiting (uses /64)
pub const IPV6_PREFIX_SEGMENTS: usize = 4;

/// Log sampling configuration
pub const LOG_SAMPLE_RATE: u32 = 10;
pub const LOG_SAMPLE_OFFSET: u32 = 1;

/// Lua script return codes
pub const SCRIPT_ALLOWED: i64 = 1;
pub const SCRIPT_DENIED: i64 = 0;

/// Redis TTL sentinel values
pub const TTL_NO_EXPIRY: i64 = -1;
pub const TTL_KEY_NOT_FOUND: i64 = -2;
```

**Step 3: Create types**

```rust
// server/src/ratelimit/types.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateLimitCategory {
    AuthLogin,
    AuthRegister,
    AuthPasswordReset,
    AuthOther,
    Write,
    Social,
    Read,
    WsConnect,
    WsMessage,
    FailedAuth,
}

impl RateLimitCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AuthLogin => "auth_login",
            Self::AuthRegister => "auth_register",
            Self::AuthPasswordReset => "auth_pwd_reset",
            Self::AuthOther => "auth_other",
            Self::Write => "write",
            Self::Social => "social",
            Self::Read => "read",
            Self::WsConnect => "ws_connect",
            Self::WsMessage => "ws_message",
            Self::FailedAuth => "failed_auth",
        }
    }

    pub fn all() -> &'static [RateLimitCategory] {
        &[
            Self::AuthLogin,
            Self::AuthRegister,
            Self::AuthPasswordReset,
            Self::AuthOther,
            Self::Write,
            Self::Social,
            Self::Read,
            Self::WsConnect,
            Self::WsMessage,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct RateLimitResult {
    pub allowed: bool,
    pub limit: u32,
    pub remaining: u32,
    pub reset_at: u64,
    pub retry_after: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockedIpInfo {
    pub ip: String,
    pub failed_attempts: u32,
    pub blocked_until: u64,
}

/// Normalized IP address stored in request extensions
#[derive(Debug, Clone)]
pub struct NormalizedIp(pub String);
```

**Step 4: Add module to lib.rs**

```rust
// Add to server/src/lib.rs (or main.rs depending on structure)
pub mod ratelimit;
```

**Step 5: Verify compilation**

Run: `cd server && cargo check`
Expected: Compilation succeeds

**Step 6: Commit**

```bash
git add server/src/ratelimit/
git commit -m "feat(ratelimit): add constants and types"
```

---

## Task 2: Configuration

**Files:**
- Create: `server/src/ratelimit/config.rs`
- Modify: `server/src/ratelimit/mod.rs`

**Step 1: Create config structs**

```rust
// server/src/ratelimit/config.rs

use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub redis_key_prefix: String,
    pub fail_open: bool,
    pub trust_proxy: bool,
    pub allowlist: HashSet<String>,
    pub limits: RateLimits,
}

#[derive(Debug, Clone)]
pub struct RateLimits {
    pub auth_login: LimitConfig,
    pub auth_register: LimitConfig,
    pub auth_password_reset: LimitConfig,
    pub auth_other: LimitConfig,
    pub write: LimitConfig,
    pub social: LimitConfig,
    pub read: LimitConfig,
    pub ws_connect: LimitConfig,
    pub ws_message: LimitConfig,
    pub failed_auth: FailedAuthConfig,
}

#[derive(Debug, Clone)]
pub struct LimitConfig {
    pub requests: u32,
    pub window_secs: u64,
}

#[derive(Debug, Clone)]
pub struct FailedAuthConfig {
    pub max_failures: u32,
    pub block_duration_secs: u64,
    pub window_secs: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            redis_key_prefix: "canis:rl".to_string(),
            fail_open: true,
            trust_proxy: false,
            allowlist: HashSet::new(),
            limits: RateLimits::default(),
        }
    }
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            auth_login: LimitConfig { requests: 3, window_secs: 60 },
            auth_register: LimitConfig { requests: 5, window_secs: 60 },
            auth_password_reset: LimitConfig { requests: 2, window_secs: 60 },
            auth_other: LimitConfig { requests: 20, window_secs: 60 },
            write: LimitConfig { requests: 30, window_secs: 60 },
            social: LimitConfig { requests: 10, window_secs: 60 },
            read: LimitConfig { requests: 200, window_secs: 60 },
            ws_connect: LimitConfig { requests: 10, window_secs: 60 },
            ws_message: LimitConfig { requests: 60, window_secs: 60 },
            failed_auth: FailedAuthConfig {
                max_failures: 10,
                block_duration_secs: 900,
                window_secs: 300,
            },
        }
    }
}

impl RateLimitConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(val) = std::env::var("RATE_LIMIT_ENABLED") {
            config.enabled = val.parse().unwrap_or(true);
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_PREFIX") {
            config.redis_key_prefix = val;
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_FAIL_OPEN") {
            config.fail_open = val.parse().unwrap_or(true);
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_TRUST_PROXY") {
            config.trust_proxy = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("RATE_LIMIT_ALLOWLIST") {
            config.allowlist = val.split(',').map(|s| s.trim().to_string()).collect();
        }

        // Parse per-category limits (format: "requests,window_secs")
        if let Ok(val) = std::env::var("RATE_LIMIT_AUTH_LOGIN") {
            if let Some(limit) = parse_limit_config(&val) {
                config.limits.auth_login = limit;
            }
        }

        config
    }
}

fn parse_limit_config(val: &str) -> Option<LimitConfig> {
    let parts: Vec<&str> = val.split(',').collect();
    if parts.len() == 2 {
        let requests = parts[0].trim().parse().ok()?;
        let window_secs = parts[1].trim().parse().ok()?;
        Some(LimitConfig { requests, window_secs })
    } else {
        None
    }
}
```

**Step 2: Update mod.rs exports**

```rust
// server/src/ratelimit/mod.rs
pub mod config;
pub mod constants;
pub mod types;

pub use config::*;
pub use constants::*;
pub use types::*;
```

**Step 3: Verify compilation**

Run: `cd server && cargo check`
Expected: Compilation succeeds

**Step 4: Commit**

```bash
git add server/src/ratelimit/
git commit -m "feat(ratelimit): add configuration"
```

---

## Task 3: IP Extraction

**Files:**
- Create: `server/src/ratelimit/ip.rs`
- Modify: `server/src/ratelimit/mod.rs`

**Step 1: Create IP handling module**

```rust
// server/src/ratelimit/ip.rs

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::ratelimit::constants::IPV6_PREFIX_SEGMENTS;

pub fn extract_client_ip(
    headers: &HeaderMap,
    connect_info: Option<&ConnectInfo<SocketAddr>>,
    trust_proxy: bool,
) -> IpAddr {
    if trust_proxy {
        if let Some(forwarded) = headers.get("X-Forwarded-For") {
            if let Ok(s) = forwarded.to_str() {
                if let Some(first_ip) = s.split(',').next() {
                    if let Ok(ip) = first_ip.trim().parse() {
                        return ip;
                    }
                }
            }
        }
        if let Some(real_ip) = headers.get("X-Real-IP") {
            if let Ok(s) = real_ip.to_str() {
                if let Ok(ip) = s.trim().parse() {
                    return ip;
                }
            }
        }
    }
    connect_info
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
}

pub fn normalize_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let seg = v6.segments();
            let prefix: Vec<String> = (0..IPV6_PREFIX_SEGMENTS)
                .map(|i| format!("{:x}", seg[i]))
                .collect();
            format!("{}::/64", prefix.join(":"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    #[test]
    fn test_normalize_ipv4() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(normalize_ip(ip), "192.168.1.100");
    }

    #[test]
    fn test_normalize_ipv6() {
        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0x85a3, 0x1234, 0, 0, 0, 1));
        assert_eq!(normalize_ip(ip), "2001:db8:85a3:1234::/64");
    }
}
```

**Step 2: Update mod.rs and run tests**

Run: `cd server && cargo test ratelimit::ip`
Expected: Tests pass

**Step 3: Commit**

```bash
git add server/src/ratelimit/
git commit -m "feat(ratelimit): add IP extraction and normalization"
```

---

## Task 4: Error Types

**Files:**
- Create: `server/src/ratelimit/error.rs`

**Step 1: Create error types with HTTP responses**

```rust
// server/src/ratelimit/error.rs

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use axum::http::header::HeaderValue;
use serde::Serialize;
use crate::ratelimit::RateLimitResult;

#[derive(Debug)]
pub enum RateLimitError {
    RedisUnavailable,
    LimitExceeded(RateLimitResult),
    IpBlocked { retry_after: u64 },
}

#[derive(Serialize)]
pub struct RateLimitErrorResponse {
    pub error: &'static str,
    pub message: String,
    pub retry_after: u64,
    pub limit: u32,
    pub remaining: u32,
}

impl IntoResponse for RateLimitError {
    fn into_response(self) -> Response {
        match self {
            Self::RedisUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "service_unavailable"})),
            ).into_response(),
            Self::LimitExceeded(result) => {
                let body = RateLimitErrorResponse {
                    error: "rate_limited",
                    message: format!("Too many requests. Wait {} seconds.", result.retry_after),
                    retry_after: result.retry_after,
                    limit: result.limit,
                    remaining: 0,
                };
                let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
                let headers = response.headers_mut();
                if let Ok(v) = HeaderValue::from_str(&result.retry_after.to_string()) {
                    headers.insert("Retry-After", v);
                }
                response
            }
            Self::IpBlocked { retry_after } => {
                let body = RateLimitErrorResponse {
                    error: "ip_blocked",
                    message: format!("IP blocked. Wait {} seconds.", retry_after),
                    retry_after,
                    limit: 0,
                    remaining: 0,
                };
                (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response()
            }
        }
    }
}
```

**Step 2: Update mod.rs and verify**

Run: `cd server && cargo check`

**Step 3: Commit**

```bash
git add server/src/ratelimit/
git commit -m "feat(ratelimit): add error types"
```

---

## Task 5: Lua Script

**Files:**
- Create: `server/src/ratelimit/rate_limit.lua`

**Step 1: Create atomic rate limit script**

```lua
-- server/src/ratelimit/rate_limit.lua
-- KEYS[1] = key, ARGV[1] = ttl, ARGV[2] = limit
local count = tonumber(redis.call('GET', KEYS[1]) or '0')
local limit = tonumber(ARGV[2])
if count >= limit then
    local ttl = redis.call('TTL', KEYS[1])
    if ttl < 0 then ttl = tonumber(ARGV[1]) end
    return {count, 0, ttl}
end
count = redis.call('INCR', KEYS[1])
if count == 1 then
    redis.call('EXPIRE', KEYS[1], ARGV[1])
end
local ttl = redis.call('TTL', KEYS[1])
return {count, 1, ttl}
```

**Step 2: Commit**

```bash
git add server/src/ratelimit/rate_limit.lua
git commit -m "feat(ratelimit): add Lua script"
```

---

## Task 6: Core RateLimiter Service

**Files:**
- Create: `server/src/ratelimit/limiter.rs`

**Step 1: Implement RateLimiter**

See design doc for full implementation. Key methods:
- `new()`, `init()`, `ping()`
- `check()` - Check and increment rate limit
- `peek()` - Check without incrementing
- `clear()` - Clear limits for identifier
- `record_failed_auth()` - Track failed login attempts
- `is_blocked()` - Check if IP is blocked
- `list_blocked()` - List all blocked IPs

**Step 2: Verify and commit**

Run: `cd server && cargo check`

```bash
git add server/src/ratelimit/
git commit -m "feat(ratelimit): add core RateLimiter service"
```

---

## Task 7: Middleware

**Files:**
- Create: `server/src/ratelimit/middleware.rs`

**Step 1: Create Axum middleware**

- `rate_limit_by_ip()` - For unauthenticated endpoints
- `rate_limit_by_user()` - For authenticated endpoints
- `check_ip_not_blocked()` - Check failed auth blocks
- Helper functions for creating layers

**Step 2: Verify and commit**

```bash
git add server/src/ratelimit/
git commit -m "feat(ratelimit): add middleware"
```

---

## Task 8: AppState Integration

**Files:**
- Modify: `server/src/main.rs` or state file

**Step 1: Add RateLimiter to AppState and initialize**

**Step 2: Verify and commit**

```bash
git commit -am "feat(ratelimit): integrate with AppState"
```

---

## Task 9: Apply to Auth Routes

**Files:**
- Modify: `server/src/auth/` routes

**Step 1: Add IP rate limiting to login/register**
**Step 2: Add failed auth tracking to login handler**
**Step 3: Verify and commit**

```bash
git commit -am "feat(ratelimit): apply to auth routes"
```

---

## Task 10: Apply to API Routes

**Files:**
- Modify: `server/src/api/` routes

**Step 1: Add user rate limiting to read/write/social routes**
**Step 2: Verify and commit**

```bash
git commit -am "feat(ratelimit): apply to API routes"
```

---

## Task 11: Health Check

**Files:**
- Modify: `server/src/api/health.rs`

**Step 1: Update to include rate limiter status**
**Step 2: Commit**

```bash
git commit -am "feat(ratelimit): update health check"
```

---

## Task 12: Integration Tests

**Files:**
- Create: `server/tests/ratelimit_test.rs`

**Step 1: Write tests for:**
- Under limit allows
- Over limit blocks
- Failed auth blocks IP
- Allowlist bypasses

**Step 2: Run tests (requires Redis)**

Run: `cd server && cargo test ratelimit --ignored`

**Step 3: Commit**

```bash
git add server/tests/
git commit -m "test(ratelimit): add integration tests"
```

---

## Task 13: Documentation

**Step 1: Add rate limiting docs**
**Step 2: Commit**

```bash
git commit -am "docs: add rate limiting documentation"
```

---

## Final Verification

```bash
cd server && cargo test
cd server && cargo test --ignored  # With Redis
```

Manual test: Start server, hit login endpoint 5 times, verify 429 response.
