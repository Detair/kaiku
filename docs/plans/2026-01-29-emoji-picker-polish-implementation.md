# Emoji Picker Polish — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix EmojiPicker positioning bugs (clipping at viewport edges, cut off by scrollable containers) by introducing `@floating-ui/dom` for smart positioning, rendering via Portal, and adapting max-height to available viewport space.

**Architecture:** New `FloatingEmojiPicker` wrapper component handles positioning via `@floating-ui/dom` + Solid.js Portal. The existing `EmojiPicker` component stays unchanged (pure content). All usage sites switch to the floating wrapper.

**Tech Stack:** `@floating-ui/dom` (MIT license, ~3KB), Solid.js Portal, existing EmojiPicker component.

---

## Context

### Existing Infrastructure (DO NOT recreate)

| Component | Location | What it does |
|-----------|----------|--------------|
| `EmojiPicker` | `client/src/components/emoji/EmojiPicker.tsx` | Self-contained picker with search, recents, guild emojis, 6 categories |
| `ReactionBar` | `client/src/components/messages/ReactionBar.tsx` | Shows reactions + "add reaction" button that opens EmojiPicker |
| `MessageItem` | `client/src/components/messages/MessageItem.tsx:316-335` | Hover "add reaction" button (no existing reactions) opens EmojiPicker |
| `ContextMenu` | `client/src/components/ui/ContextMenu.tsx` | Uses Portal + manual viewport flipping (pattern reference) |
| `Toast` | `client/src/components/ui/Toast.tsx` | Uses Portal (pattern reference) |

### What's Broken

1. **Viewport clipping** — EmojiPicker uses static `absolute bottom-full left-0` in both MessageItem.tsx:327 and ReactionBar.tsx:63. For messages near the top of the viewport, the picker renders above the viewport and is invisible.
2. **Container overflow clipping** — The message list has `overflow-y-auto`. Absolutely positioned children inside a scrollable container can be clipped by the container's bounds. No Portal is used.
3. **Fixed max-height** — `max-h-96` (384px) is applied regardless of available viewport space. If only 200px is available in the chosen direction, the picker still tries to be 384px and clips.
4. **No click-outside handling** — EmojiPicker has no built-in click-outside dismiss. Each usage site handles closing differently (MessageItem uses a signal toggle, ReactionBar uses a signal toggle). Neither has a proper click-outside listener.
5. **Category truncation** — Each emoji category is hard-capped at 32 emojis via `.slice(0, 32)` with no "show more" or scroll behavior within categories.

### What's NOT Broken (leave alone)

- Background opacity — `bg-surface-layer2` is opaque (the roadmap mentioned transparency but research confirmed backgrounds are solid)
- Search functionality works correctly
- Guild emoji integration works correctly
- Recent emojis work correctly

---

## Files to Modify

### Client
| File | Changes |
|------|---------|
| `client/package.json` | Add `@floating-ui/dom` dependency |
| `client/src/components/emoji/FloatingEmojiPicker.tsx` | **NEW** — Wrapper that handles positioning + Portal + click-outside |
| `client/src/components/emoji/EmojiPicker.tsx` | Remove `.slice(0, 32)` cap, minor style tweaks |
| `client/src/components/messages/MessageItem.tsx` | Replace inline EmojiPicker with FloatingEmojiPicker |
| `client/src/components/messages/ReactionBar.tsx` | Replace inline EmojiPicker with FloatingEmojiPicker |

---

## Implementation Tasks

### Task 1: Add @floating-ui/dom Dependency

**Files:**
- Modify: `client/package.json`

**Step 1: Install the package**

```bash
cd client && bun add @floating-ui/dom
```

`@floating-ui/dom` is MIT licensed (compatible with project constraints per CLAUDE.md).

**Step 2: Verify installation**

```bash
cd client && bun run check
```

**Note:** `@floating-ui/dom` is framework-agnostic (pure DOM). There is no Solid.js-specific wrapper needed — we use `computePosition()` directly in an effect.

---

### Task 2: Create FloatingEmojiPicker Component

**Files:**
- Create: `client/src/components/emoji/FloatingEmojiPicker.tsx`

