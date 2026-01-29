# Emoji Picker Polish — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix emoji picker UX regressions (transparent background, viewport clipping) and implement smart positioning that adapts to available screen space.

**Architecture:** Migrate from fixed CSS positioning to Portal-based rendering with `@floating-ui/dom` for dynamic positioning. Picker detects viewport boundaries and flips position (top/bottom) automatically.

**Tech Stack:** Solid.js Portal, `@floating-ui/dom`, existing EmojiPicker component (refactor positioning only).

---

## Context

### Current Issues

1. **Fixed Upward Positioning** — Always opens upward (`bottom-full`) regardless of available space
2. **Viewport Clipping** — Picker cut off when message is near top of viewport
3. **No Dynamic Repositioning** — Cannot flip to bottom when no room at top
4. **Parent Container Clipping** — Can be cut off by `overflow:hidden` containers
5. **Possible Transparency** — Background may not render properly in certain contexts

### Current Implementation

| Component | Location | Issue |
|-----------|----------|-------|
| `EmojiPicker` | `client/src/components/emoji/EmojiPicker.tsx` | Background/sizing OK, but positioning rigid |
| MessageItem usage | `client/src/components/messages/MessageItem.tsx:326-334` | Fixed `bottom-full left-0` positioning |
| ReactionBar usage | `client/src/components/messages/ReactionBar.tsx:62-70` | Fixed `bottom-full left-0` positioning |

**Current positioning pattern:**
```tsx
<div class="absolute bottom-full left-0 mb-2 z-50">
  <EmojiPicker ... />
</div>
```

**Problems:**
- `absolute` positioning relative to parent (can clip)
- `bottom-full` always opens upward
- No viewport boundary detection
- `z-50` may not be high enough in complex stacking contexts

---

## Files to Modify

### Dependencies
| File | Changes |
|------|---------|
| `client/package.json` | Add `@floating-ui/dom` |

### New Files
| File | Purpose |
|------|---------|
| `client/src/components/emoji/EmojiPickerPortal.tsx` | Portal wrapper with floating-ui positioning |

### Modified Files
| File | Changes |
|------|---------|
| `client/src/components/emoji/EmojiPicker.tsx` | Fix background opacity, ensure proper sizing |
| `client/src/components/messages/MessageItem.tsx` | Replace fixed positioning with Portal |
| `client/src/components/messages/ReactionBar.tsx` | Replace fixed positioning with Portal |

---

## Implementation Tasks

### Task 1: Install @floating-ui/dom

**Files:**
- Modify: `client/package.json`

**Step 1: Add dependency**

```bash
cd client && bun add @floating-ui/dom
```

Expected version: `^1.5.0` or later.

**Step 2: Verify installation**

```bash
cd client && bun run check
```

**Step 3: Commit**

```bash
git add client/package.json client/bun.lockb
git commit -m "chore(client): add @floating-ui/dom for emoji picker positioning"
```

---

### Task 2: Fix EmojiPicker Background and Sizing

**Files:**
- Modify: `client/src/components/emoji/EmojiPicker.tsx`

**Purpose:** Ensure picker has proper background opacity and consistent sizing before adding dynamic positioning.

**Step 1: Update root div classes**

Find the root `<div>` (line 31) and update:

```tsx
// BEFORE:
<div class="bg-surface-layer2 rounded-lg shadow-xl w-80 max-h-96 overflow-hidden flex flex-col border border-white/10">

// AFTER:
<div class="bg-surface-layer2/95 backdrop-blur-sm rounded-lg shadow-2xl w-80 max-h-96 overflow-hidden flex flex-col border border-white/10">
```

**Changes:**
- `bg-surface-layer2/95` → Adds 95% opacity (fixes potential transparency)
- `backdrop-blur-sm` → Adds background blur for better readability
- `shadow-2xl` → Stronger shadow for better visual separation

**Step 2: Ensure consistent height**

The `max-h-96` (384px) is good. Verify that the picker doesn't collapse when there are few emojis. The `flex flex-col` layout should handle this, but let's add a min-height for consistency:

```tsx
// AFTER (add min-h to ensure picker doesn't become tiny):
<div class="bg-surface-layer2/95 backdrop-blur-sm rounded-lg shadow-2xl w-80 min-h-64 max-h-96 overflow-hidden flex flex-col border border-white/10">
```

This ensures picker is at least 256px tall (min-h-64).

**Step 3: Verify z-index handling**

The z-index will be handled by the Portal (renders at document root), so remove z-index concerns from EmojiPicker itself. This component should not set z-index.

**Verification:**
```bash
cd client && bun run check
```

**Commit:**
```bash
git add client/src/components/emoji/EmojiPicker.tsx
git commit -m "fix(emoji): improve picker background opacity and sizing"
```

---

