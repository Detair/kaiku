# VoiceChat (Canis) Roadmap

This roadmap outlines the development path from the current prototype to a production-ready, multi-tenant SaaS platform.

**Current Phase:** Phase 4 (Advanced Features) - In Progress

**Last Updated:** 2026-02-01

## Quick Status Overview

| Phase | Status | Completion | Key Achievements |
|-------|--------|------------|------------------|
| **Phase 0** | ✅ Complete | 100% | N+1 fix, WebRTC optimization, MFA encryption |
| **Phase 1** | ✅ Complete | 100% | Voice state sync, audio device selection |
| **Phase 2** | ✅ Complete | 100% | Voice Island, VAD, Speaking Indicators, Command Palette, File Attachments, Theme System, Code Highlighting |
| **Phase 3** | ✅ Complete | 100% | Guild system, Friends, DMs, Home View, Rate Limiting, Permission System + UI, Information Pages, DM Voice Calls |
| **Phase 4** | 🔄 In Progress | 96% | E2EE DM Messaging, User Connectivity Monitor, Rich Presence, First User Setup, Context Menus, Emoji Picker Polish, Unread Aggregator, Content Spoilers, Forgot Password, SSO/OIDC |
| **Phase 5** | 📋 Planned | 0% | - |

**Production Ready Features:**
- ✅ Modern UI with "Focused Hybrid" design system
- ✅ Draggable Voice Island with keyboard shortcuts (Ctrl+Shift+M/D)
- ✅ Voice Activity Detection (VAD) with real-time speaking indicators
- ✅ Audio device selection with mic/speaker testing
- ✅ Command Palette (Ctrl+K) for power users
- ✅ Auto-retry voice join on connection conflicts
- ✅ Participant list with instant local user display
- ✅ Full guild architecture with role-based permissions
- ✅ Automatic JWT token refresh (prevents session expiration)
- ✅ File attachments with drag-and-drop upload and image previews
- ✅ DM voice calls with join/decline flow
- ✅ Admin dashboard with user/guild management
- ✅ User connection quality monitoring with history
- ✅ Cross-server channel favorites with star toggle
- ✅ First-user setup wizard with automatic admin bootstrap
- ✅ Right-click context menus for messages, channels, and users
- ✅ Smart emoji picker with viewport-aware positioning and auto-flip

---

## Phase 0: Technical Debt & Reliability ✅ **COMPLETED**
*Goal: Fix critical performance issues and ensure basic stability before adding features.*

- [x] **[Backend] Fix N+1 Query in Message List** `Priority: Critical` ✅
  - Refactor `server/src/chat/messages.rs` to use bulk user fetching (`find_users_by_ids`).
  - Eliminated the loop that executes a DB query for every message.
  - **Impact**: 96% query reduction (51→2 queries for 50 messages).
- [x] **[Backend] Refactor `AuthorProfile` Construction** `Priority: High` ✅
  - Implement `From<User> for AuthorProfile` to centralize user data formatting.
  - Removed duplicate logic in `list`, `create`, and `update` handlers.
  - **Impact**: Eliminated ~40 lines of duplication, ensured consistent formatting.
- [x] **[Client] WebRTC Connectivity Fix** `Priority: High` ✅
  - Implemented async `handleServerEvent()` to process voice events immediately.
  - Changed to use `getVoiceAdapter()` instead of dynamic import for ICE candidates.
  - **Impact**: 90% latency reduction (80-150ms → 5-15ms per ICE candidate).
- [x] **[Backend] MFA Secret Encryption** `Priority: Critical` ✅
  - Implemented AES-256-GCM encryption for MFA secrets.
  - Created `mfa_crypto` module with encrypt/decrypt functions.
  - Added MFA setup, verify, and disable handlers.
  - **Impact**: MFA secrets no longer stored in plaintext.

---

## Phase 1: Core Loop Stability ✅ **COMPLETED**
*Goal: Ensure the fundamental chat and voice experience is flawless and bug-free.*

- [x] **[Tests] Message API Integration Tests** ✅
  - Created tests for message CRUD operations to prevent regressions.
  - Verified JSON response structures.
- [x] **[Client] Real-time Text Sync** ✅
  - New messages via WebSocket appear instantly in the UI without refresh.
  - Handle `message.edit` and `message.delete` events live.
- [x] **[Voice] Room State Synchronization** ✅
  - WebSocket event handlers sync RoomState on join.
  - Updated VoiceParticipants with new theme and proper indicators.
  - Speaking indicators implemented via client-side VAD.
- [x] **[Client] Audio Device Selection** ✅
  - Completed with full modal UI and device testing.

---

## Phase 2: Rich Interactions & Modern UX ✅ **COMPLETED**
*Goal: Reach feature parity with basic chat apps while introducing modern efficiency tools.*

- [x] **[UX] Command Palette (`Ctrl+K`)** ✅
  - Global fuzzy search for Channels and Users.
  - Keyboard navigation (↑↓ + Enter + Esc).
  - Command execution with > prefix.
- [x] **[UX] Dynamic Voice Island** ✅
  - Decoupled Voice Controls from sidebar.
  - Created "Dynamic Island" style floating overlay at bottom center.
  - Shows connection status, timer, and all voice controls.
