<!-- Parent: ../../AGENTS.md -->

# Chat Module

Text messaging with end-to-end encryption (E2EE) using vodozemac (Olm/Megolm), file uploads to S3-compatible storage.

## Purpose

- Channel-based text messaging (guild channels + DMs)
- Message CRUD (create, list, edit, delete)
- File attachments with S3 storage
- Direct message (DM) channel management
- E2EE metadata support (future: actual encryption in client)

## Key Files

- `mod.rs` — Router setup for channels, messages, DM endpoints
- `channels.rs` — Channel CRUD handlers (list, create, update, delete, member management)
- `messages.rs` — Message handlers (list, create, edit, delete)
- `dm.rs` — DM channel creation and management
- `uploads.rs` — File upload/download handlers with multipart form support
- `send.rs` — Shared send path: `authorize_send` (access, permissions, blocks, mentions, content filter) and `dispatch_new_message` (broadcast, crosspost, push wake via `push::spawn_wake_for_new_message`, bot events) used by both text messages and uploads
- `mentions.rs` — C21 @mention tokenizer (`extract_mentions`) and per-user push targets (`resolve_mention_targets`)
- `s3.rs` — S3Client wrapper for object storage (AWS S3, RustFS, etc.)

## For AI Agents

### Message Architecture

**Channel Types**:
- Guild text channels (type: `text`, linked to guild via `guild_id`)
- DM channels (type: `dm`, `guild_id` is NULL, linked via `dm_participants` table)
- Guild voice channels (type: `voice`, messages not supported yet)

**Message Model** (in `db::models`):
```rust
{
    id: Uuid,
    channel_id: Uuid,
    author_id: Uuid,
    content: String,
    created_at: DateTime,
    updated_at: Option<DateTime>,
    // E2EE fields (currently unused, client-side encryption planned)
    is_encrypted: bool,
    encryption_version: Option<String>,
}
```

**Soft Deletes**: Messages set `deleted_at` timestamp instead of hard delete. Content replaced with `[deleted]` in responses.

### Author Shape and Presence

`AuthorProfile` (`types.rs`) is `{ id, username, display_name, avatar_url, status }`. `status` is a wire-compat placeholder that is always `"offline"` (`AUTHOR_STATUS_PLACEHOLDER`); message, pin, thread and bot-gateway payloads never carry live presence. Live presence reaches clients only through the WebSocket presence store (`presence_update` events to the presence audience). Do not populate `status` from `users.status`.

### File Upload Flow

**Storage Options**:
1. S3-compatible (AWS S3, RustFS, DigitalOcean Spaces) if `AppState.s3.is_some()`
2. Fallback: Return error if S3 not configured (local filesystem not supported)

**Upload Endpoint**: `POST /api/messages/upload`
- Multipart form: `file` field with binary data
- Returns `{ "url": "https://bucket.s3.region.amazonaws.com/path", "file_id": "uuid" }`
- Stores metadata in `message_attachments` table

**Combined Upload**: `POST /api/messages/channel/:channel_id/upload`
- Create message + upload file in single request
- Multipart form: `file` + `content` (optional text)
- Order: parse multipart → size/MIME/magic-byte validation → `send::authorize_send`
  (same rules as text, plus `ATTACH_FILES`) → S3 configured (503) → upload
  variants + original under `attachments/{channel}/{message_id}/{file_id}.{ext}`
  → one transaction inserting message (pre-chosen UUIDv7) + attachment →
  `send::dispatch_new_message`
- Atomic: a storage failure leaves no message row; a DB failure deletes the
  uploaded objects
- New send paths must go through `send::authorize_send` and
  `send::dispatch_new_message`; do not duplicate the checks

**Download Endpoint**: `GET /api/messages/attachments/:id/download`
- Public route with auth via `?token=jwt` query parameter (browser compatibility)
- Generates signed S3 URL (presigned URL with 1-hour expiry)
- Returns redirect to S3 URL or proxied file content

**Size Limits**: Controlled by `AppState` body limit (default 50MB). Adjust `max_upload_size` in config.

**Security Considerations**:
- Validate file type (future: restrict to images, videos, documents)
- Scan for malware (future: integrate with ClamAV or similar)
- Check user has permission to upload to channel
- S3 bucket should be private (no public-read ACL)
- Use presigned URLs with short expiry (1 hour)

### S3 Configuration

**Required Environment Variables**:
```bash
S3_ENDPOINT=https://s3.us-east-1.amazonaws.com
S3_REGION=us-east-1
S3_BUCKET=voicechat-uploads
S3_ACCESS_KEY=AKIA...
S3_SECRET_KEY=...
S3_PUBLIC_URL=https://cdn.example.com  # Optional CDN
```

