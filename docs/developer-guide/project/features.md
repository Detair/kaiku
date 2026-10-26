# Kaiku — Implemented Features

> Verified against the codebase on 2026-03-07. Only features with working server **and** client
> code on the `main` branch are listed. Design-only documents, unmerged branches, and stub/TODO
> code are excluded.

---

## 1. Authentication & Account Security

### 1.1 Email/Password Registration & Login
Full user registration with email validation, username validation (3-32 chars, alphanumeric + underscore), and login with Argon2id password verification. Rate-limited endpoints prevent brute-force attacks.

- **Server:** `server/src/auth/handlers.rs` — `register()`, `login()`
- **Client:** Login and registration forms with validation feedback

### 1.2 JWT Token Authentication
EdDSA (Ed25519) signed tokens with strict access/refresh separation. Access tokens expire after 15 minutes, refresh tokens after 7 days. Refresh tokens delivered via HttpOnly cookies (browser) or JSON body (Tauri desktop). Automatic proactive refresh 60 seconds before expiration.

- **Server:** `server/src/auth/jwt.rs` — token generation and validation
- **Client:** `client/src/lib/tauri.ts` — automatic token refresh logic

### 1.3 Multi-Factor Authentication (TOTP)
Full MFA lifecycle: setup with QR code generation, 6-digit TOTP verification, and disable flow. TOTP secrets encrypted at rest with AES-256-GCM via dedicated `mfa_crypto` module. Comprehensive test suite for encryption roundtrips.

- **Server:** `server/src/auth/handlers.rs` — `mfa_setup()`, `mfa_verify()`, `mfa_disable()`
- **Server:** `server/src/auth/mfa_crypto.rs` — AES-256-GCM encryption
- **Client:** `client/src/components/settings/MfaSetupModal.tsx`

### 1.4 MFA Backup Codes
Generates 10 alphanumeric backup codes per user, hashed with Argon2id before storage. Single-use enforcement with `used_at` tracking. Count endpoint reports used vs total codes.

- **Server:** `server/src/auth/backup_codes.rs`
- **Client:** `client/src/components/settings/BackupCodesDisplay.tsx`

### 1.5 Forgot Password / Password Reset
Email-based password reset with SHA-256 hashed tokens, 1-hour expiration, and single-use enforcement. User enumeration protection via generic responses. Automatic session invalidation after reset. Transaction-based atomicity.

- **Server:** `server/src/auth/handlers.rs` — `forgot_password()`, `reset_password()`
- **Dependency:** Requires configured email service (SMTP)

### 1.6 SSO / OIDC Integration
Admin-configurable OpenID Connect providers (Google, GitHub, etc.) with dynamic discovery. Client secrets encrypted at rest (AES-256-GCM). PKCE flow support, nonce verification, and state parameter backed by Redis. Automatic account linking with username collision handling.

- **Server:** `server/src/auth/oidc.rs` — `OidcProviderManager`
- **Server:** `server/src/auth/handlers.rs` — `oidc_authorize()`, `oidc_callback()`

### 1.7 Session Management
View, revoke, and manage active auth sessions. Each session displays a friendly device name (parsed from user-agent via `woothee`), IP address, and approximate city/country (GeoIP lookup via configurable HTTP API with 2s timeout). Revoke individual sessions or bulk-revoke all other sessions with current-session protection. Password change includes optional "log out all other devices" prompt. Tauri clients identified via `X-Refresh-Token` header; browsers use HttpOnly cookies.

- **Server:** `server/src/auth/handlers.rs` — `list_sessions()`, `revoke_session()`, `revoke_all_other_sessions()`
- **Server:** `server/src/auth/geoip.rs` — `resolve_location()`
- **Server:** `server/src/auth/ua_parser.rs` — `parse_device_name()`
- **Client:** `client/src/components/settings/SessionsSection.tsx`

### 1.8 First User Setup Wizard
First registered user automatically receives admin/superuser permissions with PostgreSQL row-level locking to prevent race conditions. Setup wizard configures server name, registration policy (open/invite-only/closed), and optional legal URLs. Compare-and-swap pattern for atomic completion.

- **Server:** `server/src/api/setup.rs` — status, config, complete endpoints
- **Client:** `client/src/components/SetupWizard.tsx`

---

## 2. End-to-End Encryption (E2EE)

