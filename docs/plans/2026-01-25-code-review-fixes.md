# Code Review Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

> **Review Status:** ✅ Plan reviewed and updated on 2026-01-25
> - Fixed base64 API usage (modern Engine-based API)
> - Fixed fred Redis API usage
> - Noted XSS fix is simpler (DOMPurify already installed)
> - Noted MFA verification code already exists
> - Added migration strategy for JWT algorithm change
> - Reordered implementation (XSS first - easiest, highest impact)
> - Added license check requirement for dashmap

**Goal:** Address all 32 issues identified in the comprehensive code review.

**Architecture:** Fixes organized by priority (CRITICAL first), with each issue having clear steps, files, and verification.

**Tech Stack:** Rust (axum, sqlx, vodozemac), TypeScript (Solid.js), PostgreSQL

---

## Phase 1: CRITICAL Security Fixes

### Issue #51: MFA Verification Bypass

**Priority:** P0 - BLOCKING
**Estimated Effort:** 1 hour (simpler than initially thought - TOTP code exists)

**Files:**
- Modify: `server/src/auth/handlers.rs:284-288`
- Create: `server/tests/mfa_test.rs`

**Note:** The `mfa_verify` function already exists at handlers.rs:606-665 and `InvalidMfaCode` error variant already exists at error.rs:50-52. The bug is that the login handler only checks if a code was *provided*, not if it's *valid*.

**Step 1: Update login handler to verify the MFA code**

In `server/src/auth/handlers.rs`, replace the TODO block at line 284-288:

```rust
// Verify MFA if enabled
if let Some(ref encrypted_secret) = user.mfa_secret {
    let code = body.mfa_code.as_ref().ok_or(AuthError::MfaRequired)?;

    let encryption_key = state.config.mfa_encryption_key
        .as_ref()
        .ok_or_else(|| AuthError::Internal("MFA encryption key not configured".into()))?;

    let key_bytes = hex::decode(encryption_key)
        .map_err(|_| AuthError::Internal("Invalid MFA encryption key".into()))?;

    let secret_str = decrypt_mfa_secret(encrypted_secret, &key_bytes)
        .map_err(|e| AuthError::Internal(format!("MFA decryption failed: {e}")))?;

    let secret = Secret::Encoded(secret_str);
    let totp = TOTP::new(
        Algorithm::SHA1, 6, 1, 30,
        secret.to_bytes().map_err(|_| AuthError::Internal("Invalid secret".into()))?,
        Some("VoiceChat".to_string()),
        user.username.clone(),
    ).map_err(|e| AuthError::Internal(format!("TOTP creation failed: {e}")))?;

    if !totp.check_current(code).unwrap_or(false) {
        record_failed_login(&state.redis, &client_ip).await;
        return Err(AuthError::InvalidMfaCode);
    }
}
```

**Step 2: Add tests**

Create `server/tests/mfa_test.rs`:

```rust
#[tokio::test]
#[ignore = "requires database"]
async fn test_login_requires_mfa_code_when_enabled() { ... }

#[tokio::test]
#[ignore = "requires database"]
async fn test_login_rejects_invalid_mfa_code() { ... }

#[tokio::test]
#[ignore = "requires database"]
async fn test_login_accepts_valid_mfa_code() { ... }
```

**Verification:**
```bash
cargo test -p vc-server mfa --ignored -- --nocapture
```

---

### Issue #52: JWT Algorithm (HS256 → RS256)

**Priority:** P0 - BLOCKING
**Estimated Effort:** 3-4 hours

**Files:**
- Modify: `server/src/config.rs` - Add key pair config
- Modify: `server/src/auth/jwt.rs` - Use RS256
- Create: `scripts/generate-jwt-keys.sh`
- Modify: `.env.example`

**Step 1: Update config**

In `server/src/config.rs`:

```rust
pub struct Config {
    // ... existing fields

    /// RSA private key for JWT signing (PEM format)
    #[serde(default)]
    pub jwt_private_key: Option<String>,

    /// RSA public key for JWT verification (PEM format)
    #[serde(default)]
    pub jwt_public_key: Option<String>,
}
```

**Step 2: Create key generation script**

Create `scripts/generate-jwt-keys.sh`:

```bash
#!/bin/bash
openssl genrsa -out jwt_private.pem 2048
openssl rsa -in jwt_private.pem -pubout -out jwt_public.pem
echo "JWT_PRIVATE_KEY=$(cat jwt_private.pem | base64 -w0)"
echo "JWT_PUBLIC_KEY=$(cat jwt_public.pem | base64 -w0)"
```

**Step 3: Update JWT encoding**

In `server/src/auth/jwt.rs`:

```rust
use base64::{Engine, engine::general_purpose::STANDARD};

pub fn create_tokens(
    user_id: Uuid,
    config: &Config,
) -> Result<(String, String, i64), JwtError> {
    let private_key = config.jwt_private_key
        .as_ref()
        .ok_or(JwtError::MissingKey)?;

    // Use modern base64 API
    let key_bytes = STANDARD.decode(private_key)
        .map_err(|_| JwtError::InvalidKey)?;
    let encoding_key = EncodingKey::from_rsa_pem(&key_bytes)?;

    let access_token = encode(
        &Header::new(Algorithm::RS256),
        &access_claims,
        &encoding_key,
    )?;

    // ... similar for refresh token
}
```

**Step 4: Update JWT decoding**

```rust
use base64::{Engine, engine::general_purpose::STANDARD};

pub fn verify_access_token(token: &str, config: &Config) -> Result<Claims, JwtError> {
    let public_key = config.jwt_public_key
        .as_ref()
        .ok_or(JwtError::MissingKey)?;

    // Use modern base64 API
    let key_bytes = STANDARD.decode(public_key)
        .map_err(|_| JwtError::InvalidKey)?;
    let decoding_key = DecodingKey::from_rsa_pem(&key_bytes)?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&["voicechat"]);

    let token_data = decode::<Claims>(token, &decoding_key, &validation)?;
    Ok(token_data.claims)
}
```

**Step 5: Migration Strategy for Existing Tokens**

Existing HS256 tokens will stop working immediately. Options:
1. **Clean break**: Force all users to re-login (recommended for security)
2. **Dual support**: Accept both algorithms during transition (add security risk)

Recommend option 1 - clear all sessions on deployment.

**Step 5: Update .env.example**

```
# JWT Keys (generate with scripts/generate-jwt-keys.sh)
JWT_PRIVATE_KEY=base64_encoded_private_key
JWT_PUBLIC_KEY=base64_encoded_public_key
```

**Verification:**
```bash
# Generate keys
./scripts/generate-jwt-keys.sh

# Test login and verify token uses RS256
cargo test jwt -- --nocapture
```

---

### Issue #53: Admin Elevation Cache Bypass

**Priority:** P0 - BLOCKING
**Estimated Effort:** 1-2 hours

**Files:**
- Modify: `server/src/admin/mod.rs:27-38`

**Step 1: Update is_elevated_admin to check database on cache miss**

```rust
use fred::prelude::*;

pub async fn is_elevated_admin(
    db: &PgPool,
    redis: &RedisClient,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let cache_key = format!("admin:elevated:{}", user_id);

    // Check cache first (fred API)
    let cached: Option<String> = redis.get(&cache_key).await.ok().flatten();
    if let Some(value) = cached {
        return Ok(value == "1");
    }

    // Cache miss - check database (fail-secure)
    let is_elevated: (bool,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1 FROM elevated_sessions
            WHERE user_id = $1 AND expires_at > NOW()
        )"
    )
    .bind(user_id)
    .fetch_one(db)
    .await?;

    // Update cache with 60 second TTL (fred API)
    let value = if is_elevated.0 { "1" } else { "0" };
    let _: Result<(), _> = redis
        .set(&cache_key, value, Some(Expiration::EX(60)), None, false)
        .await;

    Ok(is_elevated.0)
}
```

**Step 2: Update all callers to pass db pool**

Search for all `is_elevated_admin` calls and add `&state.db` parameter.

**Verification:**
```bash
# Test with Redis stopped
docker stop canis-redis
# Verify admin operations still work correctly (check database)
cargo test admin -- --nocapture
docker start canis-redis
```

---

### Issue #54: XSS in MessageItem Markdown

**Priority:** P0 - BLOCKING
**Estimated Effort:** 30 minutes (DOMPurify dependency already exists!)

**Files:**
- Modify: `client/src/components/messages/MessageItem.tsx:89-108`

**Note:** DOMPurify is already in package.json (line 20: `"dompurify": "^3.3.1"`). The code just doesn't use it for markdown rendering.

