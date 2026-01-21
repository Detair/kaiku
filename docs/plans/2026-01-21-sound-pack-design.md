# Design: Sound Pack — Notification Sounds

**Date:** 2026-01-21
**Status:** Design Complete
**Priority:** Accessibility-first, Branding, Customization, Discord Parity

## Overview

A notification sound system for chat messages that works on both web and desktop (Tauri). Users hear audio cues for important events (DMs, mentions) with customizable sounds and per-channel notification levels.

## MVP Scope

**Single feature:** Chat message notification sounds

**Trigger conditions (smart defaults):**
- Direct messages → always play sound
- @mentions (@user, @everyone, @here) → always play sound
- Regular channel messages → no sound by default

**Per-channel override:** Users can set notification level per channel:
- All messages
- Mentions only (default for channels)
- None (muted)

**Sound options:** 5 built-in WAV sounds selectable in Settings → Notifications

**Platform support:** Works on web browser and Tauri desktop client

### Non-goals for MVP

- Voice channel sounds (join/leave, mute, PTT)
- UI feedback sounds (clicks, navigation)
- Custom sound uploads
- Cross-client read sync
- Server-synced settings

## Architecture

### Component Overview

```
┌─────────────────────────────────────────────────────────────┐
│                        Frontend (Solid.js)                   │
│  ┌─────────────────┐  ┌─────────────────┐  ┌──────────────┐ │
│  │ WebSocket       │  │ SoundService    │  │ Settings UI  │ │
│  │ (message events)│→ │ (playback logic)│← │ (user prefs) │ │
│  └─────────────────┘  └────────┬────────┘  └──────────────┘ │
└────────────────────────────────┼────────────────────────────┘
                                 │
              ┌──────────────────┴──────────────────┐
              ▼                                     ▼
┌─────────────────────────┐           ┌─────────────────────────┐
│   Web Browser Context   │           │     Tauri Context       │
│  ┌───────────────────┐  │           │  ┌───────────────────┐  │
│  │  Web Audio API    │  │           │  │  Rust Audio Cmd   │  │
│  │  + Notification   │  │           │  │  (rodio crate)    │  │
│  │    API fallback   │  │           │  └───────────────────┘  │
│  └───────────────────┘  │           └─────────────────────────┘
└─────────────────────────┘
```

### Key Components

1. **SoundService** (frontend) — Central service that:
   - Detects environment (Tauri vs browser)
   - Loads user preferences from settings store
   - Exposes `playNotification(event: SoundEvent)` function
   - Handles per-channel mute state
   - Implements cooldown throttling

2. **Sound assets** — WAV files in `client/public/sounds/`
   - Single source of truth for both platforms
   - Tauri accesses via webview or embedded at build

3. **Tauri command** — `play_sound(sound_id: String)` in Rust for native playback

4. **Settings store** — Persists user preferences in localStorage (MVP)

## Data Flow

### Message Notification Flow

```
1. WebSocket receives message (with server-provided mention_type field)
     │
2. Quick exits:
     ├─► Own message? → skip
     ├─► Conversation muted? → skip (works for channels AND DMs)
     │
3. Eligibility check:
     ├─► Is DM? → eligible
     ├─► mention_type != null? → eligible
     ├─► Channel notification setting = "all"? → eligible
     └─► else → skip
     │
4. Playback conditions:
     ├─► Viewing source channel at bottom AND app focused? → skip
     ├─► Cooldown active (2s)? → skip
     ├─► (Web) Not leader tab? → skip
     │
5. Play sound:
     ├─► Tauri detected? → invoke('play_sound', { id })
     └─► Web → try Web Audio, fallback to Notification API, fallback visual-only
```

### Tab Leadership (Web Multi-Tab)

Uses `BroadcastChannel` API with `localStorage` fallback for Safari <15.4:
1. On page load, tabs elect a "leader" via timestamp race
2. Leader tab handles all sound playback
3. If leader closes/hangs (no heartbeat for 5s), remaining tabs re-elect
4. Prevents duplicate sounds across multiple browser tabs

## Data Model

### Sound Settings (localStorage)