**S3Client Methods**:
- `upload(key, data, content_type)` — Upload bytes to S3
- `get_presigned_url(key, expiry)` — Generate download URL
- `delete(key)` — Remove object (for message deletion)

**Path Convention**: `{channel_id}/{message_id}/{filename}` (deterministic, avoids collisions)

### E2EE Integration (Future)

**Current State**: Messages stored in plaintext. `is_encrypted` always `false`.

**Planned** (vodozemac):
- Client generates Olm identity keys
- Olm 1:1 for DMs, Megolm for group channels
- Server stores encrypted content (opaque bytes)
- Server never has decryption keys
- Client handles encryption/decryption

**Agent Guidance**: When implementing E2EE:
1. Update `Message` model to store `Vec<u8>` instead of `String` for `content`
2. Add key distribution endpoints (clients share public keys)
3. Server blindly forwards encrypted payloads
4. Verify E2EE changes with Faramir (ensure no key leakage)

### DM Channel Logic

**Creation**:
- `POST /api/dm` with `{ "recipient_id": "uuid" }`
- Checks if DM already exists between users (via `dm_participants` table)
- Creates new channel (type: `dm`) if not exists
- Adds both users to `dm_participants`

**Listing**: `GET /api/dm` returns all DM channels for current user (joins `dm_participants`)

**Leave DM**: `POST /api/dm/:id/leave` removes user from `dm_participants` (other user retains access)

**Read Receipts**: `POST /api/dm/:id/read` updates `last_read_message_id` in `dm_participants` (future feature)

### Mentions (C21)

**Syntax**: `@username` counts only when the `@` starts the text or follows whitespace or one of `( [ { < " ' ,`; the username is `[A-Za-z0-9_]{3,32}` (lowercased) and is not glued to a further letter/digit. Fenced and inline code spans are stripped first, so e-mail addresses and code never mention anyone. `@everyone`/`@here` follow the same boundary rule.

**Rules**:
- `extract_mentions` is the only tokenizer: `detect_mention_type` (message-level `mention_type`) and the `MENTION_EVERYONE` check in `authorize_send` both use it. Do not add `contains("@...")` checks.
- `MentionSet.usernames` is deduplicated and capped at `MAX_MENTIONED_USERNAMES` (20).
- `resolve_mention_targets` returns push targets for guild channels only: author dropped, every target must pass `permissions::require_channel_access`, block in either direction (one batched Postgres `friendships` query, the source of truth; the Redis block cache is not consulted) or a failed block lookup drops the target. `@everyone` loads at most 101 members; the result is capped at `MENTION_EVERYONE_PUSH_CAP` (100). `@here` never pushes.

### Message Editing

**Rules**:
- Only author can edit (enforce in handler)
- Sets `updated_at` timestamp
- Broadcasts `MessageEdit` event via WebSocket (see `ws::ServerEvent`)
- No edit history (future: store revisions in separate table)

**Permissions**: Check `author_id == current_user_id` before allowing edit/delete.

### Pagination

**List Messages**: `GET /api/messages/channel/:channel_id?before={message_id}&limit=50`
- Default limit: 50 messages
- `before` parameter for cursor-based pagination (returns messages older than cursor)
- Ordered by `created_at DESC` (newest first)

**Performance**: Index on `(channel_id, created_at)` in PostgreSQL for fast lookups.

### WebSocket Integration

**Real-time Events** (broadcasted via Redis pub/sub):
- `MessageNew` — New message created
- `MessageEdit` — Message edited
- `MessageDelete` — Message deleted

**Flow**:
1. Handler creates message in DB
2. Calls `ws::broadcast_to_channel()` with `ServerEvent::MessageNew`
3. WebSocket module publishes to Redis `channel:{channel_id}`
4. All connected clients subscribed to channel receive event

See `ws/mod.rs` for event definitions.

### Testing

**Required Tests**:
- [ ] Create message in channel user has access to
- [ ] Reject message if user not member of channel
- [ ] Edit message as author (success)
- [ ] Edit message as non-author (403 Forbidden)
- [ ] Delete message (soft delete, content replaced)
- [ ] Upload file, verify metadata saved
- [ ] Download file with valid token
- [ ] Pagination (before cursor, limit)

### Common Pitfalls

**DO NOT**:
- Return full message content in list responses without pagination (OOM on large channels)
- Allow editing/deleting messages by non-authors (security bug)
- Store files on local filesystem (breaks horizontal scaling)
- Use sequential IDs (use UUIDv7 for time-ordered IDs)

**DO**:
- Validate channel membership before message operations
- Broadcast WebSocket events after DB commit (not before)
- Use transactions for combined operations (message + attachment)
- Clean up S3 objects when messages are permanently deleted