### 2.1 E2EE for DM Messages (Olm Double Ratchet)
End-to-end encryption for DM messages using vodozemac (Matrix Olm protocol). LocalKeyStore with encrypted SQLite storage for Olm sessions. CryptoManager handles session lifecycle, encrypt/decrypt operations. Graceful fallback to unencrypted when E2EE is not available. Encryption indicator in DM headers.

- **Shared:** `shared/vc-crypto/src/olm.rs` — Double Ratchet protocol
- **Server:** `server/src/crypto/handlers.rs` — key upload, prekey claim
- **Client (Tauri):** `client/src-tauri/src/commands/crypto.rs`

### 2.2 E2EE Key Backup & Recovery
256-bit recovery key (Base58 encoded, 4-char chunked). AES-256-GCM encrypted backup with Argon2id key derivation (64MB memory, 3 iterations). Upload/download/status endpoints. Recovery key modal with copy/download, backup reminder banner, configurable mandatory setup via `REQUIRE_E2EE_SETUP`.

- **Shared:** `shared/vc-crypto/src/recovery.rs`
- **Server:** `server/src/crypto/handlers.rs` — backup CRUD
- **Client:** `client/src/components/settings/RecoveryKeyModal.tsx`

### 2.3 Megolm Group Encryption (Feature-Gated)
Group encryption sessions via vodozemac Megolm, gated behind a feature flag. Outbound/inbound sessions with key export and message index ratcheting.

- **Shared:** `shared/vc-crypto/src/megolm.rs`

### 2.4 Clipboard Protection
Context-aware clipboard security with auto-clear timeouts (recovery phrase: 60s, invite links: 120s). SHA-256 tamper detection compares hash before and after paste. Three protection levels (Minimal, Standard, Strict) plus paranoid mode. Tauri native and browser fallback implementations.

- **Client (Tauri):** `client/src-tauri/src/commands/clipboard.rs`
- **Client:** `client/src/lib/clipboard.ts`, `client/src/components/clipboard/`

---

## 3. Voice & WebRTC

### 3.1 SFU Voice Chat
Selective Forwarding Unit (SFU) architecture supporting up to 25 participants per room. Opus audio at 48kHz stereo, VP9/VP8/H.264 video codec support. Lock-free DashMap for RTP hot path. Full peer lifecycle management with automatic cleanup on disconnect.

- **Server:** `server/src/voice/sfu.rs` (739 LOC), `server/src/voice/track.rs`
- **Client (Browser):** `client/src/lib/webrtc/browser.ts` (~1400 LOC)
- **Client (Tauri):** `client/src-tauri/src/webrtc/mod.rs`

### 3.2 Voice State Synchronization
Real-time join/leave/mute state broadcasting via WebSocket. `ParticipantInfo` tracks user_id, username, display_name, muted, deafened, screen_sharing, webcam_active. Client Solid.js store maintains reactive participant list.

- **Server:** `server/src/voice/sfu.rs` — participant tracking and event broadcast
- **Client:** `client/src/stores/voice.ts`

### 3.3 Audio Device Selection
Device enumeration for microphones and speakers with default device detection. Input switching via MediaStream constraints, output via `setSinkId` API. Tauri native audio via cpal crate at 48kHz. Microphone test with real-time volume indicator and speaker test (440Hz tone).

- **Client:** `client/src/components/voice/AudioDeviceSettings.tsx` (22.7KB)
- **Client (Tauri):** `client/src-tauri/src/audio/mod.rs`

### 3.4 Voice Activity Detection (VAD)
Client-side VAD using Web Audio API AnalyserNode with FFT size 256 (10ms resolution). Threshold-based detection at 100ms intervals. Configurable sensitivity slider (0-100%). Respects local mute state.

- **Client:** `client/src/lib/webrtc/browser.ts` — `startVAD()`
- **Client:** `client/src/components/settings/VoiceSettings.tsx` — sensitivity controls

### 3.5 Speaking Indicators
Real-time visual feedback for local and remote speakers. Green highlight with pulse animation on the speaking participant. Animated Volume2 icon in participant list. Data flow: Browser VAD -> store update -> WebSocket broadcast.

- **Client:** `client/src/components/voice/VoiceParticipants.tsx`

### 3.6 Voice Island (Floating Panel)
Persistent sidebar voice panel showing connection status, elapsed time, and connection quality. Sub-components: VoiceControls (mute/deafen/screen share/webcam/settings), VoiceParticipants (per-participant quality indicators), quality pickers. Dynamic border colors based on speaking state.

