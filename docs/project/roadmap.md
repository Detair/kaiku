# VoiceChat (Canis) Roadmap

This roadmap outlines the development path from the current prototype to a production-ready, multi-tenant SaaS platform.

**Current Phase:** Phase 6 (Competitive Differentiators & Mastery) - In Progress

**Last Updated:** 2026-02-27

## Quick Status Overview

| Track | Phase | Status | Completion | Key Achievements |
|-------|-------|--------|------------|------------------|
| **Foundation** | **Phase 0** | ✅ Complete | 100% | N+1 fix, WebRTC optimization, MFA encryption |
| **Foundation** | **Phase 1** | ✅ Complete | 100% | Voice state sync, audio device selection |
| **Foundation** | **Phase 2** | ✅ Complete | 100% | Voice Island, VAD, Speaking Indicators, Command Palette, File Attachments, Theme System, Code Highlighting |
| **Foundation** | **Phase 3** | ✅ Complete | 100% | Guild system, Friends, DMs, Home View, Rate Limiting, Permission System + UI, Information Pages, DM Voice Calls |
| **Foundation** | **Phase 4** | ✅ Complete | 100% | E2EE DM Messaging, User Connectivity Monitor, Rich Presence, First User Setup, Context Menus, Emoji Picker Polish, Unread Aggregator, Content Spoilers, Forgot Password, SSO/OIDC, User Blocking & Reports |
| **Expansion** | **Phase 5** | ✅ Complete | 100% (17/17) | E2E suite, CI hardening, bot platform, search upgrades, threads, multi-stream partial, slash command reliability, production-scale polish, content filters, webhooks, bulk read management, guild discovery & onboarding, guild resource limits, progressive image loading, data governance |
| **Expansion** | **Phase 6** | 🔄 In Progress | 40% (2/5) | Personal workspaces, digital library, mobile, live session toolkits, focus engine (sovereign guild model deferred) |
| **Scale and Trust** | **Phase 7** | 📋 Planned | 0% | Billing, accessibility, identity trust, observability |
| **Scale and Trust** | **Phase 8** | 📋 Planned | 0% | Performance budgets, chaos drills, upgrade safety, FinOps, isolation testing |
| **Scale and Trust** | **Phase 10** | 📋 Planned | 0% | SaaS scaling architecture |

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
- ✅ User blocking with Redis-backed enforcement across DMs, friend requests, and voice
- ✅ User reporting system with admin claim/resolve workflow and real-time notifications

## High-Level Topic Map (Official)

This section is the canonical high-level roadmap view. Detailed implementation checklists remain in phase sections and linked plans.

### Foundation (Delivered)
- Core collaboration platform (guilds, channels, DMs, permissions, realtime chat).
- Voice and media core (voice rooms, DM calls, screen sharing, connection monitoring).
- Security baseline (MFA encryption, rate limiting, OIDC/SSO, password reset, E2EE DM and key backup).
- Productivity UX (command palette, favorites, unread sync, context menus, search, threads).
- Admin and governance baseline (admin tooling, reporting workflows, audit-oriented controls).

### Expansion (Delivered / In Progress)
- ✅ Developer ecosystem growth (bot platform, webhooks, gateway, slash commands).
- ✅ Safety maturity (advanced moderation filters, content policy tooling, data governance).
- ✅ Growth and onboarding (guild discovery, first-time experience, activation/retention UX).
- ✅ Voice/media maturity (multi-stream, advanced media processing, progressive image loading).
- Mobile strategy execution (Android-first path and shared Rust core evolution).
- Personal workspaces, sovereign guild model, live session toolkits, focus engine, digital library.

### Scale and Trust (Planned)
- SRE foundations (SLOs, observability standards, alerting, incident playbooks) ([Design](../plans/2026-02-15-sre-foundations-design.md), [OTel Reference](../plans/2026-02-15-opentelemetry-grafana-reference-design.md), [Implementation](../plans/2026-02-15-operational-safety-implementation-plan.md)).
- Backup, restore, and disaster recovery drills (database, object storage, key material) ([Design](../plans/2026-02-15-backup-restore-drills-design.md), [Implementation](../plans/2026-02-15-topic-2-expansion-implementation-plan.md)).
- Release governance (feature flags, staged rollouts, rollback playbooks) ([Design](../plans/2026-02-15-release-governance-design.md), [Implementation](../plans/2026-02-15-operational-safety-implementation-plan.md)).
- Documentation governance (canonical feature matrix and active/superseded plan lifecycle) ([Design](../plans/2026-02-15-documentation-governance-design.md), [Implementation](../plans/2026-02-15-documentation-governance-implementation.md)).
- Security verification cadence (threat-model refreshes, boundary regression suites, external testing) ([Design](../plans/2026-02-15-security-verification-cadence-design.md), [Implementation](../plans/2026-02-15-operational-safety-implementation-plan.md)).
- SaaS and compliance readiness (limits/monetization, data governance, accessibility, identity trust) ([Design](../plans/2026-02-15-saas-compliance-readiness-design.md), [Implementation](../plans/2026-02-15-topic-2-expansion-implementation-plan.md)).
- Infrastructure scale-out (storage and CDN architecture for large media workloads) ([Design](../plans/2026-02-15-infrastructure-scale-out-design.md), [Implementation](../plans/2026-02-15-topic-2-expansion-implementation-plan.md)).
- Sovereign/BYO deployment options (customer-controlled storage and relay paths) ([Design](../plans/2026-02-15-sovereign-byo-deployment-design.md), [Implementation](../plans/2026-02-15-topic-2-expansion-implementation-plan.md)).
- Reliability and operability excellence (performance budgets, chaos testing, upgrade safety, FinOps, policy-as-code, plugin security, tenancy isolation, operator supportability) ([Implementation](../plans/2026-02-15-phase-8-reliability-implementation.md)).

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

