# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
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
- Screen share viewer with three view modes (Spotlight, PiP, Theater)
- Volume control for screen share audio
- Screen share list in voice panel showing active shares
- Click-to-view screen shares from participant indicators

### Changed

### Deprecated

### Removed

### Fixed

### Security
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
