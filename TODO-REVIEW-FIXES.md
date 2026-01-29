# TODO: Unified File Size Limits - Review Fixes

**Last Updated:** 2026-01-29
**Feature Branch:** feature/unified-file-size-limits
**Review Date:** 2026-01-29
**Overall Status:** 7 CRITICAL BLOCKERS - NOT READY TO MERGE

---

## Critical Issues (MUST FIX BEFORE MERGE)

### 1. Fix Emoji Error Handling Unreachable Panic ⏱️ 30 min
**File:** `server/src/guild/emojis.rs:98`
**Issue:** `unreachable!()` inside wildcard match arm will panic if code is refactored
**Priority:** CRITICAL (Code Quality)
**Confidence:** 92%

**Current Code:**
```rust
impl IntoResponse for EmojiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            EmojiError::FileTooLarge { max_size } => { /* ... */ }
            _ => {
                let (status, code, message) = match &self {
                    // ... other variants ...
                    EmojiError::FileTooLarge { .. } => unreachable!("Handled above"),
                };
                // ...
            }
        }
    }
}
```

**Fix:** Flatten the match to handle all variants explicitly:
```rust
impl IntoResponse for EmojiError {
    fn into_response(self) -> axum::response::Response {
        let (status, body) = match self {
            EmojiError::FileTooLarge { max_size } => {
                let message = format!("File too large (max {} for emojis)",
                    crate::util::format_file_size(max_size));
                (StatusCode::PAYLOAD_TOO_LARGE, json!({
                    "error": "FILE_TOO_LARGE",
                    "message": message,
                    "max_size_bytes": max_size
                }))
            }
            EmojiError::GuildNotFound => (StatusCode::NOT_FOUND, json!({ /* ... */ })),
            EmojiError::EmojiNotFound => (StatusCode::NOT_FOUND, json!({ /* ... */ })),
            // ... handle all other variants explicitly
        };
        (status, Json(body)).into_response()
    }
}
```

---

### 2. Add S3 Deletion Error Logging ⏱️ 30 min
**File:** `server/src/guild/emojis.rs:436-443`
**Issue:** S3 deletion failures silently ignored with `let _ =`
**Priority:** CRITICAL (Reliability)
**Impact:** Storage orphaning, cost accumulation, no operational visibility

**Current Code:**
```rust
if let Some(s3) = &state.s3 {
    let extensions = ["png", "jpg", "gif", "webp"];
    for ext in extensions {
         let key = format!("emojis/{}/{}.{}", guild_id, emoji_id, ext);
         let _ = s3.delete(&key).await;  // SILENT FAILURE
    }
}
```

**Fix:**
```rust
if let Some(s3) = &state.s3 {
    let extensions = ["png", "jpg", "gif", "webp"];
    for ext in extensions {
         let key = format!("emojis/{}/{}.{}", guild_id, emoji_id, ext);
         if let Err(e) = s3.delete(&key).await {
             tracing::warn!(
                 error = %e,
                 s3_key = %key,
                 emoji_id = %emoji_id,
                 guild_id = %guild_id,
                 "Failed to delete emoji file from S3 - storage may be orphaned"
             );
         }
    }
}
```

---

### 3. Add Redis Broadcast Error Tracking ⏱️ 1 hour
**Files:** Multiple (7 locations)
**Issue:** Redis broadcast failures lack error IDs for Sentry tracking
**Priority:** CRITICAL (Reliability)
**Impact:** Cannot track WebSocket reliability, no alerting on systemic failures

**Locations:**
- `server/src/guild/emojis.rs:307-325` (create)
- `server/src/guild/emojis.rs:385-403` (update)
- `server/src/guild/emojis.rs:459-477` (delete)
- `server/src/chat/dm.rs:558-574` (DM name)
- `server/src/chat/dm.rs:793-809` (mark read)
- `server/src/chat/uploads.rs:560-576` (message upload)
- `server/src/auth/handlers.rs:827-844` (profile update)

**Fix Pattern:**
```rust
if let Err(e) = state.redis.publish::<(), _, _>(channel, payload).await {
    tracing::error!(
        error = %e,
        error_id = "REDIS_PUBLISH_FAILED",  // Add to constants/errorIds.ts
        guild_id = %guild_id,
        event = "GuildEmojiUpdated",
        "Failed to broadcast emoji creation via Redis - clients may see stale data"
    );
}
```