### Task 3: Create EmojiPickerPortal Component

**Files:**
- Create: `client/src/components/emoji/EmojiPickerPortal.tsx`

**Purpose:** Portal wrapper that uses floating-ui to dynamically position the picker based on viewport boundaries.

```tsx
/**
 * EmojiPickerPortal - Smart positioning wrapper for EmojiPicker.
 * 
 * Uses Portal to render outside parent hierarchy and @floating-ui/dom
 * for dynamic positioning that adapts to viewport boundaries.
 */
import { Component, createEffect, onCleanup, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { computePosition, flip, shift, offset, autoUpdate } from "@floating-ui/dom";
import EmojiPicker from "./EmojiPicker";

interface EmojiPickerPortalProps {
  /** Reference element (the button that opened the picker) */
  anchorElement: HTMLElement | null;
  /** Whether the picker is visible */
  show: boolean;
  /** Callback when emoji selected */
  onSelect: (emoji: string) => void;
  /** Callback to close picker */
  onClose: () => void;
  /** Optional guild ID for guild emojis */
  guildId?: string;
}

const EmojiPickerPortal: Component<EmojiPickerPortalProps> = (props) => {
  let floatingEl: HTMLDivElement | undefined;

  createEffect(() => {
    if (!props.show || !props.anchorElement || !floatingEl) return;

    // Auto-update positioning when scrolling or resizing
    const cleanup = autoUpdate(
      props.anchorElement,
      floatingEl,
      () => {
        if (!props.anchorElement || !floatingEl) return;

        computePosition(props.anchorElement, floatingEl, {
          placement: "top-start", // Prefer top-left, but will flip if no room
          middleware: [
            offset(8), // 8px gap from anchor
            flip({
              fallbackPlacements: ["bottom-start", "top-end", "bottom-end"],
              padding: 8, // 8px padding from viewport edges
            }),
            shift({
              padding: 8, // Ensure picker stays within viewport horizontally
            }),
          ],
        }).then(({ x, y, placement }) => {
          if (!floatingEl) return;

          Object.assign(floatingEl.style, {
            left: `${x}px`,
            top: `${y}px`,
          });

          // Optional: Add data attribute for placement-specific styling
          floatingEl.dataset.placement = placement;
        });
      },
      {
        // Update on scroll and resize
        ancestorScroll: true,
        elementResize: true,
      }
    );

    onCleanup(cleanup);
  });

  return (
    <Show when={props.show}>
      <Portal>
        {/* Click-outside backdrop */}
        <div
          class="fixed inset-0 z-50"
          onClick={props.onClose}
          onContextMenu={(e) => {
            e.preventDefault();
            props.onClose();
          }}
        />
        
        {/* Floating picker */}
        <div
          ref={floatingEl}
          class="fixed z-50"
          style={{
            // Initial position (will be updated by floating-ui)
            left: "0px",
            top: "0px",
          }}
          onClick={(e) => e.stopPropagation()} // Prevent backdrop click
        >
          <EmojiPicker
            onSelect={props.onSelect}
            onClose={props.onClose}
            guildId={props.guildId}
          />
        </div>
      </Portal>
    </Show>
  );
};

export default EmojiPickerPortal;
```

**Key Features:**
1. **Portal rendering** — Renders at document root, bypasses parent clipping
2. **floating-ui positioning** — Smart placement that flips based on space
3. **autoUpdate** — Repositions on scroll/resize
4. **Click-outside** — Backdrop closes picker
5. **Fallback placements** — Tries top-start → bottom-start → top-end → bottom-end
6. **Viewport padding** — 8px padding from screen edges

**Verification:**
```bash
cd client && bun run check
```

**Commit:**
```bash
git add client/src/components/emoji/EmojiPickerPortal.tsx
git commit -m "feat(emoji): add portal wrapper with floating-ui positioning"
```

---

### Task 4: Update MessageItem to Use Portal

**Files:**
- Modify: `client/src/components/messages/MessageItem.tsx`

**Step 1: Import EmojiPickerPortal**

Replace the EmojiPicker import:

```typescript
// BEFORE:
import EmojiPicker from "@/components/emoji/EmojiPicker";

// AFTER:
import EmojiPickerPortal from "@/components/emoji/EmojiPickerPortal";
```

**Step 2: Add ref for anchor element**

Find the reaction button that opens the picker (around line 320). Add a ref:

```tsx
let reactionButtonRef: HTMLButtonElement | undefined;

// In the button:
<button
  ref={reactionButtonRef}
  class="... existing classes ..."
  onClick={() => setShowReactionPicker(true)}
  title="Add reaction"
>
  <SmilePlus class="w-4 h-4" />
</button>
```

**Step 3: Replace the picker rendering**

Find the section that renders EmojiPicker (around line 326-334). Replace:

```tsx
// BEFORE:
<Show when={showReactionPicker()}>
  <div class="absolute bottom-full left-0 mb-2 z-50">
    <EmojiPicker
      onSelect={handleAddReaction}
      onClose={() => setShowReactionPicker(false)}
      guildId={props.guildId}
    />
  </div>
</Show>

// AFTER:
<EmojiPickerPortal
  anchorElement={reactionButtonRef ?? null}
  show={showReactionPicker()}
  onSelect={handleAddReaction}
  onClose={() => setShowReactionPicker(false)}
  guildId={props.guildId}
/>
```

**Note:** The Portal renders outside the component hierarchy, so the wrapping `<div>` is no longer needed.

**Verification:**
```bash
cd client && bun run check
```

**Commit:**
```bash
git add client/src/components/messages/MessageItem.tsx
git commit -m "refactor(emoji): use portal positioning in MessageItem"
```

---

### Task 5: Update ReactionBar to Use Portal

**Files:**
- Modify: `client/src/components/messages/ReactionBar.tsx`

**Step 1: Import EmojiPickerPortal**

Replace the EmojiPicker import:

```typescript
// BEFORE:
import EmojiPicker from "@/components/emoji/EmojiPicker";

// AFTER:
import EmojiPickerPortal from "@/components/emoji/EmojiPickerPortal";
```

**Step 2: Add ref for anchor element**

Find the button that opens the picker (around line 50). Add a ref:

```tsx
let emojiButtonRef: HTMLButtonElement | undefined;

// In the button:
<button
  ref={emojiButtonRef}
  onClick={() => setShowPicker(!showPicker())}
  class="... existing classes ..."
  title="Add reaction"
>
  {/* SVG content */}
</button>
```

**Step 3: Replace the picker rendering**

Find the section that renders EmojiPicker (around line 62-70). Replace:

```tsx
// BEFORE:
<Show when={showPicker()}>
  <div class="absolute bottom-full left-0 mb-2 z-50">
    <EmojiPicker
      onSelect={handleAddReaction}
      onClose={() => setShowPicker(false)}
      guildId={props.guildId}
    />
  </div>
</Show>

// AFTER:
<EmojiPickerPortal
  anchorElement={emojiButtonRef ?? null}
  show={showPicker()}
  onSelect={handleAddReaction}
  onClose={() => setShowPicker(false)}
  guildId={props.guildId}
/>
```

**Verification:**
```bash
cd client && bun run check
```

**Commit:**
```bash
git add client/src/components/messages/ReactionBar.tsx
git commit -m "refactor(emoji): use portal positioning in ReactionBar"
```

---

### Task 6: CHANGELOG Update

**Files:**
- Modify: `CHANGELOG.md`

Add under `### Fixed` in the `[Unreleased]` section:

```markdown
- Emoji Picker positioning and visibility issues
  - Smart positioning with `@floating-ui/dom` adapts to viewport boundaries
  - Picker automatically flips from top to bottom when space is limited
  - Portal rendering prevents clipping by parent containers
  - Improved background opacity (95%) with backdrop blur for better readability
  - Consistent minimum height (256px) to prevent tiny picker
  - Click-outside-to-close with backdrop overlay
  - Auto-repositions on scroll and window resize
```

**Verification:**
```bash
cd client && bun run check
```

**Commit:**
```bash
git add CHANGELOG.md
git commit -m "docs: add emoji picker polish to changelog"
```

---

## Verification

### Build Check
```bash
cd client && bun run check
```

### Manual Testing

**Basic Functionality:**
1. Open a chat channel with messages
2. Hover over a message → click "Add reaction" button (SmilePlus icon)
3. Emoji picker should appear
4. Verify picker has solid background (not transparent)
5. Select an emoji → picker closes, reaction added

**Viewport Boundary Testing:**

**Test 1: Top of viewport (picker should flip down)**
1. Scroll so a message is near the top of the screen (first visible message)
2. Click "Add reaction" on that message
3. Expected: Picker opens BELOW the button (flipped from default top placement)
4. Verify picker is fully visible, not cut off

**Test 2: Bottom of viewport (picker should open up)**
1. Scroll so a message is near the bottom of the screen
2. Click "Add reaction"
3. Expected: Picker opens ABOVE the button (default placement)
4. Verify picker is fully visible

**Test 3: Left edge (picker should shift right)**
1. Find a message at the left edge of the screen
2. Click "Add reaction"
3. Expected: Picker shifts horizontally to stay within viewport
4. Verify picker doesn't extend past left edge

**Test 4: Right edge (picker should shift left)**
1. Find a message at the right edge of the screen (or resize window narrow)
2. Click "Add reaction"
3. Expected: Picker shifts left to stay within viewport
4. Verify picker doesn't extend past right edge

