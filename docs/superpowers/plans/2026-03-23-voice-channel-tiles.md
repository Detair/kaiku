# Voice Channel Tile View — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the voice channel participant list with a tile-based grid that supports focus mode for screen shares, webcam video, and pop-out windows.

**Architecture:** Three new components (`VoiceTileGrid`, `VoiceTile`, `VoiceTileStrip`) replace the inline participant rendering in `VoiceChannelView.tsx`. A utility module (`screenSharePopOut.ts`) manages pop-out windows. State is local to `VoiceTileGrid` (no voice store changes). Video tracks come from existing `screenShareViewer` and `webcamViewer` stores.

**Tech Stack:** Solid.js, TypeScript, UnoCSS, lucide-solid icons

**Spec:** `docs/superpowers/specs/2026-03-23-voice-channel-tiles.md`

**Branch:** `feature/voice-channel-tiles`

---

### Task 1: Square-fit algorithm utility

Pure function, no UI. Easy to test in isolation.

**Files:**
- Create: `client/src/components/voice/tileLayout.ts`
- Create: `client/src/components/voice/__tests__/tileLayout.test.ts`

- [ ] **Step 1: Write tests for the square-fit algorithm**

Create `client/src/components/voice/__tests__/tileLayout.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import { calculateGrid } from "../tileLayout";

describe("calculateGrid", () => {
  it("returns 0 cols for 0 tiles", () => {
    const result = calculateGrid(0, 800, 600);
    expect(result.cols).toBe(0);
    expect(result.rows).toBe(0);
  });

  it("returns 1x1 for 1 tile", () => {
    const result = calculateGrid(1, 800, 600);
    expect(result.cols).toBe(1);
    expect(result.rows).toBe(1);
  });

  it("returns 2x1 for 2 tiles in landscape", () => {
    const result = calculateGrid(2, 800, 400);
    expect(result.cols).toBe(2);
    expect(result.rows).toBe(1);
  });

  it("returns 2x2 for 4 tiles in square-ish container", () => {
    const result = calculateGrid(4, 800, 600);
    expect(result.cols).toBe(2);
    expect(result.rows).toBe(2);
  });

  it("caps at 5 columns", () => {
    const result = calculateGrid(20, 1920, 1080);
    expect(result.cols).toBeLessThanOrEqual(5);
  });

  it("returns tile dimensions with 4:3 aspect ratio", () => {
    const result = calculateGrid(4, 800, 600);
    const ratio = result.tileWidth / result.tileHeight;
    expect(ratio).toBeCloseTo(4 / 3, 1);
  });

  it("respects minimum tile width of 120px", () => {
    const result = calculateGrid(20, 400, 300);
    expect(result.tileWidth).toBeGreaterThanOrEqual(120);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/detair/GIT/detair/kaiku/client && ~/.bun/bin/bun run test:run -- --reporter=verbose 2>&1 | grep -E "tileLayout|FAIL|PASS"`
Expected: FAIL (module not found)

- [ ] **Step 3: Implement the square-fit algorithm**

Create `client/src/components/voice/tileLayout.ts`:

```typescript
/** Result of the square-fit grid calculation. */
export interface GridLayout {
  cols: number;
  rows: number;
  tileWidth: number;
  tileHeight: number;
}

const ASPECT_RATIO = 4 / 3;
const MIN_TILE_WIDTH = 120;
const MAX_COLS = 5;

/**
 * Calculate optimal grid dimensions for n tiles in a container of W x H.
 * Uses a square-fit algorithm: tries each column count and picks the one
 * that maximizes coverage (least wasted space) with a uniform 4:3 aspect ratio.
 */
export function calculateGrid(n: number, containerWidth: number, containerHeight: number): GridLayout {
  if (n === 0) return { cols: 0, rows: 0, tileWidth: 0, tileHeight: 0 };

  let bestCols = 1;
  let bestCoverage = 0;

  for (let cols = 1; cols <= Math.min(n, MAX_COLS); cols++) {
    const rows = Math.ceil(n / cols);
    const tileW = containerWidth / cols;
    const tileH = containerHeight / rows;
    // Constrain to 4:3 aspect ratio
    const actualW = Math.min(tileW, tileH * ASPECT_RATIO);
    const actualH = actualW / ASPECT_RATIO;

    // Skip if tile would be too small
    if (actualW < MIN_TILE_WIDTH) continue;

    const coverage = (actualW * actualH * n) / (containerWidth * containerHeight);
    if (coverage > bestCoverage) {
      bestCoverage = coverage;
      bestCols = cols;
    }
  }

  const rows = Math.ceil(n / bestCols);
  const tileW = containerWidth / bestCols;
  const tileH = containerHeight / rows;
  const tileWidth = Math.min(tileW, tileH * ASPECT_RATIO);
  const tileHeight = tileWidth / ASPECT_RATIO;

  return { cols: bestCols, rows, tileWidth, tileHeight };
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /home/detair/GIT/detair/kaiku/client && ~/.bun/bin/bun run test:run -- --reporter=verbose 2>&1 | grep -E "tileLayout|FAIL|PASS"`
Expected: all PASS