- **Client:** `client/src/components/voice/VoicePanel.tsx`
- **Shortcuts:** Ctrl+Shift+M (mute), Ctrl+Shift+D (deafen)

### 3.7 Noise Suppression
Browser-level noise suppression, echo cancellation, and auto-gain control via MediaStream constraints. Enabled by default with runtime toggle. Settings persisted in app preferences.

- **Client:** `client/src/lib/webrtc/browser.ts` — `setNoiseSuppression()`
- **Client:** `client/src/components/settings/VoiceSettings.tsx`

### 3.8 DM Voice Calls
Full 1:1 and group DM voice calling with event-sourced state machine (Redis Streams for multi-node coordination). States: Ringing -> Connected -> Ended. 90-second ring timeout, 5-second cleanup delay. HTTP handlers for start/join/decline/leave. CallBanner UI with accept/decline buttons, call duration display, and ringing audio.

- **Server:** `server/src/voice/call.rs`, `call_service.rs`, `call_handlers.rs`
- **Client:** `client/src/components/call/CallBanner.tsx`, `client/src/stores/call.ts`

### 3.9 Screen Sharing (Multi-Stream)
Browser `getDisplayMedia()` capture with quality tiers (480p@10fps to 1440p@30fps). Source picker for Tauri native (monitors/windows with thumbnails). Per-channel screen share limits with permission checks. ScreenShareViewer with spotlight/PiP/theater modes. Keyboard shortcuts (Escape, M, V). Simultaneous webcam + screen share support.

- **Server:** `server/src/voice/screen_share.rs`
- **Client:** ScreenShareButton, ScreenShareSourcePicker, ScreenShareQualityPicker, ScreenShareViewer

### 3.10 Tauri Native Webcam Capture
Full capture -> VP9 encode -> RTP pipeline for desktop webcam sharing. Device enumeration, quality selection, graceful shutdown. Registered as Tauri commands (`start_webcam`, `stop_webcam`).

- **Client (Tauri):** `client/src-tauri/src/commands/webcam.rs`, `client/src-tauri/src/capture/webcam.rs`

### 3.11 Connection Quality Monitoring
Real-time RTCStats polling with 3-second sampling. Tracks latency, packet loss, jitter. Quality classification (poor/fair/good/excellent). Per-participant quality dots with tooltip details. Threshold-based alerts (3% warning, 7% critical) with toast notifications. Connection history page with daily charts.

- **Server:** `server/src/connectivity/handlers.rs`
- **Client:** `client/src/components/settings/ConnectionChart.tsx`, `client/src/components/voice/QualityIndicator.tsx`

---

## 4. Chat & Messaging

### 4.1 Channel Text Messaging
Full CRUD for channel messages with cursor-based pagination. Real-time delivery via WebSocket (`MessageCreate`, `MessageUpdate`, `MessageDelete`). Content filtering integration on create and edit.

- **Server:** `server/src/chat/messages.rs` — create, list, update, delete
- **Client:** MessageList, MessageInput, MessageItem components

### 4.2 Message Editing
Server API `PATCH /api/messages/{id}` with content filtering and owner-only validation. Client inline edit UI: replace message text with pre-filled textarea, Enter to save, Escape to cancel. "(edited)" indicator with timestamp. Context menu integration.

- **Server:** `server/src/chat/messages.rs` — `update()`
- **Client:** Edit button in message actions, `setEditingMessageId` state

### 4.3 Message Deletion
Soft-delete (sets `deleted_at` timestamp) with anonymization support. WebSocket broadcast for real-time removal across clients.

- **Server:** `server/src/chat/messages.rs` — `delete()`

### 4.4 Direct Messages
DM channels with `get_or_create_dm()` and group DM creation. DM-specific read state tracking. Block checking before message delivery (Redis-cached). Leave functionality, DM avatar upload, and name customization.

- **Server:** `server/src/chat/dm.rs` (996 lines)
- **Client:** HomeSidebar with DM list, DMConversation, DMItem components

### 4.5 Message Threads (Slack-style)
Thread replies with `parent_id` FK. Automatic counter maintenance (`thread_reply_count`, `thread_last_reply_at`). Per-user per-thread read state tracking. Thread sidebar with parent message display and scrollable replies. Thread indicator on parent messages with participant avatars and unread dot. Per-guild enable/disable toggle.

