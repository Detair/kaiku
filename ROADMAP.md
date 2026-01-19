# VoiceChat (Canis) Roadmap

This roadmap outlines the development path from the current prototype to a production-ready, multi-tenant SaaS platform.

**Current Phase:** Phase 4 (Advanced Features) - In Progress

**Last Updated:** 2026-01-19

## Quick Status Overview

| Phase | Status | Completion | Key Achievements |
|-------|--------|------------|------------------|
| **Phase 0** | ✅ Complete | 100% | N+1 fix, WebRTC optimization, MFA encryption |
| **Phase 1** | ✅ Complete | 100% | Voice state sync, audio device selection |
| **Phase 2** | ✅ Complete | 100% | Voice Island, VAD, Speaking Indicators, Command Palette, File Attachments, Theme System, Code Highlighting |
| **Phase 3** | ✅ Complete | 100% | Guild system, Friends, DMs, Home View, Rate Limiting, Permission System + UI, Information Pages, DM Voice Calls |
| **Phase 4** | 🔄 In Progress | 15% | E2EE Key Backup foundation (PR #22) |
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

## Phase 1: Core Loop Stability
*Goal: Ensure the fundamental chat and voice experience is flawless and bug-free.*

- [ ] **[Tests] Message API Integration Tests**
  - Create tests for message CRUD operations to prevent regressions.
  - Verify JSON response structures.
- [ ] **[Client] Real-time Text Sync**
  - Ensure new messages via WebSocket appear instantly in the UI without refresh.
  - Handle `message.edit` and `message.delete` events live.
- [x] **[Voice] Room State Synchronization** ✅
  - WebSocket event handlers already sync RoomState on join.
  - Updated VoiceParticipants with new theme and proper indicators.
  - Speaking indicators now implemented via client-side VAD (see Phase 2).
- [x] **[Client] Audio Device Selection** ✅ (Moved to Phase 2)
  - Completed with full modal UI and device testing.
  - See Phase 2 for implementation details.

---

## Phase 2: Rich Interactions & Modern UX
*Goal: Reach feature parity with basic chat apps while introducing modern efficiency tools.*

- [x] **[UX] Command Palette (`Ctrl+K`)** `New` ✅
  - Implemented global fuzzy search for Channels and Users.
  - Keyboard navigation (↑↓ + Enter + Esc).
  - Command execution with > prefix.
  - **Location**: `client/src/components/layout/CommandPalette.tsx`
- [x] **[UX] Dynamic Voice Island** `New` ✅
  - Decoupled Voice Controls from sidebar.
  - Created "Dynamic Island" style floating overlay at bottom center.
  - Shows connection status, timer, and all voice controls.
  - **Location**: `client/src/components/layout/VoiceIsland.tsx`
- [x] **[UX] Modern Theme System** `New` ✅
  - Implemented "Focused Hybrid" design (Discord structure + Linear efficiency).
  - New color palette with surface layers and semantic tokens.
  - Created AppShell 3-pane layout with ServerRail preparation.
  - **Impact**: Professional UI ready for production.
  - **Location**: `client/uno.config.ts`, `client/src/components/layout/AppShell.tsx`
- [x] **[Client] Audio Device Selection** ✅
  - Created AudioDeviceSettings modal with device enumeration.
  - Integrated with VoiceIsland settings button.
  - Microphone test with real-time volume indicator.
  - User-friendly error messages for device issues.
  - **Location**: `client/src/components/voice/AudioDeviceSettings.tsx`
- [x] **[UX] Component Theme Updates** ✅
  - Updated all existing components to new theme system.
  - Removed confusing non-functional buttons from UserPanel.
  - Improved VoiceParticipants with proper color tokens.
  - **Impact**: Cohesive visual experience across all UI.
- [x] **[Voice] Voice Activity Detection (VAD)** `New` ✅
  - Implemented continuous VAD using Web Audio API AnalyserNode.
  - Real-time speaking indicators for local and remote participants.
  - Pulsing animation in channel list when participants are speaking.
  - **Location**: `client/src/lib/voice/browser.ts`, `client/src/stores/voice.ts`
- [x] **[Voice] Auto-Retry on Connection Conflicts** `New` ✅
  - Automatic leave/rejoin when server reports "Already in voice channel".
  - Handles browser refresh and connection state mismatches gracefully.
  - **Location**: `client/src/stores/websocket.ts`
- [x] **[UX] Instant Participant Display** `New` ✅
  - Local user shown immediately when joining voice channel.
  - Participant count updates during "connecting" state, not just "connected".
  - Speaking/muted indicators for local user in channel list.
  - Fixed duplicate user display by filtering current user from server participants.
  - **Location**: `client/src/components/voice/VoiceParticipants.tsx`, `client/src/components/channels/ChannelItem.tsx`
- [x] **[UX] Draggable Voice Island** `New` ✅
  - Voice Island can be dragged anywhere on screen.
  - Position persists within session.
  - Keyboard shortcuts: Ctrl+Shift+M (mute), Ctrl+Shift+D (deafen).
  - Settings modal rendered via Portal for proper z-index stacking.
  - **Location**: `client/src/components/layout/VoiceIsland.tsx`
- [x] **[Voice] Basic Noise Reduction (Tier 1)**
  - Implemented in `browser.ts` via constraints.
  - UI Toggle in Audio Settings.
- [x] **[Auth] Automatic Token Refresh** `New` ✅
  - JWT access tokens auto-refresh 60 seconds before expiration.
  - Refresh tokens stored and managed in browser state.
  - Seamless session continuity without manual re-login.
  - **Location**: `client/src/lib/tauri.ts`
- [x] **[Media] File Attachments & Previews** ✅
  - [x] **Backend:** Implement `Proxy Method` for authenticated file downloads.
  - [x] **Backend:** Token query parameter support for browser requests (img src, a href).
  - [x] **Backend:** Configurable upload size limit (default 50MB).
  - [x] **Client:** Implement drag-and-drop file upload.
  - [x] **UX:** Upload Preview Tray with image thumbnails and remove buttons.
  - [x] **UI:** Render image previews in the message list.
- [x] **[Text] Markdown & Emojis**
  - **Note:** `solid-markdown` enabled and verified.
  - Add an Emoji Picker component using `picmo` + `@floating-ui/dom`.
- [x] **[Text] Code Blocks & Syntax Highlighting** ✅
  - Custom CodeBlock component with highlight.js integration.
  - Languages: JavaScript, TypeScript, Python, Rust, JSON, Bash.
  - Theme-aware syntax colors via CSS variables.
  - **Location**: `client/src/components/ui/CodeBlock.tsx`, `client/src/styles/highlight-theme.css`
- [x] **[UX] Theme System Expansion** ✅
  - CSS variable swapping with `data-theme` attribute.
  - Three themes: Focused Hybrid, Solarized Dark, Solarized Light.
  - Settings modal with theme picker and live preview.
  - **Location**: `client/src/stores/theme.ts`, `client/src/components/settings/`

---

## Phase 3: Guild Architecture & Security (The Big Refactor)
*Goal: Transform from "Simple Chat" to "Multi-Server Platform" (Discord-like architecture).*

- [x] **[DB] Guild (Server) Entity** ✅
  - Created `guilds` table with full CRUD operations.
  - Channels now belong to `guild_id`.
  - Guild members with join/leave functionality.
  - **Location:** `server/src/guild/`, `server/migrations/20240201000000_guilds.sql`
- [x] **[Social] Friends & Status System** ✅
  - **DB:** `friendships` table (pending/accepted/blocked).
  - **API:** Friend Request system (send/accept/reject/block).
  - **UI:** FriendsList component with tabs, AddFriend modal.
  - **Location:** `server/src/social/`, `client/src/components/friends/`
- [x] **[Chat] Direct Messages & Group DMs** ✅
  - Reused `channels` with `type='dm'`.
  - DM creation, listing, leave functionality.
  - **Location:** `server/src/chat/dm.rs`
- [x] **[UI] Server Rail & Navigation** ✅
  - Vertical Server List sidebar (ServerRail).
  - Context switching between guilds.
  - **Location:** `client/src/components/layout/ServerRail.tsx`
- [x] **[UX] Unified Home View** ✅
  - Home dashboard with DM sidebar and conversations.
  - Unread counts, last message previews.
  - **Location:** `client/src/components/home/`
- [x] **[Auth] Permission System Design** ✅
  - Comprehensive design document for role-based permissions.
  - **Location:** `docs/plans/permission-system-design-2026-01-13.md`
- [x] **[Auth] Permission System Implementation** ✅
  - [x] Backend API handlers for admin, roles, and overrides (PR #17).
  - [x] Permission checking middleware and guild permission queries.
  - [x] Admin UI for role management and permission assignment (PR #20).
  - [x] Role picker in guild settings, channel permission overrides.
  - [x] Admin Dashboard with user/guild management and audit log.
  - **Design:** `docs/plans/permission-system-implementation-2026-01-13.md`
  - **Backend:** `server/src/permissions/`
  - **Frontend:** `client/src/components/admin/`, `client/src/components/guilds/`
- [x] **[Voice] DM Voice Calls** ✅
  - Voice calling in DM and group DM conversations (PR #21).
  - Call signaling via Redis Streams, reuses existing SFU.
  - Join/Decline flow with CallBanner UI component.
  - CallCapabilities for future video/screenshare extensibility.
  - **Design:** `docs/plans/2026-01-14-dm-voice-calls-design.md`
  - **Backend:** `server/src/voice/call.rs`, `server/src/voice/call_service.rs`
  - **Frontend:** `client/src/stores/call.ts`, `client/src/components/call/`
- [x] **[Content] Information Pages** ✅
  - Platform-wide pages (ToS, Privacy Policy) in Home view.
  - Guild-level pages (Rules, FAQ) in sidebar above channels.
  - Markdown editor with live preview and Mermaid diagram support.
  - Page acceptance tracking with scroll-to-bottom requirement.
  - Audit logging for all page operations.
  - **Location:** `server/src/pages/`, `client/src/components/pages/`
- [x] **[Security] Rate Limiting** ✅
  - Redis-based fixed window rate limiting with Lua scripts.
  - Hybrid IP/user identification with configurable trust proxy.
  - Failed auth tracking with automatic IP blocking.
  - Category-based limits (Auth, Read, Write, Social, Voice).
  - **Location:** `server/src/ratelimit/`

---

## Phase 4: Advanced Features
*Goal: Add competitive differentiators and mobile support.*

- [ ] **[Social] Rich Presence (Game Activity)** `New` `Designed`
  - Detect running games via Process Scan (Tauri) or RPC.
  - Display "Playing X" status in Friends List and User Popups.
  - Enable "Ask to Join" logic.
  - **Design:** `docs/plans/2026-01-19-rich-presence-design.md`
- [ ] **[UX] Cross-Server Favorites** `New`
  - Allow pinning channels from different guilds into a single "Favorites" list.
- [ ] **[Auth] SSO / OIDC Integration**
  - Enable "Login with Google/Microsoft" via `openidconnect`.
- [ ] **[Security] E2EE Key Backup & Recovery** `New` `In Progress`
  - Element X-style Security Key (256-bit random, Base58-encoded).
  - Optional backup after registration, skippable with reminder.
  - QR-code transfer between devices (60s timeout, optional PIN).
  - Full key verification with paste support before backup completion.
  - Auto-clear clipboard after 60s with visible countdown.
  - Key rotation with re-encryption when old key available.
  - **Design:** `docs/plans/2026-01-19-e2ee-key-backup-design.md`
  - **Plan:** `docs/plans/2026-01-19-e2ee-implementation.md`
  - **Backend PR:** #22 (OlmAccount, OlmSession, RecoveryKey, EncryptedBackup, DB tables, APIs)
- [ ] **[Voice] Screen Sharing**
  - Update SFU to handle multiple video tracks (Webcam + Screen).
  - Update Client UI to render "Filmstrip" or "Grid" layouts.
- [ ] **[Client] Mobile Support**
  - Adapt Tauri frontend for mobile or begin Flutter/Native implementation.
- [ ] **[Content] Information Pages v2** `Future`
  - Full version history with diff view.
  - Public pages (accessible without login).
  - Page templates and search.
  - PDF export functionality.

---

## Phase 5: Ecosystem & SaaS Readiness
*Goal: Open the platform to developers and prepare for massive scale.*

- [ ] **[Storage] SaaS Scaling Architecture**
  - Transition from **Proxy Method** (Phase 2) to **Signed URLs/Cookies**.
  - **CDN Integration:** Use CloudFront/Cloudflare with Signed Cookies to cache content at the edge.
  - **Auth:** Server becomes Key Issuer (generating short-lived tokens) instead of Data Pipe.
  - **Rationale:** Reduces server bandwidth/CPU load for 100k+ users; enables global low-latency file access.
- [ ] **[API] Bot Ecosystem**
  - Add `is_bot` user flag.
  - Create a "Gateway" WebSocket for bot events (`MESSAGE_CREATE`).
  - Implement Slash Commands structure.
- [ ] **[Content] Custom Emojis**
  - Allow Guilds to upload custom emoji packs.
  - Update client parser to handle `<:name:id>` syntax.
- [ ] **[Voice] Multi-Stream Support**
  - Allow simultaneous Webcam and Screen Sharing from the same user.
  - Implement Simulcast (quality tiers) for bandwidth management.
- [ ] **[SaaS] Limits & Monetization Logic**
  - Enforce limits (storage, members) per Guild.
  - Prepare "Boost" logic for lifting limits.

---

# Developer Appendix: Implementation Prompts

These prompts are designed to be used by implementation agents to execute specific roadmap items.

## [UI/UX] Global Shell & Modern Design
**Goal:** Implement the "Focused Hybrid" layout.
- **Stack:** Solid.js + UnoCSS.
- **Layout:** 3-Pane structure (`ServerRail` -> `ContextSidebar` -> `MainChat`).
- **Voice Island:** Floating overlay at `bottom-center` with connection stats and large controls.
- **Semantic Colors:** Use `bg-surface-base` (#1E1E2E), `bg-surface-layer1` (#252535), `bg-surface-layer2` (#2A2A3C).
- **Interactions:** Use `<Show>` and `<For>` for Solid logic. Render modals via `<Portal>`.

## [Phase 2] Code Blocks & Emoji Picker
**Task A: Code Blocks (Highlight.js)**
- **Specs:** Auto-language detection, monospace font stack (Unispace/Fira Code).
- **Themes:** Dynamic swap between `solarized-dark.css` and `solarized-light.css`.
- **Integration:** Custom renderer passed to `SolidMarkdown` components prop.

**Task B: Emoji Picker (Picmo)**
- **Specs:** Framework-agnostic `Picmo` library.
- **Positioning:** Use `@floating-ui/dom` to anchor above the message input.
- **Lazy Loading:** Use `lazy()` to load the picker on first interaction.

**Task C: Upload Preview Tray (UX Enhancement)**
- **Goal:** Show images locally before they are uploaded to the server.
- **Specs:** 
    - Use `URL.createObjectURL(file)` to generate instant previews for `image/*` files.
    - Render a horizontal, scrollable list of "Preview Cards" above the text area in `MessageInput.tsx`.
    - Each card must have a "Remove" button to cancel that specific file.
    - **Logic:** Shift `handleFileUpload` to trigger *only* when the Send button/Enter key is pressed, rather than on every drop.

**Task D: Rich Text Toolbar (UX Enhancement)**
- **Goal:** Provide Slack-like formatting controls without clutter.
- **UI:** 
    - Add a formatting toggle button ("Aa") to the input bar.
    - When active, show a toolbar above the text area containing: Bold, Italic, Strike, Spoiler (`||`), Quote, Code Block, Inline Code.
- **Logic:** 
    - Clicking a button wraps the currently selected text in Markdown syntax (e.g., `**selected**`).
    - If no text is selected, insert the syntax with cursor in between.
    - Support keyboard shortcuts (`Ctrl+B`, `Ctrl+I`).

**Task E: Shortcuts Cheat Sheet**
- **Goal:** Help users discover power features.
- **UI:** A modal overlay triggered by `?` or a help icon.
- **Content:** List all global shortcuts (e.g., `Ctrl+K` Palette, `Ctrl+Shift+M` Mute, Markdown syntax).
- **Design:** Clean, two-column grid using the "Focused Hybrid" theme.

**Task F: Slash Command System**
- **Trigger:** Typing `/` at the start of the message input.
- **UI:** A popover menu listing available commands (reusing logic from Emoji Picker/Command Palette).
- **Core Commands:**
    - `/help` or `/?`: Opens the Shortcuts Cheat Sheet.
    - `/shrug`: Appends `¯\_(ツ)_/¯`.
    - `/spoiler [text]`: Wraps text in spoiler tags.
    - `/me [action]`: Formats message as an action (italicized).
    - `/roll [NdX]`: Rolls dice (e.g., `/roll d6`, `/roll 2d20`).
    - `/flip`: Flips a coin (Heads/Tails).
    - `/slap @user`: "User slaps @user around a bit with a large trout."
    - `/tableflip`: `(╯°□°）╯︵ ┻━┻`
    - `/unflip`: `┬─┬ノ( º _ ºノ)`

## [Phase 3] Friends, Status & Social Graph
**Task A: Social Backend**
- **DB:** `friendships` table (user_id_1, user_id_2, status: pending/accepted/blocked).
- **Presence:** `status_message` and `last_seen_at` columns on `users` table.
- **Fan-out:** Redis broadcast of `presence_update` only to a user's friends' channels.

**Task B: Social Frontend**
- **View:** `Friends.tsx` dashboard with "Online", "All", "Pending" tabs.
- **Actions:** Friend Request by username, blocking, and private status editing.