**TODO:** Add "REDIS_PUBLISH_FAILED" to `constants/errorIds.ts`

---

### 4. Fix Test Comment About Validation Logic ⏱️ 10 min
**File:** `server/tests/upload_limits_test.rs:104`
**Issue:** Comment says "handler uses <= check" but handler uses `>` (opposite)
**Priority:** CRITICAL (Documentation)
**Impact:** Could mislead developers into implementing validation backwards

**Current:**
```rust
assert!(
    exactly_at_limit <= config.max_avatar_size,
    "File exactly at limit should be accepted (handler uses <= check)"
);
```

**Fix:**
```rust
assert!(
    exactly_at_limit <= config.max_avatar_size,
    "File exactly at limit should be accepted (handler rejects when data.len() > max_size)"
);
```

---

### 5. Fix Config Comment About Middleware ⏱️ 10 min
**File:** `server/src/config.rs:56`
**Issue:** Comment claims max_upload_size governs all uploads (wrong)
**Priority:** CRITICAL (Documentation)

**Current:**
```rust
/// Used by DefaultBodyLimit middleware as final safety net for all uploads.
/// Should be ≥ all specific upload limits (avatar, emoji).
pub max_upload_size: usize,
```

**Fix:**
```rust
/// Maximum file upload size in bytes (default: 50MB)
///
/// Used by DefaultBodyLimit middleware for general API routes (message attachments).
/// Specific routes may have their own limits (e.g., avatar upload uses max_avatar_size).
/// For consistency, this should typically be ≥ max_avatar_size and max_emoji_size.
pub max_upload_size: usize,
```

---

### 6. Remove Hardcoded Line Number References ⏱️ 5 min
**File:** `server/tests/upload_limits_test.rs:363`
**Issue:** Line numbers will become stale after refactoring
**Priority:** CRITICAL (Documentation)

**Current:**
```rust
// Handler validation pattern (see auth/handlers.rs:672, guild/emojis.rs:227):
```

**Fix:**
```rust
// Handler validation pattern (see upload_avatar and create_emoji handlers):
// Handlers check: if data.len() > max_size { reject }
```

---

### 7. Add DM Test Implementation Checklist ⏱️ 10 min
**File:** `server/tests/upload_limits_test.rs:173`
**Issue:** Ignored test lacks guidance for when DM feature is implemented
**Priority:** CRITICAL (Documentation)

**Current:**
```rust
#[ignore = "DM feature not yet implemented - enable once dm_conversations table exists"]
async fn test_dm_icon_uses_avatar_limit_not_attachment_limit() {
```

**Fix:**
```rust
#[ignore = "DM feature not yet implemented"]
// TODO(dm-implementation): When adding DM feature:
// 1. Create dm_conversations table
// 2. Add DM icon upload handler using max_avatar_size validation
// 3. Remove #[ignore] from this test and verify it passes
// 4. Add HTTP integration test for /api/dm/{id}/icon endpoint
async fn test_dm_icon_uses_avatar_limit_not_attachment_limit() {
```

---

## Important Issues (SHOULD FIX)

### 8. Add HTTP Integration Tests ⏱️ 3-4 hours
**File:** `server/tests/upload_limits_integration_test.rs` (NEW FILE)
**Issue:** No tests for actual HTTP upload behavior
**Priority:** HIGH (Testing)
**Criticality:** 10/10

**Missing Coverage:**
- Avatar uploads return HTTP 413 for oversized files
- Error response format includes `max_size_bytes`
- Authorization checked before size validation (401/403 before 413)
- Frontend-backend limit alignment verification
- Emoji upload enforces guild membership