- [ ] **Step 5: Commit**

```bash
git add client/src/components/voice/tileLayout.ts client/src/components/voice/__tests__/tileLayout.test.ts
git commit -m "feat(client): square-fit tile layout algorithm for voice grid

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: VoiceTile component

Single tile that renders a participant (avatar or webcam) or screen share (video). Handles speaking indicator, mute badge, click-to-focus.

**Files:**
- Create: `client/src/components/voice/VoiceTile.tsx`

- [ ] **Step 1: Create the VoiceTile component**

The component accepts a `Tile` type prop and renders accordingly:

```typescript
// Tile type definition (put at top of file or in a shared types file)
export type TileData =
  | { type: "participant"; userId: string; displayName: string; username: string; muted: boolean; deafened: boolean; speaking: boolean; }
  | { type: "screen_share"; streamId: string; userId: string; username: string; };
```

**Participant tile rendering:**
- Check `webcamViewer.getTrack(userId)` — if available, render `<video>` with stream, name overlay at bottom
- Otherwise, render avatar circle (initial letter) with name below
- Speaking: `border-2 border-accent-success shadow-[0_0_12px_rgba(67,181,129,0.3)]`
- Muted: small badge bottom-right `bg-accent-danger/25 text-accent-danger`
- Entire tile is `role="button"` with `tabindex="0"`, `aria-label="{name}'s tile"`

**Screen share tile rendering:**
- Check `screenShareViewer` store for `availableTracks.has(streamId)`:
  - If track available: render `<video>` with the stream
  - If not yet: render loading placeholder (spinner or "Connecting..." text)
- Label at bottom: "{username}'s Screen"
- Pop-out button: small icon (top-right, visible on hover) — functionality comes in Task 5

**Video attachment pattern** (for both webcam and screen share):
```typescript
let videoRef: HTMLVideoElement | undefined;
createEffect(() => {
  if (!videoRef) return;
  const track = getTrackSomehow(); // from viewer store
  if (track) {
    videoRef.srcObject = new MediaStream([track]);
  } else {
    videoRef.srcObject = null;
  }
});
onCleanup(() => { if (videoRef) videoRef.srcObject = null; });
```

**Props:**
- `tile: TileData`
- `onClick: () => void`
- `focused?: boolean` (for visual indicator in strip)
- `size?: "normal" | "small"` (normal for grid, small for strip)
- `poppedOut?: boolean` (for screen share tiles — show placeholder)
- `onPopOut?: () => void` (for screen share tiles — trigger pop-out)
- `onBringBack?: () => void` (for popped-out tiles — bring back)

Follow existing patterns: UnoCSS utility classes, `Show`/`Switch`/`Match` for conditional rendering, `classList` for dynamic classes.

Read the existing `VoiceChannelView.tsx` participant rendering (lines 79–131) for the exact avatar/name/badge pattern to match.

- [ ] **Step 2: Verify it compiles**

Run: `cd /home/detair/GIT/detair/kaiku/client && npx tsc --noEmit 2>&1 | head -20`
Expected: no errors in VoiceTile.tsx (other pre-existing errors may exist)

- [ ] **Step 3: Commit**

```bash
git add client/src/components/voice/VoiceTile.tsx
git commit -m "feat(client): VoiceTile component for participant and screen share tiles

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: VoiceTileStrip component

The sidebar (vertical) or bottom bar (horizontal) strip of unfocused tiles shown during focus mode.

**Files:**
- Create: `client/src/components/voice/VoiceTileStrip.tsx`

- [ ] **Step 1: Create the VoiceTileStrip component**

