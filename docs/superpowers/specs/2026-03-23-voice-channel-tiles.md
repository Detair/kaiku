# Voice Channel View — Tile-Based Overhaul

**Date:** 2026-03-23
**Status:** Approved
**Branch:** `feature/voice-channel-tiles`

## Problem

The current voice channel view (`VoiceChannelView.tsx`) shows a simple participant list with small avatars and text. There is no video tile grid, no inline screen share viewing, and no way to see webcam feeds alongside other participants. The existing `ScreenShareViewer.tsx` is a separate overlay/portal that disconnects from the voice channel context.

## Solution

Replace the voice channel view with a tile-based layout inspired by Discord's voice UI. Two modes that switch automatically based on context:

1. **Grid mode** — equal-sized tiles filling available space (default)
2. **Focus mode** — one large focused tile + smaller tiles in a strip (activates on screen share or user click)

## Layout Modes

### Grid Mode (default)

Active when no screen share is running and no tile is manually focused.

- All tiles (participants + screen shares) are displayed in an equal grid
- **Square-fit algorithm** calculates optimal rows/columns to fill available space with minimal waste — similar to Zoom's gallery view
- Each tile is clickable — clicking focuses it and switches to focus mode
- Tiles maintain a minimum size; if too many participants, allow scrolling

### Focus Mode

Activates automatically when a screen share starts, or manually when a user clicks any tile.

- One large tile takes the majority of space (the focused content)
- Remaining tiles are displayed in a strip
- **≤6 tiles in strip:** Right sidebar, vertical stack
- **>6 tiles in strip:** Bottom strip, horizontal row with overflow scroll
- Click any strip tile to switch focus to it
- Click the focused tile again (or press Escape) to return to grid mode
- When a screen share starts, it automatically gets focus
- When the focused screen share stops, return to grid mode (or focus next screen share if one exists)

## Tile Types

### Participant Tile

- **Without webcam:** Shows user avatar (initials on colored circle), display name below, mute/deafen badge
- **With webcam:** Video stream fills the tile, name overlaid at bottom with gradient background
- **Speaking indicator:** Green border with subtle glow (`box-shadow`)
- **Muted indicator:** Small badge in bottom-right corner

### Screen Share Tile

- Shows live video stream from the screen share
- Label at bottom: "{username}'s Screen" with translucent background
- Appears as an additional tile alongside the user's participant tile (one person can have both a participant tile and a screen share tile)
- **Pop-out button:** Small icon button (top-right corner, visible on hover) opens the stream in a separate browser window. See "Screen Share Pop-Out" section below.

## Tile Sizing

### Grid Mode — Square-Fit Algorithm

Given `n` tiles and container dimensions `W x H`:

Use a uniform 4:3 aspect ratio for all tiles in grid mode (screen share tiles show a letterboxed preview — they get 16:9 only when focused).

```
If n = 0: show empty-state placeholder ("Waiting for others...")
AR = 4/3  // uniform aspect ratio

For cols = 1 to min(n, 5):
  rows = ceil(n / cols)
  tileW = W / cols
  tileH = H / rows
  // Constrain to aspect ratio
  actualW = min(tileW, tileH * AR)
  actualH = actualW / AR
  coverage = (actualW * actualH * n) / (W * H)

Pick cols that maximizes coverage
```

Minimum tile size: 120px wide. Maximum columns: 5.

### Focus Mode — Strip Sizing

- **Right sidebar (≤6):** Strip width ~160px, tiles stack with equal height
- **Bottom strip (>6):** Strip height ~80px, tiles are ~80px wide, horizontal scroll

## Component Architecture

### New Components

- `VoiceTileGrid.tsx` — grid/focus layout manager, handles mode switching
- `VoiceTile.tsx` — single tile (participant or screen share), handles click, speaking indicator, video attachment
- `VoiceTileStrip.tsx` — the sidebar/bottom strip of unfocused tiles in focus mode
- `ScreenSharePopOut.ts` — utility for managing pop-out windows (window.open, stream transfer, cleanup)

### Modified Components

- `VoiceChannelView.tsx` — replace current participant list with `VoiceTileGrid`
- `VoiceControls.tsx` — no changes needed (stays at bottom). Note: `VoiceControls` already renders its own `border-t border-white/10` — do NOT add a wrapping div with another `border-t` (the current `VoiceChannelView` has this double-border bug)

### Removed/Replaced

- The current inline participant rendering in `VoiceChannelView.tsx` is replaced by the tile grid
- `ScreenShareViewer.tsx` portal overlay is no longer needed for the voice channel view (may still be used for the message view path via `VoicePanel`)

## State Management

### New Signals (in voice store or local component state)

- `focusedTileId: string | null` — ID of the currently focused tile. Format: `"{userId}"` for participant tiles, `"screen:{stream_id}"` for screen share tiles (where `stream_id` is `ScreenShareInfo.stream_id`, a UUID)
- `viewMode: "grid" | "focus"` — derived from `focusedTileId` and active screen shares
- `poppedOutStreams: Set<string>` — stream IDs of screen shares currently in pop-out windows

### Tile List Construction

Build a unified tile list from voice store state:

```typescript
const tiles = createMemo(() => {
  const result: Tile[] = [];
  // One tile per participant (webcam state checked via webcamViewer store)
  for (const p of participants()) {
    const hasWebcamTrack = webcamViewerState.availableTracks.has(p.user_id);
    result.push({ type: "participant", userId: p.user_id, hasWebcam: hasWebcamTrack, ... });
  }
  // One tile per active screen share
  for (const s of screenShares()) {
    const hasTrack = screenShareViewerState.availableTracks.has(s.stream_id);
    result.push({ type: "screen_share", streamId: s.stream_id, userId: s.user_id, hasTrack, ... });
  }
  return result;
});
```

Note: `hasWebcam` and `hasTrack` are reactive — the memo re-runs when tracks arrive in the viewer stores. Participant tiles with `hasWebcam: true` render the webcam video instead of the avatar. Screen share tiles with `hasTrack: false` show a loading placeholder.

### Auto-Focus Logic

- When `screenShares()` changes from empty to non-empty: auto-set `focusedTileId` to the new screen share — but **only for remote screen shares** (skip the local user's own screen share, matching current `VoiceChannelView` behavior that skips `startViewing` for own shares)
- When the focused screen share stops: if other screen shares exist, focus the next one; otherwise clear focus (return to grid)
- Manual focus (click) overrides auto-focus — including clicking your own screen share tile to focus it deliberately

## Video Track Attachment

Video tracks are NOT available directly from the WebRTC adapter. They are managed by separate viewer stores:

- **Screen share tracks:** `screenShareViewer.ts` → `viewerState.availableTracks` (a `Map<string, AvailableTrackInfo>`)
- **Webcam tracks:** `webcamViewer.ts` → `viewerState.availableTracks` (a `Map<string, MediaStreamTrack>`)

**Important timing gap:** `voiceState.screenShares` (server-signalled metadata) can arrive before the WebRTC track does. The tile should show a loading/placeholder state until the track is available in the viewer store. The existing `screenShareViewer.ts` already handles this with a retry loop — the tile component should check `availableTracks.has(streamId)` reactively and render the video only when the track exists.

To attach a track to a `<video>` element:
1. Read `MediaStreamTrack` from the viewer store
2. Wrap in `new MediaStream([track])` (as `ScreenShareViewer.tsx` does on line 79)
3. Set `videoElement.srcObject = stream`
4. Clean up on unmount: `videoElement.srcObject = null`

Use a `ref` on the `<video>` element and a `createEffect` to attach/detach reactively.

## Screen Share Pop-Out

Screen share tiles have a pop-out button that opens the video stream in a separate browser window.

### Behavior

- **Trigger:** Click the pop-out icon (top-right of screen share tile, visible on hover)
- **Opens:** `window.open()` with a minimal page containing just the `<video>` element, dark background, and the stream label
- **Main view tile:** Stays in the grid/strip but shows a "Popped out" placeholder with a "Bring back" button instead of the video stream
- **Close pop-out:** Closing the window (or clicking "Bring back" in the main tile) returns the stream to the inline tile
- **Multiple pop-outs:** Each screen share can be popped out independently
- **Focus mode interaction:** A popped-out screen share can still be the focused tile — the focus area shows the placeholder, and the actual video is in the pop-out window

### Implementation

- Use `window.open('', '_blank', 'width=960,height=540')` to create a minimal window
- Write a minimal HTML document into the new window's `document` with dark background styling
- Transfer the `MediaStream` to the pop-out window's `<video>` element via `videoEl.srcObject = stream`
- Track pop-out state per stream: `poppedOutStreams: Set<string>` (stream IDs)
- Listen for the pop-out window's `beforeunload` event to restore the stream to the inline tile
- The `MediaStreamTrack` stays alive — it's shared between windows, not moved

### New Component

- `ScreenSharePopOut.ts` — utility module (not a component) that manages `window.open`, stream attachment, and cleanup. Exports `popOut(streamId)` and `bringBack(streamId)`.

## Styling

Follow existing UnoCSS patterns from CLAUDE.md:

- Tile background: `bg-surface-layer2`
- Tile border: `border border-surface-highlight` (default), `border-2 border-accent-success` (speaking)
- Speaking glow: `shadow-[0_0_12px_rgba(67,181,129,0.3)]`
- Text: `text-text-primary` for names, `text-text-secondary` for status
- Mute badge: `bg-accent-danger/25 text-accent-danger`
- Rounded corners: `rounded-xl` for tiles
- Transitions: `transition-all duration-200` for mode switches

## Accessibility

- Tiles are `role="button"` with `aria-label="{name}'s tile"` and keyboard focusable
- Focus mode announced via `aria-live="polite"` region
- Escape key exits focus mode
- Tab navigation cycles through tiles

## What Does Not Change

- `VoicePanel.tsx` sidebar — keeps its current compact participant list
- `VoiceControls.tsx` — unchanged, stays at bottom
- Audio playback — hidden audio elements continue as-is
- Voice store — no structural changes, just reading existing state
- WebRTC adapter — no changes

## Testing

1. `bun run test:run` — existing frontend tests still pass
2. Manual: join voice with 1-4 users → grid mode shows tiles
3. Manual: start screen share → auto-switches to focus mode
4. Manual: click tile in strip → switches focus
5. Manual: click focused tile → returns to grid
6. Manual: 7+ participants with screen share → bottom strip layout

## Future Work

- Info-rich tile mode as a user setting (latency, quality dot, activity badges)
- Picture-in-picture for focused screen share
- Drag-and-drop tile reordering
- Pin tile (prevent auto-focus from moving it)