**Purpose:** A wrapper component that:
1. Renders EmojiPicker inside a Portal (escapes scrollable containers)
2. Uses `@floating-ui/dom` for viewport-aware positioning
3. Handles click-outside to close
4. Adapts max-height to available space

```tsx
/**
 * FloatingEmojiPicker — Renders EmojiPicker in a Portal with smart positioning.
 * Uses @floating-ui/dom to avoid viewport clipping and adapt to available space.
 */
import { Component, Show, onMount, onCleanup, createSignal } from "solid-js";
import { Portal } from "solid-js/web";
import { computePosition, flip, shift, offset, size } from "@floating-ui/dom";
import EmojiPicker from "./EmojiPicker";

interface FloatingEmojiPickerProps {
  /** The trigger element to anchor the picker to */
  anchorRef: HTMLElement;
  /** Called when an emoji is selected */
  onSelect: (emoji: string) => void;
  /** Called when the picker should close (click outside, Escape, selection) */
  onClose: () => void;
  /** Guild ID for custom emoji support */
  guildId?: string;
}

const FloatingEmojiPicker: Component<FloatingEmojiPickerProps> = (props) => {
  let pickerRef: HTMLDivElement | undefined;
  const [position, setPosition] = createSignal({ x: 0, y: 0 });
  const [maxHeight, setMaxHeight] = createSignal(384); // default max-h-96
  const [ready, setReady] = createSignal(false);

  const updatePosition = async () => {
    if (!pickerRef || !props.anchorRef) return;

    const result = await computePosition(props.anchorRef, pickerRef, {
      placement: "top-start",
      middleware: [
        offset(8), // 8px gap between anchor and picker
        flip({
          fallbackPlacements: ["bottom-start", "top-end", "bottom-end"],
          padding: 8,
        }),
        shift({ padding: 8 }),
        size({
          padding: 8,
          apply({ availableHeight }) {
            // Adapt max-height to available space (min 200px, max 384px)
            setMaxHeight(Math.max(200, Math.min(384, availableHeight)));
          },
        }),
      ],
    });

    setPosition({ x: result.x, y: result.y });
    setReady(true);
  };

  // Click outside handler
  const handleClickOutside = (e: MouseEvent) => {
    if (
      pickerRef && !pickerRef.contains(e.target as Node) &&
      !props.anchorRef.contains(e.target as Node)
    ) {
      props.onClose();
    }
  };

  // Escape key handler
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      props.onClose();
    }
  };

  // Scroll handler — reposition on scroll
  const handleScroll = () => {
    updatePosition();
  };

  onMount(() => {
    updatePosition();
    // Use capture to catch clicks before they bubble
    document.addEventListener("mousedown", handleClickOutside, true);
    document.addEventListener("keydown", handleKeyDown);
    // Reposition on scroll (message list scrolling)
    window.addEventListener("scroll", handleScroll, true);
    window.addEventListener("resize", updatePosition);
  });

  onCleanup(() => {
    document.removeEventListener("mousedown", handleClickOutside, true);
    document.removeEventListener("keydown", handleKeyDown);
    window.removeEventListener("scroll", handleScroll, true);
    window.removeEventListener("resize", updatePosition);
  });

  return (
    <Portal>
      <div
        ref={pickerRef}
        class="fixed z-[9990]"
        style={{
          left: `${position().x}px`,
          top: `${position().y}px`,
          // Hide until position is calculated to prevent flash at 0,0
          visibility: ready() ? "visible" : "hidden",
        }}
      >
        <div style={{ "max-height": `${maxHeight()}px` }}>
          <EmojiPicker
            onSelect={props.onSelect}
            onClose={props.onClose}
            guildId={props.guildId}
          />
        </div>
      </div>
    </Portal>
  );
};

export default FloatingEmojiPicker;
```

**Design decisions:**