**Create New Test File:**
```rust
// server/tests/upload_limits_integration_test.rs

#[tokio::test]
async fn test_avatar_upload_rejects_oversized_file_with_413() {
    let app = create_test_app().await;
    let token = create_test_user_and_login(&app).await;

    let oversized_data = vec![0u8; 6 * 1024 * 1024]; // Over 5MB limit
    let multipart = create_multipart_avatar(oversized_data);

    let response = app
        .post("/auth/me/avatar")
        .header("Authorization", format!("Bearer {token}"))
        .multipart(multipart)
        .await;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let body: serde_json::Value = response.json().await;
    assert_eq!(body["error"], "validation");
    assert!(body["message"].as_str().unwrap().contains("too large"));
    assert_eq!(body["max_size_bytes"], 5 * 1024 * 1024);
}

#[tokio::test]
async fn test_unauthenticated_upload_returns_401_not_413() {
    let app = create_test_app().await;
    let oversized_data = vec![0u8; 100 * 1024 * 1024];

    let response = app
        .post("/auth/me/avatar")
        .multipart(create_multipart_avatar(oversized_data))
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn test_emoji_upload_enforces_guild_membership_before_size() {
    let (app, guild_id) = setup_guild().await;
    let non_member_token = create_test_user_and_login(&app).await;

    let oversized_emoji = vec![0u8; 512 * 1024];

    let response = app
        .post(&format!("/api/guilds/{guild_id}/emojis"))
        .header("Authorization", format!("Bearer {non_member_token}"))
        .multipart(create_multipart_emoji("test", oversized_emoji))
        .await;

    // Should fail membership check before size validation
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_ne!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
```

---

### 9. Remove or Fix Tautological Tests ⏱️ 30 min
**File:** `server/tests/upload_limits_test.rs:102-114, 128-140`
**Issue:** Tests assert `x <= x` which can never fail
**Priority:** MEDIUM (Testing)

**Current:**
```rust
let exactly_at_limit = config.max_avatar_size;
assert!(
    exactly_at_limit <= config.max_avatar_size,  // Always true!
    "File exactly at limit should be accepted"
);
```

