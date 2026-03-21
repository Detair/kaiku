# Voice Channel Main View — Design

**Date:** 2026-03-21
**Status:** Planned

## Problem

When a voice channel is selected, the main content area shows MessageList/MessageInput (same as text channels). There is no visual representation of the voice session — participants, webcams, screen shares, or connection quality.

## Design

### What to show

When `channel().channel_type === "voice"` in Main.tsx, render a **VoiceChannelView** instead of the message list. The view has two states:

**Not connected (preview):** Show the channel name, participant list (who's already in), and a "Join Voice" button. Users can see who's in the channel before joining.

**Connected:** Show a participant grid with:
- Each participant as a card showing avatar, display name, mute/deafen/speaking indicators
- Webcam video feeds (when active) replace the avatar in the card
- Screen share thumbnails below the participant grid, clickable to open ScreenShareViewer
- Connection quality indicator per participant
- Voice controls bar at the bottom (mute, deafen, screen share, webcam, disconnect)

### Layout

```
┌──────────────────────────────────────┐
│  # voice-channel-name                │  ← header (same as text)
├──────────────────────────────────────┤
│                                      │
│   ┌──────┐  ┌──────┐  ┌──────┐     │  ← participant grid
│   │avatar│  │webcam│  │avatar│     │     (flex-wrap, centered)
│   │ name │  │ name │  │ name │     │
│   │ 🎤🔊 │  │ 🎤🔊 │  │ 🎤🔊 │     │
│   └──────┘  └──────┘  └──────┘     │
│                                      │
│   ┌─────────────┐  ┌────────────┐   │  ← screen share thumbnails
│   │  share #1   │  │  share #2  │   │     (clickable)
│   └─────────────┘  └────────────┘   │
│                                      │
├──────────────────────────────────────┤
│  🎤  🔇  📺  📷  ⚙️  ❌            │  ← controls bar
└──────────────────────────────────────┘
```

### Implementation steps

1. Create `VoiceChannelView.tsx` component in `client/src/components/voice/`
2. In Main.tsx, add a `<Match when={channel()?.channel_type === "voice"}>` that renders `VoiceChannelView` instead of MessageList/MessageInput
3. Use existing stores: `voiceState` for participants/state, `screenShareViewer` for shares
4. Reuse `VoiceControls` component for the controls bar
5. Webcam tracks: use `voiceState.webcams` to get MediaStreamTrack per participant, render in `<video>` elements
6. Screen share thumbnails: use `viewerState.availableTracks` to render small preview videos

### Existing components to reuse
- `VoiceControls.tsx` — mute/deafen/share/webcam buttons
- `QualityIndicator.tsx` — per-participant quality dot
- `VoiceParticipants.tsx` — participant list (may need refactoring for grid layout)

### Not in scope
- Text chat within voice channels stays as-is (accessible via sidebar)
- No picture-in-picture for voice when switching to a text channel