**Test 5: Scroll repositioning**
1. Open emoji picker
2. While picker is open, scroll the message list
3. Expected: Picker follows the anchor button smoothly
4. Verify picker stays positioned relative to button

**Test 6: Window resize**
1. Open emoji picker
2. Resize browser window (make it narrower)
3. Expected: Picker repositions to stay within viewport
4. Try making window very narrow → picker should stay visible

**Test 7: Click outside to close**
1. Open emoji picker
2. Click anywhere outside the picker (on the backdrop)
3. Expected: Picker closes immediately
4. Right-click outside → also closes

**Test 8: Narrow container**
1. Open DM conversation (typically narrower than guild chat)
2. Click "Add reaction"
3. Verify picker positions correctly and doesn't clip

**ReactionBar Testing:**
1. Find a message with existing reactions
2. Click the smile icon in the ReactionBar (below reactions)
3. Verify picker opens with same smart positioning
4. Test all viewport boundary cases (top, bottom, left, right)

**Search Functionality:**
1. Open picker → type "heart" in search
2. Verify filtered results show correctly
3. Search results should scroll if many matches

**Guild Emojis:**
1. In a guild with custom emojis: open picker
2. Verify "Server Emojis" section appears
3. Click a custom emoji → should insert `:emojiname:`

**Recents:**
1. Use several emojis
2. Open picker again
3. Verify "Recent" section shows last used emojis at top

---

## Edge Cases & Known Limitations

### Edge Cases Handled
1. ✅ Picker at screen edges (shift middleware)
2. ✅ Picker at top/bottom of viewport (flip middleware)
3. ✅ Scroll repositioning (autoUpdate)
4. ✅ Window resize (autoUpdate)
5. ✅ Click outside to close (backdrop)
6. ✅ Parent container clipping (Portal bypasses)

### Known Limitations

**1. Multi-Monitor Setups**
- If user drags window between monitors with different DPIs, picker may need manual reopen
- **Mitigation:** autoUpdate handles most cases, but extreme DPI changes may glitch momentarily

**2. Mobile/Touch Devices**
- Click-outside uses `onClick`, not touch events
- **Future Enhancement:** Add `onTouchStart` to backdrop for better mobile support
- **Current Status:** Should work on mobile but may feel less responsive

**3. Very Small Viewports**
- If viewport is smaller than picker minimum size (256px height + 320px width), picker may extend past edges
- **Mitigation:** shift middleware keeps it within viewport horizontally, but may overlap content
- **Future Enhancement:** Add responsive sizing for mobile (e.g., full-screen modal on <640px)

**4. Accessibility**
- No keyboard navigation to open picker (must click button)
- **Future Enhancement:** Add keyboard shortcut (e.g., Ctrl+E when message focused)
- Picker itself should be keyboard navigable (verify with Tab key)

**5. Performance with Many Emojis**
- Rendering 500+ emojis in categories may cause scroll lag
- **Current:** Limits to 32 emojis per category (`slice(0, 32)`)
- **Future Enhancement:** Virtualized scrolling for full emoji set

---

## Performance Considerations

### Before (Fixed Positioning)
- ❌ Re-renders on parent scroll (repaints absolute positioned element)
- ❌ Can trigger expensive layout recalculations
- ❌ Z-index stacking issues with other modals

### After (Portal + floating-ui)
- ✅ Renders at document root (stable stacking context)
- ✅ autoUpdate throttles position updates efficiently
- ✅ GPU-accelerated transforms (left/top properties)
- ⚠️ Slightly more JavaScript overhead for position calculations
- **Net Result:** Better UX, acceptable performance cost

### Optimization Notes
- `autoUpdate` is efficient and throttles updates automatically
- Consider adding `{ strategy: 'fixed' }` to computePosition options if experiencing jank on scroll
- Current `strategy: 'absolute'` (default) is fine for most cases

---

## Future Enhancements (Out of Scope)

1. **Emoji Skin Tone Picker** — Hold emoji to select skin tone variant
2. **Emoji Categories Tab Bar** — Quick jump to category (Smileys, Animals, Food, etc.)
3. **GIF Support** — Integrate Tenor API for GIF picker
4. **Emoji Paste** — Paste emoji directly from system emoji picker
5. **Virtualized Scrolling** — Handle thousands of emojis without lag
6. **Mobile Optimization** — Full-screen modal on small devices
7. **Keyboard Shortcuts** — Arrow keys to navigate, Enter to select
8. **Recent Emojis Persistence** — Save to server preferences (currently localStorage only)

---

## Dependencies

### Package Versions
- `@floating-ui/dom`: ^1.5.0 (MIT license)

### Browser Compatibility
- Chrome/Edge: 90+
- Firefox: 88+
- Safari: 15+

Solid.js Portal requires modern browsers. If IE11 support needed (not recommended), would require polyfills.