**Step 1: Add DOMPurify import** (dependency already installed)

```typescript
import DOMPurify from "dompurify";
```

**Step 2: Sanitize markdown HTML output**

In the `contentBlocks` memo, update the text block creation:

```typescript
// Parse markdown
const html = marked.parse(text, { async: false }) as string;

// Sanitize HTML to prevent XSS
const sanitizedHtml = DOMPurify.sanitize(html, {
  ALLOWED_TAGS: ['p', 'br', 'strong', 'em', 'code', 'pre', 'a', 'ul', 'ol', 'li', 'blockquote', 'h1', 'h2', 'h3'],
  ALLOWED_ATTR: ['href', 'target', 'rel'],
  ALLOW_DATA_ATTR: false,
});

blocks.push({ type: 'text', html: sanitizedHtml });
```

**Verification:**
```bash
cd client && bun run build
# Manual test: Send message with <script>alert('xss')</script>
# Verify script is stripped, not executed
```

---

## Phase 2: CRITICAL Reliability Fixes

### Issue #55: Silent Voice Session Finalization

**Priority:** P1
**Estimated Effort:** 2 hours

**Files:**
- Modify: `server/src/voice/ws_handler.rs:234-252`

**Step 1: Add retry logic with exponential backoff**

```rust
async fn finalize_session_with_retry(
    pool: PgPool,
    user_id: Uuid,
    session_id: Uuid,
    channel_id: Uuid,
    guild_id: Uuid,
    connected_at: DateTime<Utc>,
) {
    let mut retries = 0;
    let max_retries = 3;

    while retries < max_retries {
        match finalize_session(&pool, user_id, session_id, channel_id, guild_id, connected_at).await {
            Ok(_) => {
                info!(session_id = %session_id, "Session finalized successfully");
                return;
            }
            Err(e) if retries < max_retries - 1 => {
                let delay = Duration::from_secs(2u64.pow(retries));
                warn!(
                    session_id = %session_id,
                    retry = retries + 1,
                    delay_secs = delay.as_secs(),
                    error = %e,
                    "Session finalization failed, retrying"
                );
                tokio::time::sleep(delay).await;
                retries += 1;
            }
            Err(e) => {
                error!(
                    user_id = %user_id,
                    session_id = %session_id,
                    error = %e,
                    "Session finalization failed after {} retries",
                    max_retries
                );
                // TODO: Push to dead-letter queue for manual recovery
                return;
            }
        }
    }
}
```

**Step 2: Update spawn call**

```rust
tokio::spawn(finalize_session_with_retry(
    pool_clone,
    user_id,
    session_id,
    channel_id,
    guild_id,
    connected_at,
));
```

**Verification:**
```bash
cargo test voice -- --nocapture
```

---

### Issue #56: Health Check Missing DB/Redis

**Priority:** P1
**Estimated Effort:** 1 hour

**Files:**
- Modify: `server/src/api/mod.rs:175-180`

**Step 1: Update HealthResponse struct**

```rust
#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    database: bool,
    redis: bool,
    rate_limiting: bool,
}
```

**Step 2: Update health_check handler**

```rust
async fn health_check(State(state): State<AppState>) -> Result<Json<HealthResponse>, StatusCode> {
    // Quick database check (1 second timeout)
    let db_healthy = tokio::time::timeout(
        Duration::from_secs(1),
        sqlx::query("SELECT 1").fetch_one(&state.db)
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false);

    // Quick Redis check
    let redis_healthy = state.redis
        .ping()
        .await
        .is_ok();

    let all_healthy = db_healthy && redis_healthy;

    let response = HealthResponse {
        status: if all_healthy { "ok" } else { "degraded" },
        database: db_healthy,
        redis: redis_healthy,
        rate_limiting: state.rate_limiter.is_some(),
    };

    if all_healthy {
        Ok(Json(response))
    } else {
        // Return 503 for load balancer to stop routing traffic
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}
```

**Verification:**
```bash
# Test healthy state
curl http://localhost:8080/health

# Test with DB down
docker stop canis-postgres
curl http://localhost:8080/health  # Should return 503
docker start canis-postgres
```

---

### Issue #57: Silent Redis Broadcast Failures

**Priority:** P1
**Estimated Effort:** 1 hour