**Props:**
- `tiles: TileData[]` — tiles to show in the strip (excluding the focused one)
- `orientation: "vertical" | "horizontal"` — vertical for ≤6, horizontal for >6
- `focusedTileId: string | null` — for highlighting
- `onTileClick: (tileId: string) => void`
- `poppedOutStreams: Set<string>` — for pop-out state
- `onPopOut: (streamId: string) => void`
- `onBringBack: (streamId: string) => void`

**Vertical layout (right sidebar, ≤6 tiles):**
- `flex flex-col gap-1.5` with `w-40` (160px)
- Each tile uses `VoiceTile` with `size="small"`
- Tiles expand to fill vertical space equally: `flex-1`

**Horizontal layout (bottom strip, >6 tiles):**
- `flex gap-1.5 overflow-x-auto` with `h-20` (80px)
- Each tile is `w-20 flex-shrink-0` (80px square)
- Horizontal scroll when tiles overflow

- [ ] **Step 2: Commit**

```bash
git add client/src/components/voice/VoiceTileStrip.tsx
git commit -m "feat(client): VoiceTileStrip for sidebar/bottom unfocused tiles

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: VoiceTileGrid — main layout manager

Orchestrates grid mode and focus mode. This is the core component that replaces the participant list in `VoiceChannelView.tsx`.

**Files:**
- Create: `client/src/components/voice/VoiceTileGrid.tsx`
- Modify: `client/src/components/voice/VoiceChannelView.tsx:76-178` (replace connected state content)

- [ ] **Step 1: Create VoiceTileGrid component**

**State (local signals):**
```typescript
const [focusedTileId, setFocusedTileId] = createSignal<string | null>(null);
const [containerSize, setContainerSize] = createSignal({ width: 0, height: 0 });
const [poppedOutStreams, setPoppedOutStreams] = createSignal<Set<string>>(new Set());
```

**Tile list construction:**
```typescript
const tiles = createMemo(() => {
  const result: TileData[] = [];
  for (const p of participants()) {
    result.push({ type: "participant", userId: p.user_id, ... });
  }
  for (const s of screenShares()) {
    result.push({ type: "screen_share", streamId: s.stream_id, ... });
  }
  return result;
});
```

**View mode derivation:**
```typescript
const viewMode = () => focusedTileId() ? "focus" : "grid";
```

**Auto-focus logic:**
```typescript
createEffect(() => {
  const shares = screenShares();
  // Auto-focus first remote screen share when shares go from 0 to 1+
  if (shares.length > 0 && !focusedTileId()) {
    const remoteShare = shares.find(s => s.user_id !== authState.user?.id);
    if (remoteShare) setFocusedTileId(`screen:${remoteShare.stream_id}`);
  }
  // Clear focus if focused screen share stopped
  const focused = focusedTileId();
  if (focused?.startsWith("screen:")) {
    const streamId = focused.slice(7);
    if (!shares.some(s => s.stream_id === streamId)) {
      const nextRemote = shares.find(s => s.user_id !== authState.user?.id);
      setFocusedTileId(nextRemote ? `screen:${nextRemote.stream_id}` : null);
    }
  }
});
```

**Container size tracking:** Use `ResizeObserver` on the grid container div to track available space. Feed dimensions to `calculateGrid()`.

**Grid mode rendering:**
- Use `calculateGrid(tiles().length, width, height)` for dimensions
- CSS Grid with computed `grid-template-columns: repeat(${cols}, ${tileWidth}px)`
- Center the grid in the container
- Each tile rendered via `<VoiceTile>`

**Focus mode rendering:**
- Split into focused tile (large) + strip (remaining tiles)
- Focused tile: `flex-1` fills available space
- Strip: `<VoiceTileStrip>` with `orientation` based on tile count (≤6 → vertical, >6 → horizontal)
- Layout: `flex` (row for sidebar, column for bottom strip)

**Click handling:**
- Tile click: `setFocusedTileId(tileId)` — if already focused, `setFocusedTileId(null)`
- Escape key: `setFocusedTileId(null)`

**Keyboard:** Add `onKeyDown` handler on the container for Escape.

**Cleanup:** `onCleanup(() => closeAll())` from `screenSharePopOut.ts` to close pop-outs on disconnect.

- [ ] **Step 2: Integrate into VoiceChannelView**

In `VoiceChannelView.tsx`, replace lines 76–183 (the entire connected-state content) with:

```tsx
<Show when={isConnected()}>
  <div class="flex-1 min-h-0">
    <VoiceTileGrid
      participants={participants()}
      screenShares={screenShares()}
    />
  </div>
  <VoiceControls />
