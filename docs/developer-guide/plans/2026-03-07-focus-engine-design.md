# Context-Aware Focus Engine — Design

**Date:** 2026-03-07
**Status:** Implemented
**Phase:** 6 (Competitive Differentiators & Mastery)

## Overview

Complete the Focus Engine by wiring the existing policy evaluation into the notification pipeline, adding native OS desktop notifications via `tauri-plugin-notification`, and implementing foreground app detection via `sysinfo` process scanning. Delivered as three independent layers, each shippable on its own.

## Current State

Already implemented:
- `focus.ts` (248 lines) — Focus modes, auto-activation, `evaluateFocusPolicy()` with O(1) VIP lookups
- `FocusSettings.tsx` (654 lines) — Full UI for mode management, VIP lists, emergency keywords
- 3 built-in modes: Gaming, Deep Work, Streaming
- Activity detection event plumbing (`presence:activity_changed`)
- Preferences sync (server JSONB + localStorage + WebSocket cross-device)
- Sound system with DND check, quiet hours, per-channel notification levels

Not wired up:
- `evaluateFocusPolicy()` is never called from the sound/notification path
- No OS-level notifications (toast-only)
- No Tauri-side process scanner emitting `presence:activity_changed`

## Layer 1: Policy Wiring

Insert focus policy evaluation into the notification path.

### Flow

```
Current:  SoundEvent -> isDndActive()? -> play sound
New:      SoundEvent -> shouldNotify(event)? -> play sound + OS notification
```

`shouldNotify(event: SoundEvent): boolean` combines:
1. DND check (absolute block, no exceptions)
2. `evaluateFocusPolicy(event)` — suppress/allow with VIP user, VIP channel, emergency keyword, and mention/DM pass-through
3. Channel notification level ("all", "mentions", "muted")

Single gate function used by both the sound path and OS notification path.

## Layer 2: OS Notifications

Add native desktop notifications via `tauri-plugin-notification`.

### When to Show

- App window is **not focused** (backgrounded/minimized)
- `shouldNotify()` gate passes
- Channel notification level permits it

### Notification Content

| Event | Title | Body |
|-------|-------|------|
| `message_dm` | "{username}" | Message preview (truncated ~100 chars) |
| `message_mention` | "#{channel} in {guild}" | "@you: preview..." |
| `message_thread` | "Thread reply in #{channel}" | "{username}: preview..." |
| `call_incoming` | "Incoming call" | "{username} is calling you" |

Clicking a notification navigates to the relevant channel/DM via Tauri event deep link.

### Privacy

- "Show notification content" toggle (default: on). When off, generic body: "New message" / "New mention"
- E2EE messages always show generic body (content unavailable in plaintext)

### New User Preferences

```typescript
notifications: {
  os_enabled: boolean;       // Master toggle, default true
  show_content: boolean;     // Preview in notification, default true
  flash_taskbar: boolean;    // Taskbar/dock badge, default true
}
```

Added to `NotificationSettings.tsx` as a "Desktop Notifications" section with test button.

## Layer 3: Foreground App Detection

Tauri-side Rust task using `sysinfo` to detect the active foreground application.

### Mechanism

- Background task runs every **15 seconds**
- Gets the **foreground window's process** (not all processes):
  - Linux: X11/Wayland APIs for active window PID, then resolve via `sysinfo`
  - Windows: `GetForegroundWindow` API
  - macOS: `NSWorkspace.frontmostApplication`
- Matches process name against built-in category map + user-defined custom entries
- Emits `presence:activity_changed` only when the detected category **changes**

### Built-in Process Categories

| Category | Example processes |
|----------|-------------------|
| `game` | `steam`, `lutris`, `gamescope`, common launchers |
| `coding` | `code`, `zed`, `nvim`, `idea`, `cursor`, `windsurf` |
| `listening` | `spotify`, `tidal`, `rhythmbox` |
| `watching` | `vlc`, `mpv`, `obs` |

### User Customization

- Settings UI for custom process name to category mappings
- Stored in preferences: `focus.custom_app_rules: Record<string, FocusTriggerCategory>`
- Built-in list is append-only (users add, cannot remove defaults)
- Added to `FocusSettings.tsx` as "App Detection" section

### Privacy

- **Opt-in** (off by default, toggle in Focus Settings)
- Process names never leave the device — only the matched **category** is emitted
- Activity sent to server: `{ type: "game", name: "Gaming" }` — no process name

### Flow

```
Every 15s: get foreground PID -> resolve process name -> match category
  -> category changed? -> emit presence:activity_changed(category)
  -> focus store auto-activates matching mode (if enabled)
  -> no match? -> emit null -> deactivate auto-activated mode
```

## Testing Strategy

- Unit tests for `shouldNotify()` covering all policy combinations (DND, focus modes, VIP, keywords, channel levels)
- Unit tests for process name to category matching
- Integration test for notification suppression during active focus mode
- Manual test for OS notifications on Linux/Windows/macOS

## Files Changed

| File | Change |
|------|--------|
| `client/src/stores/sound.ts` | Call `shouldNotify()` gate before playing sounds |
| `client/src/stores/focus.ts` | Export `shouldNotify()` combining DND + focus + channel level |
| `client/src/stores/preferences.ts` | Add `notifications` preference block |
| `client/src/components/settings/NotificationSettings.tsx` | Desktop Notifications section |
| `client/src/components/settings/FocusSettings.tsx` | App Detection section, custom rules |
| `client/src-tauri/tauri.conf.json` | Add `tauri-plugin-notification` capability |
| `client/src-tauri/Cargo.toml` | Add `tauri-plugin-notification` dependency |
| `client/src-tauri/src/commands/` | New `activity_scanner.rs` for process detection |
| `client/src/lib/notifications.ts` | New: OS notification wrapper with deep-link click handling |