**Files:**
- Modify: `server/src/chat/messages.rs:361-370`
- Modify: `server/src/ws/mod.rs` (similar patterns)

**Step 1: Replace silent failures with logging**

Search for `let _ = broadcast_to_channel` and replace with:

```rust
if let Err(e) = broadcast_to_channel(&state.redis, channel_id, &event).await {
    error!(
        channel_id = %channel_id,
        event_type = ?event,
        error = %e,
        "Failed to broadcast event to channel"
    );
    // Optionally: metrics::counter!("redis.broadcast.failures").increment(1);
}
```

**Step 2: Apply same pattern to all broadcast calls**

Use grep to find all instances:
```bash
grep -rn "let _ = broadcast" server/src/
```

**Verification:**
```bash
# Stop Redis and send a message
docker stop canis-valkey
# Check server logs for broadcast failure errors
docker start canis-valkey
```

---

## Phase 3: CRITICAL Performance Fixes

### Issue #58: Memory Leak in VoiceStatsLimiter

**Priority:** P1
**Estimated Effort:** 1-2 hours

**Files:**
- Modify: `server/src/voice/rate_limit.rs`
- Modify: `server/src/voice/sfu.rs` (add cleanup task)

**Step 1: Add periodic cleanup**

In `server/src/voice/sfu.rs`, add cleanup task when creating SfuServer:

```rust
impl SfuServer {
    pub fn new(stats_limiter: VoiceStatsLimiter) -> Self {
        let limiter = stats_limiter.clone();

        // Spawn periodic cleanup task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                limiter.cleanup().await;
            }
        });

        Self {
            // ... existing fields
        }
    }
}
```

**Verification:**
```bash
# Monitor memory over time with many voice joins/leaves
cargo test voice -- --nocapture
```

---

### Issue #59: Lock Contention in RTP Forwarding

**Priority:** P0 - Voice Quality
**Estimated Effort:** 3-4 hours

**Files:**
- Modify: `server/src/voice/track.rs:91-113`
- Add dependency: `dashmap` in Cargo.toml

**Step 1: Add dashmap dependency (after license check)**

First, verify license compliance (REQUIRED by CLAUDE.md):
```bash
cd server
# Add to Cargo.toml temporarily
echo 'dashmap = "5.5"' >> Cargo.toml
cargo deny check licenses
# If passes, keep. If fails, use alternative approach.
```

In `server/Cargo.toml`:
```toml
dashmap = "5.5"  # MIT license - allowed
```

**Note:** Consider profiling first to confirm this is an actual bottleneck before adding the dependency.

**Step 2: Replace RwLock with DashMap**

```rust
use dashmap::DashMap;

pub struct TrackRouter {
    subscriptions: DashMap<(Uuid, TrackSource), Vec<Subscriber>>,
}

impl TrackRouter {
    pub async fn forward_rtp(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
        rtp_packet: &RtpPacket,
    ) {
        // No lock needed - DashMap handles concurrent access
        if let Some(subscribers) = self.subscriptions.get(&(source_user_id, source_type)) {
            for sub in subscribers.iter() {
                if let Err(e) = sub.local_track.write_rtp(rtp_packet).await {
                    warn!(error = %e, "Failed to forward RTP packet");
                }
            }
        }
    }
}
```

**Verification:**
```bash
cargo test voice -- --nocapture
# Load test with 25 participants
```

---

### Issue #60: Buffer Allocation in RTP Hot Path

**Priority:** P0 - Voice Quality
**Estimated Effort:** 2-3 hours

**Files:**
- Modify: `server/src/voice/track.rs:352`

**Step 1: Use stack allocation for fixed buffer**

```rust
pub fn spawn_rtp_forwarder(
    track: Arc<TrackRemote>,
    router: Arc<TrackRouter>,
    source_user_id: Uuid,
    source_type: TrackSource,
) {
    tokio::spawn(async move {
        // Stack-allocated buffer (no heap allocation per packet)
        let mut buf = [0u8; 1500];

        loop {
            match track.read(&mut buf).await {
                Ok((packet, _attributes)) => {
                    router.forward_rtp(source_user_id, source_type, &packet).await;
                }
                Err(e) => {
                    debug!(error = %e, "RTP read error, stopping forwarder");
                    break;
                }
            }
        }
    });
}
```