- **Server:** Thread reply CRUD, WebSocket events (`thread_reply_new`, `thread_reply_delete`, `thread_read`)
- **Client:** `ThreadSidebar.tsx`, `ThreadIndicator.tsx`

### 4.6 File Attachments & Uploads
Drag-and-drop file upload with image previews. S3-compatible storage (RustFS). Media processing generates thumbnails and metadata. Authenticated proxy-based file downloads.

- **Server:** `server/src/chat/uploads.rs`, `server/src/chat/s3.rs`
- **Client:** Drag-and-drop in MessageInput, file preview before send

### 4.7 Progressive Image Loading (Blurhash)
Uploaded images processed to generate blurhash color previews plus thumbnail (256px WebP) and medium (1024px WebP) variants. Client displays instant blurhash placeholder during load with smooth fade-in transition. Aspect-ratio CSS prevents layout shift. Click opens full-resolution original.

- **Server:** Blurhash generation in upload pipeline
- **Client:** `client/src/components/ui/BlurhashPlaceholder.tsx`

### 4.8 Emoji Reactions
Add/remove reactions with Unicode and custom guild emojis. Real-time broadcast via `ReactionAdd`/`ReactionRemove` events. Quick reaction toolbar on message hover. ReactionBar displays reaction pills with counts and user lists.

- **Server:** `server/src/api/reactions.rs`
- **Client:** `ReactionBar.tsx`, quick reaction buttons, Alt+1..4 shortcuts

### 4.9 Emoji Picker
Categorized emoji picker with search, recent emojis tracking, and guild custom emoji support with image display. Portal-based rendering with `@floating-ui/dom` for viewport-aware positioning. Click-outside, Escape, and scroll-to-close behaviors. Integrated in both message input (composer button) and message hover actions (reactions).

- **Client:** `EmojiPicker.tsx`, `PositionedEmojiPicker.tsx`

### 4.10 Content Spoilers
`||text||` syntax via custom marked.js extension with 500-char limit (ReDoS prevention). Click-to-reveal functionality with per-message spoiler state tracking. DOMPurify sanitization.

- **Client:** `spoilerExtension.ts`, reveal logic in `MessageItem.tsx`

### 4.11 @Mentions & Autocomplete
Autocomplete triggers for `@user`, `#channel`, `:emoji:`, and `/command` in the message composer. Keyboard navigation (up/down + Enter/Tab). Server-side `@everyone`/`@here` permission validation (`MENTION_EVERYONE` permission bit 23).

- **Server:** `detect_mention_type()` in `messages.rs`
- **Client:** `AutocompletePopup.tsx`, autocomplete logic in `MessageInput.tsx`

### 4.12 Typing Indicators
WebSocket-based typing event broadcast with 3-second inactivity timeout. Animated dots display with user list ("X people are typing...").

- **Client:** `TypingIndicator.tsx`, typing events in `websocket.ts`

### 4.13 Code Block Syntax Highlighting
Custom `CodeBlock` component with highlight.js integration. Supports JavaScript, TypeScript, Python, Rust, JSON, Bash, and more. Triple-backtick with optional language hint. Dark theme with syntax coloring.

- **Client:** `client/src/components/ui/CodeBlock.tsx`

### 4.14 Markdown Rendering
GFM (GitHub Flavored Markdown) via `marked` library with custom spoiler extension. DOMPurify sanitization with whitelisted HTML tags. Full support for bold, italic, code, links, code blocks, lists, quotes, tables, headings.

- **Client:** Integrated in MessageItem rendering pipeline

### 4.15 Message Formatting Toolbar
Toolbar in message composer with Bold, Italic, Code, and Spoiler buttons. `insertFormatting()` wraps selected text with markdown delimiters and repositions cursor. Uses lucide-solid icons.

- **Client:** Format buttons in `MessageInput.tsx`

### 4.16 Full-Text Search
PostgreSQL `tsvector` with GIN index. Guild-scoped, DM-scoped, and global search endpoints. Advanced filters: date range, channel, author, `has:link`, `has:file`. Relevance ranking via `ts_rank` with sort toggle (Relevance/Date). Server-side context snippets via `ts_headline` with `<mark>` tags. Search syntax help tooltip. 38 integration tests.

- **Server:** `server/src/guild/search.rs`, `server/src/chat/dm_search.rs`, `server/src/api/global_search.rs`
- **Client:** `SearchPanel.tsx` with Ctrl+Shift+F shortcut, `SearchSyntaxHelp.tsx`

