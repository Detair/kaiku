# Unified File Size Upload Limits — Implementation Plan (v2)

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Standardize file size restrictions across all upload types with explicit, configurable limits. Fix security issue where user avatars have no size validation.

**Architecture:** Add `max_avatar_size` and `max_emoji_size` to server `Config`. Replace hardcoded limits with config values. Add explicit size checks to all upload handlers. Add frontend validation to provide immediate feedback before upload starts.

**Tech Stack:** Rust (server config, handler validation), TypeScript/Solid.js (frontend validation), existing multipart upload infrastructure.

---

## Context

### Current State (Problems)

| Upload Type | Current Limit | Issues |
|-------------|--------------|--------|
| **User Profile Avatars** | ❌ **NONE** | **Security issue** - relies only on axum's DefaultBodyLimit (50MB) |
| **DM Group Avatars** | 50MB (uses `max_upload_size`) | Too large for avatars, should be ~5MB |
| **Guild Emojis** | 256KB (hardcoded) | Not configurable, magic number in code |
| **File Attachments** | 50MB (uses `max_upload_size`) | ✅ Correct |

### Desired State

| Upload Type | New Limit | Config Field |
|-------------|-----------|--------------|
| **User Profile Avatars** | 5MB (default) | `max_avatar_size` |
| **DM Group Avatars** | 5MB (default) | `max_avatar_size` |
| **Guild Emojis** | 256KB (default) | `max_emoji_size` |
| **File Attachments** | 50MB (default) | `max_upload_size` (unchanged) |

### Files with Upload Handlers

| File | Handler | Current Validation |
|------|---------|-------------------|
| `server/src/auth/handlers.rs` | `upload_avatar()` | ❌ No size check |
| `server/src/chat/dm.rs` | `upload_dm_avatar()` | ✅ Uses `max_upload_size` (wrong limit) |
| `server/src/guild/emojis.rs` | `create_emoji()` | ⚠️ Hardcoded 256KB |
| `server/src/chat/uploads.rs` | `upload_file()`, `upload_via_proxy()` | ✅ Uses `max_upload_size` (correct) |

### Existing Infrastructure (DO NOT recreate)

| Component | Location | What it does |
|-----------|----------|--------------|
| `Config` struct | `server/src/config.rs` | Server configuration from env vars |
| `max_upload_size` | `config.rs:54` | Current single file size limit (50MB default) |
| `DefaultBodyLimit` | `server/src/api/mod.rs` | Axum layer that sets max request body size |
| `CHANGELOG.md` | Root directory | keepachangelog.com format changelog |

---

## Task 1: Add Config Fields

**Files:**
- Modify: `server/src/config.rs`
- Modify: `.env.example` (if exists)

### Step 1: Add new fields to Config struct

In `server/src/config.rs`, add two new fields after `max_upload_size` (line ~54):

```rust
/// Maximum file upload size in bytes (default: 50MB)
pub max_upload_size: usize,

/// Maximum avatar size in bytes (user profiles and DM groups, default: 5MB)
pub max_avatar_size: usize,

/// Maximum emoji size in bytes (guild custom emojis, default: 256KB)
pub max_emoji_size: usize,
```

### Step 2: Load from environment in `from_env()`

After the `max_upload_size` parsing (around line 100-110), add:

```rust
max_upload_size: env::var("MAX_UPLOAD_SIZE")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(50 * 1024 * 1024), // 50MB default

max_avatar_size: env::var("MAX_AVATAR_SIZE")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(5 * 1024 * 1024), // 5MB default

max_emoji_size: env::var("MAX_EMOJI_SIZE")
    .ok()
    .and_then(|v| v.parse().ok())
    .unwrap_or(256 * 1024), // 256KB default
```

### Step 3: Update .env.example

Add documentation for new env vars:

