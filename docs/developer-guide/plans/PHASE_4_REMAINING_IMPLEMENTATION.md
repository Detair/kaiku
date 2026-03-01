# Phase 4 Remaining Features — Sonnet Agent Implementation Manual

**Purpose:** Step-by-step instructions for a Claude Sonnet agent to implement remaining Phase 4 features.
**Date:** 2026-01-28

---

## Table of Contents

1. [First User Setup (Admin Bootstrap)](#1-first-user-setup-admin-bootstrap)
2. [Forgot Password Workflow](#2-forgot-password-workflow)
3. [SSO / OIDC Integration](#3-sso--oidc-integration)
4. [Screen Sharing](#4-screen-sharing)
5. [Advanced Browser Context Menus](#5-advanced-browser-context-menus)
6. [Home Page Unread Aggregator](#6-home-page-unread-aggregator)
7. [Content Spoilers & Enhanced Mentions](#7-content-spoilers--enhanced-mentions)
8. [Emoji Picker Polish](#8-emoji-picker-polish)
9. [Mobile Support](#9-mobile-support)

---

## General Instructions for Sonnet Agent

- **Read CLAUDE.md first** — it contains code style, commit conventions, changelog rules, and license constraints.
- **Read the existing design doc** if one exists (`docs/plans/`) before writing code.
- **Branch naming:** `feature/<name>` or `fix/<name>`.
- **Commit format:** `type(scope): subject` (max 72 chars, imperative mood).
- **Always update CHANGELOG.md** under `[Unreleased]` for user-facing changes.
- **Run `cargo test`** (server) and `bun run build` (client) before committing.
- **Run `cargo fmt --check && cargo clippy -- -D warnings`** before pushing.
- **License check:** Run `cargo deny check licenses` after adding any new crate.
- **No GPL/AGPL/LGPL dependencies.** See CLAUDE.md for allowed licenses.

### Stack Reference

| Layer | Tech | Key Files |
|-------|------|-----------|
| Server | Rust, axum, sqlx, tokio | `server/src/` |
| Client | Solid.js, TypeScript, Tauri 2.0 | `client/src/` |
| DB | PostgreSQL (sqlx migrations) | `server/migrations/` |
| Cache | Valkey/Redis (fred crate) | `server/src/redis_tests.rs` |
| Auth | JWT (EdDSA), Argon2id | `server/src/auth/` |

### Codebase Patterns to Follow

- **Server handlers:** See `server/src/chat/messages.rs` or `server/src/guild/mod.rs` for axum handler patterns.
- **Server routes:** Routes are composed in `server/src/api/mod.rs` using axum routers.
- **DB queries:** All in `server/src/db/queries.rs`, use `sqlx::query_as` with raw SQL.
- **Migrations:** `server/migrations/YYYYMMDDHHMMSS_description.sql`.
- **Client components:** `client/src/components/<domain>/`. Follow existing patterns (e.g., `CreateGuildModal.tsx`).
- **Client stores:** `client/src/stores/<domain>.ts`. Use Solid.js signals/createStore.
- **Tauri commands:** Defined in Rust in `src-tauri/src/`, invoked via `@tauri-apps/api/core` `invoke()`.
- **WebSocket events:** Server broadcasts via Redis pub/sub, client handles in `client/src/stores/websocket.ts`.
- **Permissions:** Check `server/src/permissions/` for permission bits and middleware.

---

## 1. First User Setup (Admin Bootstrap)

**Goal:** First registered user automatically gets admin/superuser permissions. Fresh-install detection and admin setup wizard.

### Prerequisites
- Understand `server/src/auth/` (registration flow).
- Understand `server/src/permissions/` (permission bits, roles).
- Understand `server/src/admin/` (admin panel).

### Step-by-Step

#### Step 1: Add Server Flag for Fresh Install

1. Read `server/src/db/queries.rs` to understand existing user queries.
2. Create migration: `server/migrations/YYYYMMDDHHMMSS_first_user_setup.sql`
   ```sql
   -- Add a server_settings table if not exists, or add a row to track setup state
   CREATE TABLE IF NOT EXISTS server_settings (
       key TEXT PRIMARY KEY,
       value TEXT NOT NULL,
       updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );
   INSERT INTO server_settings (key, value) VALUES ('setup_complete', 'false')
   ON CONFLICT (key) DO NOTHING;
   ```
3. Add query functions in `server/src/db/queries.rs`:
   - `is_setup_complete(pool) -> bool`
   - `mark_setup_complete(pool)`
   - `count_users(pool) -> i64`

#### Step 2: Modify Registration to Grant Admin on First User

1. Read `server/src/auth/register.rs` (or equivalent registration handler).
2. After successful user creation, check `count_users(pool)`:
   - If count == 1 (this is the first user):
     - Grant superuser/admin permission.
     - Mark `setup_complete = true`.
3. Add admin role assignment — check how roles work in `server/src/permissions/`.

#### Step 3: Admin Setup Wizard (Client)

1. Create `client/src/components/admin/SetupWizard.tsx`.
2. Detect fresh install via new API endpoint `GET /api/setup/status`.
3. Wizard steps:
   - Server name configuration.
   - Admin account confirmation.
   - Basic settings (registration open/closed, etc.).
4. Only show wizard when `setup_complete == false` and user is admin.

#### Step 4: API Endpoint

1. Add `GET /api/setup/status` — returns `{ setup_complete: bool }`.
2. Add `POST /api/setup/complete` — marks setup done (admin only).
3. Wire routes in `server/src/api/mod.rs`.

#### Step 5: Tests

1. Test that first registered user gets admin.
2. Test that second user does NOT get admin.
3. Test setup status endpoint.

#### Changelog Entry
```markdown
### Added
- First user setup: initial registered user automatically receives admin permissions
- Admin setup wizard for fresh installations
```

---

## 2. Forgot Password Workflow

**Goal:** Email-based password reset with secure token generation, rate limiting, and single-use tokens.

### Prerequisites
- Understand `server/src/auth/` (login, password hashing with Argon2id).
- Understand `server/src/ratelimit/` (existing rate limiting).
- Choose email sending crate: `lettre` (MIT license, well-maintained).

### Step-by-Step

#### Step 1: Database Schema

1. Create migration: `server/migrations/YYYYMMDDHHMMSS_password_reset.sql`
   ```sql
   CREATE TABLE password_reset_tokens (
       id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
       user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
       token_hash TEXT NOT NULL UNIQUE,
       expires_at TIMESTAMPTZ NOT NULL,
       used_at TIMESTAMPTZ,
       created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
   );
   CREATE INDEX idx_password_reset_tokens_hash ON password_reset_tokens(token_hash) WHERE used_at IS NULL;
   ```
   - Store hashed token (SHA-256), not plaintext.
   - `used_at` for single-use enforcement.
   - Expiry: 1 hour.

#### Step 2: Server - Add Email Service

1. Add `lettre` to `Cargo.toml`. Run `cargo deny check licenses`.
2. Create `server/src/auth/email.rs`:
   - `send_password_reset_email(to: &str, token: &str, base_url: &str)`
   - Use SMTP config from environment variables (`SMTP_HOST`, `SMTP_PORT`, `SMTP_USER`, `SMTP_PASS`).
3. Add email config to `server/src/config.rs`.

#### Step 3: Server - Password Reset Handlers

1. Create `server/src/auth/password_reset.rs`:

   **`POST /api/auth/forgot-password`**
   - Input: `{ email: String }`
   - Rate limit: 3 requests per email per hour.
   - Generate cryptographically random token (32 bytes, base64url).
   - Hash token with SHA-256, store in DB.
   - Send email with reset link.
   - **Always return 200** (don't reveal if email exists).

   **`POST /api/auth/reset-password`**
   - Input: `{ token: String, new_password: String }`
   - Hash incoming token, lookup in DB.
   - Verify: not expired, not used.
   - Update password (Argon2id hash).
   - Mark token as used.
   - Invalidate all existing sessions for user.
   - Return 200.

2. Add queries to `server/src/db/queries.rs`:
   - `create_password_reset_token(pool, user_id, token_hash, expires_at)`
   - `find_valid_reset_token(pool, token_hash) -> Option<(user_id, token_id)>`
   - `mark_reset_token_used(pool, token_id)`
   - `invalidate_user_sessions(pool, user_id)`
   - `cleanup_expired_reset_tokens(pool)` (background job)

#### Step 4: Wire Routes

1. In `server/src/api/mod.rs`, add routes under `/api/auth/`:
   - `POST /forgot-password` → `password_reset::request_reset`
   - `POST /reset-password` → `password_reset::reset_password`

#### Step 5: Client - Forgot Password UI

1. Create `client/src/components/auth/ForgotPassword.tsx`:
   - Email input form.
   - Success message: "If an account exists, a reset link has been sent."
2. Create `client/src/components/auth/ResetPassword.tsx`:
   - New password + confirm password inputs.
   - Parse token from URL query param.
   - Success → redirect to login.
3. Add "Forgot Password?" link to login page.

#### Step 6: Tests

1. Test token generation and validation.
2. Test expired token rejection.
3. Test used token rejection.
4. Test rate limiting.
5. Test password actually changes.
6. Test session invalidation after reset.

#### Changelog Entry
```markdown
### Added
- Forgot password workflow with email-based secure token reset
- Rate-limited password reset requests (3 per hour per email)
```

---

## 3. SSO / OIDC Integration

**Goal:** "Login with Google/Microsoft" via OpenID Connect.

### Prerequisites
- Understand `server/src/auth/` (JWT issuance, session management).
- The `openidconnect` crate is already listed in CLAUDE.md as approved.
- Read `server/src/config.rs` for configuration patterns.

### Step-by-Step

#### Step 1: Database Schema

1. Create migration: `server/migrations/YYYYMMDDHHMMSS_oidc_providers.sql`
   ```sql
   CREATE TABLE oidc_linked_accounts (
       id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
       user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
       provider TEXT NOT NULL,          -- 'google', 'microsoft', etc.
       provider_user_id TEXT NOT NULL,  -- Subject claim from OIDC
       email TEXT,
       created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
       UNIQUE(provider, provider_user_id)
   );
   CREATE INDEX idx_oidc_linked_user ON oidc_linked_accounts(user_id);
   ```

#### Step 2: Server - OIDC Configuration

1. Add to `server/src/config.rs`:
   ```
   OIDC_GOOGLE_CLIENT_ID, OIDC_GOOGLE_CLIENT_SECRET
   OIDC_MICROSOFT_CLIENT_ID, OIDC_MICROSOFT_CLIENT_SECRET
   OIDC_REDIRECT_BASE_URL
   ```
2. Create `server/src/auth/oidc.rs`:
   - Initialize OIDC clients for each provider using `openidconnect` crate.
   - Discovery URL for Google: `https://accounts.google.com/.well-known/openid-configuration`
   - Discovery URL for Microsoft: `https://login.microsoftonline.com/common/v2.0/.well-known/openid-configuration`

#### Step 3: Server - OIDC Handlers

1. **`GET /api/auth/oidc/:provider/authorize`**
   - Generate PKCE challenge.
   - Store CSRF state + PKCE verifier in server-side session (Redis, TTL 10min).
   - Return redirect URL to provider's authorization endpoint.

2. **`GET /api/auth/oidc/:provider/callback`**
   - Validate CSRF state.
   - Exchange authorization code for tokens using PKCE verifier.
   - Extract user info (sub, email, name) from ID token.
   - Lookup `oidc_linked_accounts` by `(provider, provider_user_id)`:
     - **Found:** Log in as linked user, issue JWT.
     - **Not found:** Create new user account, link OIDC account, issue JWT.
   - Redirect to client with JWT (via secure cookie or query fragment).

3. **`POST /api/auth/oidc/link`** (authenticated)
   - Link current user to an OIDC provider account.

4. **`DELETE /api/auth/oidc/link/:provider`** (authenticated)
   - Unlink OIDC provider (require at least one auth method remaining).

#### Step 4: Client Changes

1. Add "Sign in with Google" / "Sign in with Microsoft" buttons to login page.
2. Buttons open popup/redirect to `GET /api/auth/oidc/:provider/authorize`.
3. Handle callback redirect — extract JWT, store in auth store.
4. Settings page: show linked accounts, allow link/unlink.

#### Step 5: Tests

1. Test OIDC flow with mock provider.
2. Test account linking/unlinking.
3. Test duplicate prevention (same OIDC account can't link to two users).
4. Test first-time login creates account.

#### Changelog Entry
```markdown
### Added
- SSO login with Google and Microsoft via OpenID Connect
- Account linking for OIDC providers in user settings
```

---

## 4. Screen Sharing

**Goal:** SFU-based screen sharing with video track alongside existing audio.

### Prerequisites
- Read existing screen sharing design docs:
  - `docs/plans/2026-01-19-screen-sharing-design.md`
  - `docs/plans/2026-01-19-screen-sharing-phase-1.md` through `phase-4.md`
- Understand `server/src/voice/` (SFU implementation).
- Understand `client/src/stores/voice.ts` and `client/src/components/voice/`.
- Understand `client/src/stores/screenShareViewer.ts` (already exists).

### Step-by-Step

**Note:** This is the most complex remaining feature. Follow the existing phase docs closely.

#### Step 1: Server - SFU Video Track Support

1. Read `server/src/voice/` to understand current SFU architecture.
2. Extend SFU to handle video tracks:
   - Add video track negotiation in WebRTC session description.
   - Route video tracks from sharer to viewers (selective forwarding, no transcoding).
   - Add signaling events: `screen_share.start`, `screen_share.stop`.
3. WebSocket events for screen share state:
   - Broadcast to channel when someone starts/stops sharing.

#### Step 2: Client - Screen Capture

1. Use Tauri's screen capture API or `getDisplayMedia()` WebAPI.
2. Add screen share button to Voice Island controls.
3. Create `client/src/stores/screenShare.ts`:
   - `startScreenShare()` — request capture, create video track, add to peer connection.
   - `stopScreenShare()` — remove track, notify server.
   - Track who is currently sharing.

#### Step 3: Client - Viewer UI

1. Extend `client/src/stores/screenShareViewer.ts` (already exists).
2. Create `client/src/components/voice/ScreenShareView.tsx`:
   - Filmstrip layout: main video + small thumbnails of other participants.
   - Grid layout: equal-size tiles.
   - Toggle between layouts.
3. Handle remote video track events — render `<video>` elements.

#### Step 4: Bandwidth Management

1. Implement simulcast: send multiple quality tiers (high/medium/low).
2. SFU selects appropriate tier based on viewer's bandwidth.
3. This is a Phase 5 enhancement but basic single-quality works for Phase 4.

#### Step 5: Tests

1. Test screen share start/stop signaling.
2. Test video track forwarding through SFU.
3. Test UI layout switching.

#### Changelog Entry
```markdown
### Added
- Screen sharing in voice channels with filmstrip and grid layouts
```

---

## 5. Advanced Browser Context Menus

**Goal:** Global right-click context menu system for desktop-like feel.

### Prerequisites
- Understand Solid.js Portals (used in existing modals like `CreateGuildModal.tsx`).
- Read `client/src/components/messages/MessageItem.tsx`.
- Read `client/src/components/channels/ChannelItem.tsx`.

### Step-by-Step

#### Step 1: Context Menu Provider

1. Create `client/src/components/ui/ContextMenu.tsx`:
   ```typescript
   // ContextMenuProvider wraps the app
   // Provides showContextMenu(event, items) function
   // Renders Portal with positioned menu

   interface ContextMenuItem {
     label: string;
     icon?: Component;
     action: () => void;
     danger?: boolean;
     separator?: boolean;
     disabled?: boolean;
   }
   ```
2. Position logic:
   - Use `event.clientX`, `event.clientY`.
   - Flip if near viewport edge (use `floating-ui` or manual calculation).
   - Close on click outside, Escape, or scroll.

3. Wrap app in provider — add to `client/src/App.tsx`.

#### Step 2: Message Context Menu

1. In `MessageItem.tsx`, add `onContextMenu` handler:
   ```
   Items:
   - Reply
   - Edit (own messages only)
   - Delete (own or with permission)
   - Copy Text
   - Copy Message Link
   - Copy ID
   - Pin Message (with permission)
   - Quote Message
   - Mark as Unread
   ```
2. Check permissions for conditional items.

#### Step 3: Channel Context Menu

1. In `ChannelItem.tsx`, add `onContextMenu`:
   ```
   Items:
   - Mark as Read
   - Mute Channel
   - Edit Channel (with permission)
   - Copy Channel ID
   - Add to Favorites
   ```

#### Step 4: User Context Menu

1. In member list / user mentions, add `onContextMenu`:
   ```
   Items:
   - View Profile
   - Send Message
   - Add Friend / Remove Friend
   - Block
   - Copy User ID
   ```

#### Step 5: Tests

1. Test menu positioning at viewport edges.
2. Test permission-based item visibility.
3. Test keyboard navigation (arrow keys, Enter, Escape).

#### Changelog Entry
```markdown
### Added
- Right-click context menus for messages, channels, and users
```

---

## 6. Home Page Unread Aggregator

**Goal:** Centralized view of unread activity across all guilds and DMs on the Home page.

### Prerequisites
- Read `client/src/components/home/` (existing Home view components).
- Read `server/src/db/queries.rs` — understand `channel_read_state` table.
- Read `client/src/stores/channels.ts` and `client/src/stores/dms.ts`.

### Step-by-Step

#### Step 1: Server - Aggregate Unread Query

1. Add to `server/src/db/queries.rs`:
   ```rust
   /// Returns unread counts per channel for all guilds the user belongs to
   pub async fn get_user_unread_summary(
       pool: &PgPool,
       user_id: Uuid,
   ) -> sqlx::Result<Vec<UnreadSummary>> {
       // Query channels where user is guild member or DM participant
       // LEFT JOIN channel_read_state
       // COUNT messages after last_read_at
       // GROUP BY guild_id, channel_id
       // Return: guild_id, guild_name, channel_id, channel_name, unread_count, last_message_at
   }
   ```
2. Define `UnreadSummary` struct.

#### Step 2: Server - API Endpoint

1. Add `GET /api/users/@me/unreads` handler.
2. Returns JSON: list of `{ guild_id, guild_name, channel_id, channel_name, unread_count, last_message_at }`.
3. Wire route in `server/src/api/mod.rs`.

#### Step 3: Client - Unread Dashboard Module

1. Create `client/src/components/home/UnreadAggregator.tsx`:
   - Grouped by guild.
   - Show channel name + unread count + time since last message.
   - Click to navigate to channel.
   - "Mark All Read" button per guild and globally.
2. Add as a module in `HomeRightPanel.tsx` (follow existing modular sidebar pattern).

#### Step 4: Real-time Updates

1. Listen to WebSocket events for new messages to increment counts.
2. Listen to read-sync events to decrement/clear counts.

#### Step 5: Tests

1. Test aggregate query correctness.
2. Test with zero unreads (empty state).
3. Test mark-all-read functionality.

#### Changelog Entry
```markdown
### Added
- Home page unread aggregator showing activity across all guilds and DMs
```

---

## 7. Content Spoilers & Enhanced Mentions

**Goal:** `||spoiler||` syntax support and `@everyone`/`@here` mention permissions.

### Prerequisites
- Read `client/src/components/messages/MessageItem.tsx` (message rendering).
- Read markdown rendering setup (solid-markdown usage).
- Read `server/src/permissions/` for permission bits.
- Read `server/src/chat/messages.rs` for message creation handler.

### Step-by-Step

#### Step 1: Spoiler Rendering (Client)

1. Add spoiler parsing to message renderer:
   - Regex: `/\|\|(.+?)\|\|/g` matches `||spoiler text||`.
   - Wrap matched content in `<span class="spoiler">` component.
2. Create `client/src/components/messages/Spoiler.tsx`:
   ```typescript
   // Signal-based reveal toggle
   const [revealed, setRevealed] = createSignal(false);
   // CSS: filter: blur(4px) when not revealed
   // Click to toggle revealed state
   ```
3. Add CSS for spoiler:
   ```css
   .spoiler {
     background: var(--surface-3);
     filter: blur(4px);
     cursor: pointer;
     border-radius: 2px;
   }
   .spoiler.revealed {
     filter: none;
     background: var(--surface-2);
   }
   ```

#### Step 2: Enhanced Mentions — Server Permission

1. Add `MENTION_EVERYONE` permission bit to `server/src/permissions/`:
   - Add bit 23 (or next available) to `GuildPermissions`.
2. In `server/src/chat/messages.rs` message creation handler:
   - Parse message content for `@everyone` or `@here`.
   - Check if sender has `MENTION_EVERYONE` permission.
   - If not, strip or reject the mention (decide: strip silently or return error).
3. Store mention metadata in message for client-side highlighting.

#### Step 3: Mention Rendering (Client)

1. Parse `@everyone` and `@here` in message renderer.
2. Highlight with distinct style (e.g., colored background like Discord).
3. Trigger notification sound for mentioned users.

#### Step 4: Tests

1. Test spoiler parsing and rendering.
2. Test mention permission check (authorized vs unauthorized).
3. Test notification triggering on mention.

#### Changelog Entry
```markdown
### Added
- Spoiler text support with `||spoiler||` syntax and click-to-reveal
- @everyone and @here mentions with permission-based access control
```

---

## 8. Emoji Picker Polish

**Goal:** Fix UI regressions (transparency, cutoff) and improve positioning.

### Prerequisites
- Read `client/src/components/emoji/EmojiPicker.tsx`.
- Understand current positioning issues.

### Step-by-Step

#### Step 1: Diagnose Issues

1. Read `EmojiPicker.tsx` fully to understand current implementation.
2. Identify:
   - Transparency/opacity CSS issues.
   - `max-height` or `overflow` problems.
   - Container bounds causing clipping.

#### Step 2: Fix Styling

1. Ensure solid background (no transparency):
   ```css
   background: var(--surface-1);
   opacity: 1;
   ```
2. Fix max-height to prevent overflow.
3. Add `z-index` high enough to float above all content.

#### Step 3: Smart Positioning with floating-ui

1. Add `@floating-ui/dom` dependency (MIT license).
2. Use `computePosition` with `flip` and `shift` middleware:
   ```typescript
   import { computePosition, flip, shift, offset } from '@floating-ui/dom';
   // Position picker relative to trigger button
   // flip: switch sides if not enough space
   // shift: slide along edge to stay visible
   ```
3. Recalculate position on scroll/resize.

#### Step 4: Tests

1. Test picker visibility at all viewport edges.
2. Test on messages near bottom of chat.
3. Test opacity and background rendering.

#### Changelog Entry
```markdown
### Fixed
- Emoji picker transparency and positioning issues
```

---

## 9. Mobile Support

**Goal:** Adapt Tauri frontend for mobile or begin native mobile implementation.

### Prerequisites
- Read Tauri 2.0 mobile documentation.
- Understand current responsive CSS state.
- This is the largest and most open-ended task.

### Step-by-Step

#### Step 1: Assess Current State

1. Test current UI at mobile viewport sizes (375px, 414px widths).
2. Identify all breakpoints and layout issues.
3. List components that need mobile adaptations.

#### Step 2: Responsive Layout Refactor

1. Implement responsive breakpoints:
   - Desktop: > 1024px (current layout).
   - Tablet: 768-1024px (collapsible sidebar).
   - Mobile: < 768px (single-column, drawer navigation).
2. Key layout changes:
   - Server Rail → bottom tab bar or hamburger menu.
   - Channel sidebar → drawer (swipe from left).
   - Member list → drawer (swipe from right) or separate page.
   - Voice Island → bottom sheet.

#### Step 3: Touch Interactions

1. Replace hover-based interactions with touch alternatives.
2. Add swipe gestures for navigation.
3. Ensure all tap targets are at least 44x44px.
4. Long-press for context menus (instead of right-click).

#### Step 4: Tauri Mobile Build

1. Configure `src-tauri/tauri.conf.json` for iOS/Android.
2. Test on emulators/simulators.
3. Handle platform-specific permissions (microphone, notifications).

#### Step 5: Performance on Mobile

1. Optimize bundle size.
2. Implement virtual scrolling for message lists (if not done).
3. Reduce animations on low-power devices.

#### Changelog Entry
```markdown
### Added
- Mobile-responsive layout with drawer navigation and touch interactions
```

---

## Recommended Implementation Order

1. **First User Setup** — Small scope, high user value, no external dependencies.
2. **Context Menus** — Client-only, improves UX significantly, unblocks other features.
3. **Spoilers & Mentions** — Moderate scope, affects both server and client.
4. **Emoji Picker Polish** — Small bug fix, quick win.
5. **Home Unread Aggregator** — Moderate scope, requires new query + UI.
6. **Forgot Password** — Requires email infrastructure (lettre crate).
7. **SSO / OIDC** — Depends on external provider config, moderate complexity.
8. **Screen Sharing** — High complexity, existing design docs available.
9. **Mobile Support** — Largest scope, should be last.

---

## Per-Feature Effort Estimates

| Feature | Server Changes | Client Changes | DB Migration | New Crate | Complexity |
|---------|---------------|----------------|--------------|-----------|------------|
| First User Setup | Small | Small | Yes | No | Low |
| Forgot Password | Medium | Small | Yes | lettre | Medium |
| SSO / OIDC | Large | Medium | Yes | openidconnect | High |
| Screen Sharing | Large | Large | No | No | Very High |
| Context Menus | None | Large | No | floating-ui | Medium |
| Unread Aggregator | Medium | Medium | No | No | Medium |
| Spoilers & Mentions | Small | Medium | No | No | Low-Medium |
| Emoji Picker Polish | None | Small | No | floating-ui | Low |
| Mobile Support | None | Very Large | No | No | Very High |
