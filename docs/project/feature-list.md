# Kaiku Feature List (Phases 0-5)

This document catalogs the major features implemented in the Kaiku (VoiceChat) project up through Phase 5 of the architectural roadmap.

## Core Collaboration Foundation
- **Multi-Server "Guild" Architecture**: Discord-like server structure with distinct channels and role-based access.
- **Direct Messaging & Group DMs**: 1:1 and group conversations outside of the guild structure.
- **Friends System**: Send, accept, reject, and block users with real-time status updates.
- **Personal Workspaces**: Cross-guild "Mission Control" views for grouping relevant channels.
- **Hierarchical Channel Categories**: 2-level folder hierarchy with drag-and-drop reordering.
- **Threaded Conversations**: Slack-style message threads for organized side-discussions.

## Real-Time Communication
- **Text Chat with Rich Media**: Markdown support, code syntax highlighting, custom guild emojis, and animated emojis.
- **File Attachments**: Drag-and-drop secure file uploads with progressive blurhash image loading.
- **Mention System & Autocomplete**: `@user`, `@here`, and `@everyone` support with fuzzy matching dropdowns.
- **Content Spoilers**: Click-to-reveal `||hidden||` text handling.
- **Reactions**: Quick emoji reactions and custom emoji picker.
- **Global & Contextual Search**: Full-text PostgreSQL search across guilds, DMs, and users with result highlighting.

## Advanced Voice & Video
- **WebRTC Voice Channels**: Low-latency Rust/Tauri backed audio with automatic connection dispute handling.
- **DM Voice Calls**: Ringing and join/decline workflows for direct conversations.
- **Voice Activity Detection (VAD)**: Real-time speaking indicators and intelligent bandwidth scaling.
- **Dynamic Voice Island**: Draggable picture-in-picture overlay for global voice controls (mute, deafen, disconnect).
- **Native Screen Sharing**: Multi-stream capability (VP9) with 480p to 1080p quality tiers and viewer layouts (Spotlight, PiP, Theater).
- **Native Webcam Support**: Multi-stream webcam capabilities alongside screen sharing.
- **Quality Telemetry**: Live ping, packet loss, and jitter monitoring with 30-day historical analytics.

## Security & Privacy
- **End-to-End Encryption (E2EE)**: Olm protocol implementation for 100% secure direct messaging.
- **Device & Key Management**: Encrypted local key storage with Argon2id and base58 recovery phrases.
- **Absolute Blocking**: Server-enforced user blocking applying to DMs, friend requests, and voice interactions.
- **Moderation Engine**: Built-in content filters (Aho-Corasick + Regex) for hate speech, spam, and custom guild rules.
- **User Reporting Workflow**: Report handling pipeline for server administrators.
- **Data Governance API**: One-click account data export (ZIP archive) and GDPR-compliant account deletion.

## Administration & Ecosystem
- **Granular Permissions System**: Bitfield-based channel (`VIEW_CHANNEL`) and role permissions.
- **Admin Dashboard**: Global user banning, guild suspension, and audit log analysis.
- **Bot Platform & Webhooks**: Bot application management, secure tokens, HMAC-SHA256 webhooks, and gateway intents.
- **Slash Commands**: Autocomplete global and guild-specific bots via `/command` interface.
- **Guild Discovery**: Public server browsing with tags and one-click joining.

## UX & Personalization
- **"Focused Hybrid" Theme Engine**: CachyOS Nordic palette baseline, Pixel Cozy variants, and CSS structural tokens.
- **Context-Aware Focus Engine**: Auto-activation quiet mode prioritizing essential notifications.
- **Rich Presence**: Real-time game process detection and broadcasting.
- **Cross-Device State Sync**: Read states, unread badges, and user preferences (Sound, Theme) automatically synchronize. 
- **Modular Home Sidebar**: Expandable active friends, pending requests, and cross-guild favorites panel.