**Verification:**
```bash
# Profile memory allocation in voice path
cargo test voice -- --nocapture
```

---

## Phase 4: CRITICAL API Design Fixes

### Issue #61 & #62: Error Response Format Standardization

**Priority:** P1
**Estimated Effort:** 4-6 hours

**Files to modify:**
- `server/src/auth/error.rs`
- `server/src/guild/handlers.rs`
- `server/src/chat/channels.rs`
- `server/src/chat/messages.rs`
- `server/src/api/reactions.rs`

**Step 1: Create shared error response helper**

Create `server/src/api/error_response.rs`:

```rust
use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::Serialize;

#[derive(Serialize)]
pub struct ErrorBody {
    pub error: &'static str,
    pub message: String,
}

pub fn error_response(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    (status, Json(ErrorBody { error: code, message: message.into() })).into_response()
}
```

**Step 2: Update AuthError**

In `server/src/auth/error.rs`, update IntoResponse:

```rust
impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            Self::UserAlreadyExists => (StatusCode::CONFLICT, "user_exists"),
            Self::MfaRequired => (StatusCode::UNAUTHORIZED, "mfa_required"),
            Self::InvalidMfaCode => (StatusCode::UNAUTHORIZED, "invalid_mfa_code"),
            // ... other variants
        };

        error_response(status, code, self.to_string())
    }
}
```

**Step 3: Apply same pattern to all error types**

Update GuildError, ChannelError, MessageError, ReactionError to use `error_response()`.

**Verification:**
```bash
# Test error responses match format
curl -X POST http://localhost:8080/auth/login -d '{"username":"x","password":"y"}'
# Should return {"error": "invalid_credentials", "message": "..."}
```

---

## Phase 5: CRITICAL Testing Fixes

### Issue #63: Auth Flow Tests

**Priority:** P1
**Estimated Effort:** 4-6 hours

**Files:**
- Create: `server/tests/auth_test.rs`

**Implementation:**

```rust
//! Integration tests for authentication flows.

use sqlx::PgPool;
use uuid::Uuid;

async fn create_test_pool() -> PgPool { ... }

#[tokio::test]
#[ignore = "requires database"]
async fn test_register_creates_user() {
    let pool = create_test_pool().await;
    // POST /auth/register with valid data
    // Verify user created in database
    // Verify tokens returned
}

#[tokio::test]
#[ignore = "requires database"]
async fn test_register_rejects_duplicate_username() {
    // Create user, try to register with same username
    // Verify 409 Conflict
}

#[tokio::test]
#[ignore = "requires database"]
async fn test_login_valid_credentials() {
    // Create user, login with correct password
    // Verify tokens returned
}

#[tokio::test]
#[ignore = "requires database"]
async fn test_login_invalid_password() {
    // Create user, login with wrong password
    // Verify 401 Unauthorized
}

#[tokio::test]
#[ignore = "requires database"]
async fn test_token_refresh_extends_session() {
    // Login, use refresh token
    // Verify new access token returned
}

#[tokio::test]
#[ignore = "requires database"]
async fn test_logout_invalidates_refresh_token() {
    // Login, logout, try to use refresh token
    // Verify refresh fails
}
```

---

### Issues #64, #65, #66: Additional Test Suites

**Create similar test files for:**
- `server/tests/admin_elevation_test.rs`
- `server/tests/e2ee_test.rs`
- `client/src/**/*.test.ts` (using Vitest)

---

## Phase 6: WARNING Fixes

### Issue #67: WebSocket Token in Query Parameter

**Estimated Effort:** 2-3 hours

Use `Sec-WebSocket-Protocol` header instead:

```rust
// Client sends: Sec-WebSocket-Protocol: access_token, <JWT>
let token = headers
    .get("sec-websocket-protocol")
    .and_then(|h| h.to_str().ok())
    .and_then(|s| s.split(',').nth(1))
    .map(str::trim)
    .ok_or(WsError::MissingToken)?;
```

---

### Issue #68: User-Agent Sanitization

**Estimated Effort:** 30 minutes

```rust
fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers.get(USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .map(|s| {
            s.chars()
                .filter(|c| !c.is_control() || c.is_whitespace())
                .take(512)
                .collect()
        })
}
```

---

### Issue #69: Avatar MIME Validation

**Estimated Effort:** 1 hour