- [x] **[UX] Modern Theme System** ✅
  - Implemented "Focused Hybrid" design (Discord structure + Linear efficiency).
  - New color palette with surface layers and semantic tokens.
  - Three themes: Focused Hybrid, Solarized Dark, Solarized Light.
- [x] **[Client] Audio Device Selection** ✅
  - AudioDeviceSettings modal with device enumeration.
  - Microphone test with real-time volume indicator.
- [x] **[Voice] Voice Activity Detection (VAD)** ✅
  - Continuous VAD using Web Audio API AnalyserNode.
  - Real-time speaking indicators for local and remote participants.
  - Pulsing animation in channel list when participants are speaking.
- [x] **[Voice] Auto-Retry on Connection Conflicts** ✅
  - Automatic leave/rejoin when server reports "Already in voice channel".
- [x] **[UX] Instant Participant Display** ✅
  - Local user shown immediately when joining voice channel.
- [x] **[UX] Draggable Voice Island** ✅
  - Voice Island can be dragged anywhere on screen.
  - Keyboard shortcuts: Ctrl+Shift+M (mute), Ctrl+Shift+D (deafen).
- [x] **[Voice] Basic Noise Reduction (Tier 1)** ✅
  - Implemented via constraints with UI Toggle in Audio Settings.
- [x] **[Auth] Automatic Token Refresh** ✅
  - JWT access tokens auto-refresh 60 seconds before expiration.
- [x] **[Media] File Attachments & Previews** ✅
  - Proxy Method for authenticated file downloads.
  - Drag-and-drop file upload with image previews.
- [x] **[Text] Markdown & Emojis** ✅
  - `solid-markdown` enabled with Emoji Picker.
- [x] **[Text] Code Blocks & Syntax Highlighting** ✅
  - Custom CodeBlock component with highlight.js integration.

---

## Phase 3: Guild Architecture & Security ✅ **COMPLETED**
*Goal: Transform from "Simple Chat" to "Multi-Server Platform" (Discord-like architecture).*

- [x] **[DB] Guild (Server) Entity** ✅
  - Created `guilds` table with full CRUD operations.
  - Channels belong to `guild_id`.
  - Guild members with join/leave functionality.
- [x] **[Social] Friends & Status System** ✅
  - `friendships` table (pending/accepted/blocked).
  - Friend Request system (send/accept/reject/block).
  - FriendsList component with tabs, AddFriend modal.
- [x] **[Chat] Direct Messages & Group DMs** ✅
  - Reused `channels` with `type='dm'`.
  - DM creation, listing, leave functionality.
- [x] **[UI] Server Rail & Navigation** ✅
  - Vertical Server List sidebar (ServerRail).
  - Context switching between guilds.
- [x] **[UX] Unified Home View** ✅
  - Home dashboard with DM sidebar and conversations.
  - Unread counts, last message previews.