### 4.17 Unread Tracking & Aggregator
Per-channel, per-DM, and per-thread read state tracking with `last_read_at` and `last_read_message_id`. Centralized `GET /api/me/unread` returns aggregated unread counts across all guilds and DMs; each channel entry also has `last_read_message_id` (the read cursor, `null` if the channel was never read). UnreadModule in home sidebar with guild-grouped and DM unread counts, direct navigation, automatic refresh on window focus.

- **Server:** `server/src/api/unread.rs`
- **Client:** UnreadModule in home sidebar

### 4.18 Bulk Read Management
Bulk mark-as-read endpoints: all channels (`POST /api/me/read-all`), per-guild (`POST /api/guilds/{id}/read-all`), all DMs (`POST /api/dm/read-all`). Context menu "Mark as Read" per channel. Cross-device sync via `ChannelRead`/`DmRead` WebSocket events.

- **Server:** `server/src/api/unread.rs` — `mark_all_read()`

### 4.19 Cross-Client Read Sync
Read position syncs instantly across all user devices/tabs via dedicated `user:{user_id}` Redis channel. Clear unread badges on any client when read on another.

### 4.20 Persistent Message Drafts
Auto-saved per-channel drafts restored on navigation. Draft content persists across channel switches.

### 4.21 Slash Commands
Bot slash command framework with registration, option types (String, Integer, Boolean, User, Channel, Role), and invocation routing. Built-in `/ping` command. Command response delivery via WebSocket relay with 30-second timeout. Frontend autocomplete with hyphen support and ambiguity labels.

- **Server:** `server/src/api/commands.rs`
- **Client:** `/command` autocomplete in `MessageInput.tsx`

### 4.22 Personal User Pins
User-level pin system with Note, Link, and Message pin types. Up to 50 pins per user with position ordering and drag-and-drop reordering. Pins module in home sidebar.

- **Server:** `server/src/api/pins.rs` (411 lines)
- **Client:** PinsModule in home sidebar

---

## 5. Guild & Channel Management

### 5.1 Guild CRUD
Full guild lifecycle: create, update (name, description, icon), delete. Owner-only operations with user guild limit enforcement (config-driven, advisory lock protected).

- **Server:** `server/src/guild/handlers.rs`
- **Client:** `CreateGuildModal.tsx`, `GuildSettingsModal.tsx`

### 5.2 Guild Invites
Cryptographically random 8-char invite codes with configurable expiry (30m, 1h, 1d, 7d, never). Use count tracking, expiration enforcement. Global ban and guild-specific ban checks on join. Race condition protection via PostgreSQL advisory locks.

- **Server:** `server/src/guild/invites.rs`
- **Client:** `InvitesTab.tsx`, `JoinGuildModal.tsx`

### 5.3 Role System
Full role management with create, update, delete, assign, and remove operations. Default @everyone role auto-created on guild creation. Draggable role reordering with position tracking. Color picker support. Role hierarchy enforcement prevents managing roles at or above your level.

- **Server:** `server/src/guild/roles.rs`
- **Client:** `RolesTab.tsx`, `RoleEditor.tsx`, `MemberRoleDropdown.tsx`

### 5.4 Permission System
25 distinct guild permissions using bitflags covering content, voice, moderation, guild management, invites, pages, screen sharing, and mentions. Resolution algorithm: owner override -> @everyone base -> role layer (by position) -> channel-level overrides (allow/deny). Channel permission override UI with allow/deny/inherit states.

- **Server:** `server/src/permissions/` — guild.rs, resolver.rs, helpers.rs, models.rs, queries.rs, system.rs
- **Client:** `ChannelPermissions.tsx`, `RoleEditor.tsx`

### 5.5 Channel Categories
Two-level nesting support (parent categories with subcategories). Position-based ordering. Create, list, update, delete, and bulk reorder operations.

- **Server:** `server/src/guild/categories.rs`
- **Client:** `CategoryHeader.tsx`

### 5.6 Channel Drag-and-Drop Reordering
Drag channels within and between categories with visual drop zone feedback. Optimistic updates with server position sync. Also used for role reordering.

- **Client:** `ChannelDragContext.tsx`

### 5.7 Guild Custom Emojis
Upload PNG/JPEG/GIF/WebP emojis with animated emoji detection. Rename and delete operations. Configurable file size limits. S3-compatible storage. Emoji Manager UI in guild settings.