```bash
# File upload size limits (in bytes)
# MAX_UPLOAD_SIZE=52428800        # 50MB default for file attachments
# MAX_AVATAR_SIZE=5242880         # 5MB default for user/DM avatars
# MAX_EMOJI_SIZE=262144           # 256KB default for guild emojis

# IMPORTANT: DefaultBodyLimit uses MAX_UPLOAD_SIZE. To allow avatars/emojis
# larger than this, you must increase MAX_UPLOAD_SIZE accordingly.
# Example: For 10MB avatars, set MAX_UPLOAD_SIZE=10485760 and MAX_AVATAR_SIZE=10485760
```

### Step 4: Verify config loads

```bash
cd server && cargo check --all-features
```

Expected: Compiles without errors.

---

## Task 2: Add Size Formatting Helper

**Files:**
- Modify: `server/src/auth/handlers.rs` (or create `server/src/util/format.rs`)

### Step 1: Add helper function

Add this helper function (either in `auth/handlers.rs` at module level, or in a shared utility module):

```rust
/// Format file size in human-readable units
fn format_file_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} bytes", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
```

### Step 2: Verify compilation

```bash
cd server && cargo check
```

Expected: Compiles successfully.

---

## Task 3: Fix User Avatar Upload (SECURITY)

**Files:**
- Modify: `server/src/auth/handlers.rs`

### Step 1: Add size check in `upload_avatar()`

In `server/src/auth/handlers.rs`, after the line `let data = file_data.ok_or(...)?;` (around line 556), add size validation:

```rust
let data = file_data.ok_or(AuthError::Validation("No avatar file provided".to_string()))?;

// SECURITY: Check file size (was missing!)
if data.len() > state.config.max_avatar_size {
    return Err(AuthError::Validation(format!(
        "Avatar file too large ({}). Maximum size is {}",
        format_file_size(data.len()),
        format_file_size(state.config.max_avatar_size)
    )));
}

// Validate mime type from header
let mime = content_type
    .unwrap_or_else(|| "application/octet-stream".to_string());
```

### Step 2: Verify compilation

```bash
cd server && cargo check
```

Expected: Compiles successfully.

### Step 3: Manual test (after full implementation)

Upload test:
1. Try uploading a 6MB avatar → Should reject with error showing "6.0MB" and "5.0MB"
2. Try uploading a 4MB avatar → Should succeed
3. Check error message formatting is user-friendly

---

## Task 4: Fix DM Avatar Upload

**Files:**
- Modify: `server/src/chat/dm.rs`

### Step 1: Find and update size check

Search for the DM avatar upload handler. The size check currently uses `max_upload_size`:

```rust
if data.len() > state.config.max_upload_size {
    return Err(UploadError::TooLarge { max_size: state.config.max_upload_size });
}
```

Replace with:

```rust
if data.len() > state.config.max_avatar_size {
    return Err(UploadError::TooLarge { max_size: state.config.max_avatar_size });
}
```

### Step 2: Verify compilation

```bash
cd server && cargo check
```

Expected: Compiles successfully.

---

## Task 5: Make Emoji Size Configurable

**Files:**
- Modify: `server/src/guild/emojis.rs`

### Step 1: Update EmojiError enum

Add size field to `FileTooLarge` variant:

```rust
#[derive(Debug, thiserror::Error)]
pub enum EmojiError {
    #[error("Guild not found")]
    GuildNotFound,
    #[error("Emoji not found")]
    EmojiNotFound,
    #[error("Insufficient permissions")]
    Forbidden,
    #[error("Invalid filename")]
    InvalidFilename,
    #[error("File too large (maximum {max_size} bytes)")]
    FileTooLarge { max_size: usize },  // CHANGED: Added max_size field
    #[error("Invalid file type (must be PNG, JPEG, GIF, or WebP)")]
    InvalidFileType,
    // ... rest of variants
}
```

### Step 2: Update error response

Update the `into_response()` method to use dynamic size:

```rust
EmojiError::FileTooLarge { max_size } => {
    let message = format!("File too large (max {}KB for emojis)", max_size / 1024);
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(json!({
            "error": "FILE_TOO_LARGE",
            "message": message,
            "max_size_bytes": max_size
        }))
    ).into_response()
}
```