```typescript
// Key: "canis:sound:settings"
interface SoundSettings {
  enabled: boolean;              // Master on/off (default: true)
  volume: number;                // 0-100 (default: 80)
  selectedSound: SoundOption;    // Which sound (default: "default")
}

type SoundOption = "default" | "subtle" | "ping" | "chime" | "bell";
```

### Channel Notification Settings (localStorage)

```typescript
// Key: "canis:sound:channels"
interface ChannelNotificationSettings {
  [channelId: string]: NotificationLevel;
}

type NotificationLevel = "all" | "mentions" | "none";

// Defaults:
// - DMs: "all"
// - Channels: "mentions"
```

### Server Message Payload Addition

```typescript
interface Message {
  // ... existing fields
  mention_type: "direct" | "everyone" | "here" | null;
}
```

**Fallback:** If server doesn't provide `mention_type`, client parses message content for @username, @everyone, @here.

## UI Design

### Settings → Notifications Tab

New tab in SettingsModal with radio card pattern (matches AppearanceSettings):

```
┌─────────────────────────────────────────────────────────────┐
│ Sound Notifications                                          │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│ ☑ Enable notification sounds                                │
│                                                              │
│ ─────────────────────────────────────────────────────────── │
│                                                              │
│ Notification Sound                                           │
│                                                              │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ ● Default                                               │ │
│ │   Clean, neutral notification chime                     │ │
│ └─────────────────────────────────────────────────────────┘ │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ ○ Subtle                                                │ │
│ │   Soft, minimal tone                                    │ │
│ └─────────────────────────────────────────────────────────┘ │
│                         ... more options ...                 │
│                                                              │
│ ─────────────────────────────────────────────────────────── │
│                                                              │
│ Volume                                                       │
│ ◀ ───────────────●─────────── ▶  80%      [ ▶ Test ]       │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### Channel Settings Modal — Notifications Section

Add to existing ChannelSettingsModal:

```
┌─────────────────────────────────────────────────────────────┐
│ Notifications                                                │
│                                                              │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ ○ All messages                                          │ │
│ └─────────────────────────────────────────────────────────┘ │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ ● Mentions only (default)                               │ │
│ └─────────────────────────────────────────────────────────┘ │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ ○ None (muted)                                          │ │
│ └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### Muted Channel Indicator

ChannelItem shows mute icon when channel notification level is "none".

## File Structure

### New Files

```
client/
├── public/
│   └── sounds/
│       ├── default.wav
│       ├── subtle.wav
│       ├── ping.wav
│       ├── chime.wav
│       └── bell.wav
│
├── src/
│   ├── components/
│   │   └── settings/
│   │       └── NotificationSettings.tsx
│   │
│   ├── lib/
│   │   └── sound/
│   │       ├── index.ts         # Platform detection + exports
│   │       ├── types.ts         # SoundEvent, SoundOption types
│   │       ├── browser.ts       # Web Audio API + Notification API
│   │       ├── tauri.ts         # Tauri command wrapper
│   │       └── tab-leader.ts    # BroadcastChannel coordination
│   │
│   └── stores/
│       └── sound.ts             # Sound settings + channel levels

client/src-tauri/
└── src/
    └── commands/
        └── sound.rs             # Native audio playback (rodio)
```

### Files to Modify

```
client/src/stores/settings.ts → Rename to connection.ts
client/src/components/settings/SettingsModal.tsx → Add notifications tab
client/src/components/channels/ChannelSettingsModal.tsx → Add notification section
client/src/components/channels/ChannelItem.tsx → Add muted indicator
client/src/stores/websocket.ts → Integrate SoundService
client/src-tauri/src/commands/mod.rs → Export sound module
client/src-tauri/src/lib.rs → Register play_sound command
server/src/chat/messages.rs → Add mention_type to response
```

## Implementation Sequence

### Phase 1: Foundation

1. Rename `stores/settings.ts` → `stores/connection.ts` (update imports)
2. Create `stores/sound.ts` — settings interface, localStorage persistence
3. Create `lib/sound/types.ts` — type definitions
4. Create `lib/sound/browser.ts` — Web Audio API wrapper with preloading
5. Create `lib/sound/tab-leader.ts` — BroadcastChannel + localStorage fallback
6. Create `lib/sound/index.ts` — SoundService with platform detection, cooldown