</Show>
```

Remove the wrapping `div` with `border-t border-white/10` around `VoiceControls` — it already has its own border.

Remove unused imports: `For`, `MonitorPlay`, `MonitorOff`, `getParticipantMetrics`, `startViewing`, `stopScreenShare`, `QualityIndicator`.

- [ ] **Step 3: Run frontend tests**

Run: `cd /home/detair/GIT/detair/kaiku/client && ~/.bun/bin/bun run test:run`
Expected: all existing tests pass

- [ ] **Step 4: Commit**

```bash
git add client/src/components/voice/VoiceTileGrid.tsx client/src/components/voice/VoiceChannelView.tsx
git commit -m "feat(client): tile-based voice channel view with grid and focus modes

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Screen share pop-out

Utility module that opens a screen share stream in a separate browser window.

**Files:**
- Create: `client/src/components/voice/screenSharePopOut.ts`

- [ ] **Step 1: Create the pop-out utility**

Create `client/src/components/voice/screenSharePopOut.ts`:

The utility manages pop-out windows using `window.open`. It uses DOM manipulation (createElement/appendChild) to build the pop-out page — NOT `document.write` (XSS risk).

```typescript
/** Manages pop-out windows for screen share streams. */

const popOutWindows = new Map<string, Window>();

/** Open a screen share in a new browser window. */
export function popOut(
  streamId: string,
  track: MediaStreamTrack,
  label: string,
  onClose: () => void,
): void {
  // Close existing if any
  bringBack(streamId);

  const win = window.open("", "_blank", "width=960,height=540,menubar=no,toolbar=no");
  if (!win) {
    console.warn("[PopOut] Popup blocked by browser");
    return;
  }

  popOutWindows.set(streamId, win);

  // Build page via DOM manipulation (not document.write)
  const doc = win.document;
  doc.title = label;

  const style = doc.createElement("style");
  style.textContent = `
    * { margin: 0; padding: 0; box-sizing: border-box; }
    body { background: #0d0d1a; display: flex; align-items: center; justify-content: center; height: 100vh; overflow: hidden; }
    video { max-width: 100%; max-height: 100%; object-fit: contain; }
    .label { position: fixed; bottom: 12px; left: 12px; color: #ccc; font-family: system-ui; font-size: 13px; background: rgba(0,0,0,0.6); padding: 4px 12px; border-radius: 6px; }
  `;
  doc.head.appendChild(style);

  const video = doc.createElement("video");
  video.autoplay = true;
  video.playsInline = true;
  video.srcObject = new MediaStream([track]);
  doc.body.appendChild(video);

  const labelEl = doc.createElement("div");
  labelEl.className = "label";
  labelEl.textContent = label;
  doc.body.appendChild(labelEl);

  // Handle window close
  win.addEventListener("beforeunload", () => {
    popOutWindows.delete(streamId);
    onClose();
  });
}

/** Bring a popped-out stream back to inline. */
export function bringBack(streamId: string): void {
  const win = popOutWindows.get(streamId);
  if (win && !win.closed) {
    win.close();
  }
  popOutWindows.delete(streamId);
}

/** Check if a stream is currently popped out. */
export function isPoppedOut(streamId: string): boolean {
  const win = popOutWindows.get(streamId);
  return !!win && !win.closed;
}

/** Close all pop-out windows (e.g., on disconnect). */
export function closeAll(): void {
  for (const [, win] of popOutWindows) {
    if (!win.closed) win.close();
  }
  popOutWindows.clear();
}
```

The pop-out button in VoiceTile (Task 2) and the `poppedOutStreams` state in VoiceTileGrid (Task 4) are already wired up via the props defined in those tasks. This task just provides the utility implementation.

- [ ] **Step 2: Run tests**

Run: `cd /home/detair/GIT/detair/kaiku/client && ~/.bun/bin/bun run test:run`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add client/src/components/voice/screenSharePopOut.ts
git commit -m "feat(client): screen share pop-out to separate browser window

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: CHANGELOG + full verification

- [ ] **Step 1: Update CHANGELOG.md**

Add under `[Unreleased]` → `### Changed`:
```markdown
- Voice channel view uses tile-based grid with focus mode for screen shares and webcams
- Screen shares can be popped out to separate browser windows
```

- [ ] **Step 2: Run full test suite**

Run: `cd /home/detair/GIT/detair/kaiku/client && ~/.bun/bin/bun run test:run`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: changelog for voice channel tile view

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```