**Note:** This matches the existing error response pattern in `emojis.rs` which uses `Json(json!({...})).into_response()`.

### Step 3: Replace hardcoded limit in create_emoji()

In `server/src/guild/emojis.rs`, find line ~216 with the hardcoded limit:

```rust
if data.len() > 256 * 1024 { // 256KB limit for emojis
     return Err(EmojiError::FileTooLarge);
}
```

Replace with:

```rust
if data.len() > state.config.max_emoji_size {
    return Err(EmojiError::FileTooLarge {
        max_size: state.config.max_emoji_size
    });
}
```

### Step 4: Verify compilation

```bash
cd server && cargo check
```

Expected: Compiles successfully.

---

## Task 6: Frontend Validation - API Wrapper

**Files:**
- Modify: `client/src/lib/tauri.ts` (or wherever API calls are defined)

### Step 1: Add size check helper

Add a helper function near the top of the API module:

```typescript
// File size limits (sync with server config defaults)
// ⚠️ LIMITATION: These are hardcoded and may drift from server config.
// If admin changes MAX_AVATAR_SIZE on server, frontend validation will be out of sync.
// Future enhancement: Fetch these from /api/config endpoint on app startup.
const FILE_SIZE_LIMITS = {
  avatar: 5 * 1024 * 1024,      // 5MB
  emoji: 256 * 1024,             // 256KB
  attachment: 50 * 1024 * 1024,  // 50MB
} as const;

type UploadType = keyof typeof FILE_SIZE_LIMITS;

/**
 * Validate file size on frontend before upload
 * @returns Error message if file is too large, null if valid
 */
function validateFileSize(file: File, type: UploadType): string | null {
  const maxSize = FILE_SIZE_LIMITS[type];
  if (file.size > maxSize) {
    const maxSizeMB = (maxSize / (1024 * 1024)).toFixed(1);
    const fileSizeMB = (file.size / (1024 * 1024)).toFixed(1);
    return `File too large (${fileSizeMB}MB). Maximum size is ${maxSizeMB}MB.`;
  }
  return null;
}

// Export for use in components
export { validateFileSize };
```

### Step 2: Add validation to upload functions

Update the avatar upload function:

```typescript
export async function uploadAvatar(file: File): Promise<UserProfile> {
  // Frontend validation
  const error = validateFileSize(file, 'avatar');
  if (error) {
    throw new Error(error);
  }

  const formData = new FormData();
  formData.append('avatar', file);
  // ... rest of upload logic
}
```

Similarly for emoji uploads:

```typescript
export async function uploadGuildEmoji(guildId: string, name: string, file: File): Promise<GuildEmoji> {
  // Frontend validation
  const error = validateFileSize(file, 'emoji');
  if (error) {
    throw new Error(error);
  }

  const formData = new FormData();
  formData.append('name', name);
  formData.append('file', file);
  // ... rest of upload logic
}
```

And for file attachments (if not already validated):

```typescript
export async function uploadAttachment(channelId: string, file: File): Promise<FileAttachment> {
  // Frontend validation
  const error = validateFileSize(file, 'attachment');
  if (error) {
    throw new Error(error);
  }

  const formData = new FormData();
  formData.append('file', file);
  // ... rest of upload logic
}
```

### Step 3: Verify TypeScript compilation

```bash
cd client && bun run typecheck
```

Expected: No type errors.

---

## Task 7: Frontend Validation - File Input Components

**Files:**
- Find and modify: Avatar upload components
- Find and modify: Emoji upload components
- Find and modify: File attachment components

### Step 1: Find upload components

```bash
cd client/src/components
find . -name "*.tsx" -exec grep -l "input.*type.*file\|FileList\|uploadAvatar\|uploadEmoji" {} \;
```

Expected: List of component files with file uploads.

### Step 2: Add validation on file selection

For each component with file input, add validation in the `onChange` handler.

**Example for Avatar Upload Component:**