**Options:**
1. Convert to real HTTP behavior tests (see #8)
2. Mark as `#[ignore]` with TODO to replace
3. Delete entirely (keep only defaults and sensibility tests)

**Recommendation:** Mark as ignored and add to HTTP integration test plan:
```rust
#[test]
#[ignore = "Replace with HTTP integration test - see upload_limits_integration_test.rs"]
fn test_avatar_size_validation_logic() {
    // TODO: This test validates arithmetic but not behavior.
    // See test_avatar_upload_rejects_oversized_file_with_413() for real test.
}
```

---

### 10. Add Retry Logic to Upload Limits Fetch ⏱️ 1 hour
**File:** `client/src/lib/tauri.ts:109-151`
**Issue:** Network failures fall back silently to defaults
**Priority:** MEDIUM (Reliability)
**Impact:** Users validate against wrong limits

**Current:**
```typescript
try {
    // ... fetch logic ...
} catch (error) {
    console.error('[Upload Limits] Unexpected error:', error);
    // Falls back to defaults silently
}
```

**Fix:** Add retry logic and structured logging:
```typescript
const MAX_RETRIES = 2;
let attempt = 0;

while (attempt <= MAX_RETRIES) {
    try {
        const response = await fetch(`${serverUrl}/api/config/upload-limits`);

        if (!response.ok && attempt < MAX_RETRIES && response.status >= 500) {
            attempt++;
            await new Promise(resolve => setTimeout(resolve, 1000 * attempt));
            continue;
        }

        // ... rest of logic ...
        return;
    } catch (error) {
        if (attempt < MAX_RETRIES) {
            attempt++;
            await new Promise(resolve => setTimeout(resolve, 1000 * attempt));
            continue;
        }
        console.error('[Upload Limits] All retries failed - using defaults');
        return;
    }
}
```

---

### 11. Improve Multipart Parsing Error Messages ⏱️ 1 hour
**Files:** `server/src/guild/emojis.rs`, `server/src/auth/handlers.rs`, etc.
**Issue:** Generic "Validation error" doesn't distinguish failure types
**Priority:** MEDIUM (Reliability)

**Current:**
```rust
let data = field.bytes().await.map_err(|e| EmojiError::Validation(e.to_string()))?;
```

**Fix:** Create specific error variants:
```rust
#[derive(Error, Debug)]
pub enum EmojiError {
    // ... existing variants ...
    #[error("Failed to read multipart field '{field}': {details}")]
    MultipartField { field: String, details: String },
}

// Usage:
let data = field.bytes().await.map_err(|e| EmojiError::MultipartField {
    field: "file".to_string(),
    details: e.to_string()
})?;
```

---

### 12. Return Error Reasons from Token Refresh ⏱️ 45 min
**File:** `client/src/lib/tauri.ts:259-337`
**Issue:** Users logged out without explanation
**Priority:** MEDIUM (Reliability)

**Current:**
```typescript
export async function refreshAccessToken(): Promise<boolean> {
    // ... returns true/false only
}
```

**Fix:**
```typescript
export type RefreshResult =
  | { success: true }
  | { success: false; reason: 'no_token' | 'network' | 'invalid_token' | 'server_error'; message: string };

export async function refreshAccessToken(): Promise<RefreshResult> {
    if (!browserState.refreshToken) {
        return { success: false, reason: 'no_token', message: 'Not logged in' };
    }

    try {
        const response = await fetch(/* ... */);

        if (!response.ok) {
            const reason = response.status === 401 ? 'invalid_token' : 'server_error';
            const message = response.status === 401
                ? 'Session expired. Please log in again.'
                : `Server error (${response.status}). Please try again.`;
            return { success: false, reason, message };
        }
        return { success: true };
    } catch (error) {
        return { success: false, reason: 'network', message: 'Network error. Check your connection.' };
    }
}
```

---

### 13. Improve Error Response Parsing ⏱️ 1 hour
**Files:** `client/src/lib/tauri.ts` (uploadAvatar, uploadFile, uploadMessageWithFile, uploadGuildEmoji)
**Issue:** Failed JSON parsing loses server error details
**Priority:** MEDIUM (Reliability)

**Apply to all upload functions:**
```typescript
if (!response.ok) {
    let errorMessage = `Upload failed (HTTP ${response.status})`;
    let errorCode: string | undefined;

    try {
        const errorBody = await response.json();
        errorMessage = errorBody.message || errorBody.error || errorMessage;
        errorCode = errorBody.error;
    } catch (parseError) {
        console.warn('[uploadAvatar] Failed to parse error JSON:', parseError);

        try {
            const text = await response.text();
            if (text && text.length > 0 && text.length < 500) {
                errorMessage = text;
            }
        } catch { /* use statusText fallback */ }
    }

    console.error('[uploadAvatar] Upload failed:', {
        status: response.status,
        error: errorMessage,
        errorCode,
    });

    throw new Error(errorMessage);
}
```

---

## Optional Enhancements

### 14. Expand Test Documentation ⏱️ 30 min
**File:** `server/tests/upload_limits_test.rs:345-358`
**Issue:** Validation pattern documentation missing security context

**Add:**
```rust
/// Documents the file size validation pattern used in upload handlers
///
/// VALIDATION ORDER (critical for security):
/// 1. Multipart form parsing extracts file data into memory
/// 2. Size check: if data.len() > max_size { reject with 413 or error }
/// 3. MIME type validation (header + magic bytes for images)
/// 4. Image format detection and processing
///
/// BOUNDARY BEHAVIOR:
/// - Files exactly at limit are accepted (check uses >, not >=)
/// - One byte over the limit is rejected
///
/// ERROR RESPONSES:
/// - auth/handlers.rs: AuthError::Validation with human-readable message
/// - guild/emojis.rs: EmojiError::FileTooLarge with max_size_bytes field
```

---

### 15. Add S3 Cleanup Tracking ⏱️ 2 hours
**File:** `server/src/chat/uploads.rs:509-529`
**Issue:** Background cleanup may not complete on shutdown

**Add cleanup queue table:**
```sql
CREATE TABLE orphaned_s3_objects (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
    s3_key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    cleanup_attempted_at TIMESTAMPTZ,
    cleanup_failed_count INT DEFAULT 0
);

CREATE INDEX idx_orphaned_s3_pending ON orphaned_s3_objects(created_at)
WHERE cleanup_attempted_at IS NULL;
```

**Add to cleanup logic:**
```rust
sqlx::query!(
    "INSERT INTO orphaned_s3_objects (s3_key) VALUES ($1)
     ON CONFLICT (s3_key) DO NOTHING",
    s3_key
).execute(&state.db).await.ok();
```

---

### 16. Add Concurrent Upload Tests ⏱️ 2 hours
**File:** `server/tests/upload_limits_integration_test.rs`
**Issue:** No tests for resource exhaustion scenarios

**Add:**
```rust
#[tokio::test]
async fn test_concurrent_large_uploads_dont_exhaust_memory() {
    let app = create_test_app().await;
    let tokens = create_multiple_users(&app, 10).await;

    let uploads = tokens.iter().map(|token| async {
        upload_avatar(token, vec![0u8; 5 * 1024 * 1024]).await
    });

    let results = futures::future::join_all(uploads).await;
    assert!(results.iter().all(|r| r.is_ok()));
}
```

---

## Implementation Order

### Phase 1: Critical Fixes (2 hours total) - REQUIRED FOR MERGE
1. ✅ Fix emoji error unreachable panic (30 min)
2. ✅ Add S3 deletion logging (30 min)
3. ✅ Add Redis broadcast error tracking (1 hour)
4. ✅ Fix all documentation issues #4-7 (35 min)

**Checkpoint:** Run tests, verify all comments accurate

### Phase 2: Important Improvements (6-7 hours) - RECOMMENDED
5. ✅ Add HTTP integration tests (3-4 hours)
6. ✅ Fix tautological tests (30 min)
7. ✅ Add upload limits retry logic (1 hour)
8. ✅ Improve error handling #11-13 (2.5 hours)

**Checkpoint:** Full test suite passes, review error scenarios manually

### Phase 3: Optional Enhancements (4-5 hours) - NICE TO HAVE
9. ✅ Expand documentation (30 min)
10. ✅ Add S3 cleanup tracking (2 hours)
11. ✅ Add concurrent upload tests (2 hours)

---

## Testing Checklist

After fixes, verify:

- [ ] All tests pass: `cargo test --all-features`
- [ ] Client tests pass: `cd client && bun test`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] Formatting clean: `cargo fmt --check`
- [ ] Manual test: Avatar upload at boundary (5MB exactly)
- [ ] Manual test: Avatar upload over limit (5MB + 1 byte)
- [ ] Manual test: Emoji upload with guild membership
- [ ] Manual test: Unauthenticated upload attempt
- [ ] Manual test: Upload limits endpoint returns correct values
- [ ] Check logs for S3 deletion attempts
- [ ] Check logs for Redis broadcast errors (if Redis unavailable)