## Phase 4: Advanced Features ✅ **COMPLETED**
*Goal: Add competitive differentiators and trust/security foundations.*

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
- [x] **[Safety] Absolute User Blocking** ✅
  - Block/unblock via context menu with confirmation modal.
  - Redis SET-based block cache (`blocks:{user_id}`, `blocked_by:{user_id}`) for O(1) lookups.
  - Blocked users cannot send DMs, friend requests, or initiate voice calls.
  - Messages from blocked users filtered in DM channel message lists.
  - WebSocket events (typing, presence, messages) filtered in real-time.
  - Block/unblock WebSocket events for instant client-side sync.
  - 8 HTTP integration tests covering block, unblock, self-block, auth, and enforcement.
- [x] **[Safety] User Reporting & Admin Workflow** ✅
  - Report users or messages with categories (harassment, spam, inappropriate content, impersonation, other).
  - Rate-limited (5 reports/hour) with duplicate prevention via unique index.
  - Admin report queue with claim, resolve (dismiss/warn/ban/escalate), and stats endpoints.
  - Real-time admin notifications via WebSocket when reports are created or resolved.
  - Admin reports panel in dashboard with status/category filters and pagination.
  - 17 HTTP integration tests covering user reports (6) and admin report management (11).
---

## Phase 5: Ecosystem & SaaS Readiness ✅ **COMPLETED**
*Goal: Open the platform to developers and prepare for massive scale.*
- Implementation coverage for remaining open items is tracked per checklist item via dedicated Design and Implementation links.

- [x] **[Test] E2E UI Test Coverage Suite** ✅
  - Comprehensive Playwright E2E test suite covering 68 UI items across 12 spec files.
  - Shared test helpers (`e2e/helpers.ts`) with login, navigation, and utility functions.
  - Coverage tracking document (`docs/testing/ui-coverage.md`) mapping every UI item to its test.
  - First run: 8/68 passing without backend (auth form rendering); remaining 58 need running backend + seed data.
  - Areas covered: Auth, Navigation, Messaging, Guild, Channels, Friends/DMs, Settings, Voice, Admin, Search, Permissions.
- [x] **[Infra] CI Pipeline Hardening & Green Build** ✅
  - All CI jobs passing: Rust Lint (fmt + clippy), Rust Tests, Frontend, License Compliance, Secrets Scan, Docker Build, Tauri (Ubuntu + macOS).
  - Moved `@everyone` security test into `rust-test` job (was exhausting disk space in standalone Docker build).
  - Fixed Dockerfile shared crate paths and bumped Rust image to 1.88 for edition2024 support.
  - Added `icon.ico` to repo for Tauri deb bundling.
  - Added CI pipeline documentation at `docs/development/ci.md`.
  - **Known limitation:** Windows Tauri build fails (`libvpx` not available via choco), marked `continue-on-error: true`.
- [x] **[API] Bot Ecosystem** ✅
  - ✅ Database schema (bot_applications, slash_commands, guild_bot_installations, users.is_bot)
  - ✅ Bot application management API (create, list, get, delete, token reset)
  - ✅ Secure bot token auth (Argon2id, indexed O(1) lookup, TOCTOU protection)
  - ✅ Slash command registration API (guild-scoped + global, bulk registration)
  - ✅ Bot Gateway WebSocket (`/api/gateway/bot`) with Redis pub/sub event system
  - ✅ Command invocation routing with ambiguity detection
  - ✅ Bot message sending with channel membership authorization
  - ✅ Command response handling with Redis storage and 5-minute TTL
  - ✅ Guild bot installation API with permission checks
  - ✅ Frontend: Bot applications management UI in settings
  - ✅ Frontend: API client library for all bot operations
  - ✅ 14 comprehensive integration tests
  - ✅ Frontend: Slash command management UI (register, view, delete commands with options)
  - ✅ Developer documentation / bot API guide (`docs/development/bot-system.md`)
  - ✅ Frontend: Guild bot management UI in guild settings (list, remove)
- [x] **[Voice] Multi-Stream Support**
  - ✅ Simultaneous Webcam and Screen Sharing (browser).
  - ✅ SFU renegotiation for dynamic track add/remove mid-session.
  - ✅ Track source identification via pending source queue.
  - [ ] Tauri native webcam capture (Rust-side `start_webcam`/`stop_webcam` commands).
  - [ ] Implement Simulcast (quality tiers) for bandwidth management.