```tsx
import { uploadAvatar, validateFileSize } from '@/lib/tauri';
import { createSignal } from 'solid-js';

export function AvatarUpload() {
  const [error, setError] = createSignal<string | null>(null);
  const [uploading, setUploading] = createSignal(false);

  const handleFileSelect = async (event: Event) => {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;

    setError(null);

    // Frontend validation before attempting upload
    const validationError = validateFileSize(file, 'avatar');
    if (validationError) {
      setError(validationError);
      return;
    }

    // Proceed with upload
    try {
      setUploading(true);
      await uploadAvatar(file);
      // Success handling
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Upload failed');
    } finally {
      setUploading(false);
    }
  };

  return (
    <div>
      <input
        type="file"
        accept="image/*"
        onChange={handleFileSelect}
        disabled={uploading()}
      />
      <span class="text-xs text-text-secondary">
        Maximum size: 5MB
      </span>
      {error() && (
        <span class="text-xs text-danger-text">
          {error()}
        </span>
      )}
    </div>
  );
}
```

### Step 3: Update UI to show limits

Add helper text showing size limits for all upload inputs:

```tsx
// User avatars
<span class="text-xs text-text-secondary">Maximum size: 5MB</span>

// Guild emojis
<span class="text-xs text-text-secondary">Maximum size: 256KB</span>

// File attachments
<span class="text-xs text-text-secondary">Maximum size: 50MB</span>
```

### Step 4: Verify manually

Test in browser:
1. Select a file larger than the limit → Should show error immediately (no network request)
2. Select a valid file → Should proceed with upload
3. Error message is user-friendly and shows both file size and limit

---

## Task 8: Integration Testing

**Files:**
- Create: `server/tests/upload_limits_test.rs` (if integration test directory exists)

### Step 1: Check existing test infrastructure

```bash
ls -la server/tests/
```

If tests directory doesn't exist or test utilities are missing, adapt the test code to match existing patterns.

### Step 2: Test user avatar upload limits

```rust
#[cfg(test)]
mod upload_limits_tests {
    use super::*;

    // Note: Adjust this based on your actual test setup
    fn test_config() -> Config {
        Config {
            bind_address: "0.0.0.0:8080".into(),
            database_url: "postgresql://test".into(),
            // ... other required fields with test values
            max_upload_size: 50 * 1024 * 1024,
            max_avatar_size: 1024, // 1KB for testing
            max_emoji_size: 512,   // 512B for testing
            // ... rest of fields
        }
    }

    #[tokio::test]
    async fn test_avatar_size_limit_exceeded() {
        let config = test_config();

        // Create 2KB file (exceeds 1KB limit)
        let large_file = vec![0u8; 2048];

        // Attempt upload (adjust based on your test setup)
        let response = test_upload_avatar(&config, large_file).await;

        // Should reject
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.text().await.unwrap();
        assert!(body.contains("too large"));
    }

    #[tokio::test]
    async fn test_avatar_within_limit() {
        let config = test_config();

        // Create 512B file (within 1KB limit)
        let valid_file = vec![0u8; 512];

        let response = test_upload_avatar(&config, valid_file).await;

        // Should succeed
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_zero_byte_avatar() {
        let config = test_config();
        let empty_file = vec![];

        let response = test_upload_avatar(&config, empty_file).await;

        // Define expected behavior - probably should fail for different reason
        // (not a valid image format)
        assert!(response.status().is_client_error());
    }
}
```

### Step 3: Test emoji size limits

```rust
#[tokio::test]
async fn test_emoji_size_limit_exceeded() {
    let config = test_config();

    // Create 1KB file (exceeds 512B limit)
    let large_emoji = vec![0u8; 1024];

    let response = test_upload_emoji(&config, large_emoji).await;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = response.json().await.unwrap();
    assert_eq!(body["error"], "FILE_TOO_LARGE");
}

#[tokio::test]
async fn test_emoji_within_limit() {
    let config = test_config();
    let valid_emoji = vec![0u8; 256];

    let response = test_upload_emoji(&config, valid_emoji).await;

    assert_eq!(response.status(), StatusCode::OK);
}
```

