# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- First user setup wizard with automatic admin bootstrap
  - First user to register automatically receives system admin permissions
  - Mandatory setup wizard for configuring server name, registration policy, and legal URLs
  - Setup completion is permanent and cannot be re-triggered (prevents takeover)
  - Transaction-based race condition prevention for concurrent first-user registrations
  - Setup status exposed in authentication responses (`setup_required` flag)
  - API endpoints: `GET /api/setup/status`, `GET /api/setup/config`, `POST /api/setup/complete`
- Right-click context menus for messages, channels, and users
  - Message menu: Copy Text, Copy Message Link, Copy ID, Delete (own messages)
  - Channel menu: Mark as Read, Mute/Unmute, Add to Favorites, Edit Channel, Copy ID
  - User menu: View Profile, Send Message, Add Friend, Block, Copy User ID
  - Keyboard navigation with Arrow keys, Enter, Home/End, and Escape
- Custom avatars for DM and Group DM conversations (#104)
  - Click avatar in conversation header to upload custom icon
  - Avatars visible in sidebar and conversation header
  - Works for both 1:1 DMs and group DMs
- Custom display name for group DMs (#91)
  - PATCH `/api/dm/:id/name` endpoint to rename group DMs
  - Inline editing in conversation header (click group name to edit)
  - Real-time name sync via WebSocket `dm_name_updated` event
- Join Server button in server rail with invite code/link input modal
- Pixel art theme system with "Pixel Cozy" warm earth-tone theme
- Theme family architecture allowing community theme variants
- Structural CSS tokens (radii, fonts, shadows) for theme-aware UI
- Dithered texture patterns for pixel theme backgrounds
- Press Start 2P pixel font for UI elements
- Theme creation guide (THEME_GUIDE.md) for community contributors
- Cross-family fade transition when switching between theme types
- Theme consistency tests for validating theme definitions
- WebSocket notifications for friend requests
  - Real-time notification when receiving a friend request
  - Real-time notification when a friend request is accepted
- Profile update endpoint (`POST /auth/me`) for changing display_name and email
- Real-time state synchronization via WebSocket patch events
  - Incremental updates for users, guilds, and members (only changed fields sent)
  - Automatic subscription to guild event channels for member updates
  - Client-side patch handlers with field validation
- Fedora Atomic (Silverblue/Kinoite) support in development setup scripts
  - Automatic detection of immutable Fedora systems
  - Distrobox-based development container with all dependencies
  - Optional rpm-ostree layering fallback (with reboot requirement)
  - Export of development tools (cargo, bun, sqlx) to host `~/.local/bin`
  - Podman support alongside Docker for container orchestration
- Full-text message search using PostgreSQL tsvector
  - Search messages within a guild using `websearch_to_tsquery` syntax (supports AND, OR, quotes)
  - GIN index for fast search performance
  - Search panel in sidebar with debounced input (300ms)
  - Results show channel name, author, timestamp, and highlighted content preview
  - Click results to navigate directly to the message
  - Pagination support with "Load more" for large result sets
- Cross-server favorites: pin channels from different guilds into a unified Favorites section
  - Star icon on channels to toggle favorites (appears on hover, filled when favorited)
  - Expandable Favorites section in Sidebar grouped by guild
  - Click favorites to navigate directly to that guild and channel
  - Maximum 25 favorites per user
  - Automatic cleanup when last channel from a guild is unfavorited
- Modular Home Sidebar with collapsible modules:
  - Active Now module shows friends currently playing games
  - Pending module with quick accept/decline for friend requests
  - Pins module for saving notes and links across devices
  - Module collapse state syncs across all devices via server preferences
- Server-synced user preferences
  - Theme, sound settings, quiet hours, and per-channel notifications sync across all devices
  - Real-time updates via WebSocket when preferences change on another device
  - Offline support with automatic sync on reconnect
  - Migration from legacy localStorage keys
- Cross-client read sync for DMs
  - When reading a DM on one device, unread badges clear on all other devices instantly
  - Uses new `user:{user_id}` Valkey channel for user-targeted events
  - Real-time synchronization via WebSocket
- Unread message tracking for guild channels
  - Unread badge on text channels shows count of unread messages (capped at 99+)
  - Auto-marks channel as read after 1 second when viewing
  - Cross-device sync: reading on one device clears badges on all devices
  - Category headers indicate when any channel within has unread messages
- Do Not Disturb mode for notifications
  - Notification sounds suppressed when user status is "Do Not Disturb" (Busy)
  - Scheduled quiet hours with configurable start/end times
  - Handles overnight ranges (e.g., 22:00 to 08:00)
  - Call ring sounds also suppressed during DND
  - Quiet Hours settings UI in Settings > Notifications
- DM voice calls client integration
  - Voice connection wiring: WebRTC starts after accepting/starting calls
  - Tauri commands for call lifecycle (start, join, decline, leave)
  - Ring sound notification with looping playback for incoming calls
  - Call indicator in DM sidebar with pulsing animation for incoming calls
- Clipboard protection system for secure copy/paste operations
  - ClipboardGuard service with SHA-256 hash-based tamper detection
  - Auto-clear timers based on sensitivity (Critical: 60s, Sensitive: 120s)
  - Paranoid mode with 30s timeout for all sensitive content
  - Protection levels: Minimal, Standard, Strict (configurable in Settings)
  - UI components: ClipboardToast, ClipboardIndicator, TamperWarningModal
  - Browser fallback support when not running in Tauri
  - Integration with recovery phrase and invite link copying
- Sound notification system for chat messages
  - 5 notification sounds: Default, Subtle, Ping, Chime, Bell
  - Global notification settings in Settings > Notifications tab
  - Per-channel notification levels: All messages, Mentions only, or Muted
  - Volume control with test sound button
  - Smart playback with cooldown, tab leader election (web), and mention detection
  - Native audio playback via rodio in Tauri, Web Audio API fallback in browser
  - Muted channel indicator (bell-off icon) in channel list
- Home View Overhaul with "Friends First" design
  - Unified `HomeSidebar` replacing the legacy double-sidebar layout
  - Default "Friends" landing view with filter search (Online/All/Pending/Blocked)
  - "Active Now" panel showing real-time friend activity (games, voice, etc.)
  - Information section for server-wide pages (Rules, Announcements)
  - Collapsible Direct Messages list sorted by recent activity
- Custom Avatars system
  - `POST /auth/me/avatar` endpoint with S3/MinIO storage backend
  - "My Account" settings tab with avatar upload and preview
  - Client-side validation for image type and size (5MB limit)
  - Instant profile update propagation across the UI
- Status Picker in User Panel (Online, Away, Do Not Disturb, Invisible)
- Rich Presence (Game Activity) showing "Playing X" status in member lists
  - Automatic game detection via process scanning (sysinfo crate)
  - 15+ pre-configured games (Minecraft, Valorant, League of Legends, CS2, Fortnite, etc.)
  - Activity display in guild member list, friends list, and DM panels
  - Privacy toggle in settings to disable activity sharing
  - Real-time activity sync via WebSocket
- Screen sharing capability in browser clients with quality selection (480p/720p/1080p)
- Screen share button in voice controls with quality picker dialog
- Screen share indicator on participant avatars in voice panel
- WebSocket event handlers for screen share state synchronization
- User feature flags system for premium feature control (PREMIUM_VIDEO)
- Quality enum for screen share quality tiers (Low/Medium/High/Premium)
- User Connectivity Monitor for real-time voice connection quality tracking
  - Live quality indicators (latency, packet loss, jitter) in VoiceIsland and participant list
  - Toast notifications for connection issues (warning at 3% loss, critical at 7%)
  - Connection History page (`/settings/connection`) with 30-day analytics
  - TimescaleDB storage with automatic compression and 7-day retention
- E2EE key management with Olm protocol using vodozemac library
- Recovery key generation with Base58 display format for user backup
- Encrypted key backup with AES-256-GCM and Argon2id key derivation
- Multi-device support with per-device identity keys
- One-time prekey upload and atomic claiming for session establishment
- E2EE Key Backup UI with recovery key modal, security settings, and setup prompts
  - Recovery key modal with copy/download and confirmation flow
  - Security Settings tab showing backup status
  - Post-login E2EE setup prompt (skippable or mandatory via server config)
  - Backup reminder banner for users without backup
  - Server configuration option `REQUIRE_E2EE_SETUP` for mandatory setup
- End-to-End Encrypted DM messaging
  - LocalKeyStore with encrypted SQLite storage for Olm accounts and sessions
  - CryptoManager for session management and encrypt/decrypt operations
  - Tauri commands for E2EE initialization, encryption, and decryption
  - E2EE store for frontend state management with Solid.js signals
  - E2EE setup modal with recovery key generation and secure clipboard integration
  - Encryption indicator (lock/unlock icon) in DM conversation headers
  - Graceful fallback to unencrypted messaging when E2EE not available
  - Automatic decryption of incoming encrypted messages
- Information Pages system for platform-wide and guild-specific content (ToS, Privacy Policy, FAQ, rules, guides)
- Markdown editor with live preview, toolbar, and cheat sheet
- Mermaid diagram support in markdown preview
- Page acceptance tracking with scroll-to-bottom requirement for mandatory pages
- Page ordering via drag-and-drop with position persistence
- Audit logging for all page operations (create, update, delete, reorder)
- CHANGELOG.md following keepachangelog.com format
- Changelog maintenance guidelines in CLAUDE.md
- Admin Dashboard with user management, guild oversight, and audit log viewing
- AdminQuickModal for quick admin access with elevation status and stats
- Session elevation system with MFA verification and 15-minute expiry
- Ban/unban users and suspend/unsuspend guilds (requires elevation)
- Admin panel Phase 1 improvements
  - User avatars and guild icons displayed in admin lists
  - Skeleton loading animations replacing text placeholders
  - Keyboard navigation (Arrow keys, Enter, Escape) in user/guild tables
- Admin panel Phase 2 improvements
  - Server-side search for users (by username, display name, email) and guilds (by name)
  - Debounced search input with 300ms delay for reduced API calls
  - Audit log advanced filters: date range picker and action type dropdown
  - Active filter indicators showing current filter state
- Admin panel Phase 3 improvements
  - User detail expansion showing last login, guild count, and guild memberships
  - Guild member preview with owner info and stacked avatar display of top members
  - Lazy-loaded detail panels to reduce initial load time
- Admin panel Phase 4 improvements
  - CSV export for users and guilds with current search/filter applied
  - Bulk ban users with checkbox selection and confirmation dialog
  - Bulk suspend guilds with multi-select and reason input
  - Select all / clear selection controls in bulk action bar
- Admin panel Phase 5 improvements
  - Real-time updates via WebSocket for admin actions (ban/unban, suspend/unsuspend)
  - Undo functionality with toast notifications for ban and suspend actions (5-second window)
  - Toast action buttons for immediate undo capability
  - Admin event subscription for elevated admins
- Screen share viewer with three view modes (Spotlight, PiP, Theater)
- Volume control for screen share audio
- Screen share list in voice panel showing active shares
- Click-to-view screen shares from participant indicators
- Voice quality indicators with real-time latency, packet loss, and jitter display
- Accessibility shapes (circle/triangle/hexagon) for colorblind-friendly status indicators
- User status picker with Online, Idle, Do Not Disturb, and Invisible options
- Custom status with emoji and auto-expiry timer
- Automatic idle detection after configurable inactivity timeout
- Message reactions with emoji picker
- Emoji search, recent emojis, and favorites support
- Guild custom emoji database schema and API
- Channel categories with 2-level folder hierarchy
- Collapsible category headers with unread indicators
- Drag-and-drop reordering for channels and categories
- Display preferences for indicator modes (dense/minimal/discord)
- Guild Emoji Manager
  - Custom guild emoji support with `MANAGE_EMOJIS_AND_STICKERS` permission
  - Animated emoji support (GIF, WebP)
  - Drag-and-drop upload in Guild Settings > Emojis tab
  - Bulk upload utility script (`scripts/upload_emojis.py`)

### Changed
- **BREAKING:** Authentication responses now include `setup_required` boolean field
  - Affects `POST /auth/register`, `POST /auth/login`, and `POST /auth/refresh` endpoints
  - Existing clients must handle the new field or update their deserialization logic
  - Field indicates whether server setup wizard should be displayed
- JWT signing algorithm upgraded from HS256 → EdDSA (Ed25519) for stronger cryptographic security
- Infrastructure: Redis replaced with Valkey for open-source compatibility
- API error response format standardized across all endpoints (consistent error codes)
- Health endpoint now verifies database and Redis connectivity (returns "degraded" status on failure)
- Consolidated ThemeName type to single source of truth in types.ts
- UnoCSS theme config now uses CSS custom properties for border-radius and shadows
- StatusIndicator component now uses SVG shapes instead of colored dots
- UserStatus type extended from 4 to 5 statuses (added 'dnd')
- Improved UI contrast and accessibility
  - Fixed unreadable text in selected Settings tabs (high contrast text)
  - Updated error banners to use semantic theme tokens for better visibility
  - Added clearer separator lines in sidebars for visual hierarchy
  - Increased border visibility to `border-white/10` for main layout framing

### Deprecated

### Removed
- Legacy unscoped `GET /api/channels` endpoint that returned all channels across all guilds

### Fixed
- Friend requests failing in desktop client (#107)
  - Removed dead Tauri `invoke()` branches for friend commands (no corresponding Rust commands existed)
  - All friend operations now use HTTP API consistently
- DM avatar upload now works in desktop client and updates reactively (#104)
- DM list now shows online status for 1:1 conversations using presence store
- Kick member button now properly checks KICK_MEMBERS permission
- Voice session finalization with retry logic for reliability
- Redis broadcast failures now logged instead of silently ignored
- VoiceStatsLimiter periodic cleanup prevents memory leaks
- Admin elevated session cache falls back to database on cache miss
- Pixel font rendering fixed (blurry text in pixel themes)
- Reaction API endpoints corrected with proper WebSocket handlers
- Message pagination cursor comparison corrected for stable ordering
- Message pagination cursor comparison corrected for stable ordering
- Server build failure fixed by adding `Deserialize` to `GuildEmoji`
- **Fixed**: Pinned notes functionality restored (fixed API wrapper implementation) (#105)
- Documentation formatting in admin handlers fixed

### Security
- Attachment access now enforces guild/DM membership (previously any authenticated user could access any file)
- CORS hardened with configurable origins for production (development mode allows any origin)
- Request-ID header propagation for security tracing and correlation
- MFA verification enforced during login flow
- Markdown HTML output sanitized with DOMPurify to prevent XSS
- Prevented `@everyone` role from being assigned dangerous permissions (e.g., `MANAGE_GUILD`, `BAN_MEMBERS`) via API validation
- XSS hardening for Mermaid SVG rendering (forbid foreignObject, style, script tags)
- Ownership verification in page reorder operations prevents cross-guild attacks
- Fail-fast permission checks on database errors (no silent auth bypass)

## [0.1.0] - 2026-01-18

### Added
- Permission system with API handlers for admin, roles, and overrides
- Rate limiting middleware with Redis-based tracking and route integration
- Hierarchical AGENTS.md documentation for codebase navigation
- Git workflow design with commit conventions and worktree strategy
- Docker-based test infrastructure with proper database permissions
- Admin panel with two-tier privilege model (base admin + elevated admin)
- System audit logging for compliance and security tracing
- Global user bans and guild suspension capabilities
- System announcements feature

### Changed
- Hardened Docker infrastructure with Bitnami images
- Reworked persona system to concern-based code reviews
- Updated roadmap with Phase 3 status (rate limiting complete)

### Fixed
- File upload and download issues
- Merge conflict resolution from cherry-pick
- Dockerfile path reference in CI workflow
- MinIO image configuration (switched to official minio/minio)
- Bitnami images using :latest tag
- Secondary sort key for stable message ordering
- SQLx test fixtures with proper db permissions
- Code formatting and clippy warnings

[Unreleased]: https://github.com/Wolftown-io/canis/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Wolftown-io/canis/releases/tag/v0.1.0
