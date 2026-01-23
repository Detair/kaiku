# VoiceChat (Canis) Roadmap

This roadmap outlines the development path from the current prototype to a production-ready, multi-tenant SaaS platform.

**Current Phase:** Phase 4 (Advanced Features) - In Progress

**Last Updated:** 2026-01-23

## Quick Status Overview

| Phase | Status | Completion | Key Achievements |
|-------|--------|------------|------------------|
| **Phase 0** | ✅ Complete | 100% | N+1 fix, WebRTC optimization, MFA encryption |
| **Phase 1** | ✅ Complete | 100% | Voice state sync, audio device selection |
| **Phase 2** | ✅ Complete | 100% | Voice Island, VAD, Speaking Indicators, Command Palette, File Attachments, Theme System, Code Highlighting |
| **Phase 3** | ✅ Complete | 100% | Guild system, Friends, DMs, Home View, Rate Limiting, Permission System + UI, Information Pages, DM Voice Calls |
| **Phase 4** | 🔄 In Progress | 50% | E2EE Key Backup (PR #22, #29), User Connectivity Monitor (PR #23), E2EE DM Messaging (PR #41) |
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
- [ ] **[Settings] Server-Synced User Preferences** `Planned`
  - Persist user settings (sound, theme, etc.) on server.
  - Sync preferences across devices.
- [x] **[UX] Do Not Disturb Mode** ✅
  - Notification sounds suppressed when user status is "Busy" (DND).
  - Scheduled quiet hours with configurable start/end times.
  - Handles overnight ranges (e.g., 22:00 to 08:00).
  - Call ring sounds also suppressed during DND.
  - **Design:** `docs/plans/2026-01-23-read-sync-dnd-design.md`
- [ ] **[UX] Modular Home Sidebar**
  - "Active Now" panel showing friends' activities (implemented).
  - "Pending & Suggestions" quick-action module (planned).
  - "Global Pins / Scratchpad" personal utility module (planned).
  - "Server Pulse" activity summary module (planned).
- [ ] **[UX] Cross-Server Favorites**
  - Allow pinning channels from different guilds into a single "Favorites" list.
- [ ] **[Auth] SSO / OIDC Integration**
  - Enable "Login with Google/Microsoft" via `openidconnect`.
- [ ] **[Voice] Screen Sharing**
  - Update SFU to handle multiple video tracks (Webcam + Screen).
  - Update Client UI to render "Filmstrip" or "Grid" layouts.
- [ ] **[Client] Mobile Support**
  - Adapt Tauri frontend for mobile or begin Flutter/Native implementation.

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
- [ ] **[Content] Custom Emojis**
  - Allow Guilds to upload custom emoji packs.
- [ ] **[Voice] Multi-Stream Support**
  - Simultaneous Webcam and Screen Sharing.
  - Implement Simulcast (quality tiers) for bandwidth management.
- [ ] **[SaaS] Limits & Monetization Logic**
  - Enforce limits (storage, members) per Guild.
  - Prepare "Boost" logic for lifting limits.

---

## Recent Changes

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