- **`z-[9990]`** — Below ContextMenu (`z-[9999]`) but above everything else. Context menus should overlay emoji pickers if both are somehow open.
- **Portal** — Escapes all parent overflow containers. Same pattern as ContextMenu and Toast.
- **`placement: "top-start"`** — Default opens upward (above the reaction button), like Discord. `flip` middleware falls back to bottom if no room above.
- **`shift`** — Slides horizontally to stay within viewport bounds.
- **`size` middleware** — Dynamically reduces `maxHeight` when there isn't enough vertical space.
- **`visibility: hidden` until ready** — Prevents a flash at `(0,0)` before `computePosition` resolves.
- **Click outside uses `mousedown`** — Catches clicks before they trigger other handlers (same reason ContextMenu uses `click` with capture).
- **Scroll repositioning** — Re-runs `computePosition` when the message list scrolls, keeping the picker anchored.

**Verification:**
```bash
cd client && bun run check
```

---

### Task 3: Update EmojiPicker Internal Styles

**Files:**
- Modify: `client/src/components/emoji/EmojiPicker.tsx`

**Purpose:** Two fixes:
1. Remove the hard `.slice(0, 32)` category cap (show all emojis, the scrollable container handles overflow)
2. Move `max-h-96` from the root `<div>` to be controlled by the FloatingEmojiPicker wrapper (via the `maxHeight` style). The root div should use `max-h-full` instead so it respects the parent's constraint.

**Step 1: Update root div class**

Change line 33:

```tsx
// Before:
<div class="bg-surface-layer2 rounded-lg shadow-xl w-80 max-h-96 overflow-hidden flex flex-col border border-white/10">

// After:
<div class="bg-surface-layer2 rounded-lg shadow-xl w-80 max-h-full overflow-hidden flex flex-col border border-white/10">
```

**Rationale:** The parent (`FloatingEmojiPicker` wrapper div) now controls max-height dynamically. `max-h-full` means the picker fills whatever height the parent allows.

**Step 2: Remove the .slice(0, 32) cap**

Change line 112:

```tsx
// Before:
<For each={category.emojis.slice(0, 32)}>

// After:
<For each={category.emojis}>
```

**Rationale:** The picker is already scrollable (`overflow-y-auto` on the grid container). Capping at 32 emojis per category hides most emojis. Users should be able to scroll through the full set.

**Verification:**
```bash
cd client && bun run check
```

---

### Task 4: Update MessageItem.tsx Usage

**Files:**
- Modify: `client/src/components/messages/MessageItem.tsx`

**Purpose:** Replace the inline absolute-positioned EmojiPicker with FloatingEmojiPicker.

**Step 1: Update import**

```typescript
// Remove (if only used for emoji picker positioning):
// No import change needed for EmojiPicker (FloatingEmojiPicker imports it internally)

// Add:
import FloatingEmojiPicker from "@/components/emoji/FloatingEmojiPicker";
```

Remove the direct `EmojiPicker` import since it's no longer used directly:

```typescript
// Remove this line:
import EmojiPicker from "@/components/emoji/EmojiPicker";
```

**Step 2: Add ref for the trigger button**

The "add reaction" button (lines 318-324) needs a ref so FloatingEmojiPicker can anchor to it:

```tsx
let reactionBtnRef: HTMLButtonElement | undefined;
```

Add `ref={reactionBtnRef}` to the button element at line 318:

```tsx
<button
  ref={reactionBtnRef}
  class="w-6 h-6 flex items-center justify-center rounded hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors"
  onClick={() => setShowReactionPicker(!showReactionPicker())}
  title="Add reaction"
>
  <SmilePlus class="w-4 h-4" />
</button>
```

**Step 3: Replace the EmojiPicker rendering**

Replace lines 326-334:

```tsx
// Before:
<Show when={showReactionPicker()}>
  <div class="absolute bottom-full left-0 mb-2 z-50">
    <EmojiPicker
      onSelect={handleAddReaction}
      onClose={() => setShowReactionPicker(false)}
      guildId={props.guildId}
    />
  </div>
</Show>

// After:
<Show when={showReactionPicker() && reactionBtnRef}>
  <FloatingEmojiPicker
    anchorRef={reactionBtnRef!}
    onSelect={handleAddReaction}
    onClose={() => setShowReactionPicker(false)}
    guildId={props.guildId}
  />
</Show>
```

**Note:** The `<Show>` no longer needs a wrapping `<div>` with absolute positioning — FloatingEmojiPicker handles all positioning internally via Portal.

