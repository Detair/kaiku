# Phase 5: Ecosystem & SaaS Readiness — Sonnet Agent Implementation Manual

**Lifecycle:** Active
**Purpose:** Step-by-step instructions for a Claude Sonnet agent to implement all Phase 5 features.
**Date:** 2026-01-28
**Supersedes:** `PHASE_5_IMPLEMENTATION.md` (outdated)

---

## Table of Contents

### Foundation Tier (do first — other features depend on these)
1. [Absolute User Blocking](#1-absolute-user-blocking)
2. [Multi-line Input & Persistent Drafts](#2-multi-line-input--persistent-drafts)
3. [Message Threads](#3-message-threads)

### Safety & Trust Tier
4. [Advanced Moderation & Safety Filters](#4-advanced-moderation--safety-filters)
5. [User Reporting & Workflow](#5-user-reporting--workflow)
6. [SaaS Trust & Data Governance (GDPR)](#6-saas-trust--data-governance-gdpr)

### UX Polish Tier
7. [Production-Scale Polish (Virtual Lists + Toasts)](#7-production-scale-polish)
8. [Quick Message Actions & Reactions Toolbar](#8-quick-message-actions--reactions-toolbar)
9. [Smart Input Auto-complete](#9-smart-input-auto-complete)
10. [Advanced Search & Bulk Read](#10-advanced-search--bulk-read)

### Ecosystem Tier
11. [Bot Ecosystem & Gateway](#11-bot-ecosystem--gateway)
12. [Webhooks](#12-webhooks)
13. [SaaS Limits & Monetization Logic](#13-saas-limits--monetization-logic)

### Infrastructure Tier
14. [SaaS Scaling (Signed URLs + CDN)](#14-saas-scaling-signed-urls--cdn)
15. [Multi-Stream Video Support](#15-multi-stream-video-support)
16. [Advanced Media Processing](#16-advanced-media-processing)

### Growth Tier
17. [Guild Discovery & Onboarding](#17-guild-discovery--onboarding)

---

## General Instructions for Sonnet Agent

- **Read CLAUDE.md first** — code style, commit conventions, changelog rules, license constraints.
- **Branch naming:** `feature/<name>` or `fix/<name>`.
- **Commit format:** `type(scope): subject` (max 72 chars, imperative mood).
- **Always update CHANGELOG.md** under `[Unreleased]`.
- **Run `cargo test`** (server) and `bun run build` (client) before committing.
- **Run `cargo fmt --check && cargo clippy -- -D warnings`** before pushing.
- **License check:** `cargo deny check licenses` after adding any new crate.
- **No GPL/AGPL/LGPL dependencies.**

### Codebase Quick Reference

| Layer | Key Files |
|-------|-----------|
| Server routes | `server/src/api/mod.rs` |
| Server handlers | `server/src/chat/messages.rs`, `server/src/guild/mod.rs` |
| DB queries | `server/src/db/queries.rs` |
| Migrations | `server/migrations/YYYYMMDDHHMMSS_description.sql` |
| WebSocket | `server/src/ws/mod.rs` — events, pub/sub channels, broadcasting |
| Permissions | `server/src/permissions/` — permission bits, middleware |
| Admin | `server/src/admin/` — two-tier admin (normal + elevated) |
| Social | `server/src/social/friends.rs` — friendships table (pending/accepted/blocked) |
| Client components | `client/src/components/<domain>/` |
| Client stores | `client/src/stores/<domain>.ts` — Solid.js signals/createStore |
| Message input | `client/src/components/messages/MessageInput.tsx` |
| Message list | `client/src/components/messages/MessageList.tsx` |
| Message item | `client/src/components/messages/MessageItem.tsx` |

### Existing Infrastructure to Build On

- **WebSocket events:** `ClientEvent` / `ServerEvent` enums in `server/src/ws/mod.rs`. Redis pub/sub channels: `channel:{id}`, `user:{id}`, `presence:{id}`, `guild:{id}`, `admin:events`.
- **File uploads:** S3-based, proxy download via `GET /api/messages/attachments/:id/download`. Presigned URL method exists but unused.
- **Search:** PostgreSQL full-text search with `content_search tsvector` column + GIN index. Guild-scoped only via `GET /api/guilds/:id/search`.
- **Blocking:** Uses `friendships` table with `status = 'blocked'`. Currently only prevents friend requests — does NOT block messages or events.
- **Messages:** `reply_to` field exists in DB (self-referencing FK). Used for replies but no threaded UI.
- **Admin:** Two-tier (normal + elevated via MFA). Audit log, ban/suspend, CSV export.

---

## 1. Absolute User Blocking

**Goal:** When user A blocks user B, B's messages, typing indicators, voice events, and presence are completely invisible to A — both in DMs and shared guilds.

### Why First
Other safety features (moderation, reporting) assume blocking works properly. Currently blocking only prevents friend requests.

### Prerequisites
- Read `server/src/social/friends.rs` — current blocking flow.
- Read `server/src/ws/mod.rs` — event broadcasting.
- Read `server/src/chat/messages.rs` — message delivery.
- Read `client/src/stores/websocket.ts` — client event handling.

### Step-by-Step

#### Step 1: Server — Block Check Utility

1. Add to `server/src/db/queries.rs`:
   ```rust
   /// Returns true if blocker_id has blocked blocked_id
   pub async fn is_user_blocked(pool: &PgPool, blocker_id: Uuid, blocked_id: Uuid) -> sqlx::Result<bool>

   /// Returns set of user IDs that a user has blocked (for batch filtering)
   pub async fn get_blocked_user_ids(pool: &PgPool, user_id: Uuid) -> sqlx::Result<HashSet<Uuid>>
   ```
2. Query the `friendships` table where `(requester_id = blocker_id AND addressee_id = blocked_id AND status = 'blocked')`.

#### Step 2: Server — Filter Messages from Blocked Users

1. In `server/src/chat/messages.rs`, `list` handler:
   - After fetching messages, fetch `get_blocked_user_ids(pool, requesting_user_id)`.
   - Filter out messages where `message.user_id` is in the blocked set.
   - **Do NOT modify the SQL query** — filter in application code so message IDs remain consistent for cursors.
2. In `server/src/guild/search.rs`:
   - Same approach — filter search results post-query.

#### Step 3: Server — Filter WebSocket Events

1. In `server/src/ws/mod.rs`, before forwarding events to a client:
   - For `MessageNew`, `TypingStart`, `TypingStop`, `ReactionAdd`, `ReactionRemove`: check if the event's `user_id` is blocked by the receiving user.
   - For `PresenceUpdate`, `RichPresenceUpdate`: check block relationship.
   - For `VoiceUserJoined`, `VoiceUserLeft`, `VoiceUserMuted`, `VoiceUserUnmuted`, `VoiceUserStats`: check block relationship.
   - **Implementation:** Maintain a per-connection `blocked_ids: HashSet<Uuid>` loaded on connect and updated on block/unblock.
2. Add `BlockUpdated` WebSocket event so clients can update their local block list without reconnecting.

#### Step 4: Server — Prevent Blocked User Actions

1. In DM creation: prevent creating a DM with a user who has blocked you.
2. In friend request: already handled (verify).
3. In voice: don't prevent joining same channel, but hide presence of blocked users.

#### Step 5: Client — Filter Blocked Users in UI

1. In `client/src/stores/websocket.ts`:
   - Maintain `blockedUserIds` signal.
   - Filter incoming events against block list.
2. In `client/src/components/messages/MessageList.tsx`:
   - Filter out messages from blocked users (defense in depth — server should already filter).
3. In member list components:
   - Show blocked users with indicator or hide entirely (decide: hide is more consistent).

#### Step 6: Tests

1. Test blocked user's messages don't appear in list.
2. Test blocked user's typing events don't propagate.
3. Test blocked user's presence is hidden.
4. Test bidirectional: A blocks B, B doesn't see A either? (decide: only one-directional per Discord convention).
5. Test DM creation with blocked user fails.
6. Test unblock restores visibility.

#### Changelog Entry
```markdown
### Added
- Absolute user blocking: blocked users' messages, typing, presence, and voice events are fully hidden
```

---

## 2. Multi-line Input & Persistent Drafts

**Goal:** Replace single-line input with auto-expanding textarea. Save unsent messages when switching channels.

### Why Early
Almost every Phase 5 UX feature builds on the message input. Upgrade it first.

### Prerequisites
- Read `client/src/components/messages/MessageInput.tsx` — current input component.
- Read `client/src/stores/messages.ts` — message state management.

### Step-by-Step

#### Step 1: Multi-line Textarea

1. In `MessageInput.tsx`:
   - Replace `<input type="text">` with `<textarea>`.
   - Auto-expand height based on content (up to max ~200px, then scroll).
   - **Enter** = send message. **Shift+Enter** = new line.
   - CSS: `resize: none`, smooth height transitions.
   ```typescript
   function autoResize(el: HTMLTextAreaElement) {
     el.style.height = 'auto';
     el.style.height = Math.min(el.scrollHeight, 200) + 'px';
   }
   ```
2. Ensure all existing functionality still works: typing indicators, file drag-and-drop, emoji insertion.

#### Step 2: Persistent Drafts

1. In `client/src/stores/messages.ts` (or new `client/src/stores/drafts.ts`):
   ```typescript
   // Map channel_id -> draft text
   const [drafts, setDrafts] = createStore<Record<string, string>>({});

   export function getDraft(channelId: string): string
   export function setDraft(channelId: string, content: string): void
   export function clearDraft(channelId: string): void
   ```
2. Store drafts in memory (not persisted to disk — they're ephemeral).
3. In `MessageInput.tsx`:
   - On mount: load draft for current channel.
   - On content change: save draft.
   - On send: clear draft.
   - On channel switch (unmount): draft is already saved from onChange.

#### Step 3: Tests

1. Test Shift+Enter creates newline.
2. Test Enter sends message.
3. Test textarea auto-expands and shrinks.
4. Test draft persists across channel switches.
5. Test draft clears after sending.

#### Changelog Entry
```markdown
### Added
- Multi-line message input with auto-expanding textarea (Shift+Enter for new lines)
- Persistent message drafts preserved when switching channels
```

---

## 3. Message Threads ✅ COMPLETED

**Status:** Fully implemented. See `feature/thread-avatars-unread` PR #184. Includes DB migration (`parent_id`, `thread_reply_count`, `thread_last_reply_at`, `thread_read_state`), thread reply CRUD, WebSocket events, `ThreadSidebar`/`ThreadIndicator` components, batch thread info with participant avatars and unread indicators, 11+ integration tests. Remaining: guild-level toggle.

**Goal:** Slack-style side threads for organized discussions. Uses existing `reply_to` column.

### Prerequisites
- Read `server/src/chat/messages.rs` — message handlers.
- Read `server/src/db/queries.rs` — message queries. Note `reply_to UUID` already exists.
- Read `client/src/components/messages/MessageItem.tsx` — current reply rendering.
- Read `client/src/components/messages/MessageList.tsx`.

### Step-by-Step

#### Step 1: Database Schema Extension

1. Create migration: `server/migrations/YYYYMMDDHHMMSS_message_threads.sql`
   ```sql
   -- Add thread metadata to messages
   ALTER TABLE messages ADD COLUMN IF NOT EXISTS thread_reply_count INTEGER DEFAULT 0;
   ALTER TABLE messages ADD COLUMN IF NOT EXISTS thread_last_reply_at TIMESTAMPTZ;

   -- Index for fetching thread replies efficiently
   CREATE INDEX IF NOT EXISTS idx_messages_reply_to ON messages(reply_to, created_at)
       WHERE reply_to IS NOT NULL AND deleted_at IS NULL;
   ```
   - `thread_reply_count` and `thread_last_reply_at` are denormalized on the parent message for display.

#### Step 2: Server — Thread Query Functions

1. Add to `server/src/db/queries.rs`:
   ```rust
   /// Fetch replies to a message (thread view), cursor-paginated
   pub async fn list_thread_replies(
       pool: &PgPool,
       parent_id: Uuid,
       before: Option<(DateTime<Utc>, Uuid)>,
       limit: i64,
   ) -> sqlx::Result<Vec<Message>>

   /// Update parent message thread metadata after reply
   pub async fn increment_thread_reply_count(pool: &PgPool, parent_id: Uuid) -> sqlx::Result<()>

   /// Decrement on reply delete
   pub async fn decrement_thread_reply_count(pool: &PgPool, parent_id: Uuid) -> sqlx::Result<()>
   ```

#### Step 3: Server — Thread API Endpoints

1. `GET /api/channels/:channel_id/messages/:message_id/thread` — List thread replies (paginated).
2. `POST /api/channels/:channel_id/messages/:message_id/thread` — Create a thread reply.
   - Sets `reply_to = message_id`.
   - Increments parent's `thread_reply_count`.
   - Updates parent's `thread_last_reply_at`.
3. Wire routes in `server/src/api/mod.rs`.

#### Step 4: Server — WebSocket Events for Threads

1. Add `ThreadReplyNew` event to `ServerEvent`:
   ```rust
   ThreadReplyNew {
       parent_id: Uuid,
       message: MessageResponse,
   }
   ```
2. Broadcast to channel subscribers when a thread reply is created.
3. Include `thread_reply_count` and `thread_last_reply_at` in `MessageResponse`.

#### Step 5: Client — Thread Indicator on Messages

1. In `MessageItem.tsx`:
   - If `message.thread_reply_count > 0`, show thread indicator:
     - "N replies" link + last reply timestamp.
     - Click opens thread sidebar.
2. Add "Reply in Thread" action to message hover actions (or context menu if Phase 4 context menus are done).

#### Step 6: Client — Thread Sidebar

1. Create `client/src/components/messages/ThreadSidebar.tsx`:
   - Slides in from the right side of the chat area.
   - Header: parent message (read-only display).
   - Body: thread replies (same `MessageList` pattern with cursor pagination).
   - Footer: message input for thread replies.
   - Close button to dismiss.
2. Manage state in `client/src/stores/threads.ts`:
   ```typescript
   const [activeThread, setActiveThread] = createSignal<string | null>(null); // parent message ID
   const [threadMessages, setThreadMessages] = createStore<Record<string, Message[]>>({});

   export function openThread(messageId: string): void
   export function closeThread(): void
   export function loadThreadReplies(messageId: string, before?: string): Promise<void>
   ```

#### Step 7: Guild Admin Toggle

1. Add `threads_enabled` boolean to `guilds` table (default: true).
2. Guild settings UI toggle.
3. Server checks guild setting before allowing thread creation.

#### Step 8: Tests

1. Test creating a thread reply.
2. Test thread reply count updates.
3. Test thread pagination.
4. Test thread WebSocket events.
5. Test guild toggle disables threads.

#### Changelog Entry
```markdown
### Added
- Message threads for organized side-discussions in channels
- Thread sidebar with reply count indicators and real-time updates
- Guild setting to enable/disable threads
```

---

## 4. Advanced Moderation & Safety Filters

**Goal:** Backend moderation service with configurable content filters for hate speech, discrimination, and abusive language.

### Prerequisites
- Read `server/src/admin/` — admin system.
- Read `server/src/chat/messages.rs` — message creation pipeline.
- Read `server/src/permissions/` — guild permissions.

### Step-by-Step

#### Step 1: Database Schema

1. Create migration: `server/migrations/YYYYMMDDHHMMSS_moderation.sql`
   ```sql
   -- Guild moderation settings
   CREATE TABLE guild_moderation_settings (
       guild_id UUID PRIMARY KEY REFERENCES guilds(id) ON DELETE CASCADE,
       hate_speech_filter BOOLEAN NOT NULL DEFAULT false,
       discrimination_filter BOOLEAN NOT NULL DEFAULT false,
       abuse_filter BOOLEAN NOT NULL DEFAULT false,
       -- Action: 'log', 'warn', 'delete', 'shadowban'
       filter_action TEXT NOT NULL DEFAULT 'log',
       updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );

   -- Moderation log
   CREATE TABLE moderation_log (
       id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
       guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
       user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
       message_id UUID REFERENCES messages(id) ON DELETE SET NULL,
       channel_id UUID REFERENCES channels(id) ON DELETE SET NULL,
       filter_type TEXT NOT NULL,     -- 'hate_speech', 'discrimination', 'abuse'
       matched_pattern TEXT,
       action_taken TEXT NOT NULL,    -- 'logged', 'warned', 'deleted', 'shadowbanned'
       created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );
   CREATE INDEX idx_moderation_log_guild ON moderation_log(guild_id, created_at DESC);
   ```

#### Step 2: Server — Moderation Service

1. Create `server/src/moderation/mod.rs`:
   ```rust
   pub struct ModerationService {
       hate_speech_patterns: Vec<Regex>,
       discrimination_patterns: Vec<Regex>,
       abuse_patterns: Vec<Regex>,
   }

   impl ModerationService {
       pub fn check_content(&self, content: &str, filters: &GuildModerationSettings) -> Option<ModerationViolation>
   }
   ```
2. Load filter patterns from embedded data or config file.
3. **Important:** Use compiled `RegexSet` for performance — checking runs on every message.
4. Return `ModerationViolation { filter_type, matched_pattern, suggested_action }`.

#### Step 3: Server — Integrate into Message Pipeline

1. In `server/src/chat/messages.rs`, `create` handler:
   - After content validation, before DB insert:
   - Fetch guild moderation settings.
   - Run `moderation_service.check_content(content, settings)`.
   - Based on `filter_action`:
     - `log`: Insert to moderation_log, allow message.
     - `warn`: Insert to log, allow message, send warning to user via WebSocket.
     - `delete`: Insert to log, reject message, return error to user.
     - `shadowban`: Insert to log, save message but only show to sender (flag message as shadow-hidden).

#### Step 4: Server — Admin API for Moderation

1. `GET /api/guilds/:id/moderation/settings` — Get current settings (requires guild admin).
2. `PUT /api/guilds/:id/moderation/settings` — Update settings.
3. `GET /api/guilds/:id/moderation/log` — View moderation log (paginated).
4. Wire routes.

#### Step 5: Client — Guild Settings UI

1. Add "Moderation" tab to guild settings.
2. Toggle switches for each filter type.
3. Dropdown for action on violation.
4. Moderation log viewer with pagination.

#### Step 6: Tests

1. Test each filter type catches expected content.
2. Test each action (log, warn, delete, shadowban).
3. Test filter bypass for users with moderation permissions.
4. Test performance: filter check < 1ms per message.

#### Changelog Entry
```markdown
### Added
- Content moderation filters for hate speech, discrimination, and abuse
- Configurable moderation actions: log, warn, delete, shadowban
- Moderation log for guild administrators
```

---

## 5. User Reporting & Workflow

**Goal:** Users can report messages/users. Admins get a review queue.

### Prerequisites
- Read `server/src/admin/` — admin system.
- Depends on: Context Menus (Phase 4) for "Report" action, or fallback to button.

### Step-by-Step

#### Step 1: Database Schema

1. Create migration: `server/migrations/YYYYMMDDHHMMSS_reports.sql`
   ```sql
   CREATE TYPE report_status AS ENUM ('pending', 'reviewing', 'resolved', 'dismissed');
   CREATE TYPE report_reason AS ENUM (
       'spam', 'harassment', 'hate_speech', 'nsfw',
       'impersonation', 'threats', 'self_harm', 'other'
   );

   CREATE TABLE reports (
       id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
       reporter_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
       reported_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
       message_id UUID REFERENCES messages(id) ON DELETE SET NULL,
       guild_id UUID REFERENCES guilds(id) ON DELETE SET NULL,
       reason report_reason NOT NULL,
       description TEXT,                 -- Optional user explanation
       status report_status NOT NULL DEFAULT 'pending',
       resolved_by UUID REFERENCES users(id),
       resolution_note TEXT,
       created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
       resolved_at TIMESTAMPTZ
   );
   CREATE INDEX idx_reports_status ON reports(status, created_at DESC);
   CREATE INDEX idx_reports_guild ON reports(guild_id, status);
   ```

#### Step 2: Server — Report Handlers

1. Create `server/src/admin/reports.rs`:

   **`POST /api/reports`** (authenticated):
   - Input: `{ reported_user_id, message_id?, guild_id?, reason, description? }`
   - Rate limit: 5 reports per user per hour.
   - Prevent self-reporting.
   - Store report.

   **`GET /api/admin/reports`** (admin):
   - List reports with filters: status, guild_id, reason.
   - Paginated.
   - Include reporter info, reported user info, message content snapshot.

   **`POST /api/admin/reports/:id/resolve`** (elevated admin):
   - Input: `{ action: 'dismiss' | 'warn' | 'ban', resolution_note? }`
   - Update report status.
   - Execute action if applicable (ban user, etc.).

#### Step 3: Client — Report Dialog

1. Create `client/src/components/ui/ReportDialog.tsx`:
   - Reason selector (radio buttons).
   - Optional description textarea.
   - Submit button.
2. Trigger from context menu "Report Message" or "Report User" actions.
3. Fallback if no context menu: add report button to user profile popup.

#### Step 4: Client — Admin Report Queue

1. Create `client/src/components/admin/ReportQueue.tsx`:
   - Table/list of pending reports.
   - Click to expand: see full message content, user history, previous reports.
   - Action buttons: Dismiss, Warn, Ban.
2. Add to admin dashboard navigation.

#### Step 5: Tests

1. Test report creation.
2. Test rate limiting.
3. Test admin report listing with filters.
4. Test report resolution with ban action.

#### Changelog Entry
```markdown
### Added
- User and message reporting with reason categories
- Admin report queue with resolve/dismiss/ban actions
```

---

## 6. SaaS Trust & Data Governance (GDPR)

**Goal:** Data export and account erasure for GDPR/CCPA compliance.

### Prerequisites
- Read `server/src/admin/` — user management.
- Read `server/src/db/queries.rs` — understand all user-related tables.

### Step-by-Step

#### Step 1: Map All User Data

1. Enumerate all tables containing user data:
   - `users` — profile, email, password hash
   - `sessions` — login sessions
   - `messages` — authored messages
   - `friendships` — social graph
   - `guild_members` — guild memberships
   - `channel_read_state` — read positions
   - `file_attachments` (via messages) — uploaded files
   - `dm_participants` — DM memberships
   - `prekeys`, `olm_sessions`, `olm_accounts` — E2EE keys
   - `user_preferences` — settings
   - `favorites` — channel favorites
   - `reactions` — message reactions
   - `password_reset_tokens` (if implemented)
   - `oidc_linked_accounts` (if implemented)
   - `moderation_log` (if implemented)
   - `reports` (if implemented)

#### Step 2: Server — Data Export

1. Create `server/src/admin/data_export.rs`:

   **`POST /api/users/@me/data-export`** (authenticated):
   - Rate limit: 1 request per 24 hours.
   - Queue background job (don't block request).
   - Return `{ export_id, status: 'pending' }`.

   **`GET /api/users/@me/data-export/:id`** (authenticated):
   - Check status: pending, processing, ready, expired.
   - If ready: return download URL (signed, expires in 24h).

2. Background job:
   - Fetch all user data from every table.
   - Generate JSON files per category (profile.json, messages.json, etc.).
   - Package as ZIP.
   - Upload to S3 with 7-day expiration.
   - Notify user via WebSocket: `DataExportReady`.

#### Step 3: Server — Account Erasure

1. **`POST /api/users/@me/delete`** (authenticated, requires password confirmation):
   - Soft-delete user immediately (set `deleted_at`).
   - Queue background erasure job (30-day grace period).
   - Invalidate all sessions.
   - Return confirmation.

2. **`POST /api/users/@me/cancel-deletion`** (within grace period):
   - Reactivate account.

3. Background erasure (after 30 days):
   - Delete user from `users` table (CASCADE handles most FKs).
   - Anonymize messages: set `user_id` to system "Deleted User" account, keep content for conversation integrity.
   - Delete S3 objects for user's file attachments.
   - Delete E2EE keys.
   - Log erasure in audit log.

#### Step 4: Client — Settings UI

1. Add "Privacy & Data" section to Settings:
   - "Request Data Export" button → shows status.
   - "Delete Account" button → confirmation dialog with password.
   - Shows deletion grace period if pending.

#### Step 5: Per-Guild Rate Limiting

1. Add per-guild rate limits to prevent resource exhaustion:
   - Messages per minute per user per guild.
   - Already partially implemented — verify coverage.

#### Step 6: Tests

1. Test data export contains all user data.
2. Test export download link expiration.
3. Test account deletion grace period.
4. Test account reactivation during grace period.
5. Test full erasure after grace period.
6. Test messages anonymized (not deleted) for conversation integrity.

#### Changelog Entry
```markdown
### Added
- GDPR-compliant data export (downloadable JSON/ZIP of all user data)
- Account deletion with 30-day grace period and full data erasure
```

---

## 7. Production-Scale Polish

**Goal:** Virtualized message lists for massive chat histories. Global toast notification service.

### Prerequisites
- Read `client/src/components/messages/MessageList.tsx` — current scroll/pagination.
- Read `client/src/components/messages/MessageItem.tsx` — message rendering.

### Step-by-Step

#### Step 1: Virtualized Message List

1. Choose virtualization library: `@tanstack/solid-virtual` (MIT license) or implement custom.
2. Refactor `MessageList.tsx`:
   - Replace direct `.map()` rendering with virtualized container.
   - Only render messages visible in viewport + buffer (e.g., 5 above/below).
   - Handle variable message heights (messages with attachments, embeds, threads are taller).
   - Maintain scroll position on:
     - New messages arriving at bottom.
     - Loading older messages at top (scroll anchoring).
     - Window resize.
3. **Key challenge:** Variable height items. Options:
   - Estimate heights, measure after render, update.
   - Use `ResizeObserver` for accurate measurements.

#### Step 2: Toast Notification Service

1. Create `client/src/components/ui/Toast.tsx`:
   ```typescript
   interface ToastOptions {
     type: 'success' | 'error' | 'warning' | 'info';
     message: string;
     duration?: number;  // ms, default 5000
     action?: { label: string; onClick: () => void };
   }
   ```
2. Create `client/src/stores/toast.ts`:
   ```typescript
   export function showToast(options: ToastOptions): void
   export function dismissToast(id: string): void
   ```
3. Toast container renders at app root (Portal) with animations.
4. Stack toasts vertically (bottom-right corner). Auto-dismiss with progress bar.
5. Replace existing ad-hoc success/error displays across the app with `showToast()`.

#### Step 3: Tests

1. Test virtual list renders correct subset.
2. Test scroll anchoring on new messages.
3. Test scroll to bottom behavior.
4. Test toast auto-dismiss timing.
5. Test toast action callback.

#### Changelog Entry
```markdown
### Added
- Virtualized message list for smooth scrolling in large channels
- Global toast notification system for consistent feedback
```

---

## 8. Quick Message Actions & Reactions Toolbar

**Goal:** Floating hover toolbar on messages with one-click reactions and actions.

### Prerequisites
- Read `client/src/components/messages/MessageItem.tsx` — current reaction handling.
- Depends on: Context Menus (Phase 4) for full action list (toolbar is a subset).

### Step-by-Step

#### Step 1: Hover Toolbar Component

1. Create `client/src/components/messages/MessageToolbar.tsx`:
   ```typescript
   // Appears on hover over message, positioned top-right
   // Contains:
   // - Quick reactions: 👍 ❤️ 😂 (configurable)
   // - Action buttons: Reply, Edit (own), Delete (own/mod), More (opens full menu)
   ```
2. Position with CSS `position: absolute; top: -16px; right: 8px;`.
3. Show on message hover, hide on mouse leave (with small delay to prevent flickering).

#### Step 2: Quick Reactions

1. Default reactions: 👍, ❤️, 😂 (show as small icon buttons).
2. Click: toggle reaction (add if not reacted, remove if already reacted).
3. "+" button opens full emoji picker.
4. User can customize default reactions in settings (optional, defer if complex).

#### Step 3: Action Buttons

1. Reply → opens reply compose in MessageInput.
2. Edit → enters edit mode for own messages.
3. Delete → confirmation dialog, then delete.
4. "..." More → opens context menu or dropdown with: Pin, Copy Text, Copy Link, Report.

#### Step 4: Integration

1. In `MessageItem.tsx`, wrap message content with hover container.
2. Conditionally show toolbar based on hover state.
3. Hide toolbar when editing or during drag operations.

#### Step 5: Tests

1. Test toolbar appears/disappears on hover.
2. Test quick reaction toggle.
3. Test action buttons trigger correct behavior.
4. Test toolbar doesn't appear on own deleted messages.

#### Changelog Entry
```markdown
### Added
- Message hover toolbar with quick reactions and action buttons
```

---

## 9. Smart Input Auto-complete

**Goal:** Suggestion popups when typing `@` (users), `#` (channels), `:` (emoji) in the message input.

### Prerequisites
- Read `client/src/components/messages/MessageInput.tsx`.
- Read `client/src/stores/members.ts` — guild member list.
- Read `client/src/stores/channels.ts` — channel list.
- Read `client/src/stores/emoji.ts` — emoji data.

### Step-by-Step

#### Step 1: Auto-complete Provider

1. Create `client/src/components/messages/AutoComplete.tsx`:
   ```typescript
   interface AutoCompleteProps {
     trigger: string;        // '@', '#', ':'
     query: string;          // text after trigger
     onSelect: (item: AutoCompleteItem) => void;
     onDismiss: () => void;
   }
   ```
2. Popup appears above the input field, positioned at cursor location.
3. Keyboard navigation: ↑↓ to select, Enter/Tab to insert, Escape to dismiss.

#### Step 2: Trigger Detection

1. In `MessageInput.tsx`, on input change:
   - Detect cursor position.
   - Look backwards from cursor for trigger character (`@`, `#`, `:`).
   - If trigger found and preceded by space or start-of-line:
     - Extract query text between trigger and cursor.
     - Show auto-complete popup for that trigger type.
2. Dismiss popup when:
   - User types space (end of mention).
   - Cursor moves away from trigger.
   - Escape pressed.

#### Step 3: Data Sources

1. **`@` mentions:**
   - Source: guild members (current guild) or DM participants.
   - Filter by display name / username.
   - Insert: `@username` (or `<@user_id>` for rich formatting).
2. **`#` channels:**
   - Source: channels in current guild.
   - Filter by channel name.
   - Insert: `#channel-name` (or `<#channel_id>`).
3. **`:` emoji:**
   - Source: Unicode emoji + guild custom emoji.
   - Filter by name.
   - Insert: emoji character (Unicode) or `:emoji_name:` (custom).

#### Step 4: Rich Mention Rendering

1. In `MessageItem.tsx`, parse `<@user_id>` and `<#channel_id>` patterns.
2. Render as styled spans with user/channel names.
3. Click to navigate (user profile / channel).

#### Step 5: Tests

1. Test trigger detection at various cursor positions.
2. Test filtering accuracy.
3. Test keyboard navigation.
4. Test insertion replaces trigger + query correctly.
5. Test rich mention rendering in messages.

#### Changelog Entry
```markdown
### Added
- Smart auto-complete for @mentions, #channels, and :emoji in message input
```

---

## 10. Advanced Search & Bulk Read

**Goal:** Extend search to DMs and per-channel. Add "Mark all as read" at category/guild/global levels.

### Prerequisites
- Read `server/src/guild/search.rs` — existing guild search.
- Read `server/src/db/queries.rs` — `search_messages`, `channel_read_state` queries.
- Read `client/src/components/search/` — existing search UI.

### Step-by-Step

#### Step 1: Server — Extend Search to DMs

1. Add `search_dm_messages` query to `server/src/db/queries.rs`:
   ```sql
   SELECT m.* FROM messages m
   JOIN dm_participants dp ON m.channel_id = dp.channel_id
   WHERE dp.user_id = $1
     AND m.deleted_at IS NULL
     AND m.content_search @@ websearch_to_tsquery('english', $2)
   ORDER BY m.created_at DESC
   LIMIT $3 OFFSET $4
   ```
2. Add endpoint: `GET /api/search?q=...&scope=all|guilds|dms&guild_id=...&channel_id=...`
3. Support filtering by: scope, guild, channel, author, date range.

#### Step 2: Server — Bulk Read Management

1. Add to `server/src/db/queries.rs`:
   ```rust
   /// Mark all channels in a guild as read for user
   pub async fn mark_guild_read(pool: &PgPool, user_id: Uuid, guild_id: Uuid) -> sqlx::Result<()>

   /// Mark all channels in a category as read
   pub async fn mark_category_read(pool: &PgPool, user_id: Uuid, category_id: Uuid) -> sqlx::Result<()>

   /// Mark all channels + DMs as read
   pub async fn mark_all_read(pool: &PgPool, user_id: Uuid) -> sqlx::Result<()>
   ```
2. Add endpoints:
   - `POST /api/guilds/:id/read-all`
   - `POST /api/guilds/:id/categories/:id/read-all`
   - `POST /api/read-all`

#### Step 3: Client — Enhanced Search UI

1. Extend `SearchSidebar` with:
   - Scope selector: All / This Guild / This Channel / DMs.
   - Filter chips: Author, Date range, Has attachments.
   - Results grouped by channel/guild.
2. Search from Home view searches across everything.

#### Step 4: Client — Bulk Read Actions

1. Add "Mark All as Read" to:
   - Right-click guild icon in ServerRail.
   - Right-click category header in channel sidebar.
   - Home view: global "Mark All Read" button.
2. Update unread counts immediately (optimistic update).

#### Step 5: Tests

1. Test DM search returns correct results.
2. Test channel-scoped search.
3. Test search filters (author, date).
4. Test mark-guild-read clears all unread in guild.
5. Test mark-all-read clears everything.

#### Changelog Entry
```markdown
### Added
- Full-text search across DMs, guilds, and individual channels
- Search filters: author, date range, has attachments
- Bulk "Mark All as Read" at category, guild, and global levels
```

---

## 11. Bot Ecosystem & Gateway

**Goal:** Bot user flag, dedicated bot WebSocket gateway, slash commands.

### Prerequisites
- Read `server/src/ws/mod.rs` — WebSocket implementation.
- Read `server/src/auth/` — user creation, JWT.
- Read `server/src/permissions/`.

### Step-by-Step

#### Step 1: Database Schema

1. Create migration: `server/migrations/YYYYMMDDHHMMSS_bots.sql`
   ```sql
   ALTER TABLE users ADD COLUMN IF NOT EXISTS is_bot BOOLEAN NOT NULL DEFAULT false;
   ALTER TABLE users ADD COLUMN IF NOT EXISTS bot_owner_id UUID REFERENCES users(id);

   -- Bot applications
   CREATE TABLE bot_applications (
       id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
       owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
       name TEXT NOT NULL,
       description TEXT,
       bot_user_id UUID UNIQUE REFERENCES users(id),
       token_hash TEXT,             -- Bot auth token (hashed)
       public BOOLEAN NOT NULL DEFAULT true,
       created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );

   -- Slash commands
   CREATE TABLE slash_commands (
       id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
       application_id UUID NOT NULL REFERENCES bot_applications(id) ON DELETE CASCADE,
       guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE, -- NULL = global
       name TEXT NOT NULL,
       description TEXT NOT NULL,
       options JSONB,               -- Command parameters
       created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
       UNIQUE(application_id, guild_id, name)
   );
   ```

#### Step 2: Server — Bot Management API

1. Create `server/src/api/bots.rs`:
   - `POST /api/applications` — Create bot application.
   - `GET /api/applications` — List user's applications.
   - `POST /api/applications/:id/bot` — Create bot user for application.
   - `POST /api/applications/:id/reset-token` — Regenerate bot token.
   - `DELETE /api/applications/:id` — Delete application.

#### Step 3: Server — Bot Gateway WebSocket

1. Create `server/src/ws/bot_gateway.rs`:
   - Separate WebSocket endpoint: `GET /api/gateway/bot`.
   - Auth via `Authorization: Bot <token>` header.
   - Same event types as user gateway but:
     - No voice events (bots don't join voice initially).
     - Additional events: `SlashCommandInvoked`.
   - Rate-limited independently from user gateway.
   - Separate Redis subscription channel: `bot:{bot_id}`.

#### Step 4: Server — Slash Commands

1. Create `server/src/api/commands.rs`:
   - `PUT /api/applications/:id/commands` — Register commands (guild or global).
   - `DELETE /api/applications/:id/commands/:cmd_id` — Delete command.
2. In message handler or dedicated handler:
   - When user sends `/<command>`, check registered slash commands.
   - Route invocation to bot via bot gateway WebSocket.
   - Bot responds with message or ephemeral response.

#### Step 5: Client — Bot Invite & Slash Command UI

1. Bot invite flow: `/api/guilds/:id/bots/:bot_id/add` (guild admin).
2. Slash command picker: when user types `/`, show registered commands.
3. Command parameter form if command has options.

#### Step 6: Tests

1. Test bot creation and token auth.
2. Test bot gateway WebSocket connection.
3. Test slash command registration.
4. Test slash command invocation routing.
5. Test bot can send messages.

#### Changelog Entry
```markdown
### Added
- Bot ecosystem with applications, bot users, and dedicated gateway
- Slash commands with guild and global scope
```

---

## 12. Webhooks

**Goal:** Outgoing webhook service for third-party integrations.

### Prerequisites
- Read `server/src/chat/messages.rs` — message creation events.
- Read `server/src/ws/mod.rs` — event broadcasting patterns.

### Step-by-Step

#### Step 1: Database Schema

1. Create migration: `server/migrations/YYYYMMDDHHMMSS_webhooks.sql`
   ```sql
   CREATE TABLE webhooks (
       id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
       guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
       channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
       name TEXT NOT NULL,
       avatar_url TEXT,
       token TEXT NOT NULL UNIQUE,    -- Webhook secret token
       url TEXT NOT NULL,             -- Incoming webhook URL (POST messages to this channel)
       created_by UUID NOT NULL REFERENCES users(id),
       created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );

   -- Outgoing webhook subscriptions
   CREATE TABLE webhook_subscriptions (
       id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
       guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
       url TEXT NOT NULL,             -- URL to POST events to
       secret TEXT NOT NULL,          -- HMAC signing secret
       events TEXT[] NOT NULL,        -- ['message.create', 'member.join', etc.]
       active BOOLEAN NOT NULL DEFAULT true,
       created_by UUID NOT NULL REFERENCES users(id),
       created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );
   ```

#### Step 2: Server — Incoming Webhooks (Post Messages)

1. Create `server/src/api/webhooks.rs`:
   - `POST /api/webhooks/:id/:token` — Post message to channel (no auth, uses token).
   - Input: `{ content, username?, avatar_url?, embeds? }`
   - Creates message as webhook pseudo-user.
   - Rate limited: 30 messages per minute per webhook.

#### Step 3: Server — Outgoing Webhooks (Event Delivery)

1. Create `server/src/webhooks/delivery.rs`:
   - Background service listening to Redis events.
   - On event matching a subscription:
     - Build JSON payload.
     - Sign with HMAC-SHA256 using subscription secret.
     - POST to subscription URL with `X-Signature-256` header.
     - Retry 3 times with exponential backoff.
     - Disable subscription after 5 consecutive failures.
2. Use `reqwest` for HTTP client (MIT license, already used or similar to existing deps).

#### Step 4: Server — Webhook Management API

1. `POST /api/guilds/:id/webhooks` — Create webhook (requires manage webhooks permission).
2. `GET /api/guilds/:id/webhooks` — List webhooks.
3. `PATCH /api/guilds/:id/webhooks/:id` — Update webhook.
4. `DELETE /api/guilds/:id/webhooks/:id` — Delete webhook.
5. Same for outgoing webhook subscriptions.

#### Step 5: Client — Webhook Management UI

1. Add "Webhooks" section to guild settings.
2. Create/edit webhook forms.
3. Show webhook URL for copying.
4. Subscription management with event type checkboxes.

#### Step 6: Tests

1. Test incoming webhook message creation.
2. Test outgoing webhook delivery.
3. Test HMAC signature verification.
4. Test retry logic.
5. Test rate limiting.

#### Changelog Entry
```markdown
### Added
- Incoming webhooks for posting messages from external services
- Outgoing webhook subscriptions with HMAC-signed event delivery
```

---

## 13. SaaS Limits & Monetization Logic

**Goal:** Enforce per-guild limits (storage, members) and prepare boost/upgrade logic.

### Prerequisites
- Read `server/src/guild/mod.rs` — guild management.
- Read `server/src/chat/uploads.rs` — file upload handling.

### Step-by-Step

#### Step 1: Database Schema

1. Create migration: `server/migrations/YYYYMMDDHHMMSS_guild_limits.sql`
   ```sql
   CREATE TYPE guild_tier AS ENUM ('free', 'boosted', 'premium');

   ALTER TABLE guilds ADD COLUMN IF NOT EXISTS tier guild_tier NOT NULL DEFAULT 'free';

   CREATE TABLE guild_tier_limits (
       tier guild_tier PRIMARY KEY,
       max_members INTEGER NOT NULL,
       max_channels INTEGER NOT NULL,
       max_roles INTEGER NOT NULL,
       max_emoji INTEGER NOT NULL,
       max_file_size_mb INTEGER NOT NULL,
       max_storage_mb BIGINT NOT NULL,
       max_webhooks INTEGER NOT NULL
   );

   INSERT INTO guild_tier_limits VALUES
       ('free', 100, 50, 25, 50, 25, 500, 5),
       ('boosted', 500, 100, 50, 100, 50, 2000, 15),
       ('premium', 5000, 500, 250, 500, 100, 10000, 50);

   -- Track guild storage usage
   CREATE TABLE guild_storage_usage (
       guild_id UUID PRIMARY KEY REFERENCES guilds(id) ON DELETE CASCADE,
       used_bytes BIGINT NOT NULL DEFAULT 0,
       updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );
   ```

#### Step 2: Server — Limit Checking Middleware

1. Create `server/src/guild/limits.rs`:
   ```rust
   pub async fn check_member_limit(pool: &PgPool, guild_id: Uuid) -> Result<(), LimitError>
   pub async fn check_channel_limit(pool: &PgPool, guild_id: Uuid) -> Result<(), LimitError>
   pub async fn check_storage_limit(pool: &PgPool, guild_id: Uuid, additional_bytes: i64) -> Result<(), LimitError>
   pub async fn check_file_size_limit(pool: &PgPool, guild_id: Uuid, file_size: i64) -> Result<(), LimitError>
   ```
2. Integrate checks into:
   - Guild join → `check_member_limit`
   - Channel create → `check_channel_limit`
   - File upload → `check_storage_limit` + `check_file_size_limit`
   - Role create → check role limit
   - Emoji upload → check emoji limit
   - Webhook create → check webhook limit

#### Step 3: Server — Storage Tracking

1. On file upload: increment `guild_storage_usage.used_bytes`.
2. On file/message delete: decrement.
3. Background job: reconcile actual S3 usage with tracked amount (weekly).

#### Step 4: Server — Boost/Upgrade API

1. `GET /api/guilds/:id/limits` — Current limits and usage.
2. `POST /api/guilds/:id/upgrade` — Upgrade guild tier (placeholder for Stripe integration in Phase 7).
3. For now, admin can manually set tier.

#### Step 5: Client — Limit Display

1. Show usage bars in guild settings (members, storage, channels).
2. Warning when approaching limits (>80%).
3. "Upgrade" button/banner when at limit.

#### Step 6: Tests

1. Test each limit check rejects when at capacity.
2. Test upgrade increases limits.
3. Test storage tracking accuracy.

#### Changelog Entry
```markdown
### Added
- Guild tier system (free, boosted, premium) with configurable limits
- Storage usage tracking and enforcement
- Limit indicators in guild settings
```

---

## 14. SaaS Scaling (Signed URLs + CDN)

**Goal:** Replace proxy file downloads with signed URLs for CDN-compatible direct downloads.

### Prerequisites
- Read `server/src/chat/uploads.rs` — current proxy download.
- Read `server/src/chat/s3.rs` — S3 client (already has `presign_get()`).
- Read `server/src/db/queries.rs` — `check_attachment_access()`.

### Step-by-Step

#### Step 1: Server — Signed URL Endpoint

1. Modify `GET /api/messages/attachments/:id/download`:
   - Instead of streaming from S3, return a redirect to a signed URL.
   - Check access permission first (existing `check_attachment_access`).
   - Generate presigned URL with expiry (e.g., 15 minutes).
   - Return `302 Redirect` to presigned URL, or JSON `{ url }`.
2. Use existing `s3.presign_get()` method.

#### Step 2: CDN Configuration

1. Add config for CDN:
   ```
   CDN_ENABLED=true
   CDN_BASE_URL=https://cdn.example.com
   CDN_SIGNING_KEY=...
   ```
2. If CDN enabled, generate signed CDN URL instead of S3 presigned URL.
3. Support CloudFront signed URLs or Cloudflare signed tokens.

#### Step 3: Client — Update Attachment URLs

1. Currently: `<img src="/api/messages/attachments/:id/download?token=jwt">`.
2. Change to: fetch signed URL first, then set as img src.
3. Or: server returns signed URL in message response directly (precompute).
4. Cache signed URLs on client (they're valid for 15min).

#### Step 4: Migration Strategy

1. Support both modes (proxy and signed URL) via feature flag.
2. Default to proxy (backward compatible).
3. Switch to signed URL when CDN is configured.

#### Step 5: Tests

1. Test signed URL generation.
2. Test signed URL expiration.
3. Test access check still enforced.
4. Test CDN URL format.

#### Changelog Entry
```markdown
### Changed
- File downloads use signed URLs for CDN compatibility (configurable)
```

---

## 15. Multi-Stream Video Support

**Goal:** Simultaneous webcam + screen sharing with quality tiers.

### Prerequisites
- Read `server/src/voice/` — SFU implementation.
- Depends on: Screen Sharing (Phase 4) being implemented first.

### Step-by-Step

#### Step 1: Server — SFU Multi-Track Support

1. Extend SFU to handle multiple video tracks per participant:
   - Track type: `webcam`, `screen`.
   - Each track has independent negotiation.
2. Add simulcast support:
   - Sender encodes 3 quality tiers (high, medium, low).
   - SFU selects tier per viewer based on bandwidth estimates.
3. Signaling:
   - `VoiceAddTrack { track_type, sdp }` — Add new track.
   - `VoiceRemoveTrack { track_type }` — Remove track.
   - `VoiceSetQuality { track_type, quality }` — Request quality tier.

#### Step 2: Client — Multi-Stream Management

1. Extend `client/src/stores/voice.ts`:
   - Track multiple outgoing video tracks.
   - Track multiple incoming video tracks per participant.
   - Quality tier selection (auto or manual).
2. Webcam toggle button in Voice Island.
3. Screen share + webcam can be active simultaneously.

#### Step 3: Client — Video Layout

1. Extend voice channel UI:
   - Grid layout for multiple video streams.
   - Spotlight mode: click to enlarge one stream.
   - PiP (Picture-in-Picture) mode for self-view.
2. Show track type indicator (webcam icon, screen icon) per stream.

#### Step 4: Bandwidth Adaptation

1. Client monitors network quality.
2. Requests appropriate quality tier from SFU.
3. Visual indicator of current quality.

#### Step 5: Tests

1. Test simultaneous webcam + screen share.
2. Test simulcast tier switching.
3. Test adding/removing tracks mid-session.

#### Changelog Entry
```markdown
### Added
- Simultaneous webcam and screen sharing in voice channels
- Simulcast quality tiers for adaptive bandwidth management
```

---

## 16. Advanced Media Processing

**Goal:** Generate blurhash placeholders and thumbnails during upload.

### Prerequisites
- Read `server/src/chat/uploads.rs` — upload pipeline.
- Read `server/src/chat/s3.rs` — S3 operations.

### Step-by-Step

#### Step 1: Add Dependencies

1. Add to `server/Cargo.toml`:
   - `image` crate (MIT/Apache-2.0) — image processing.
   - `blurhash` crate (MIT) — generate blurhash strings.
2. Run `cargo deny check licenses`.

#### Step 2: Database Schema

1. Create migration:
   ```sql
   ALTER TABLE file_attachments ADD COLUMN IF NOT EXISTS blurhash TEXT;
   ALTER TABLE file_attachments ADD COLUMN IF NOT EXISTS width INTEGER;
   ALTER TABLE file_attachments ADD COLUMN IF NOT EXISTS height INTEGER;
   ALTER TABLE file_attachments ADD COLUMN IF NOT EXISTS thumbnail_s3_key TEXT;
   ```

#### Step 3: Server — Processing Pipeline

1. In `server/src/chat/uploads.rs`, after S3 upload:
   - If MIME type is image (jpeg, png, webp, gif):
     - Download from S3 (or process before upload from multipart data).
     - Decode image.
     - Compute dimensions (width, height).
     - Generate blurhash (4x3 components).
     - Generate thumbnail (max 400x400, preserve aspect ratio, JPEG 80% quality).
     - Upload thumbnail to S3: `thumbnails/{channel_id}/{attachment_id}_thumb.jpg`.
     - Store blurhash, dimensions, thumbnail_s3_key in DB.
2. Process async (don't block upload response). Options:
   - Process inline (simpler, adds latency to upload).
   - Process in background task (more complex, attachment initially has no blurhash).
   - **Recommendation:** Process inline for images under 5MB, background for larger.

#### Step 4: Server — Return Metadata

1. Include in attachment response:
   ```json
   {
     "id": "...",
     "filename": "photo.jpg",
     "mime_type": "image/jpeg",
     "size_bytes": 2048000,
     "width": 1920,
     "height": 1080,
     "blurhash": "LEHV6nWB2yk8pyo0adR*.7kCMdnj",
     "thumbnail_url": "/api/messages/attachments/{id}/thumbnail"
   }
   ```
2. Add `GET /api/messages/attachments/:id/thumbnail` endpoint.

#### Step 5: Client — Progressive Loading

1. In `MessageItem.tsx`, for image attachments:
   - Render blurhash placeholder immediately (using CSS background or canvas).
   - Load thumbnail first (fast, small).
   - Load full image on click or viewport visibility.
2. Set `width` and `height` on `<img>` to prevent layout shift.

#### Step 6: Tests

1. Test blurhash generation for various image types.
2. Test thumbnail dimensions.
3. Test non-image files skip processing.
4. Test progressive loading in client.

#### Changelog Entry
```markdown
### Added
- Blurhash image placeholders for instant visual feedback
- Automatic thumbnail generation for image attachments
```

---

## 17. Guild Discovery & Onboarding

**Goal:** Public guild directory and first-time user experience.

### Prerequisites
- Read `server/src/guild/mod.rs` — guild management.
- Read `client/src/components/guilds/` — guild components.

### Step-by-Step

#### Step 1: Database Schema

1. Create migration: `server/migrations/YYYYMMDDHHMMSS_guild_discovery.sql`
   ```sql
   ALTER TABLE guilds ADD COLUMN IF NOT EXISTS public BOOLEAN NOT NULL DEFAULT false;
   ALTER TABLE guilds ADD COLUMN IF NOT EXISTS description TEXT;
   ALTER TABLE guilds ADD COLUMN IF NOT EXISTS tags TEXT[] DEFAULT '{}';
   ALTER TABLE guilds ADD COLUMN IF NOT EXISTS member_count INTEGER NOT NULL DEFAULT 0;
   ALTER TABLE guilds ADD COLUMN IF NOT EXISTS featured BOOLEAN NOT NULL DEFAULT false;

   CREATE INDEX idx_guilds_public ON guilds(public, member_count DESC)
       WHERE public = true;
   CREATE INDEX idx_guilds_tags ON guilds USING gin(tags)
       WHERE public = true;
   ```

#### Step 2: Server — Discovery API

1. `GET /api/discover/guilds` — Public guild directory:
   - Query params: `q` (search), `tags`, `sort` (members/newest), `limit`, `offset`.
   - Only returns guilds where `public = true`.
   - Returns: id, name, description, icon, member_count, tags.
2. `GET /api/discover/featured` — Featured guilds (admin-curated).
3. `POST /api/guilds/:id/publish` — Make guild public (guild owner only).
4. `DELETE /api/guilds/:id/publish` — Make guild private.

#### Step 3: Server — Member Count Maintenance

1. Update `member_count` on guild join/leave.
2. Background job: reconcile counts periodically.

#### Step 4: Client — Discovery Page

1. Create `client/src/components/guilds/GuildDiscovery.tsx`:
   - Search bar with tag filters.
   - Grid of guild cards (icon, name, description, member count).
   - "Featured" section at top.
   - "Join" button on each card.
2. Access from Home view or ServerRail "Explore" button.

#### Step 5: Client — Onboarding Flow (FTE)

1. Create `client/src/components/onboarding/OnboardingWizard.tsx`:
   - Step 1: Welcome + username display.
   - Step 2: Theme selection (show 3 themes).
   - Step 3: Mic setup (test microphone).
   - Step 4: Join first server (show discovery or enter invite code).
   - Step 5: Done.
2. Show on first login (track in user preferences: `onboarding_complete`).
3. Skip button available on each step.

#### Step 6: Tests

1. Test guild discovery search.
2. Test tag filtering.
3. Test featured guilds.
4. Test public/private toggle.
5. Test onboarding flow completion.

#### Changelog Entry
```markdown
### Added
- Public guild directory with search, tags, and featured communities
- First-time onboarding wizard for new users
```

---

## Recommended Implementation Order

```
┌─────────────────────────────────────────────────────┐
│ FOUNDATION (do first, others depend on these)       │
│                                                      │
│  1. Absolute User Blocking                          │
│  2. Multi-line Input & Persistent Drafts            │
│  3. Message Threads                                 │
└─────────────┬───────────────────────────────────────┘
              │
┌─────────────▼───────────────────────────────────────┐
│ SAFETY & TRUST (required for public platform)       │
│                                                      │
│  4. Advanced Moderation & Safety Filters            │
│  5. User Reporting & Workflow                       │
│  6. GDPR Data Governance                            │
└─────────────┬───────────────────────────────────────┘
              │
┌─────────────▼───────────────────────────────────────┐
│ UX POLISH (parallel track, no server deps)          │
│                                                      │
│  7. Virtual Lists + Toasts        ─┐               │
│  8. Message Hover Toolbar          ├─ can be       │
│  9. Smart Input Auto-complete      │  parallel     │
│ 10. Advanced Search & Bulk Read   ─┘               │
└─────────────┬───────────────────────────────────────┘
              │
┌─────────────▼───────────────────────────────────────┐
│ ECOSYSTEM (developer platform features)             │
│                                                      │
│ 11. Bot Ecosystem & Gateway                         │
│ 12. Webhooks                                        │
│ 13. SaaS Limits & Monetization                      │
└─────────────┬───────────────────────────────────────┘
              │
┌─────────────▼───────────────────────────────────────┐
│ INFRASTRUCTURE (scaling, can defer)                  │
│                                                      │
│ 14. SaaS Scaling (Signed URLs + CDN)                │
│ 15. Multi-Stream Video                              │
│ 16. Advanced Media Processing                       │
└─────────────┬───────────────────────────────────────┘
              │
┌─────────────▼───────────────────────────────────────┐
│ GROWTH (requires safety + ecosystem ready)          │
│                                                      │
│ 17. Guild Discovery & Onboarding                    │
└─────────────────────────────────────────────────────┘
```

## Effort Summary

| # | Feature | Server | Client | Migration | New Crates | Complexity |
|---|---------|--------|--------|-----------|------------|------------|
| 1 | User Blocking | Medium | Medium | No | No | Medium |
| 2 | Multi-line + Drafts | None | Medium | No | No | Low |
| 3 | ~~Message Threads~~ ✅ | Large | Large | Yes | No | High |
| 4 | Moderation Filters | Large | Medium | Yes | No | High |
| 5 | User Reporting | Medium | Medium | Yes | No | Medium |
| 6 | GDPR Compliance | Large | Small | No | No | High |
| 7 | Virtual Lists + Toasts | None | Large | No | @tanstack/solid-virtual | High |
| 8 | Message Toolbar | None | Medium | No | No | Low-Medium |
| 9 | Auto-complete | None | Large | No | No | Medium |
| 10 | Search + Bulk Read | Medium | Medium | No | No | Medium |
| 11 | Bot Ecosystem | Very Large | Large | Yes | No | Very High |
| 12 | Webhooks | Large | Medium | Yes | reqwest | High |
| 13 | SaaS Limits | Medium | Small | Yes | No | Medium |
| 14 | Signed URLs + CDN | Medium | Small | No | No | Medium |
| 15 | Multi-Stream Video | Very Large | Large | No | No | Very High |
| 16 | Media Processing | Medium | Small | Yes | image, blurhash | Medium |
| 17 | Discovery + Onboarding | Medium | Large | Yes | No | Medium |