- **Server:** `server/src/guild/emojis.rs`
- **Client:** `EmojisTab.tsx`

### 5.8 Guild Discovery
Public guild browser with full-text search (`websearch_to_tsquery`), tag filtering, sort by members or newest, and pagination. Configurable discoverability per guild with tags editor (max 5, case-insensitive). Server config flag `ENABLE_GUILD_DISCOVERY`. Compass icon in ServerRail.

- **Server:** `server/src/discovery/handlers.rs`
- **Client:** `DiscoveryView.tsx`, `GuildCard.tsx`

### 5.9 Guild Resource Limits
Configurable per-instance limits enforced at 8 code points: guilds per user, members/channels/roles/emojis/bots per guild, webhooks per app. Environment variable driven with sensible defaults. Usage stats endpoint with parallel count queries. Frontend Usage tab with progress bars and color-coded thresholds.

- **Server:** `server/src/guild/limits.rs`
- **Client:** `UsageTab.tsx`

### 5.10 Guild Bots Management
Guild bot installation, listing, and removal. Integrated with bot ecosystem (slash commands, gateway events).

- **Client:** `BotsTab.tsx`

### 5.11 Member Kick
Owner/admin can remove members from a guild. Triggers MemberLeft bot ecosystem events.

- **Server:** `server/src/guild/handlers.rs` — `kick_member()`

---

## 6. Social & Presence

### 6.1 Friends System
Full lifecycle: send/accept/reject friend requests, remove friends, block/unblock users. Tabbed friends list (Online, All, Pending, Blocked) with search filtering. Add Friend modal.

- **Server:** `server/src/social/friends.rs`
- **Client:** `FriendsList.tsx`, `AddFriend.tsx`

### 6.2 User Blocking
Block/unblock via context menu with confirmation modal. Redis SET-based block cache for O(1) lookups. Blocked users cannot send DMs, friend requests, or initiate voice calls. Messages filtered in DM channel lists. WebSocket events filtered in real-time. 8 integration tests.

- **Server:** `server/src/social/friends.rs` — `block_user()`, `unblock_user()`
- **Client:** `BlockConfirmModal.tsx`

### 6.3 User Reporting
Report users or messages with 5 categories (harassment, spam, inappropriate content, impersonation, other). Rate-limited (5/hour) with duplicate prevention. Admin report queue with claim, resolve (dismiss/warn/ban/escalate), and stats. Real-time admin notifications. 17 integration tests.

- **Server:** `server/src/moderation/handlers.rs`
- **Client:** `ReportModal.tsx`, admin `ReportsPanel.tsx`

### 6.4 Rich Presence (Game Activity)
Automatic game/IDE detection via process scanning. Activity types: Game, Listening, Watching, Coding, Custom. Display in friends list, member list, and DM panels. Privacy toggle. Real-time sync via WebSocket. Custom app detection rules UI.

- **Server:** `server/src/presence/`
- **Client:** `client/src/stores/presence.ts`, `ActiveNowModule.tsx`

### 6.5 Online/Away/Busy/Invisible Status
Standard presence statuses with DND (Do Not Disturb) mode that suppresses notification sounds. Tauri integration for native status updates.

- **Client:** `StatusPicker.tsx`

---

## 7. User Interface & Experience

### 7.1 Home View
Three-column layout: sidebar (DM list sorted by last message), main content (friends or conversation), right panel (modular components). Unread counts and last message previews.

- **Client:** `HomeView.tsx`, `HomeSidebar.tsx`, `HomeRightPanel.tsx`

### 7.2 Server Rail & Navigation
Vertical server list sidebar for context switching between guilds. Home button, guild icons, and discovery compass.

### 7.3 Command Palette (Ctrl+K)
Global fuzzy search for channels and users. Keyboard navigation. Command execution with `>` prefix. Voice commands (Mute, Deafen). "Search Everywhere" entry point.

- **Client:** `CommandPalette.tsx`

### 7.4 Theme System
Multiple themes: CachyOS Nordic (default), Solarized Dark, Solarized Light. Visual radio cards with preview dots. Color customization support. Theme selection synced across devices via preferences.

- **Client:** `AppearanceSettings.tsx`, `client/src/stores/theme.ts`

### 7.5 Context Menus
Portal-based global context menu system with keyboard navigation (Arrow keys, Enter, Home/End, Escape). Message menu (copy, edit, delete, pin), channel menu (mark read, mute, favorite, edit, copy ID), user menu (profile, message, friend, block, copy ID).

