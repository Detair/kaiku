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

## Tile Sizing

### Grid Mode — Square-Fit Algorithm

Given `n` tiles and container dimensions `W x H`:

```
For cols = 1 to n:
  rows = ceil(n / cols)
  tileW = W / cols
  tileH = H / rows
  tileSize = min(tileW, tileH)  // maintain aspect ratio
  coverage = (tileSize * tileSize * n) / (W * H)

Pick cols that maximizes coverage
```

Minimum tile size: 120px. Maximum columns: 5. Aspect ratio: roughly 4:3 for participant tiles, 16:9 for screen share tiles.

### Focus Mode — Strip Sizing

- **Right sidebar (≤6):** Strip width ~160px, tiles stack with equal height
- **Bottom strip (>6):** Strip height ~80px, tiles are ~80px wide, horizontal scroll

## Component Architecture

### New Components

- `VoiceTileGrid.tsx` — grid/focus layout manager, handles mode switching
- `VoiceTile.tsx` — single tile (participant or screen share), handles click, speaking indicator, video attachment
- `VoiceTileStrip.tsx` — the sidebar/bottom strip of unfocused tiles in focus mode

### Modified Components

- `VoiceChannelView.tsx` — replace current participant list with `VoiceTileGrid`
- `VoiceControls.tsx` — no changes needed (stays at bottom)

### Removed/Replaced

- The current inline participant rendering in `VoiceChannelView.tsx` is replaced by the tile grid
- `ScreenShareViewer.tsx` portal overlay is no longer needed for the voice channel view (may still be used for the message view path via `VoicePanel`)

## State Management

### New Signals (in voice store or local component state)

- `focusedTileId: string | null` — ID of the currently focused tile (`"{userId}"` for participant, `"screen:{streamId}"` for screen share)
- `viewMode: "grid" | "focus"` — derived from `focusedTileId` and active screen shares

### Tile List Construction

Build a unified tile list from voice store state:

```typescript
const tiles = createMemo(() => {
  const result: Tile[] = [];
  // One tile per participant
  for (const p of participants()) {
    result.push({ type: "participant", userId: p.user_id, ... });
  }
  // One tile per active screen share
  for (const s of screenShares()) {
    result.push({ type: "screen_share", streamId: s.stream_id, userId: s.user_id, ... });
  }
  return result;
});
```

### Auto-Focus Logic

- When `screenShares()` changes from empty to non-empty: auto-set `focusedTileId` to the new screen share
- When the focused screen share stops: if other screen shares exist, focus the next one; otherwise clear focus (return to grid)
- Manual focus (click) overrides auto-focus

## Video Track Attachment

Screen share and webcam video tracks arrive via the `onRemoteTrack` event from the WebRTC adapter. The tile component needs to:

1. Get the `MediaStream` for a given track from the adapter's remote streams
2. Attach it to a `<video>` element via `srcObject`
3. Clean up on unmount or when the track is removed

Use a `ref` on the `<video>` element and a `createEffect` to attach/detach the stream when the track changes.

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