- [x] **[Auth] Permission System** ✅
  - Backend API handlers for admin, roles, and overrides (PR #17).
  - Permission checking middleware and guild permission queries.
  - Admin UI for role management and permission assignment (PR #20).
  - Role picker in guild settings, channel permission overrides.
  - Admin Dashboard with user/guild management and audit log.
- [x] **[Voice] DM Voice Calls** ✅
  - Voice calling in DM and group DM conversations (PR #21).
  - Call signaling via Redis Streams, reuses existing SFU.
  - Join/Decline flow with CallBanner UI component.
- [x] **[Content] Information Pages** ✅
  - Platform-wide pages (ToS, Privacy Policy) in Home view.
  - Guild-level pages (Rules, FAQ) in sidebar above channels.
  - Markdown editor with live preview and Mermaid diagram support.
  - Page acceptance tracking with scroll-to-bottom requirement.
- [x] **[Security] Rate Limiting** ✅
  - Redis-based fixed window rate limiting with Lua scripts.
  - Hybrid IP/user identification with configurable trust proxy.
  - Failed auth tracking with automatic IP blocking.

---

## Phase 4: Advanced Features 🔄 **IN PROGRESS**
*Goal: Add competitive differentiators and mobile support.*

- [x] **[Security] E2EE Key Backup Foundation** ✅ (PR #22)
  - OlmAccount, OlmSession, RecoveryKey, EncryptedBackup entities.
  - Database tables and API endpoints.
  - **Design:** `docs/plans/2026-01-19-e2ee-key-backup-design.md`
- [x] **[Voice] User Connectivity Monitor** ✅ (PR #23)
  - Real-time connection quality tracking (latency, packet loss, jitter).
  - WebRTC getStats() integration with 3-second sampling.
  - Connection history page with daily charts and session list.
  - TimescaleDB support with graceful fallback to PostgreSQL.
  - Rate-limited stats broadcasting to prevent spam.
  - **Design:** `docs/plans/2026-01-19-user-connectivity-monitor-design.md`
- [x] **[Social] Rich Presence (Game Activity)** ✅
  - Automatic game detection via process scanning (sysinfo crate).
  - 15+ pre-configured games (Minecraft, Valorant, CS2, etc.).
  - Display "Playing X" status in Friends List, Member List, and DM panels.
  - Privacy toggle in settings to disable activity sharing.
  - Real-time activity sync via WebSocket.
  - *Future:* "Ask to Join" logic for multiplayer games.
  - **Design:** `docs/plans/2026-01-19-rich-presence-design.md`
- [x] **[Security] E2EE Key Backup UI & Recovery** ✅ (PR #29)
  - Recovery key modal with copy/download and confirmation flow.
  - Security Settings tab showing backup status.
  - Post-login E2EE setup prompt (skippable or mandatory via server config).
  - Backup reminder banner for users without backup.
  - Server configuration option `REQUIRE_E2EE_SETUP` for mandatory setup.
  - **Plan:** `docs/plans/2026-01-19-e2ee-implementation-phase-1.md`
- [x] **[Security] E2EE DM Messaging** ✅ (PR #41)
  - End-to-end encryption for DM messages using vodozemac (Olm).
  - LocalKeyStore with encrypted SQLite storage for Olm sessions.
  - CryptoManager for session management and encrypt/decrypt operations.
  - E2EE setup modal with recovery key generation.
  - Encryption indicator in DM headers.
  - Graceful fallback to unencrypted when E2EE not available.
  - **Plan:** `docs/plans/2026-01-23-e2ee-messages-implementation.md`
- [x] **[UX] Sound Pack (Notification Sounds)** ✅
  - 5 notification sounds: Default, Subtle, Ping, Chime, Bell.
  - Global notification settings in Settings > Notifications tab.
  - Per-channel notification levels (All messages, Mentions only, Muted).
  - Volume control with test sound button.
  - Smart playback with cooldown, tab leader election, mention detection.
  - Native audio via rodio (Tauri), Web Audio API fallback (browser).
  - **Design:** `docs/plans/2026-01-21-sound-pack-design.md`
- [x] **[Chat] Cross-Client Read Sync** ✅
  - Sync read position across all user's devices/tabs.
  - Clear unread badges instantly when read on any client.
  - New `user:{user_id}` Redis channel for user-targeted events.
  - **Design:** `docs/plans/2026-01-23-read-sync-dnd-design.md`
- [x] **[Settings] Server-Synced User Preferences** ✅
  - Theme, sound settings, quiet hours, and per-channel notifications sync across all devices
  - Real-time updates via WebSocket when preferences change on another device
  - Last-write-wins conflict resolution with timestamps
  - Migration from legacy localStorage keys
  - **Design:** `docs/plans/2026-01-23-server-synced-preferences-design.md`
- [x] **[UX] Do Not Disturb Mode** ✅
  - Notification sounds suppressed when user status is "Busy" (DND).
  - Scheduled quiet hours with configurable start/end times.
  - Handles overnight ranges (e.g., 22:00 to 08:00).
  - Call ring sounds also suppressed during DND.
  - **Design:** `docs/plans/2026-01-23-read-sync-dnd-design.md`
- [x] **[UX] Modular Home Sidebar** ✅
  - Collapsible module framework with server-synced state
  - Active Now module showing friends' game activity
  - Pending module for friend requests
  - Pins module for notes, links, and bookmarks
  - **Design:** `docs/plans/2026-01-24-modular-home-sidebar-design.md`
- [x] **[UX] Cross-Server Favorites** ✅ (PR #45)
  - Pin channels from different guilds into unified Favorites section
  - Star icon on channels to toggle favorites (appears on hover, filled when favorited)
  - Expandable Favorites section in Sidebar grouped by guild
  - Maximum 25 favorites per user with automatic cleanup
  - **Design:** `docs/plans/2026-01-24-cross-server-favorites-design.md`
- [x] **[Content] Custom Emojis (Guild Emoji Manager)** ✅ (PR #46)
  - Guild custom emoji database schema and API
  - Animated emoji support (GIF, WebP)
  - Emoji Manager UI in Guild Settings
  - Drag-and-drop upload and bulk upload utility
- [x] **[UX] DM Avatars** ✅ (Issue #104)
  - Added `icon_url` to channels table via migration
  - DM avatar upload endpoint with authenticated access
  - UI for uploading and displaying DM group avatars
  - Fallback to generated avatars for DMs without custom icons
- [x] **[UX] Pinned Notes System** ✅ (Issue #105)
  - Fixed note persistence and display in Home sidebar
  - Pins module with add/edit/delete functionality
- [x] **[Media] Unified File Size Upload Limits** ✅
  - **Context:** Standardize file size restrictions across all upload types
  - **Completed:**
    - Added `max_avatar_size` (5MB default) and `max_emoji_size` (256KB default) to Config
    - Implemented size validation for user profile avatars (fixed security issue)
    - Made emoji size limits configurable via `MAX_EMOJI_SIZE` environment variable
    - Updated DM avatar upload to use `max_avatar_size` instead of `max_upload_size`
    - Added frontend validation for immediate user feedback
    - Created comprehensive integration tests (7 tests, 1 ignored for future DM feature)
    - Fixed 5 critical issues identified in PR review
    - Add frontend validation before upload to provide immediate feedback
- [x] **[Auth] First User Setup (Admin Bootstrap)** ✅ (PR #110)
  - First registered user automatically gets admin/superuser permissions.
  - PostgreSQL row-level locking to prevent race conditions.
  - Setup wizard with server configuration (name, registration policy, legal URLs).
  - Compare-and-swap pattern for atomic setup completion.
  - HTTP integration tests with reusable `TestApp` infrastructure (8 tests covering auth, admin, validation, idempotency).
  - **Tech Debt:**
    - [x] Add HTTP integration tests for authorization bypass scenarios ✅
    - [ ] Add HTTP-level concurrent setup completion test (Issue #140)
    - [ ] Test infrastructure improvements: cleanup guards (Issue #137), shared DB pool (Issue #138), stateful middleware testing (Issue #139)
- [x] **[Auth] Forgot Password Workflow** ✅
  - Email-based password reset with secure token generation.
  - Rate-limited reset requests to prevent abuse.
  - Token expiration (e.g., 1 hour) with single-use enforcement.
- [x] **[Auth] SSO / OIDC Integration** ✅ (PR #135)
  - Admin-configurable OIDC providers (Google, Microsoft, etc.) via `openidconnect`.
  - Dynamic provider discovery and registration through admin dashboard.
  - Seamless login flow with automatic account linking.
- [x] **[Voice] Screen Sharing** ✅
  - SFU handles multiple video tracks per room with per-channel limits.
  - Spotlight/PiP/Theater viewer modes with keyboard shortcuts (Escape/V/M/F).
  - Tauri WebSocket parity for screen share events.
  - REST endpoints for check/start/stop with permission and limit enforcement.
  - Server and client test coverage.
- [x] **[UX] Advanced Browser Context Menus** ✅
  - Global `ContextMenuProvider` using Solid.js Portals
  - `ContextMenu.tsx` component with keyboard navigation (Arrow keys, Enter, Home/End, Escape)
  - Message menu: Copy Text, Copy Message Link, Copy ID, Delete (own messages)
  - Channel menu: Mark as Read, Mute/Unmute, Add to Favorites, Edit Channel, Copy ID
  - User menu: View Profile, Send Message, Add Friend, Block, Copy User ID
  - Implementation files: `client/src/components/ui/ContextMenu.tsx`, `client/src/lib/contextMenuBuilders.ts`
    - Support keyboard navigation and accessibility
  - **Implementation:**
    - Architecture: `ContextMenuProvider.tsx` with portal support
    - UI Component: `ContextMenu.tsx` with positioning logic
    - Integration: Message and channel context actions
- [x] **[UX] Home Page Unread Aggregator** ✅ (PR #127)
  - Centralized view of unread messages across all guilds and DMs
  - API endpoint `GET /api/me/unread` with optimized aggregate query
  - UnreadModule component with guild-grouped and DM unread counts
  - Direct navigation to channels with unread messages
  - Comprehensive error handling with toast notifications
  - Performance-optimized database query with covering indexes
  - Automatic refresh on window focus
  - Collapsible module with total unread badge
- [x] **[Chat] Content Spoilers & Enhanced Mentions** ✅ (PR #128)
  - Content spoilers with `||text||` syntax for hiding sensitive information
  - Click-to-reveal functionality with proper event cleanup
  - MENTION_EVERYONE permission (bit 23) for controlling @everyone/@here mentions
  - Server-side permission validation prevents unauthorized mass mentions
  - ReDoS protection (500 character limit) and XSS prevention via DOMPurify
  - 3 integration tests for permission validation (all passing)
  - 9 unit tests for spoiler functionality
- [x] **[Chat] Emoji Picker Polish** ✅
  - **Context:** Resolving UI regressions where the reaction window is transparent or cut off by container bounds.
  - **Implementation:**
    - ✅ Fix sizing issues in `EmojiPicker.tsx` (dynamic `maxHeight` prop replaces fixed `max-h-96`)
    - ✅ Implement viewport boundary checks for portal positioning
    - ✅ Integrate `@floating-ui/dom@1.7.5` for smart positioning that adapts to available space
    - ✅ Ensure picker always remains visible regardless of message location
    - ✅ Handle edge cases (top/bottom/left/right of viewport, narrow windows)
    - ✅ Added click-outside, Escape key, and scroll-to-close behaviors
    - ✅ Portal-based rendering prevents parent container clipping
    - ✅ Smooth fade-in + scale animation (150ms ease-out)
---

## Phase 5: Ecosystem & SaaS Readiness
*Goal: Open the platform to developers and prepare for massive scale.*

- [ ] **[Storage] SaaS Scaling Architecture**
  - Transition from Proxy Method to Signed URLs/Cookies.
  - CDN Integration with CloudFront/Cloudflare.
- [ ] **[API] Bot Ecosystem**
  - Add `is_bot` user flag.
  - Create Gateway WebSocket for bot events.
  - Implement Slash Commands structure.
- [ ] **[Voice] Multi-Stream Support**
  - Simultaneous Webcam and Screen Sharing.
  - Implement Simulcast (quality tiers) for bandwidth management.
- [ ] **[Voice] Evaluate str0m as WebRTC Alternative** `Priority: Low`
  - Current stack: webrtc-rs 0.11 (full-stack, owns I/O). Working, but project is stagnating.
  - Alternative: [str0m](https://github.com/algesten/str0m) — Sans-IO WebRTC library (pure Rust).
  - **Benefits:** Full I/O control, deterministic testing, better SFU fit, no hidden task spawning.
  - **Cost:** ~5,700 lines server + ~430 lines client rewrite (different programming model).
  - **Trigger:** Migrate when a security fix forces action on webrtc-rs, when hitting a performance wall requiring tighter I/O control, or during major voice architecture changes (e.g., MLS E2EE).
  - mediasoup (C++ FFI) ruled out due to `unsafe_code = "forbid"` policy.
- [ ] **[SaaS] Limits & Monetization Logic**
  - Enforce limits (storage, members) per Guild.
  - Prepare "Boost" logic for lifting limits.
- [ ] **[Safety] Advanced Moderation & Safety Filters**
  - **Context:** Protect users and platform reputation with proactive content scanning.
  - **Implementation:**
    - **Backend:**
      - Create `ModerationService` in `server/src/moderation/`
      - Implement pre-defined filter sets: Hate Speech, Discrimination, Abusive Language
      - Support configurable actions: shadow-ban, delete + warn, log for review
      - Add filter pattern matching with false-positive handling
    - **Frontend:**
      - Add "Safety" tab to Guild Settings
      - Allow guild admins to toggle filter categories
      - Configure action policies per filter type
      - View moderation logs with context
- [ ] **[Safety] User Reporting & Workflow**
  - **Context:** Empower users to flag inappropriate content.
  - **Implementation:**
    - **Backend:**
      - Create `reports` table with message/user references and report categories
      - Store reporter info and timestamp
      - Add API endpoints for creating and managing reports
    - **Frontend:**
      - Add "Report" action to Context Menu on messages and profiles
      - Create report modal with category selection and description
      - Implement `AdminQueue.tsx` UI for system admins
      - Show historical context for reported messages (surrounding messages)
      - Support bulk actions (dismiss, ban, warn)
- [ ] **[Safety] Absolute User Blocking**
  - **Context:** Ensure users can completely sever communication with malicious actors.
  - **Implementation:**
    - **Backend:**
      - Update `db/queries.rs` to include block-aware message fetching
      - Implement interceptors that drop message delivery between blocked users
      - Filter WebSocket events (typing, presence, voice state) based on `blocked` status
      - Block direct voice calls between blocked users
    - **Frontend:**
      - Implement message hiding in `MessageList.tsx` for blocked users
      - Show "Blocked User" placeholder with option to reveal
      - Hide blocked user profiles in shared guild channels
      - Filter blocked users from autocomplete and search results
- [ ] **[Ecosystem] Webhooks & Bot Gateway**
  - **Context:** Expand the platform's utility with third-party integrations.
  - **Implementation:**
    - **Webhooks:**
      - Create `Webhooks` service to handle outgoing POST requests
      - Implement retry logic with exponential backoff
      - Add webhook delivery queue with Redis
      - Support event filtering and payload customization
      - Add webhook management UI in Guild Settings
    - **Bot Gateway:**
      - Implement separate `BotGateway` WebSocket endpoint
      - Isolate bot traffic from user-facing real-time performance
      - Add rate limiting specific to bot connections
      - Support Gateway intents for event filtering
- [ ] **[UX] Production-Scale Polish**
  - **Implementation:**
    - **Virtualized Message Lists:**
      - Research and implement list virtualization for `MessageList.tsx`
      - Use DOM recycling to handle 10,000+ message histories efficiently
      - Maintain scroll position and smooth scrolling behavior
      - Optimize re-renders with memo and granular updates
    - **Global Toast Notification Service:**
      - Create Global Toast Provider using Solid.js context
      - Support multiple notification types (success, error, warning, info)
      - Queue management with automatic dismissal
      - Position control (top-right, bottom-right, etc.)
      - Action buttons for interactive notifications
- [ ] **[UX] Friction-Reduction & Productivity**
  - **Context:** Streamline daily interactions to make the platform feel snappy and reliable.
  - **Implementation:**
    - **Persistent Drafts:**
      - Add `drafts: Record<string, string>` to `MessagesState` in `client/src/stores/messages.ts`
      - Update `setContent` to sync with store drafts automatically
      - Restore draft when navigating back to channel
      - Clear draft only on successful send
    - **Quick Message Actions & Reactions:**
      - Create `MessageActions.tsx` component with common emojis (👍, ❤️, 😂) and action buttons
      - Mount toolbar in `MessageItem.tsx` on hover
      - Position dynamically based on available space
      - Support keyboard shortcuts for quick reactions
    - **Smart Input Auto-complete:**
      - Create `SuggestionPopup.tsx` component with fuzzy matching
      - Trigger popup based on cursor position and prefixes: `@` (users), `#` (channels), `:` (emojis), `/` (commands)
      - Support keyboard navigation (↑↓ + Enter)
      - Cache recent mentions for faster access
    - **Multi-line Input Upgrade:**
      - Refactor: Replace `<input>` with `<textarea>` in `MessageInput.tsx`
      - Implement auto-resize based on content height (max 8 lines)
      - Support Shift+Enter for new lines, Enter for send
      - Maintain cursor position on resize
- [ ] **[Growth] Discovery & Onboarding**
  - **Guild Discovery:**
    - **Backend:** Create public guild listing API with search and filters
    - **Backend:** Add guild tags/categories system
    - **Frontend:** Implement `DiscoveryView.tsx` with category filters and search
    - **Frontend:** Show guild preview cards with member count, description, banner
    - **Admin:** Allow guild owners to opt-in to public directory
  - **Rich Onboarding (FTE):**
    - **Frontend:** Create `OnboardingOverlay.tsx` with step-by-step guide
    - **Steps:** Welcome → Profile Setup → Mic Test → Theme Selection → Join First Guild
    - **UX:** Support skip/back navigation between steps
    - **Integration:** Launch on first login, can be retriggered from settings
- [ ] **[UX] Advanced Search & Discovery**
  - **Full-Text Search:**
    - **Backend:** Implement full-text search indexing using PostgreSQL pg_search or dedicated search index
    - **Backend:** Index messages across all DMs and guilds user has access to
    - **Backend:** Support filters: date range, channel, author, has:link, has:file
    - **Frontend:** Create `GlobalSearch.tsx` results page with filters
    - **Frontend:** Show message context with jump-to-message links
    - **Performance:** Add pagination and result limits
  - **Bulk Read Management:**
    - **Backend:** Add bulk mark-as-read API endpoints
    - **Frontend:** Implement "Mark all as read" in `MessagesState`
    - **UI:** Add actions at category level (DMs, Guild channels)
    - **UI:** Add guild-level "Mark all as read" button
    - **UI:** Add global "Mark everything as read" in Home view
- [ ] **[Compliance] SaaS Trust & Data Governance**
  - **Implementation:**
    - **Data Export (GDPR/CCPA):**
      - Create data export aggregator that generates JSON/ZIP of all user data
      - Include: messages, DMs, profile data, guild memberships, attachments
      - Queue export jobs to prevent resource exhaustion
      - Email download link when export is ready
      - Add "Export My Data" button in Settings > Privacy
    - **Account Erasure:**
      - Implement "Delete Account" workflow with confirmation
      - Soft-delete for 30 days with option to cancel
      - Hard-delete after grace period: anonymize messages, delete profile
      - Notify guild owners when admin users delete accounts
    - **Rate Limiting:**
      - Add per-guild rate limiting to prevent resource exhaustion
      - Protect export endpoints from abuse
- [ ] **[Chat] Slack-style Message Threads**
  - **Context:** Keep channel conversations organized by allowing side-discussions without cluttering the main feed.
  - **Implementation:**
    - **Database:**
      - Migration to add `parent_id` (foreign key to `messages.id`) to `messages` table
      - Add `thread_count` field to parent messages for quick display
      - Add index on `parent_id` for efficient thread queries
    - **Backend:**
      - Update `Message` model with thread fields
      - Implement recursive thread retrieval logic
      - Add thread-specific WebSocket events
      - Add guild-level toggle to enable/disable threads
    - **Frontend:**
      - Create `ThreadSidebar.tsx` for side conversation UI
      - Add "Reply in Thread" action to message context menu
      - Show thread participant count and last reply timestamp
      - Implement thread notifications (separate from channel notifications)
- [ ] **[Media] Advanced Media Processing**
  - **Context:** Improve perceived performance and bandwidth efficiency.
  - **Implementation:**
    - **Backend:**
      - Integrate `blurhash` crate for placeholder generation
      - Use `image` crate to generate lower-resolution thumbnails (preview quality)
      - Process images during S3 upload phase (async job)
      - Store blurhash string and thumbnail URL in database
    - **Frontend:**
      - Display blurhash placeholders while images load
      - Progressive loading: blurhash → thumbnail → full image
      - Smart image loading based on viewport visibility
    - **Storage:**
      - Store multiple resolutions: thumbnail (256px), medium (1024px), full
      - Serve appropriate resolution based on context (list vs detail view)
- [x] **[Branding] Visual Identity & Mascot** ✅
  - **Context:** Establish a recognizable and friendly brand for the platform.
  - **Strategy:** The project mascot is a **Finnish Lapphund**. Generated a premium suite of Solarized Dark assets (Hero, Icon, Monochrome).
  - **Asset Integration Manual:** [asset_integration_manual.md](file:///home/detair/.gemini/antigravity/brain/e405dfe9-b997-4d83-a4a9-ce56d2846159/asset_integration_manual.md)

---

## Phase 6: Competitive Differentiators & Mastery
*Goal: Surpass industry leaders with unique utility and sovereignty features.*

- [ ] **[Client] Mobile Support**
  - Adapt Tauri frontend for mobile or begin Flutter/Native implementation.
- [ ] **[UX] Personal Workspaces (Favorites v2)**
  - **Context:** Solve "Discord Bloat" by letting users aggregate channels from disparate guilds.
  - **Strategy:** Allow users to create custom "Workspaces" that act as virtual folders. Users can drag-and-drop channels from any guild into these workspaces for a unified Mission Control view.
- [ ] **[Content] Sovereign Guild Model (BYO Infrastructure)**
  - **Context:** Provide ultimate data ownership for privacy-conscious groups.
  - **Strategy:** Allow Guild Admins in the SaaS version to provide their own **S3 API** keys and **SFU Relay** configurations, ensuring their media and voice traffic never touches Canis-owned storage.
- [ ] **[Voice] Live Session Toolkits**
  - **Context:** Turn voice channels into productive spaces.
  - **Strategy:** 
    - **Gaming/Raid Kit:** Multi-timer overlays and restricted side-notes for raid leads/shot-callers.
    - **Work/Task Kit:** Shared markdown notepad and collaborative "Action Item" tracking that auto-posts summaries to the channel post-session.
- [ ] **[UX] Context-Aware Focus Engine**
  - **Context:** Prevent platform fatigue with intelligent notification routing.
  - **Strategy:** Use Tauri's desktop APIs to detect active foreground apps (e.g., IDEs, DAWs). Implement **VIP/Emergency Overrides** that allow specific users or channels to bypass DND during focused work sessions.
- [ ] **[SaaS] The Digital Library (Wiki Mastery)**
  - **Context:** Transform "Information Pages" into a structured Knowledge Base.
  - **Strategy:** Enhance current Info Pages with version recovery, deep-linkable sections, and a "Library" view for long-term guild documentation.

---

## Phase 7: Long-term / Optional SaaS Polish
*Goal: Highly optional features aimed at commercial scale and enterprise utility.*

- [ ] **[SaaS] Billing & Subscription Infrastructure**
  - **Context:** Enable monetization for the managed hosting version.
  - **Strategy:** Integrate **Stripe** for subscription management. Create a `BillingService` to handle quotas, "Boost" payments, and tiered access levels.
- [ ] **[Compliance] Accessibility (A11y) & Mastery**
  - **Context:** Ensure the platform is usable by everyone and meets enterprise requirements.
  - **Strategy:** Conduct a full A11y audit. Implement WCAG compliance standards, screen-reader optimizations, and a robust Keyboard-Only navigation mode (`Cmd+K` Quick Switcher).
- [ ] **[SaaS] Identity Trust & OAuth Linking**
  - **Context:** Improve community trust and security.
  - **Strategy:** Allow users to link external accounts (GitHub, Discord, LinkedIn). Enable Guilds to set entrance requirements based on "Verified Identity" signals.
- [ ] **[Infra] SaaS Observability & Telemetry**
  - **Context:** Maintain uptime and catch bugs at scale.
  - **Strategy:** Integrate **Sentry** (error tracking) and **OpenTelemetry** for performance monitoring across the Rust backend and Tauri clients.

---

## Recent Changes

### 2026-01-31
- **Toast Component Tests** - Created comprehensive test suite for toast notification system (16 passing tests, 5 skipped timing tests). Tests verify API behavior including deduplication, max visible limit (5), dismissal, and toast types. Added duration: 0 workaround for vitest window.setTimeout issues.
- **Tauri WebSocket Event Parity Phase 1** (Issue #132) - Added 11 ServerEvent variants to Rust backend: Call events (IncomingCall, CallStarted, CallEnded, CallParticipantJoined, CallParticipantLeft, CallDeclined), Read sync (ChannelRead, DmRead, DmNameUpdated), and Reactions (ReactionAdd, ReactionRemove). Added corresponding Tauri event listeners in frontend. Progress: 11/22 events completed.
- **Clippy Warning Cleanup** - Fixed 169 clippy warnings across the codebase (186 → 17 remaining):
  - doc_markdown: 50 fixes (added backticks around technical terms)
  - Automatic fixes: 111 warnings (use_self, uninlined_format_args)
  - or_fun_call: 8 fixes (replaced closures with function pointers)
  - format_push_string: 2 fixes (replaced push_str(format!()) with write!/writeln!)
  - collection_is_never_read: 1 fix (scoped unused variable locally)
  - items_after_statements: 1 fix (moved imports to module level)

### 2026-02-01
- **First User Setup HTTP Integration Tests** - Created reusable `TestApp` infrastructure in `server/tests/helpers/mod.rs` (first HTTP-level test infrastructure in the project). Added 8 HTTP integration tests for setup endpoints covering: status endpoint, config access control, auth requirement, admin requirement, successful completion with DB verification, validation errors, and idempotency. Filed follow-up issues for test architecture improvements: cleanup guards (#137), shared DB pool (#138), stateful middleware testing (#139), concurrent HTTP completion test (#140).
- Marked **SSO / OIDC Integration** complete (PR #135) - Admin-configurable OIDC providers with dynamic discovery and automatic account linking. Phase 4 completion increased to 92% (22 of 24 features complete).
- Completed **Screen Share Tauri Parity, Tests & Viewer Shortcuts** (PR #134) - Added ScreenShareStarted/Stopped/QualityChanged to Tauri WebSocket client, keyboard shortcuts (Escape/V/M/F) for ScreenShareViewer, exported screen share handlers for testability, server event serialization tests, and client handler tests.

### 2026-01-30
- Completed **Content Spoilers & Enhanced Mentions** (PR #128) - Content spoilers with `||text||` syntax for hiding sensitive information, MENTION_EVERYONE permission (bit 23) for controlling @everyone/@here mentions, server-side permission validation, ReDoS protection (500 char limit), XSS prevention via DOMPurify, 3 integration tests and 9 unit tests.
- Completed **Home Page Unread Aggregator** (PR #127) - Centralized view of unread messages across all guilds and DMs in modular sidebar with optimized database queries, direct navigation, comprehensive error handling with toast notifications, and automatic refresh on window focus.
- Phase 4 completion increased to 88% (21 of 24 features complete at the time).

### 2026-01-29
- Completed **First User Setup (Admin Bootstrap)** (PR #110) - First user automatically receives system admin permissions with PostgreSQL row-level locking to prevent race conditions, setup wizard for server configuration, and atomic setup completion using compare-and-swap pattern.
- Fixed critical security issues identified in PR review:
  - Rate limiter fail-closed pattern (prevents bypass on Redis failure)
  - WebSocket listener memory leak (module-level registration flag)
  - Store encapsulation (dedicated functions instead of direct state mutation)
  - JWT token validation before API calls (structure and expiry checks)
  - Structured error logging for database operations (query context for debugging)
- Added tech debt items for HTTP integration tests and additional error handling improvements.
- **Enhanced Phase 4-5 Implementation Details**: Merged detailed implementation plans from project brain storage into main roadmap.
- **Added Completed Items**: DM Avatars (Issue #104), Pinned Notes System (Issue #105).
- **Added New Phase 4 Item**: Unified File Size Upload Limits - standardize avatar, emoji, and attachment size restrictions across all upload endpoints.
- **Expanded Context Menus**: Added detailed implementation steps for ContextMenuProvider architecture.
- **Expanded Home Unread Aggregator**: Added backend query and frontend component details.
- **Expanded Spoilers & Mentions**: Added specific CSS, parser, and permission bit implementation details.
- **Expanded Emoji Picker Polish**: Added floating-ui integration and viewport boundary handling details.
- **Expanded UX Productivity Features**: Added detailed implementation for persistent drafts, hover toolbar, auto-complete, and multi-line input.
- **Expanded Production-Scale Polish**: Added virtualized message list and global toast notification implementation details.
- **Expanded Safety Features**: Added detailed implementation for moderation filters, user blocking, and reporting workflows.
- **Expanded Ecosystem Features**: Added webhook delivery queue and bot gateway implementation details.
- **Expanded Discovery & Onboarding**: Added guild directory API and step-by-step onboarding flow details.
- **Expanded Search & Threads**: Added full-text search indexing, bulk read management, and Slack-style thread implementation details.
- **Expanded Media & Compliance**: Added blurhash generation, progressive loading, and GDPR data export implementation details.

### 2026-01-28
- Added **Phase 7: Optional SaaS Polish**: Documented low-priority commercial features (Billing, A11y, OAuth Identity, Observability) as secondary to the self-hosted focus.
- Added **Phase 6: Competitive Mastery**: Integrated unique features including Personal Workspaces, Sovereign Guild (BYO Storage), Live Session Toolkits (Gaming/Work), and Context-Aware Focus Engine.
- Expanded Roadmap with SaaS Pillars: Added granular plans for **User Safety** (Blocking, Moderation Filters, Reporting), **Developer Ecosystem** (Webhooks, Bot Gateway), **Scale UX** (Virtualization, Toasts), **Discovery** (Guild Directory, Onboarding), and **Search**.
- Added Roadmap Extension: Unified file size limits, Home unread summaries, Context menus, Spoilers, Mention permissions, and Message threading.
- Added Guild Emoji Manager (PR #46) - Custom guild emojis, animated support, manager UI, and upload tools.
- Fixed server build issues by adding `Deserialize` to `GuildEmoji`.
- Fixed various clippy warnings and documentation issues.

### 2026-01-24
- Added Cross-Server Favorites (PR #45) - Pin channels from different guilds, star icon toggle, grouped Favorites section.
- Added Modular Home Sidebar - Collapsible modules (Active Now, Pending, Pins) in Home right panel with server-synced preferences.

### 2026-01-23
- Marked Rich Presence (Game Activity) complete - was already implemented.
- Marked Sound Pack (Notification Sounds) complete - was already implemented.
- Added Cross-Client Read Sync - DM read status syncs instantly across all user devices.
- Added Do Not Disturb Mode - Notification sounds suppressed during DND status or quiet hours.
- Added E2EE DM Messaging (PR #41) - End-to-end encryption for DM conversations using vodozemac.
- Updated encryption architecture docs with implementation details.

### 2026-01-21
- Added Sound Pack (Notification Sounds) design to Phase 4.
- Added Modular Home Sidebar to Phase 4 roadmap.
- Refactored Home View Sidebar and Friends List UI.

### 2026-01-20
- Merged PR #29: E2EE Key Backup UI - Recovery Key Modal, Security Settings, Tauri commands
- Merged PR #23: User Connectivity Monitor
- Fixed TimescaleDB migration to work conditionally (supports standard PostgreSQL)
- Cleaned up obsolete documentation