---

## Files to Modify Summary

**Server (Rust):**
- `server/src/guild/emojis.rs` - Fix unreachable, add S3 logging, add Redis error ID
- `server/src/auth/handlers.rs` - Add Redis error ID
- `server/src/chat/dm.rs` - Add Redis error IDs (2 locations)
- `server/src/chat/uploads.rs` - Add Redis error ID, improve multipart errors
- `server/src/config.rs` - Fix comment
- `server/tests/upload_limits_test.rs` - Fix comments, mark tautology tests
- `server/tests/upload_limits_integration_test.rs` - NEW FILE

**Client (TypeScript):**
- `client/src/lib/tauri.ts` - Add retry logic, improve error parsing, return reasons

**Constants:**
- Add "REDIS_PUBLISH_FAILED" to error IDs (if using error ID system)

---

## Success Criteria

**Minimum for Merge:**
- ✅ Zero critical issues remaining
- ✅ All tests passing (288+ tests)
- ✅ All comments factually accurate
- ✅ Error handling observable (no silent failures)

**Recommended for Production:**
- ✅ HTTP integration tests covering upload endpoints
- ✅ Error handling improvements (retry, better messages)
- ✅ Frontend-backend limit alignment verified

---

## Notes

- Keep this file in worktree root for easy reference
- Update checkboxes as work progresses
- Re-run PR review after Phase 1 fixes to verify
- Consider breaking Phase 2 into separate PR if too large
- DM feature test (#7) will need revisiting when DMs implemented

**Last Reviewed By:** pr-review-toolkit (4 agents)
**Agent IDs:** a660a1d (code-reviewer), afe19f8 (test-analyzer), ae9c624 (silent-failure-hunter), a88853a (comment-analyzer)