### Step 4: Test DM avatar limits

```rust
#[tokio::test]
async fn test_dm_avatar_uses_avatar_limit() {
    let config = test_config();

    // Should use max_avatar_size (1KB), not max_upload_size (50MB)
    let file_2kb = vec![0u8; 2048];

    let response = test_upload_dm_avatar(&config, file_2kb).await;

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
```

### Step 5: Run tests

```bash
cd server && cargo test upload_limits
```

Expected: All tests pass.

---

## Task 9: Frontend Unit Tests (Optional but Recommended)

**Files:**
- Create: `client/src/lib/__tests__/tauri.test.ts` (adjust path to your test setup)

### Step 1: Add unit tests for validateFileSize

```typescript
import { describe, it, expect } from 'vitest'; // or your test framework
import { validateFileSize } from '../tauri';

describe('validateFileSize', () => {
  it('rejects avatar files larger than 5MB', () => {
    const largeFile = new File(
      [new ArrayBuffer(6 * 1024 * 1024)],
      'large.jpg',
      { type: 'image/jpeg' }
    );
    const error = validateFileSize(largeFile, 'avatar');
    expect(error).toBeTruthy();
    expect(error).toContain('6.0MB');
    expect(error).toContain('5.0MB');
  });

  it('accepts avatar files within 5MB limit', () => {
    const smallFile = new File(
      [new ArrayBuffer(4 * 1024 * 1024)],
      'small.jpg',
      { type: 'image/jpeg' }
    );
    const error = validateFileSize(smallFile, 'avatar');
    expect(error).toBeNull();
  });

  it('rejects emoji files larger than 256KB', () => {
    const largeEmoji = new File(
      [new ArrayBuffer(300 * 1024)],
      'large.gif',
      { type: 'image/gif' }
    );
    const error = validateFileSize(largeEmoji, 'emoji');
    expect(error).toBeTruthy();
  });

  it('accepts emoji files within 256KB limit', () => {
    const smallEmoji = new File(
      [new ArrayBuffer(200 * 1024)],
      'small.gif',
      { type: 'image/gif' }
    );
    const error = validateFileSize(smallEmoji, 'emoji');
    expect(error).toBeNull();
  });
});
```

### Step 2: Run frontend tests

```bash
cd client && bun test
```

Expected: All tests pass.

---

## Task 10: Documentation

**Files:**
- Modify: `README.md` (or `docs/deployment.md`)
- Modify: `docs/project/roadmap.md` (mark as complete)
- Modify: `CHANGELOG.md` (document changes)

### Step 1: Document environment variables

Add to deployment documentation (README.md or docs/deployment.md):

```markdown
## File Upload Configuration

The server supports configurable file size limits:

| Variable | Default | Purpose |
|----------|---------|---------|
| `MAX_UPLOAD_SIZE` | 50MB (52428800 bytes) | File attachments (messages) |
| `MAX_AVATAR_SIZE` | 5MB (5242880 bytes) | User profile and DM group avatars |
| `MAX_EMOJI_SIZE` | 256KB (262144 bytes) | Guild custom emojis |

Example `.env` configuration:

```bash
MAX_UPLOAD_SIZE=52428800    # 50MB in bytes
MAX_AVATAR_SIZE=5242880     # 5MB in bytes
MAX_EMOJI_SIZE=262144       # 256KB in bytes
```

**Important Notes:**
- All values must be in bytes
- The `DefaultBodyLimit` middleware uses `MAX_UPLOAD_SIZE`. To allow avatars or emojis larger than 50MB, you must also increase `MAX_UPLOAD_SIZE`
- Frontend validation uses hardcoded limits that match these defaults. If you change server limits, users may see confusing errors until a future update implements dynamic limit fetching
```

### Step 2: Update CHANGELOG.md

In `CHANGELOG.md`, add under `[Unreleased]`:

```markdown
## [Unreleased]

### Added
- Configurable file size limits via environment variables:
  - `MAX_AVATAR_SIZE` (default: 5MB) for user profile and DM group avatars
  - `MAX_EMOJI_SIZE` (default: 256KB) for guild custom emojis
- Frontend validation provides immediate feedback before upload attempts
- Human-readable error messages showing both file size and limit

### Changed
- DM group avatar uploads now use 5MB limit (down from 50MB)
- Guild emoji size limit now configurable via `MAX_EMOJI_SIZE` environment variable
- Error messages for file size violations now include dynamic size information

### Security
- **Fixed:** User profile avatar uploads now have explicit size validation (previously had no limit)

### Fixed
- Emoji upload error messages now show the actual configured limit instead of hardcoded value
```

### Step 3: Update roadmap

In `docs/project/roadmap.md`, change:

```markdown
- [ ] **[Media] Unified File Size Upload Limits**
```

To:

```markdown
- [x] **[Media] Unified File Size Upload Limits** ✅
```

---

## Verification Plan

### Manual Testing Checklist

- [ ] **User Avatar Upload**
  - [ ] Upload 6MB image → Should fail with clear error showing "6.0MB" and "5.0MB"
  - [ ] Upload 4MB image → Should succeed
  - [ ] Error message is user-friendly
  - [ ] Frontend validation shows error before upload starts (no network request in devtools)

- [ ] **DM Group Avatar Upload**
  - [ ] Upload 6MB image → Should fail
  - [ ] Upload 3MB image → Should succeed
  - [ ] Verify uses avatar limit, not upload limit

- [ ] **Guild Emoji Upload**
  - [ ] Upload 300KB file → Should fail with "File too large (max 256KB)"
  - [ ] Upload 200KB file → Should succeed
  - [ ] Error message includes actual limit

- [ ] **File Attachments (unchanged)**
  - [ ] Upload 51MB file → Should fail
  - [ ] Upload 40MB file → Should succeed

- [ ] **Frontend Validation**
  - [ ] Select oversized file → Immediate error (no network request)
  - [ ] Error message is user-friendly
  - [ ] UI shows size limits in file input help text

- [ ] **Edge Cases**
  - [ ] Upload 0-byte file → Appropriate error (invalid image format)
  - [ ] Upload file at exactly the limit → Should succeed
  - [ ] Upload file 1 byte over limit → Should fail

### Backend Verification

```bash
# Run all tests
cd server && cargo test --all-features

# Check specific upload handlers
cargo test upload

# Verify config loads
cargo run --bin server # Check startup logs for config values
```

### Frontend Verification

```bash
# Type checking
cd client && bun run typecheck

# Run tests
bun test

# Build
bun run build

# Run in dev mode and test uploads manually
bun run dev
```

### Configuration Testing

Test different config values:

```bash
# Test custom limits
MAX_AVATAR_SIZE=10485760 cargo run  # 10MB avatars
MAX_EMOJI_SIZE=524288 cargo run     # 512KB emojis

# Test edge case: avatar larger than DefaultBodyLimit
MAX_UPLOAD_SIZE=52428800 MAX_AVATAR_SIZE=104857600 cargo run  # Should fail
```

---

## Success Criteria

- [x] **Security:** User avatars now have explicit size validation
- [x] **Consistency:** All upload types use appropriate, configurable limits
- [x] **Configuration:** New env vars `MAX_AVATAR_SIZE` and `MAX_EMOJI_SIZE` work correctly
- [x] **Frontend:** Users get immediate feedback before upload starts
- [x] **Error Messages:** Clear, user-friendly error messages with dynamic size information
- [x] **Testing:** Integration tests cover all upload limit scenarios
- [x] **Documentation:** Deployment docs explain new configuration options
- [x] **CHANGELOG:** All changes documented in keepachangelog.com format
- [x] **No Breaking Changes:** Existing uploads continue to work with default values

---

## Known Limitations

### 1. Frontend/Backend Limit Sync