- [ ] **[Voice] Evaluate str0m as WebRTC Alternative** ([Design](../plans/2026-02-15-phase-5-str0m-evaluation-design.md), [Implementation](../plans/2026-02-15-phase-5-str0m-evaluation-implementation.md)) `Priority: Low`
  - Current stack: webrtc-rs 0.11 (full-stack, owns I/O). Working, but project is stagnating.
  - Alternative: [str0m](https://github.com/algesten/str0m) — Sans-IO WebRTC library (pure Rust).
  - **Benefits:** Full I/O control, deterministic testing, better SFU fit, no hidden task spawning.
  - **Cost:** ~5,700 lines server + ~430 lines client rewrite (different programming model).
  - **Trigger:** Migrate when a security fix forces action on webrtc-rs, when hitting a performance wall requiring tighter I/O control, or during major voice architecture changes (e.g., MLS E2EE).
  - mediasoup (C++ FFI) ruled out due to `unsafe_code = "forbid"` policy.
- [x] **[SaaS] Limits & Monetization Logic** ✅ (PR #247) ([Design](../plans/2026-02-15-phase-5-limits-monetization-design.md), [Implementation](../plans/2026-02-15-phase-5-limits-monetization-implementation.md))
  - ✅ Configurable per-instance resource limits: guilds per user, members/channels/roles/emojis/bots per guild, webhooks per app
  - ✅ Server-side enforcement at 8 code points with `LIMIT_EXCEEDED` (403) errors
  - ✅ Guild usage stats endpoint (`GET /api/guilds/{id}/usage`) with parallel count queries
  - ✅ Public instance limits endpoint (`GET /api/config/limits`)
  - ✅ `plan` column on guilds (default: "free") preparing for tier/boost system
  - ✅ Frontend Usage tab in guild settings with progress bars and color-coded thresholds
  - ✅ 10 integration tests covering all enforcement points
  - [ ] Implement "Boost" logic for lifting limits per guild (future billing integration)
- [x] **[Safety] Advanced Moderation & Safety Filters** ✅ (PR #206) ([Design](../plans/2026-02-15-phase-5-moderation-filters-design.md), [Implementation](../plans/2026-02-15-phase-5-moderation-filters-implementation.md))
  - Guild-configurable content filters with Aho-Corasick keyword matching and regex pattern support.
  - Built-in filter categories: Slurs, Hate Speech, Spam, Abusive Language — each with configurable actions (Block, Log, Warn).
  - Custom guild patterns with regex support and ReDoS protection (compilation + 10ms timeout validation).
  - Per-guild `FilterEngine` cache using `DashMap` with generation counters for TOCTOU-safe invalidation.
  - Moderation action log with paginated viewing and content truncation (200 chars).
  - Dry-run test endpoint for admins to preview filter behavior without affecting production cache.
  - Integrated into message create, edit, and file upload flows; skips encrypted and DM messages.
  - Frontend Safety tab in Guild Settings with category toggles, custom pattern CRUD, test panel, and moderation log viewer.
  - 17 integration tests + 9 unit tests covering config CRUD, message blocking, encrypted/DM skip, log actions, permissions, and cache invalidation.
- [x] **[Ecosystem] Webhooks & Bot Gateway** ✅ (PR #208) ([Design](../plans/2026-02-15-phase-5-webhooks-bot-gateway-design.md), [Implementation](../plans/2026-02-15-phase-5-webhooks-bot-gateway-implementation.md))
  - ✅ Webhook system with HMAC-SHA256 signed payloads and automatic retry with exponential backoff (5 attempts)
  - ✅ Event types: `message.created`, `member.joined`, `member.left`, `command.invoked`
  - ✅ Dead-letter storage for failed deliveries and delivery log for debugging
  - ✅ Webhook management UI with create/edit/delete, event type selection, test ping, and delivery log viewer
  - ✅ Bot gateway intents (`messages`, `members`, `commands`) for event filtering
  - ✅ `MemberJoined` and `MemberLeft` gateway events for bots with `members` intent
  - ✅ DNS rebinding SSRF protection and webhook signing secrets encrypted at rest
- [x] **[Ecosystem] Slash Command Reliability & /ping Reference Command** ✅ ([Design](../plans/2026-02-16-slash-command-reliability-design.md), [Implementation](../plans/2026-02-16-slash-command-reliability-implementation.md))
  - ✅ Global command uniqueness index and batch duplicate detection (409 Conflict)
  - ✅ Guild command listing shows all providers with `is_ambiguous` flag (removed DISTINCT ON)
  - ✅ Command response delivery via WebSocket relay (ephemeral + non-ephemeral)
  - ✅ 30-second response timeout notification
  - ✅ Ambiguity error includes conflicting bot names
  - ✅ Structured bot gateway error events (invalid_json, handler_error)
  - ✅ Frontend autocomplete: hyphen support, ambiguity labels, fetch retry fix
  - ✅ Built-in `/ping` command (no bot required)
  - ✅ Example bot script (`docs/examples/ping-bot.py`)
  - ✅ 4 new integration tests (21 total bot ecosystem tests passing)
- [x] **[UX] Production-Scale Polish** ([Design](../plans/2026-02-16-production-scale-polish-design.md), [Implementation](../plans/2026-02-16-production-scale-polish-implementation.md)) ✅
  - **Completed:**
    - ✅ Replaced custom virtualizer with `@tanstack/solid-virtual` (ResizeObserver-based dynamic sizing)
    - ✅ Virtualized guild member list (`MembersTab.tsx`)
    - ✅ Virtualized DM conversation sidebar (`HomeSidebar.tsx`)
    - ✅ Virtualized search results (`SearchPanel.tsx`)
    - ✅ Toast convention documentation with type/duration table
    - ✅ Standardized toast durations and dedup IDs across 23 files
    - ✅ Fixed toast test suite (18 tests, was silently excluded by vitest config)
- [x] **[UX] Friction-Reduction & Productivity** ✅
  - **Context:** Streamline daily interactions to make the platform feel snappy and reliable.
  - **Completed:**
    - ✅ Persistent message drafts per channel with auto-restore on navigation
    - ✅ Quick reaction toolbar on message hover (👍, ❤️, 😂, 😮) with full emoji picker
    - ✅ Multi-line textarea input with auto-resize (max 8 lines), Shift+Enter for newlines
    - ✅ @user and :emoji: autocomplete with keyboard navigation (↑↓ + Enter/Tab)
    - ✅ #channel autocomplete for mentioning text channels in guild messages
    - ✅ /command autocomplete for browsing slash commands from installed bots
    - ✅ Alt+1..4 keyboard shortcuts for quick reactions on hovered messages
    - ✅ Backend `GET /api/guilds/{id}/commands` endpoint for guild command listing
- [x] **[Growth] Discovery & Onboarding** ✅ (PRs #244, #245, #246) ([Design](../plans/2026-02-15-phase-5-discovery-onboarding-design.md), [Implementation](../plans/2026-02-15-phase-5-discovery-onboarding-implementation.md))
  - **Guild Discovery:**
    - ✅ Database migration with `discoverable`, `tags`, `banner_url` columns and full-text `search_vector`
    - ✅ Public browse API (`GET /api/discover/guilds`) with full-text search, tag filtering, sort (members/newest), pagination
    - ✅ Public join API (`POST /api/discover/guilds/{id}/join`) with rate limiting and `MemberJoined` WebSocket broadcast
    - ✅ Denormalized `member_count` with PostgreSQL trigger and CHECK constraints
    - ✅ Guild settings extension: discoverable toggle, tags editor (max 5, case-insensitive), banner URL with HTTPS validation
    - ✅ Frontend `DiscoveryView.tsx` with search (300ms debounce), sort toggle, pagination, loading skeletons, error states
    - ✅ Frontend `GuildCard.tsx` with banner gradient, member count, tags, join button
    - ✅ Compass icon in ServerRail for discovery navigation
    - ✅ Server config flag `ENABLE_GUILD_DISCOVERY` (default true)
  - **Rich Onboarding (FTE):**
    - ✅ 5-step `OnboardingWizard.tsx`: Welcome → Theme → Mic Setup → Join Server → Done
    - ✅ Extracted `MicTestPanel.tsx` from `MicrophoneTest.tsx` for inline embedding
    - ✅ Step 4 dual-tab: mini discovery grid (top 6 guilds) or invite code input
    - ✅ Skip/back navigation, progress dots, focus trap for accessibility
    - ✅ ARIA tabs pattern with proper `role`, `aria-controls`, `tabpanel` attributes
    - ✅ Re-trigger from Settings > Appearance ("Re-run Setup Wizard" button)
    - ✅ Shows on first login when `onboarding_completed` preference is falsy
- [x] **[UX] Advanced Search & Discovery** ✅ ([Design](../plans/2026-02-15-phase-5-search-discovery-design.md), [Implementation](../plans/2026-02-15-phase-5-search-discovery-implementation.md))
  - **Full-Text Search:**
    - ✅ **Backend:** Implement full-text search indexing using PostgreSQL tsvector with GIN index
    - ✅ **Backend:** Guild-scoped message search with `websearch_to_tsquery` (supports AND, OR, quotes, negation)
    - ✅ **Backend:** Permission validation (guild membership check) and rate limit enforcement
    - ✅ **Backend:** Bulk user/channel lookups to prevent N+1 queries
    - ✅ **Frontend:** Search panel overlay (`SearchPanel.tsx`) with debounced input and pagination
    - ✅ **Frontend:** XSS-safe result highlighting and click-to-navigate with message highlight
    - ✅ **Performance:** Pagination implemented (limit/offset with clamping to max 100)
    - ✅ **Backend:** DM message search endpoint (`GET /api/dm/search`) with same filter support
    - ✅ **Backend:** Advanced filters: date range, channel, author, has:link, has:file
    - ✅ **Backend:** Relevance ranking with `ts_rank` and sort toggle (Relevance / Date)
    - ✅ **Backend:** Server-side context snippets using `ts_headline` with `<mark>` tags
    - ✅ **Backend:** Global search across all guilds and DMs (`GET /api/search`)
    - ✅ **Frontend:** Global search UI with Ctrl+Shift+F shortcut and "Search Everywhere" in Command Palette
    - ✅ **Frontend:** Search syntax help tooltip (AND, OR, "exact phrase", -exclude)
    - ✅ **Backend:** Dedicated Search rate limit category (15 req/min)
    - ✅ **Tests:** 38 integration tests (18 global search, 20 guild/DM search) covering auth, access control (non-member 403, nonexistent guild 404), soft-deleted/encrypted exclusion, date/author/has:link/has:file filters, relevance ranking, headlines, sort (relevance/date), pagination, limit clamping, validation (data-driven). 11 shared test helpers.
    - **Tech Debt:**
      - [x] ~~Implement channel-level permission filtering~~ — Already implemented via VIEW_CHANNEL checks in guild, DM, and global search (verified 2026-02-19)
      - [x] Add integration tests for search edge cases (TD-08): special characters, long queries (>1000 chars), SQL injection, XSS, channel permission filtering (10 tests in `search_http_test.rs`)
      - [x] Add security tests (TD-08): SQL injection, XSS via search content, channel permission bypass — covered in same test file
      - [x] Add search query analytics logging (TD-30): structured tracing with user_id, query_length, result_count, duration_ms
      - [ ] Monitor and optimize search performance at scale
  - **Bulk Read Management:**
    - ✅ **Backend:** Bulk mark-as-read API endpoints (`POST /api/me/read-all`, `POST /api/guilds/{id}/read-all`, `POST /api/dm/read-all`)
    - ✅ **Frontend:** Mark-as-read in channels and DMs stores with optimistic updates
    - ✅ **UI:** Per-guild "Mark all as read" button in UnreadModule
    - ✅ **UI:** DM-level "Mark all as read" button in UnreadModule
    - ✅ **UI:** Global "Mark everything as read" button in UnreadModule
    - ✅ **UI:** Per-channel "Mark as Read" in context menu
    - ✅ Cross-device sync via `ChannelRead`/`DmRead` WebSocket events
- [x] **[Compliance] SaaS Trust & Data Governance** ✅ (PR #249) ([Design](../plans/2026-02-15-phase-5-trust-governance-design.md), [Implementation](../plans/2026-02-15-phase-5-trust-governance-implementation.md))
  - ✅ Data export pipeline: background worker gathers user data (profile, messages, guilds, friends, preferences) into versioned JSON ZIP archive, uploads to S3, sends email notification; downloads expire after 7 days
  - ✅ Account deletion lifecycle: 30-day grace period with cancellation support, password verification for local auth, guild ownership transfer required before deletion
  - ✅ Message anonymization: `messages.user_id` FK changed from CASCADE to SET NULL — deleted users' messages preserved with author removed
  - ✅ Database migration: `data_export_jobs` table, user deletion columns, 13 bare FK constraints fixed (SET NULL for audit/attribution, CASCADE for user-specific data)
  - ✅ Rate limiting: `DataGovernance` category (2 req/60s) for export and deletion mutation endpoints
  - ✅ Background cleanup workers in hourly loop: process expired deletions, clean up expired export archives and S3 objects
  - ✅ UserProfile includes `deletion_scheduled_at` for client-side cancellation banner
  - ✅ 11 integration tests covering deletion flows, password validation, guild ownership blocking, cancellation
- [x] **[Chat] Slack-style Message Threads** ✅
  - **Context:** Keep channel conversations organized by allowing side-discussions without cluttering the main feed.
  - **Completed:**
    - ✅ Database migration: `parent_id` FK, `thread_reply_count`, `thread_last_reply_at`, `thread_read_state` table
    - ✅ Backend: Thread reply CRUD, WebSocket events (`thread_reply_new`, `thread_reply_delete`, `thread_read`)
    - ✅ Frontend: `ThreadSidebar`, `ThreadIndicator` with participant avatars and unread dot
    - ✅ Batch thread info in message list response (participants, avatars, unread state) — no N+1
    - ✅ Thread unread tracking: server-side read state + client-side WebSocket tracking
    - ✅ 11+ server integration tests
  - **Remaining:**
    - [x] Guild-level toggle to enable/disable threads ✅ (migration `20260211000000_guild_threads_enabled.sql`, backend GET/PATCH `/api/guilds/{id}/settings`, frontend GeneralTab toggle, enforcement in message creation)
- [x] **[Media] Advanced Media Processing** ✅ (PR #248) ([Design](../plans/2026-02-15-phase-5-media-processing-design.md), [Implementation](../plans/2026-02-15-phase-5-media-processing-implementation.md))
  - ✅ Progressive image loading with blurhash placeholders — uploaded images processed to generate blurhash color previews, thumbnail (256px) and medium (1024px) WebP variants
  - ✅ Client displays instant blurhash placeholder while thumbnail loads, with smooth fade-in transition; click opens full-resolution original
  - ✅ Aspect-ratio CSS prevents layout shift during load
  - ✅ Image variant download support — `GET /api/messages/attachments/{id}/download?variant=thumbnail|medium` serves bandwidth-efficient WebP variants with graceful fallback
- [x] **[Branding] Visual Identity & Mascot** ✅
  - **Context:** Establish a recognizable and friendly brand for the platform.
  - **Strategy:** The project mascot is a **Finnish Lapphund**. Generated a premium suite of Solarized Dark assets (Hero, Icon, Monochrome).
  - **Asset Integration Manual:** [asset_integration_manual.md](file:///home/detair/.gemini/antigravity/brain/e405dfe9-b997-4d83-a4a9-ce56d2846159/asset_integration_manual.md)

---

## Phase 6: Competitive Differentiators & Mastery 🔄 **IN PROGRESS**
*Goal: Surpass industry leaders with unique utility and sovereignty features.*
- Implementation coverage: [Mobile + Workspaces](../plans/2026-02-15-phase-6-mobile-workspaces-implementation.md), [Sovereign + Live Toolkit](../plans/2026-02-15-phase-6-sovereign-livekit-implementation.md), [Focus + Library](../plans/2026-02-15-phase-6-focus-library-implementation.md).

- [ ] **[Client] Mobile Support** ([Design](../plans/2026-02-15-phase-6-mobile-workspaces-design.md), [Implementation](../plans/2026-02-15-phase-6-mobile-workspaces-implementation.md))
  - Adapt Tauri frontend for mobile or begin Flutter/Native implementation.
- [x] **[UX] Personal Workspaces (Favorites v2)** ([Design](../plans/2026-02-15-phase-6-mobile-workspaces-design.md), [Implementation](../plans/2026-02-15-phase-6-mobile-workspaces-implementation.md)) — (#250)
  - 9 REST endpoints, 7 WebSocket events, cross-guild channel aggregation with drag-and-drop reordering.
  - Configurable limits (`MAX_WORKSPACES_PER_USER`, `MAX_ENTRIES_PER_WORKSPACE`), atomic CTE for concurrency safety, 17 integration tests.
- [ ] **[Content] Sovereign Guild Model (BYO Infrastructure)** *(Optional — future consideration)* ([Design](../plans/2026-02-15-phase-6-sovereign-livekit-design.md), [Implementation](../plans/2026-02-15-phase-6-sovereign-livekit-implementation.md))
  - **Context:** Provide ultimate data ownership for privacy-conscious groups.
  - **Strategy:** Allow Guild Admins in the SaaS version to provide their own **S3 API** keys and **SFU Relay** configurations, ensuring their media and voice traffic never touches Canis-owned storage.
- [ ] **[Voice] Live Session Toolkits** ([Design](../plans/2026-02-15-phase-6-sovereign-livekit-design.md), [Implementation](../plans/2026-02-15-phase-6-sovereign-livekit-implementation.md))
  - **Context:** Turn voice channels into productive spaces.
  - **Strategy:** 
    - **Gaming/Raid Kit:** Multi-timer overlays and restricted side-notes for raid leads/shot-callers.
    - **Work/Task Kit:** Shared markdown notepad and collaborative "Action Item" tracking that auto-posts summaries to the channel post-session.
- [ ] **[UX] Context-Aware Focus Engine** ([Design](../plans/2026-02-15-phase-6-focus-library-design.md), [Implementation](../plans/2026-02-15-phase-6-focus-library-implementation.md))
  - **Context:** Prevent platform fatigue with intelligent notification routing.
  - **Strategy:** Use Tauri's desktop APIs to detect active foreground apps (e.g., IDEs, DAWs). Implement **VIP/Emergency Overrides** that allow specific users or channels to bypass DND during focused work sessions.
- [x] **[SaaS] The Digital Library (Wiki Mastery)** ([Design](../plans/2026-02-15-phase-6-focus-library-design.md), [Implementation](../plans/2026-02-15-phase-6-focus-library-implementation.md)) ✅
  - **Context:** Transform "Information Pages" into a structured Knowledge Base.
  - **Strategy:** Enhance current Info Pages with version recovery, deep-linkable sections, and a "Library" view for long-term guild documentation.
  - **Delivered:** Revision history with content snapshots and pruning, guild-scoped page categories with CRUD/reordering, deep-linkable heading anchors, library catalog view, per-guild configurable limits with admin overrides (13 new API endpoints). Five code review rounds addressed 11 security findings including CSS injection via SVG `<style>` tags, path traversal validation, IDOR scope checks, and best-effort revision snapshots.

---

## Phase 7: Long-term / Optional SaaS Polish 📋 **PLANNED**
*Goal: Highly optional features aimed at commercial scale and enterprise utility.*
- Implementation coverage: [Billing + Identity](../plans/2026-02-15-phase-7-billing-identity-implementation.md), [A11y + Observability](../plans/2026-02-15-phase-7-a11y-observability-implementation.md).

- [ ] **[SaaS] Billing & Subscription Infrastructure** ([Design](../plans/2026-02-15-phase-7-billing-identity-design.md), [Implementation](../plans/2026-02-15-phase-7-billing-identity-implementation.md))
  - **Context:** Enable monetization for the managed hosting version.
  - **Strategy:** Integrate **Stripe** for subscription management. Create a `BillingService` to handle quotas, "Boost" payments, and tiered access levels.
- [ ] **[Compliance] Accessibility (A11y) & Mastery** ([Design](../plans/2026-02-15-phase-7-a11y-observability-design.md), [Implementation](../plans/2026-02-15-phase-7-a11y-observability-implementation.md))
  - **Context:** Ensure the platform is usable by everyone and meets enterprise requirements.
  - **Strategy:** Conduct a full A11y audit. Implement WCAG compliance standards, screen-reader optimizations, and a robust Keyboard-Only navigation mode (`Cmd+K` Quick Switcher).
- [ ] **[SaaS] Identity Trust & OAuth Linking** ([Design](../plans/2026-02-15-phase-7-billing-identity-design.md), [Implementation](../plans/2026-02-15-phase-7-billing-identity-implementation.md))
  - **Context:** Improve community trust and security.
  - **Strategy:** Allow users to link external accounts (GitHub, Discord, LinkedIn). Enable Guilds to set entrance requirements based on "Verified Identity" signals.
- [ ] **[Infra] SaaS Observability & Telemetry** ([Design](../plans/2026-02-15-phase-7-a11y-observability-design.md), [Implementation](../plans/2026-02-15-phase-7-a11y-observability-implementation.md))
  - **Context:** Maintain uptime and catch bugs at scale.
  - **Strategy:** Integrate **Sentry** (error tracking) and **OpenTelemetry** for performance monitoring across the Rust backend and Tauri clients.
- [ ] **[Ops] Admin Command Center (Native Observability Lite)** ([Design](../plans/2026-02-27-phase-7-admin-command-center-design.md), [Implementation](../plans/2026-02-27-phase-7-admin-command-center-implementation.md), [Task Plan](../plans/2026-02-27-phase-7-admin-command-center-task-plan.md))
  - **Context:** Provide built-in operational visibility for self-hosted admins without requiring a full Grafana stack.
  - **Strategy:** Ship a cluster-wide admin panel with 30-day native retention for curated metrics, trace indexes, and logs; keep deep forensics (full traces/log search, advanced alerting) in external Tempo/Loki/Prometheus/Grafana.

---

## Phase 8: Reliability & Operability Excellence 📋 **PLANNED**
*Goal: Turn reliability, isolation, and operator workflows into enforceable release standards.*
- Implementation coverage: [Phase 8 Reliability Implementation](../plans/2026-02-15-phase-8-reliability-implementation.md).

- [ ] **[Perf] Performance Budgets as CI Gates** ([Design](../plans/2026-02-15-performance-budgets-ci-gates-design.md), [Implementation](../plans/2026-02-15-phase-8-reliability-implementation.md))
  - **Context:** Prevent silent regressions in latency, memory, and CPU usage.
  - **Strategy:** Add measurable performance budgets for voice/chat/client startup and enforce them in CI.
- [ ] **[Infra] Chaos & Resilience Testing Program** ([Design](../plans/2026-02-15-chaos-resilience-testing-design.md), [Implementation](../plans/2026-02-15-phase-8-reliability-implementation.md))
  - **Context:** Validate behavior under partial failure before incidents happen.
  - **Strategy:** Run controlled fault-injection drills for DB/Valkey/WebSocket/media paths with recovery evidence.
- [ ] **[Ops] Self-Hosted Upgrade Safety Framework** ([Design](../plans/2026-02-15-self-hosted-upgrade-safety-design.md), [Implementation](../plans/2026-02-15-phase-8-reliability-implementation.md))
  - **Context:** Reduce operator risk during upgrades and migrations.
  - **Strategy:** Provide preflight checks, compatibility matrix, rollback scripts, and documented safe-upgrade windows.
- [ ] **[SaaS] FinOps & Cost Observability Track** ([Design](../plans/2026-02-15-finops-cost-observability-design.md), [Implementation](../plans/2026-02-15-phase-8-reliability-implementation.md))
  - **Context:** Keep storage/egress/telemetry costs predictable as media usage grows.
  - **Strategy:** Add cost dashboards, budget thresholds, and forecasting for major infrastructure spend categories.
- [ ] **[Compliance] Policy-as-Code for Data Governance** ([Design](../plans/2026-02-15-data-governance-policy-as-code-design.md), [Implementation](../plans/2026-02-15-phase-8-reliability-implementation.md))
  - **Context:** Documentation-only controls are hard to verify consistently.
  - **Strategy:** Encode retention/deletion/access policies into testable rules and CI checks.
- [ ] **[Security] Plugin/Bot Security Hardening Path** ([Design](../plans/2026-02-15-plugin-bot-security-hardening-design.md), [Implementation](../plans/2026-02-15-phase-8-reliability-implementation.md))
  - **Context:** Ecosystem expansion increases trust-boundary and abuse risks.
  - **Strategy:** Define capability model, signing/verification, and runtime guardrails before broad plugin rollout.
- [ ] **[Security] Tenancy & Isolation Verification** ([Design](../plans/2026-02-15-tenancy-isolation-verification-design.md), [Implementation](../plans/2026-02-15-phase-8-reliability-implementation.md))
  - **Context:** Multi-tenant correctness must be continuously proven.
  - **Strategy:** Add isolation regression tests for data access, event routing, cache keys, and permission boundaries.
- [ ] **[Ops] Operator Supportability Pack** ([Design](../plans/2026-02-15-operator-supportability-pack-design.md), [Implementation](../plans/2026-02-15-phase-8-reliability-implementation.md))
  - **Context:** Faster triage and diagnosis reduce incident impact.
  - **Strategy:** Ship standard diagnostics bundles, health endpoints, and runbook-indexed troubleshooting flows.

---

## Phase 10: SaaS Infrastructure at Scale 📋 **PLANNED**
*Goal: Full SaaS-grade media delivery and CDN infrastructure.*
- Implementation coverage: [Phase 10 Storage Scaling](../plans/2026-02-15-phase-10-storage-scaling-implementation.md).

- [ ] **[Storage] SaaS Scaling Architecture** ([Design](../plans/2026-02-15-phase-10-storage-scaling-design.md), [Implementation](../plans/2026-02-15-phase-10-storage-scaling-implementation.md))
  - Transition from Proxy Method to Signed URLs/Cookies.
  - CDN Integration with CloudFront/Cloudflare.

---

## Recent Changes

### 2026-02-26
- **Personal Workspaces** (PR #250) — Cross-guild channel aggregation with named workspace folders. 9 REST endpoints (CRUD, entry management, reordering), 7 WebSocket events for real-time sync, `workspaces` and `workspace_entries` tables with `update_updated_at()` triggers. Configurable limits via `MAX_WORKSPACES_PER_USER` (default 20) and `MAX_ENTRIES_PER_WORKSPACE` (default 50). Atomic CTE prevents limit bypass on concurrent requests. Guild membership + VIEW_CHANNEL permission checks enforced. 17 integration tests. Three code review rounds aligned error codes (SCREAMING_SNAKE_CASE), status codes (403 for limits, 204 for reorder), structured tracing, validator crate usage, `Option<Option<String>>` icon clearing, and configurable entry limits.
- **Digital Library (Wiki Mastery)** (PR #251) — Transforms Information Pages into a structured Knowledge Base. Revision history with content snapshots (SHA-256 dedup) and configurable pruning. Guild-scoped page categories with CRUD and drag-and-drop reordering. Deep-linkable heading anchors via slugified IDs. Library catalog view with category filtering. Per-guild configurable page/revision limits with admin overrides. 13 new API endpoints, `page_revisions` and `page_categories` tables with ordering triggers, database migration. Frontend: `MarkdownPreview` component with per-instance `Marked` parser, Mermaid diagram rendering, DOMPurify sanitization, table-of-contents generation. Five code review rounds addressed 11 security findings: CSS injection via SVG `<style>` tags in DOMPurify config, `validate_path_segment` for path traversal prevention (added `%` rejection), upper-bound validation on admin limit overrides (max 1000 pages / 500 revisions), `category_id` rejection on platform pages, best-effort revision snapshots (TOCTOU-safe), platform `accept_page` scope check, admin-only revision history access, per-component Marked instances (eliminated global state mutation), and reorder array size limits.

### 2026-02-25
- **Phase 5 Complete** — All 17 Ecosystem & SaaS Readiness items delivered (str0m evaluation deferred as trigger-based). Phase 6 (Competitive Differentiators & Mastery) now in progress.
- **SaaS Trust & Data Governance** (PR #249) — GDPR-style data export and account deletion lifecycle. Users can request a full data export (profile, messages, guilds, friends, preferences) as a ZIP archive via `POST /api/me/data-export`; background worker gathers data, uploads to S3, and sends email notification when ready (7-day expiry). Account deletion via `POST /api/me/delete-account` with password verification and 30-day grace period; cancellation via `POST /api/me/delete-account/cancel`. Guild ownership must be transferred first. After grace period, hourly worker permanently deletes the user with messages anonymized (author removed via SET NULL FK). Database migration adds `data_export_jobs` table, user deletion columns, and fixes 13 bare FK constraints. New `DataGovernance` rate limit category (2 req/60s). 11 integration tests. Two code review rounds addressed AuthUser test compilation, cancel_deletion rate limit scope, email template, and comment accuracy.
- **Advanced Media Processing** (PR #248) — Progressive image loading with blurhash placeholders, thumbnail (256px) and medium (1024px) WebP variant generation, aspect-ratio CSS to prevent layout shift, and variant download endpoint with graceful fallback.

### 2026-02-24
- **Guild Resource Limits & Usage Stats** (PR #247) — Configurable per-instance resource limits enforced server-side at 8 code points (guild creation, member join via invite/discovery, channel/role/emoji/bot/webhook creation) with `LIMIT_EXCEEDED` (403) errors. Limits loaded from environment variables with sensible defaults, clamped to >= 1. New endpoints: `GET /api/guilds/{id}/usage` (member-only usage stats with parallel count queries via `tokio::join!`) and `GET /api/config/limits` (public instance limits). Database migration adds `plan` column to guilds (default: "free") for future tier/boost system. Frontend Usage tab in guild settings with progress bars and color-coded thresholds (green <70%, yellow 70-90%, red >90%). 10 integration tests. Two code review rounds addressed TOCTOU race on invite join (switched to `ON CONFLICT DO NOTHING`), config validation (`.max(1)` clamp), and missing test coverage (emoji limit, discovery join limit).
- **Guild Discovery & Onboarding Wizard** (PRs #244, #245, #246) — Three stacked PRs delivering guild discovery backend, frontend UI, and onboarding wizard. Backend: PostgreSQL migration with `discoverable`, `tags`, `banner_url` columns, full-text `search_vector` (tsvector), denormalized `member_count` with trigger, public browse/join API with rate limiting and WebSocket broadcast, server config flag. Frontend: `DiscoveryView` with search/sort/pagination, `GuildCard` with banner/tags/join, Compass icon in ServerRail, guild settings extension (discoverable toggle, tags editor, banner URL). Onboarding: 5-step wizard (Welcome → Theme → Mic Setup → Join Server → Done), extracted `MicTestPanel`, ARIA-compliant tabs with focus trap, re-triggerable from settings. Seven code review rounds addressed 40+ issues including denormalized queries, case-insensitive tags, past-end pagination, focus trap, `createUniqueId()` for element IDs, parent-level joined tracking, and AudioContext error handling.

### 2026-02-19
- **Advanced Moderation & Safety Filters** (PR #206) — Guild-configurable content filters with Aho-Corasick + regex hybrid engine. Built-in categories (slurs, hate speech, spam, abusive language) with Block/Log/Warn actions. Custom guild patterns with ReDoS protection. Per-guild cached `FilterEngine` with generation-counter invalidation. Integrated into message create/edit/upload flows (skips encrypted and DM messages). Frontend Safety tab with category toggles, custom pattern CRUD, test panel, and moderation log. 17 integration tests + 9 unit tests. Two code review rounds addressed 12 issues including TOCTOU race fix, transactional upserts, regex validation three-way match, and ephemeral test engine.
- **Tech Debt Cleanup** (branch `chore/tech-debt`) — Resolved 16 of 30 cataloged tech debt items across server, client, and shared crates:
  - **Security:** Gated megolm E2EE stubs behind feature flag (TD-01), replaced WebSocket `.expect()` panics with proper error responses (TD-05), added search query length validation (TD-08)
  - **Features:** MFA backup codes with Argon2id hashing (TD-06), E2EE backups now include real keys (TD-04), admin elevation detection fixed (TD-17), spoiler reveal persistence (TD-22), window focus check for notifications (TD-20)
  - **Code Quality:** Eliminated all `#[allow(clippy::too_many_arguments)]` via parameter structs (TD-10), removed all `#[allow(dead_code)]` suppressions (TD-11), replaced all production `as any`/`@ts-ignore` with proper types (TD-12), production console.log stripping via esbuild `pure` (TD-09)
  - **Testing:** 10 new search security/edge-case integration tests (TD-08), search analytics logging (TD-30)
  - **Already done:** Channel permission filtering (TD-02) and upload limit sync (TD-13) were found to be already implemented
  - Full inventory: `docs/project/tech-debt.md`, implementation plans: `docs/project/tech-debt-implementation.md`

### 2026-02-17
- **Production-Scale Polish — Code Review Fixes** (PR #204) - Addressed 10 issues from code review: restructured MembersTab scroll container (moved ref outside `<Show>` conditional, replaced hardcoded max-height with flex layout), changed per-command toast dedup IDs to prevent different command timeouts from suppressing each other, memoized `sortedDMs` with `createMemo`, documented reactive getter contract in `VirtualizerOptions` interface, fixed `screenShareViewer` test proxy handling with `unwrap()`, added TODO comment for future component rendering tests, added Load More behavior comment in SearchPanel, consolidated duplicate CHANGELOG headings, and removed test-only CHANGELOG entry.

### 2026-02-13
- **E2E UI Test Coverage Suite** - Created comprehensive Playwright test suite with 68 UI items across 12 spec files (10 new + 2 pre-existing). Shared helpers (`e2e/helpers.ts`) for login, navigation, and utilities. Coverage tracker at `docs/testing/ui-coverage.md`. First run: 8 passing (auth form rendering), 58 need backend, 3 not coverable. Test areas: Auth (12 items), Navigation (9), Messaging (5), Guild (5), Channels (4), Friends/DMs (5), Settings (8), Voice (6), Admin (6), Search (3), Permissions (5).

### 2026-02-10
- **Search Integration Tests** - 38 integration tests across 2 test files (18 global search, 20 guild/DM search) with 11 shared helpers in `helpers/mod.rs`. Coverage includes: auth (401), access control (non-member 403, nonexistent guild 404), soft-deleted and encrypted message exclusion, date/author/has:link/has:file filters, `ts_rank` relevance ranking, `ts_headline` snippets with `<mark>` tags, sort (relevance/date), pagination with offset verification, limit clamping (1-100), and data-driven validation tests. Extracted all inline SQL into reusable helpers (`create_guild`, `create_channel`, `insert_message`, `insert_message_at`, `insert_encrypted_message`, `insert_deleted_message`, `insert_attachment`, `add_guild_member`, `delete_guild`, `delete_dm_channel`). Updated roadmap search checklist.

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
- **User Blocking & Reports** (PR #141) - Absolute user blocking with Redis block cache, user report system with admin management, client UI (block/report modals, admin reports panel). 25 HTTP integration tests: blocking (8), reports (6), admin reports (11). Phase 4 now 100% complete.
- **First User Setup HTTP Integration Tests** - Created reusable `TestApp` infrastructure in `server/tests/helpers/mod.rs` (first HTTP-level test infrastructure in the project). Added 8 HTTP integration tests for setup endpoints covering: status endpoint, config access control, auth requirement, admin requirement, successful completion with DB verification, validation errors, and idempotency. Filed follow-up issues for test architecture improvements: cleanup guards (#137), shared DB pool (#138), stateful middleware testing (#139), concurrent HTTP completion test (#140).
- Marked **SSO / OIDC Integration** complete (PR #135) - Admin-configurable OIDC providers with dynamic discovery and automatic account linking.
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