```rust
use image::ImageFormat;

// Verify actual file content
let format = image::guess_format(&data)
    .map_err(|_| AuthError::Validation("Invalid image file".to_string()))?;

// Reject SVG (XSS risk)
if matches!(format, ImageFormat::Svg) {
    return Err(AuthError::Validation("SVG images not allowed".to_string()));
}
```

---

### Issue #70: Rate Limiter Fail-Open Logging

**Estimated Effort:** 30 minutes

```rust
Err(e) => {
    warn!(error = %e, "Redis unavailable - FAILING OPEN - rate limiting disabled");
    Ok(RateLimitResult { allowed: true, ... })
}
```

---

### Issue #71: S3 Circuit Breaker

**Estimated Effort:** 2 hours

```rust
let result = tokio::time::timeout(
    Duration::from_secs(30),
    s3_client.put_object(request)
).await
.map_err(|_| UploadError::Storage("S3 timeout".into()))?;
```

---

### Issue #72: Database Pool Configuration

**Estimated Effort:** 30 minutes

```rust
let pool = PgPoolOptions::new()
    .max_connections(20)
    .acquire_timeout(Duration::from_secs(5))
    .idle_timeout(Duration::from_secs(600))
    .test_before_acquire(true)
    .connect(database_url)
    .await?;
```

---

### Issue #73: localStorage Token Security

**Estimated Effort:** 4-6 hours (requires backend changes for httpOnly cookies)

**Note:** This is acceptable for Tauri desktop. For browser mode, consider:
1. httpOnly cookies for refresh token
2. In-memory storage for access token
3. Silent refresh via iframe

---

### Issue #74: Database Query Error Context

**Estimated Effort:** 2-3 hours

Add `.map_err()` to all database queries in `server/src/db/queries.rs`:

```rust
.await.map_err(|e| {
    tracing::error!(user_id = %id, error = %e, "Failed to fetch user");
    e
})
```

---

### Issue #75: Graceful Shutdown

**Estimated Effort:** 2-3 hours

Track spawned tasks and await them on shutdown with timeout.

---

### Issues #76, #77: Pagination Consistency

**Estimated Effort:** 2-3 hours

Document both patterns or standardize on cursor-based pagination.

---

### Issue #78: Voice Join Query Optimization

**Estimated Effort:** 1-2 hours

Use cached user info from Peer struct instead of database query.

---

### Issue #79: Room Broadcast Lock

**Estimated Effort:** 1 hour

Clone peer list before sending, use `try_send()`.

---

### Issues #80, #81: Additional Tests

**Estimated Effort:** 4-6 hours each

Create test files for voice SFU and guild invite security.

---

### Issue #82: API Versioning

**Estimated Effort:** 2-4 hours

Add `/api/v1/` prefix to all routes. Low priority - future consideration.

---

## Implementation Order

**Week 1 - Security Critical:**
1. #54 XSS MessageItem (P0) - **START HERE** (30 min, highest immediate impact, dependency exists)
2. #51 MFA Bypass (P0) - 1 hour (code exists, needs wiring)
3. #53 Admin Elevation Cache (P0) - 1-2 hours
4. #52 JWT Algorithm (P0) - 4-6 hours (requires migration strategy)

**Week 2 - Performance & Reliability:**
5. #59 Lock Contention (P0) - **Run `cargo deny check licenses` for dashmap first!**
6. #60 Buffer Allocation (P0) - Profile before optimizing
7. #58 Memory Leak (P1)
8. #55, #56, #57 Reliability fixes (P1)

**Week 3 - API & Testing:**
9. #61, #62 Error Format (P1) - **BREAKING CHANGE** - document for clients
10. #63-66 Test Suites (P1)

**Week 4 - Warnings:**
11. All WARNING issues

---

## Verification Checklist

- [ ] All CRITICAL security fixes deployed
- [ ] MFA actually verifies codes
- [ ] JWT uses RS256/EdDSA
- [ ] XSS payloads sanitized
- [ ] Voice latency <50ms under load
- [ ] No memory leaks in 24h test
- [ ] Health check returns 503 when unhealthy
- [ ] Error responses use consistent format
- [ ] Auth test suite passes
- [ ] Admin elevation test suite passes

---

## Notes

- Run `cargo deny check licenses` before adding any new dependencies
- All fixes should include tests
- Update CHANGELOG.md after each phase