- **Client:** `ContextMenu.tsx`, `client/src/lib/contextMenuBuilders.ts`

### 7.6 Modular Home Sidebar
Collapsible modules with server-synced state: UnreadModule, ActiveNowModule (friends' game activity), PendingModule (friend requests), PinsModule (notes/links/bookmarks).

- **Client:** `client/src/components/home/modules/`

### 7.7 Cross-Server Favorites
Pin channels from different guilds into unified Favorites section in sidebar. Star icon toggle on hover. Grouped by guild. Maximum 25 favorites per user.

- **Server:** `server/src/api/favorites.rs`
- **Client:** `FavoritesSection.tsx`

### 7.8 Personal Workspaces (Favorites v2)
Named workspace folders for cross-guild channel aggregation. 9 REST endpoints, 7 WebSocket events. Drag-and-drop reordering. Configurable limits (default 20 workspaces, 50 entries). Guild membership + VIEW_CHANNEL permission enforcement. 17 integration tests.

- **Server:** `server/src/workspaces/handlers.rs`

### 7.9 Sound Pack (Notification Sounds)
5 notification sounds: Default, Subtle, Ping, Chime, Bell. Volume control with test button. Per-channel notification levels (All, Mentions only, Muted). Smart playback with cooldown, tab leader election, and mention detection. Native audio via rodio (Tauri), Web Audio API fallback (browser).

- **Client:** `client/src/stores/sound.ts`

### 7.10 Do Not Disturb & Quiet Hours
Notification sounds suppressed when user status is Busy. Scheduled quiet hours with configurable start/end times. Handles overnight ranges. Call ring sounds also suppressed.

### 7.11 Keyboard Shortcuts Dialog
Ctrl+/ opens help dialog listing all shortcuts grouped by category (Navigation, Voice, Messaging). Discoverable shortcuts: Ctrl+K (command palette), Ctrl+Shift+F (global search), Ctrl+Shift+M (mute), Ctrl+Shift+D (deafen), Ctrl+B/I/E (formatting), Alt+1-4 (quick reactions).

- **Client:** `KeyboardShortcutsDialog.tsx`

### 7.12 Context-Aware Focus Engine
Intelligent notification routing based on detected foreground apps (IDEs, games, DAWs). Focus modes with suppression levels. VIP user/channel overrides for bypassing DND. Emergency keyword bypass. Custom app detection rules UI. Unified `shouldNotify()` gate (DND + focus mode + channel level). OS desktop notifications via `tauri-plugin-notification` with Web Notification API fallback. Privacy-safe generic bodies for E2EE messages.

- **Client:** `client/src/stores/focus.ts`, `FocusSettings.tsx`

### 7.13 Onboarding Wizard (First-Time Experience)
5-step wizard: Welcome (display name) -> Theme selection -> Mic setup -> Join a server (mini discovery grid or invite code) -> Done. Skip/back navigation, progress dots, focus trap for accessibility. ARIA-compliant tabs. Re-triggerable from Settings > Appearance. Shows on first login.

- **Client:** `OnboardingWizard.tsx`

### 7.14 Session Expiry Notification
When proactive token refresh fails, `kaiku:session-expired` event triggers toast notification or redirect to login.

### 7.15 Friends Tab Empty State
Contextual hint "You have N pending request(s)" with link to Pending tab when the All tab is empty but pending requests exist.

---

## 8. Information Pages & Digital Library

### 8.1 Platform & Guild Pages
Platform-wide pages (ToS, Privacy Policy) and guild-level pages (Rules, FAQ) with rich Markdown editor, live preview, and Mermaid diagram support. Page acceptance tracking with scroll-to-bottom requirement.

- **Server:** `server/src/pages/`
- **Client:** `client/src/components/pages/`

### 8.2 Digital Library (Wiki)
Revision history with content snapshots (SHA-256 dedup) and configurable pruning. Guild-scoped page categories with CRUD and drag-and-drop reordering. Deep-linkable heading anchors via slugified IDs. Library catalog view with category filtering. Per-guild configurable page/revision limits with admin overrides. 13 new API endpoints.

- **Client:** `LibraryCatalog.tsx`, `RevisionHistory.tsx`, `CategoryManager.tsx`

---

## 9. Admin & Moderation

### 9.1 Admin Dashboard
Command Center with system health monitoring, real-time metrics (uPlot charts), top routes/errors tables. Paginated log and trace search. Guild management, user management (ban/delete), platform pages editor, auth settings with OIDC provider management. Audit log viewer.

- **Server:** `server/src/admin/`
- **Client:** `client/src/components/admin/` — AdminSettings, CommandCenterPanel, GuildsPanel, UsersPanel, ReportsPanel, AuditLogPanel

### 9.2 Content Filters (Safety)
Guild-configurable content filters with hybrid Aho-Corasick keyword + regex engine. Built-in categories: Slurs, Hate Speech, Spam, Abusive Language with Block/Log/Warn actions. Custom guild patterns with ReDoS protection (10ms timeout). Per-guild cached FilterEngine with generation-counter invalidation. Dry-run test endpoint. Integrated into message create/edit/upload (skips encrypted and DM messages). 17 integration tests + 9 unit tests.

- **Server:** `server/src/moderation/filter_engine.rs`, filter_handlers.rs
- **Client:** `SafetyTab.tsx`

### 9.3 Rate Limiting
Redis-based fixed window rate limiting with Lua scripts. Hybrid IP/user identification with configurable trust proxy. Categorized limits (Auth, Messages, Search, DataGovernance, etc.). Failed auth tracking with automatic IP blocking. X-RateLimit headers.

- **Server:** `server/src/ratelimit/`

---

## 10. Developer Ecosystem

### 10.1 Bot Platform
Bot application CRUD with secure token auth (Argon2id hashing, indexed O(1) lookup). Gateway WebSocket (`/api/gateway/bot`) with Redis pub/sub. Intents system (messages, members, commands). Slash command registration (guild-scoped + global, bulk registration). Command invocation routing with ambiguity detection. Bot message sending. Guild bot installation API. 14+ integration tests.

- **Server:** `server/src/api/bots.rs`, `server/src/ws/bot_gateway.rs`
- **Client:** BotsTab in guild settings
- **Docs:** `docs/developer-guide/development/bot-system.md`

### 10.2 Webhooks
Per-application webhooks (up to 5) with HMAC-SHA256 signed payloads. Event types: `message.created`, `member.joined`, `member.left`, `command.invoked`. Automatic retry with exponential backoff (5 attempts). Dead-letter storage and delivery log. DNS rebinding SSRF protection. Signing secrets encrypted at rest.

- **Server:** `server/src/webhooks/`
- **Client:** Webhook management UI with test delivery

---

## 11. Compliance & Data Governance

### 11.1 Data Export (GDPR)
Full data export pipeline: profile, messages, guilds, friends, preferences, DMs, reactions, sessions, E2EE metadata, audit logs. Versioned JSON ZIP archive (v1.1) uploaded to S3 with email notification. 7-day download expiry. Rate-limited (2 req/60s).

- **Server:** `server/src/governance/export.rs`

### 11.2 Account Deletion
30-day grace period with cancellation support. Password verification for local auth. Guild ownership transfer required before deletion. Hourly worker processes expired deletions. Messages anonymized (`user_id` set NULL). 11 integration tests.

- **Server:** `server/src/governance/deletion.rs`

---

## 12. Infrastructure & Observability

### 12.1 Native Observability
OpenTelemetry-integrated tracing and logging. 13+ metrics tracked. PostgreSQL-backed native ingestion pipeline with retention. Voice health scoring. Command Center backend with 7 endpoints. Log and trace search.

- **Server:** `server/src/observability/`

### 12.2 Server-Synced Preferences
JSONB preferences storage with real-time WebSocket sync. Covers theme, sound, notification, channel, DnD, and focus settings. Last-write-wins conflict resolution with timestamps. Migration from legacy localStorage keys.

- **Server:** `server/src/api/preferences.rs`
- **Client:** `client/src/stores/preferences.ts`

### 12.3 CI Pipeline
All jobs passing: Rust Lint (fmt + clippy), Rust Tests, Frontend, License Compliance, Secrets Scan, Docker Build, Tauri (Ubuntu + macOS). Known limitation: Windows Tauri build fails (libvpx).

### 12.4 E2E Test Suite
68 UI items across 12 Playwright spec files. Shared test helpers for login, navigation, and utilities.

---

## 13. Branding & Visual Identity

### 13.1 Mascot & Assets
Finnish Lapphund mascot with Solarized Dark assets (Hero, Icon, Monochrome). Premium asset suite for branding consistency.