**Verification:**
```bash
cd client && bun run check
```

---

### Task 5: Update ReactionBar.tsx Usage

**Files:**
- Modify: `client/src/components/messages/ReactionBar.tsx`

**Purpose:** Same change as MessageItem — switch to FloatingEmojiPicker.

**Step 1: Update import**

```typescript
// Before:
import EmojiPicker from "@/components/emoji/EmojiPicker";

// After:
import FloatingEmojiPicker from "@/components/emoji/FloatingEmojiPicker";
```

**Step 2: Add ref for the trigger button**

Add inside the component:

```typescript
let addReactionBtnRef: HTMLButtonElement | undefined;
```

Add `ref={addReactionBtnRef}` to the button at line 49:

```tsx
<button
  ref={addReactionBtnRef}
  class="w-6 h-6 flex items-center justify-center rounded hover:bg-white/10 text-text-secondary hover:text-text-primary transition-colors"
  onClick={() => setShowPicker(!showPicker())}
  title="Add reaction"
>
```

**Step 3: Replace the EmojiPicker rendering**

Replace lines 62-70:

```tsx
// Before:
<Show when={showPicker()}>
  <div class="absolute bottom-full left-0 mb-2 z-50">
    <EmojiPicker
      onSelect={handleAddReaction}
      onClose={() => setShowPicker(false)}
      guildId={props.guildId}
    />
  </div>
</Show>

// After:
<Show when={showPicker() && addReactionBtnRef}>
  <FloatingEmojiPicker
    anchorRef={addReactionBtnRef!}
    onSelect={handleAddReaction}
    onClose={() => setShowPicker(false)}
    guildId={props.guildId}
  />
</Show>
```

**Step 4: Simplify the wrapper div**

The `<div class="relative">` wrapper around the button (line 48) was only needed for absolute positioning of the old picker. It can be simplified but keeping it doesn't cause harm — leave it for now to minimize diff.

**Verification:**
```bash
cd client && bun run check
```

---

### Task 6: CHANGELOG Update

**Files:**
- Modify: `CHANGELOG.md`

Add under `### Fixed` in the `[Unreleased]` section:

```markdown
- Emoji picker positioning fixed with smart viewport-aware placement
  - Picker no longer clips at viewport edges (uses `@floating-ui/dom` for auto-flip and shift)
  - Picker renders via Portal, preventing clipping by scrollable message list containers
  - Adaptive max-height reduces picker size when viewport space is limited
  - Click-outside and Escape key dismiss the picker consistently
  - Full emoji category display (removed 32-emoji-per-category cap)
```

**Verification:**
```bash
cd client && bun run check
```

---

## Verification

### Client
```bash
cd client && bun run check
```

### Manual Testing

**Viewport clipping (top):**
1. Send several messages to push content down
2. On the FIRST message (near viewport top), click the reaction button
3. Verify picker opens BELOW the button (flipped) instead of clipping above viewport

**Viewport clipping (bottom):**
1. Scroll to the bottom message
2. Click the reaction button on the last visible message
3. Verify picker opens ABOVE the button (default placement) or flips if needed

**Viewport clipping (right edge):**
1. If the message/reaction button is near the right edge, verify the picker shifts left to stay within viewport

**Scrollable container:**
1. Open emoji picker on any message
2. Scroll the message list — picker should reposition to stay anchored to the button
3. Verify picker is NOT clipped by the message list's overflow bounds

**Adaptive height:**
1. Resize browser window to be very short (~400px tall)
2. Open emoji picker — verify it reduces its height to fit available space
3. Verify scrolling still works inside the reduced-height picker

**Click outside:**
1. Open emoji picker
2. Click anywhere outside the picker AND outside the trigger button
3. Verify picker closes

**Escape key:**
1. Open emoji picker
2. Press Escape
3. Verify picker closes

**Full emoji categories:**
1. Open emoji picker
2. Scroll through categories
3. Verify more than 32 emojis per category are visible

**ReactionBar picker:**
1. Add a reaction to a message (so ReactionBar is visible)
2. Click the "+" button on the ReactionBar
3. Verify picker opens with proper positioning (same behavior as above tests)