### Phase 2: Tauri Integration

7. Create `commands/sound.rs` — rodio-based native playback
8. Create `lib/sound/tauri.ts` — invoke wrapper
9. Update `lib.rs` and `commands/mod.rs` — register command

### Phase 3: Assets

10. Source 5 royalty-free WAV sounds with documented licenses
11. Add to `client/public/sounds/`
12. Update LICENSE_COMPLIANCE.md with attributions

### Phase 4: WebSocket Integration

13. Update server message payload — add `mention_type` field
14. Update `stores/websocket.ts` — eligibility check + sound trigger

### Phase 5: UI

15. Create `NotificationSettings.tsx` — settings panel
16. Update `SettingsModal.tsx` — add Notifications tab
17. Update `ChannelSettingsModal.tsx` — add notification level section
18. Update `ChannelItem.tsx` — muted indicator icon

### Phase 6: Polish

19. Add unit tests for eligibility logic, cooldown, tab leadership
20. Manual testing on web and Tauri
21. Update CHANGELOG.md

## Error Handling

### Audio Playback Failures

```
Audio fails to play (file missing, device unavailable):
  → Log warning via console.warn
  → Do NOT show error to user (non-critical feature)
  → Continue normally

AudioContext suspended (web, no user interaction yet):
  → Store pending sound event
  → Play on next user interaction (click/keypress)
  → Skip if >5 seconds have passed

BroadcastChannel unavailable:
  → Fall back to localStorage coordination
  → If that fails, accept duplicate sounds (graceful degradation)
```

### Preloading Strategy

On app init, after user login:
```typescript
await SoundService.preloadSounds();
// Preload all 5 WAV files into AudioBuffers
// Log warning if any fail, don't block app startup
```

## Known Limitations (MVP)

1. **No cross-client read sync** — Sounds may play on multiple devices for same message
2. **Settings don't sync across devices** — localStorage only
3. **Background tabs (web) may not play reliably** — Browser throttling; Tauri handles this better
4. **No DND mode** — Must mute channels individually

## Compliance Checklist

Before implementation:
- [ ] Verify `rodio` crate license compatibility: `cargo deny check licenses`
- [ ] Source 5 royalty-free sounds with documented licenses
- [ ] Add attributions to LICENSE_COMPLIANCE.md
- [ ] Update THIRD_PARTY_NOTICES.md if required

## Roadmap (Post-MVP)

| Feature | Priority | Notes |
|---------|----------|-------|
| **Cross-client read sync** | Required | Clear notifications across devices when read on one |
| **Server-synced settings** | Required | Preferences persist across devices |
| **Do Not Disturb mode** | High | App-level + OS-level integration |
| **Original branded sounds** | High | Replace placeholders with Canis identity |
| **Voice channel sounds** | Medium | Join/leave, mute/unmute, PTT beeps |
| **Custom sound uploads** | Medium | Desktop-exclusive feature |
| **UI feedback sounds** | Low | Button clicks, navigation |
| **Sound packs/themes** | Low | Bundled sets to switch between |

## Testing Strategy

### Unit Tests

- Eligibility logic (DM detection, mention detection, mute check, focus state)
- Cooldown throttling (rapid notifications honored correctly)
- Tab leadership election and failover
- Settings persistence (save/load roundtrip)

### Integration Tests

- WebSocket message → sound trigger flow
- Tauri command invocation

### Manual Testing

- Web: Multiple tabs, background tab behavior
- Tauri: Native playback, background app
- Settings UI: All controls work, persist correctly

## Key Decisions Summary

| Decision | Choice | Rationale |
|----------|--------|-----------|
| MVP scope | Chat notifications only | YAGNI, deliver value fast |
| Sound format | WAV | No decoding latency, universal support |
| Asset location | `client/public/sounds/` | Single source of truth |
| Settings storage | localStorage (MVP) | Simple; server sync later |
| Multi-tab coordination | BroadcastChannel + fallback | Prevents duplicate sounds |
| Cooldown | 2 seconds | Prevents audio spam |
| Per-channel UI | Existing settings modal | No new context menu infrastructure |
| Mention detection | Server-provided + client fallback | Server is source of truth |