**Issue:** Frontend has hardcoded limits that will drift from backend config if admins change server environment variables.

**Example:** Admin sets `MAX_AVATAR_SIZE=10485760` (10MB) in production. Frontend still validates at 5MB. Users see "file too large" error on client, but server would actually accept it.

**Impact:** Confusing user experience when server limits are changed.

**Workaround:** Update frontend hardcoded limits when changing server config.

**Future Enhancement:** Implement `/api/config` endpoint that returns current server limits, fetch on app startup.

### 2. DefaultBodyLimit Interaction

**Issue:** Axum's `DefaultBodyLimit` is set to `max_upload_size` (50MB default). Avatars/emojis larger than this will fail at the body limit layer before reaching handler validation.

**Impact:** To allow >50MB avatars, you must increase both `MAX_UPLOAD_SIZE` and `MAX_AVATAR_SIZE`.

**Documentation:** This is documented in .env.example and deployment docs.

---

## Rollout Notes

### Deployment Considerations

1. **Backwards Compatible:** Default values match current behavior (except user avatars, which had no limit)
2. **No Migration:** No database changes required
3. **Environment Variables:** Optional - sensible defaults provided
4. **Restart Required:** Server restart needed to pick up new env var values
5. **Frontend Update:** Deploy frontend and backend together to ensure error messages are consistent

### Monitoring

After deployment, monitor:
- Upload rejection rates (expect increase for user avatars if people were uploading large files)
- Error logs for "too large" errors
- User feedback about error messages
- P99 latency for upload endpoints (should not change)

### Rollback Plan

If issues arise:
1. No code rollback needed - just adjust env vars to higher limits
2. Or restart server without new env vars (uses defaults)
3. Frontend continues to work (just validates at hardcoded limits)

---

## Files Modified Summary

### Server (4 files)
- `server/src/config.rs` - Add `max_avatar_size`, `max_emoji_size` fields
- `server/src/auth/handlers.rs` - Add size check to `upload_avatar()`, add `format_file_size()` helper
- `server/src/chat/dm.rs` - Change `max_upload_size` to `max_avatar_size`
- `server/src/guild/emojis.rs` - Replace hardcoded limit with `max_emoji_size`, add size to error

### Client (2+ files)
- `client/src/lib/tauri.ts` - Add `validateFileSize()` helper and validation to upload functions
- `client/src/components/**/[upload components].tsx` - Add frontend validation on file select

### Documentation (4 files)
- `.env.example` - Document new env vars with important notes
- `README.md` or `docs/deployment.md` - Document configuration and limitations
- `docs/project/roadmap.md` - Mark task as complete
- `CHANGELOG.md` - Document all changes in keepachangelog.com format

### Tests (2 files, optional but recommended)
- `server/tests/upload_limits_test.rs` - Backend integration tests
- `client/src/lib/__tests__/tauri.test.ts` - Frontend unit tests

---

## Estimated Time

- **Task 1 (Config):** 15 minutes
- **Task 2 (Size Formatter):** 5 minutes
- **Task 3 (User Avatar):** 10 minutes
- **Task 4 (DM Avatar):** 5 minutes
- **Task 5 (Emoji):** 15 minutes (error enum refactor)
- **Task 6 (Frontend API):** 20 minutes
- **Task 7 (Frontend UI):** 30 minutes
- **Task 8 (Backend Tests):** 30 minutes
- **Task 9 (Frontend Tests):** 15 minutes
- **Task 10 (Documentation):** 20 minutes

**Total:** ~2.5 hours for full implementation with testing and documentation

---

## Dependencies

- None (all infrastructure exists)

## Blockers

- None

## Related Features

- Future: Fetch size limits from `/api/config` endpoint (fixes frontend/backend sync)
- Future: Add image compression/resizing for avatars to help users stay under limits
- Future: Add upload progress indicators for large files
- Future: Consider adding per-guild emoji limits (e.g., max 100 emojis per guild)
- Future: Add automatic image optimization (compress before upload)
