# Web Responsive Fixes & Overhaul — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 5 concrete web responsive bugs and implement a full responsive overhaul with drawer-based mobile navigation at the `md` (768px) breakpoint.

**Architecture:** Build responsive infrastructure first (`useBreakpoint`, `createLongPress`, `touch:` variant), then fix the bugs, then implement the drawer-based mobile layout. The overhaul reuses existing `ServerRail` and `Sidebar` components inside a new `MobileDrawer`, toggled by `useIsMobile()`.

**Tech Stack:** Solid.js, UnoCSS, TypeScript, PointerEvents API

**Spec:** `docs/superpowers/specs/2026-04-13-mobile-implementation-fixes-design.md` — Sub-Project 4

**Branch:** `feature/web-responsive`

---

## File Map

| File | Responsibility |
|------|---------------|
| `client/src/lib/useBreakpoint.ts` | NEW — Reactive breakpoint hook |
| `client/src/lib/createLongPress.ts` | NEW — Long-press directive for touch |
| `client/src/components/layout/MobileDrawer.tsx` | NEW — Slide-out drawer |
| `client/src/components/layout/MobileHeader.tsx` | NEW — Mobile top bar |
| `client/src/components/layout/AppShell.tsx` | Toggle desktop/mobile layout |
| `client/src/components/layout/ServerRail.tsx` | Add `compact` prop |
| `client/src/components/layout/Sidebar.tsx` | Fix opacity, pass onNavigate |
| `client/src/components/ui/ContextMenu.tsx` | Add showContextMenuAt, touch targets |
| `client/src/components/guilds/GuildSettingsModal.tsx` | Fix modal overflow |
| `client/src/components/guilds/EmojisTab.tsx` | Fix hover-only delete |
| `client/src/views/Register.tsx` | Add overflow-y-auto |
| `client/src/views/Login.tsx` | Add overflow-y-auto |
| `client/src/views/ForgotPassword.tsx` | Add overflow-y-auto |
| `client/src/views/ResetPassword.tsx` | Add overflow-y-auto |
| `client/uno.config.ts` | Add touch: variant |
| `client/src/components/home/HomeSidebar.tsx` | Fix opacity |
| `client/src/components/voice/VoicePanel.tsx` | Fix opacity |
| `client/src/components/search/SearchPanel.tsx` | Fix opacity |
| `client/src/components/admin/ReportsPanel.tsx` | Fix opacity |
| `client/src/components/admin/CommandCenterPanel.tsx` | Fix opacity |
| `client/src/components/admin/AuditLogPanel.tsx` | Fix opacity |
| `client/src/components/modals/ReportModal.tsx` | Fix opacity |

---

## Task 1: Responsive infrastructure — useBreakpoint and createLongPress

**Files:**
- Create: `client/src/lib/useBreakpoint.ts`
- Create: `client/src/lib/createLongPress.ts`

- [ ] **Step 1: Create useBreakpoint hook**

Create `client/src/lib/useBreakpoint.ts`:

```typescript
import { createSignal, onCleanup } from "solid-js";

/**
 * Reactive breakpoint hook using window.matchMedia.
 * Returns a signal that updates when the viewport crosses the threshold.
 */
export function useBreakpoint(query: string): () => boolean {
  const mql = window.matchMedia(query);
  const [matches, setMatches] = createSignal(mql.matches);

  const handler = (e: MediaQueryListEvent) => setMatches(e.matches);
  mql.addEventListener("change", handler);
  onCleanup(() => mql.removeEventListener("change", handler));

  return matches;
}

/** Convenience: true when viewport is below md (768px). */
export function useIsMobile(): () => boolean {
  return useBreakpoint("(max-width: 767px)");
}
```

- [ ] **Step 2: Create createLongPress directive**

Create `client/src/lib/createLongPress.ts`:

```typescript
/**
 * Long-press handler for touch support.
 * Uses PointerEvents for unified mouse/touch/pen input.
 * Returns event handlers to spread onto elements.
 */
export function createLongPress(
  onLongPress: (x: number, y: number) => void,
  duration = 500
) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let startX = 0;
  let startY = 0;

  const onPointerDown = (e: PointerEvent) => {
    startX = e.clientX;
    startY = e.clientY;
    timer = setTimeout(() => {
      onLongPress(e.clientX, e.clientY);
      timer = null;
    }, duration);
  };

  const cancel = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  };

  const onPointerMove = (e: PointerEvent) => {
    if (timer && (Math.abs(e.clientX - startX) > 10 || Math.abs(e.clientY - startY) > 10)) {
      cancel();
    }
  };

  const onContextMenu = (e: Event) => {
    // Suppress native context menu on long-press (Android Chrome)
    if (timer) {
      e.preventDefault();
    }
  };

  return {
    onPointerDown,
    onPointerUp: cancel,
    onPointerCancel: cancel,
    onPointerMove,
    onContextMenu,
  };
}
```

- [ ] **Step 3: Build and verify**

Run: `cd client && bun run build`
Expected: PASS — no consumers yet, just verifying the files compile

- [ ] **Step 4: Commit**

```bash
git add client/src/lib/useBreakpoint.ts client/src/lib/createLongPress.ts
git commit -m "feat(client): add useBreakpoint hook and createLongPress directive"
```

---

## Task 2: UnoCSS touch variant and opacity fixes (#29, #31)

**Files:**
- Modify: `client/uno.config.ts`
- Modify: `client/src/components/layout/Sidebar.tsx:121,130`
- Modify: `client/src/components/home/HomeSidebar.tsx:55`
- Modify: `client/src/components/voice/VoicePanel.tsx:101`
- Modify: `client/src/components/search/SearchPanel.tsx:364`
- Modify: `client/src/components/admin/ReportsPanel.tsx:315`
- Modify: `client/src/components/admin/CommandCenterPanel.tsx:780,791,878`
- Modify: `client/src/components/admin/AuditLogPanel.tsx:379,382`
- Modify: `client/src/components/modals/ReportModal.tsx:159,173`
- Modify: `client/src/components/guilds/EmojisTab.tsx:161`

- [ ] **Step 1: Add touch: variant to UnoCSS config**

At `uno.config.ts`, add a `variants` array to the config. The UnoCSS variant API uses objects with `name`, `match`, and `parent`:

```typescript
import { defineConfig } from "unocss";

export default defineConfig({
  // ... existing presets, theme, shortcuts ...
  variants: [
    {
      name: "touch",
      match(matcher) {
        if (!matcher.startsWith("touch:")) return;
        return {
          matcher: matcher.slice(6),
          parent: "@media (hover: none)",
        };
      },
    },
  ],
  // ... rest of config
});
```

This creates a `touch:` prefix that wraps the utility in `@media (hover: none) { ... }`. Usage: `touch:opacity-60` renders as `@media (hover: none) { .touch\:opacity-60 { opacity: 0.6; } }`.

- [ ] **Step 2: Fix all text-text-secondary/50 occurrences**

Replace `text-text-secondary/50` with `text-text-muted` in all 13 locations. For `placeholder:text-text-secondary/50`, replace with `placeholder:text-text-muted`.

Files and lines (from grep results):
1. `Sidebar.tsx:121` — `text-text-secondary/50` → `text-text-muted`
2. `Sidebar.tsx:130` — `text-text-secondary/50` → `text-text-muted`
3. `HomeSidebar.tsx:55` — `text-text-secondary/50` → `text-text-muted`
4. `VoicePanel.tsx:101` — `text-text-secondary/50` → `text-text-muted`
5. `SearchPanel.tsx:364` — `placeholder:text-text-secondary/50` → `placeholder:text-text-muted`
6. `ReportsPanel.tsx:315` — `text-text-secondary/50` → `text-text-muted`
7. `CommandCenterPanel.tsx:780` — `placeholder:text-text-secondary/50` → `placeholder:text-text-muted`
8. `CommandCenterPanel.tsx:791` — `placeholder:text-text-secondary/50` → `placeholder:text-text-muted`
9. `CommandCenterPanel.tsx:878` — `placeholder:text-text-secondary/50` → `placeholder:text-text-muted`
10. `AuditLogPanel.tsx:379` — `text-text-secondary/50` → `text-text-muted`
11. `AuditLogPanel.tsx:382` — `text-text-secondary/50` → `text-text-muted`
12. `ReportModal.tsx:159` — `text-text-secondary/50` → `text-text-muted`
13. `ReportModal.tsx:173` — `text-text-secondary/50` → `text-text-muted`

- [ ] **Step 3: Make emoji delete button visible on touch**

At `EmojisTab.tsx:161`, change:

```
opacity-0 group-hover:opacity-100
```

To:

```
opacity-0 group-hover:opacity-100 touch:opacity-60
```

- [ ] **Step 4: Build and verify**

Run: `cd client && bun run build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add client/uno.config.ts \
  client/src/components/layout/Sidebar.tsx \
  client/src/components/home/HomeSidebar.tsx \
  client/src/components/voice/VoicePanel.tsx \
  client/src/components/search/SearchPanel.tsx \
  client/src/components/admin/ReportsPanel.tsx \
  client/src/components/admin/CommandCenterPanel.tsx \
  client/src/components/admin/AuditLogPanel.tsx \
  client/src/components/modals/ReportModal.tsx \
  client/src/components/guilds/EmojisTab.tsx
git commit -m "fix(client): replace text-text-secondary/50 with text-text-muted, add touch: variant (#29, #31)"
```

---

## Task 3: Modal overflow and form scroll fixes (#11, #30)

**Files:**
- Modify: `client/src/components/guilds/GuildSettingsModal.tsx:149`
- Modify: `client/src/views/Register.tsx:192`
- Modify: `client/src/views/Login.tsx:191`
- Modify: `client/src/views/ForgotPassword.tsx:55`
- Modify: `client/src/views/ResetPassword.tsx:66`

- [ ] **Step 1: Fix GuildSettingsModal sizing**

At `GuildSettingsModal.tsx:149`, change:

```
w-[90vw] md:w-[900px] max-w-5xl
```

To:

```
w-[90vw] max-w-[900px] overflow-x-hidden
```

Also increase the close button padding from `p-1.5` to `p-2.5` for better touch targets.

- [ ] **Step 2: Fix auth view scroll**

In all four auth views, change `min-h-screen` to `min-h-screen overflow-y-auto`:

- `Register.tsx:192`: `min-h-screen` → `min-h-screen overflow-y-auto`
- `Login.tsx:191`: `min-h-screen` → `min-h-screen overflow-y-auto`
- `ForgotPassword.tsx:55`: `min-h-screen` → `min-h-screen overflow-y-auto`
- `ResetPassword.tsx:66`: `min-h-screen` → `min-h-screen overflow-y-auto`

- [ ] **Step 3: Build and verify**

Run: `cd client && bun run build`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add client/src/components/guilds/GuildSettingsModal.tsx \
  client/src/views/Register.tsx \
  client/src/views/Login.tsx \
  client/src/views/ForgotPassword.tsx \
  client/src/views/ResetPassword.tsx
git commit -m "fix(client): fix modal overflow at 800px and add scroll to auth views (#11, #30)"
```

---

## Task 4: ContextMenu touch support (#28)

**Files:**
- Modify: `client/src/components/ui/ContextMenu.tsx:65,143`

- [ ] **Step 1: Add showContextMenuAt overload**

At `ContextMenu.tsx`, after the existing `showContextMenu` function (line 65), add:

```typescript
/** Position-based context menu trigger for touch/long-press. */
export function showContextMenuAt(
  x: number,
  y: number,
  items: ContextMenuEntry[],
  submenuParent?: string
) {
  // Reuse existing positioning logic from showContextMenu
  // but with raw x,y instead of extracting from MouseEvent
  const menuW = 200;
  const menuH = items.length * 36;
  const viewportW = window.innerWidth;
  const viewportH = window.innerHeight;

  const finalX = x + menuW > viewportW ? x - menuW : x;
  const finalY = y + menuH > viewportH ? y - menuH : y;

  setContextMenu({
    x: Math.max(0, finalX),
    y: Math.max(0, finalY),
    items,
    submenuParent: submenuParent ?? null,
  });
}
```

- [ ] **Step 2: Increase context menu item touch targets**

At `ContextMenuItemButton` (line 143), change `py-1.5` to `py-2.5` to bring item height from ~28px to ~40px.

- [ ] **Step 3: Integrate createLongPress at key call sites**

The main call sites for `showContextMenu` are `ChannelItem.tsx`, `MessageItem.tsx`, and `contextMenuBuilders.ts`. Update the two most important call sites to also support long-press:

In message list items (e.g., `MessageItem.tsx:576`), where the current pattern is:

```tsx
onContextMenu={(e) => showContextMenu(e, items)}
```

Add long-press support alongside:

```tsx
import { createLongPress } from "../../lib/createLongPress";
import { showContextMenuAt } from "../ui/ContextMenu";

// Inside the component:
const longPress = createLongPress((x, y) => {
  showContextMenuAt(x, y, buildMessageMenuItems());
});

// On the element:
<div
  onContextMenu={(e) => showContextMenu(e, buildMessageMenuItems())}
  onPointerDown={longPress.onPointerDown}
  onPointerUp={longPress.onPointerUp}
  onPointerCancel={longPress.onPointerCancel}
  onPointerMove={longPress.onPointerMove}
>
```

Apply the same pattern to channel items. This is additive — desktop right-click continues to work unchanged.

- [ ] **Step 4: Build and verify**

Run: `cd client && bun run build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add client/src/components/ui/ContextMenu.tsx \
  client/src/components/channel/MessageItem.tsx \
  client/src/components/channel/ChannelItem.tsx
git commit -m "feat(client): add showContextMenuAt for touch support, integrate long-press at call sites (#28)"
```

---

## Task 5: ServerRail compact prop

**Files:**
- Modify: `client/src/components/layout/ServerRail.tsx:35`

- [ ] **Step 1: Add compact prop to ServerRail**

At `ServerRail.tsx:35`, the current signature is `const ServerRail: Component = () => {`. Change to accept props:

```typescript
interface ServerRailProps {
  compact?: boolean;
}

const ServerRail: Component<ServerRailProps> = (props) => {
```

Add size helpers:

```typescript
const railWidth = () => props.compact ? "w-[56px]" : "w-[72px]";
const iconSize = () => props.compact ? "w-9 h-9" : "w-12 h-12";
const iconPadding = () => props.compact ? "p-1" : "p-2";
```

Apply `railWidth()` to the outermost `<nav>` element's class (currently hardcoded `w-[72px]`). Apply `iconSize()` and `iconPadding()` to each guild icon element (the `Box` or `div` wrappers inside the guild list at lines 102-159). Use template literals or `classList` to apply conditionally. Existing call sites pass no props and get `compact={false}` (default).

- [ ] **Step 2: Build and verify**

Run: `cd client && bun run build`
Expected: PASS — existing call sites pass no props, so they get the default

- [ ] **Step 3: Commit**

```bash
git add client/src/components/layout/ServerRail.tsx
git commit -m "feat(client): add compact prop to ServerRail for mobile drawer"
```

---

## Task 6: MobileDrawer and MobileHeader components

**Files:**
- Create: `client/src/components/layout/MobileDrawer.tsx`
- Create: `client/src/components/layout/MobileHeader.tsx`

- [ ] **Step 1: Create MobileDrawer**

Create `client/src/components/layout/MobileDrawer.tsx`:

```typescript
import { Component, JSX, Show, createEffect, onCleanup } from "solid-js";

interface MobileDrawerProps {
  open: boolean;
  onClose: () => void;
  children: JSX.Element;
}

const MobileDrawer: Component<MobileDrawerProps> = (props) => {
  let drawerRef: HTMLDivElement | undefined;
  let startX = 0;

  const onBackdropClick = () => props.onClose();

  // Swipe-left to close
  const onPointerDown = (e: PointerEvent) => { startX = e.clientX; };
  const onPointerUp = (e: PointerEvent) => {
    if (startX - e.clientX > 50) props.onClose();
  };

  // Prevent body scroll when drawer is open
  createEffect(() => {
    if (props.open) {
      document.body.style.overflow = "hidden";
    } else {
      document.body.style.overflow = "";
    }
  });

  onCleanup(() => { document.body.style.overflow = ""; });

  return (
    <div
      class="fixed inset-0 z-50"
      classList={{ "pointer-events-none": !props.open }}
    >
      {/* Backdrop */}
      <div
        class="absolute inset-0 bg-black/50 transition-opacity duration-200"
        classList={{
          "opacity-100": props.open,
          "opacity-0 pointer-events-none": !props.open,
        }}
        onClick={onBackdropClick}
      />

      {/* Drawer panel */}
      <div
        ref={drawerRef}
        class="absolute top-0 left-0 h-full w-[300px] flex transition-transform duration-200 bg-surface-base"
        classList={{
          "translate-x-0": props.open,
          "-translate-x-full": !props.open,
        }}
        onPointerDown={onPointerDown}
        onPointerUp={onPointerUp}
      >
        {props.children}
      </div>
    </div>
  );
};

export default MobileDrawer;
```

- [ ] **Step 2: Create MobileHeader**

Create `client/src/components/layout/MobileHeader.tsx`:

```typescript
import { Component } from "solid-js";
import { Menu } from "lucide-solid";

interface MobileHeaderProps {
  onMenuClick: () => void;
}
// Guild/channel names are read from stores directly inside the component.
// Import the guild store and derive current guild name and channel name
// from the active selection signals.

const MobileHeader: Component<MobileHeaderProps> = (props) => {
  return (
    <header class="h-[44px] flex items-center px-3 gap-3 bg-surface-layer1 border-b border-border-default shrink-0">
      <button
        class="p-1.5 rounded-lg hover:bg-white/10 transition-colors"
        onClick={props.onMenuClick}
        aria-label="Open navigation"
      >
        <Menu class="w-5 h-5 text-text-primary" />
      </button>
      <div class="flex-1 min-w-0 flex items-center gap-2 text-sm">
        {/* Read from guild store — check client/src/stores/guilds.ts for
            the current guild/channel selection signals and import them here */}
        <Show when={guildName()}>
          <span class="text-text-secondary truncate">{guildName()}</span>
        </Show>
        <Show when={channelName()}>
          <span class="text-text-primary font-medium truncate">
            #{channelName()}
          </span>
        </Show>
      </div>
    </header>
  );
};

export default MobileHeader;
```

- [ ] **Step 3: Build and verify**

Run: `cd client && bun run build`
Expected: PASS — components compile but aren't rendered yet

- [ ] **Step 4: Commit**

```bash
git add client/src/components/layout/MobileDrawer.tsx \
  client/src/components/layout/MobileHeader.tsx
git commit -m "feat(client): add MobileDrawer and MobileHeader components"
```

---

## Task 7: AppShell responsive integration

**Files:**
- Modify: `client/src/components/layout/AppShell.tsx`
- Modify: `client/src/components/layout/Sidebar.tsx`

- [ ] **Step 1: Update AppShell to toggle desktop/mobile layout**

At `AppShell.tsx`, the current file imports `ServerRail`, `Sidebar`, and `LazyErrorBoundary` but no store signals. Add imports:

```typescript
import { useIsMobile } from "../../lib/useBreakpoint";
import MobileDrawer from "./MobileDrawer";
import MobileHeader from "./MobileHeader";
import { createSignal, Show } from "solid-js";
```

For guild/channel names in the header, import from the existing guild store. Check `client/src/stores/guilds.ts` for signals like `currentGuild()` and `currentChannel()` — these are the reactive accessors. If they don't exist as direct exports, derive them from the route params and the guild store's data.

Add state and mobile detection:

```typescript
const isMobile = useIsMobile();
const [drawerOpen, setDrawerOpen] = createSignal(false);
```

Restructure the template. Note: the current Sidebar rendering uses `<Show when={props.sidebar} fallback={<Sidebar />}>{props.sidebar}</Show>` which supports custom sidebars (e.g., HomeSidebar). Preserve this pattern on both desktop and mobile:

```tsx
<div class="flex h-screen w-full overflow-hidden">
  <Show when={!isMobile()}>
    {/* Desktop: existing fixed layout */}
    <Show when={props.showServerRail}><ServerRail /></Show>
    <Show when={props.sidebar} fallback={<Sidebar />}>
      {props.sidebar}
    </Show>
  </Show>

  <Show when={isMobile()}>
    <MobileDrawer open={drawerOpen()} onClose={() => setDrawerOpen(false)}>
      <Show when={props.showServerRail}><ServerRail compact /></Show>
      <Show when={props.sidebar} fallback={
        <Sidebar onNavigate={() => setDrawerOpen(false)} />
      }>
        {/* Custom sidebars don't get onNavigate — they manage their own navigation.
            Wrap in a click listener that closes the drawer on any anchor/button click: */}
        <div onClick={(e) => {
          if ((e.target as HTMLElement).closest("a, button[data-channel]")) {
            setDrawerOpen(false);
          }
        }}>
          {props.sidebar}
        </div>
      </Show>
    </MobileDrawer>
  </Show>

  <div class="flex flex-col flex-1 min-w-0">
    <Show when={isMobile()}>
      <MobileHeader onMenuClick={() => setDrawerOpen(true)} />
    </Show>
    <main class="flex-1 min-h-0">
      {props.children}
    </main>
  </div>
</div>
```

For MobileHeader guild/channel names: read from the guild store inside MobileHeader itself (not passed from AppShell). This avoids coupling AppShell to store internals. MobileHeader imports the guild store directly and reads the current selection.

- [ ] **Step 2: Add onNavigate callback to Sidebar**

In `Sidebar.tsx`, add an optional `onNavigate` prop to the component's props interface:

```typescript
interface SidebarProps {
  onNavigate?: () => void;
}
```

Find the channel click handler (where the router navigates to a channel). After the navigation call, add `props.onNavigate?.()` to auto-close the drawer on mobile. This only fires for the default `Sidebar` — custom sidebars (HomeSidebar etc.) are handled by the click delegate wrapper in AppShell.

- [ ] **Step 3: Add swipe-right-to-open on main content area**

In `AppShell.tsx`, add a pointer event listener on the left 20px edge of the content area. If the pointer moves >50px rightward, open the drawer:

```typescript
const onEdgePointerDown = (e: PointerEvent) => {
  if (isMobile() && e.clientX < 20) {
    edgeStartX = e.clientX;
  }
};
const onEdgePointerUp = (e: PointerEvent) => {
  if (edgeStartX !== null && e.clientX - edgeStartX > 50) {
    setDrawerOpen(true);
  }
  edgeStartX = null;
};
```

- [ ] **Step 4: Build and verify**

Run: `cd client && bun run build && bun run test:run`
Expected: PASS

- [ ] **Step 5: Manual testing**

Start dev server: `cd client && bun run dev`
Test at viewport widths: 800px (desktop), 768px (threshold), 375px (mobile)
Verify:
- Desktop: existing layout unchanged
- Mobile: header visible, sidebars hidden, hamburger opens drawer
- Drawer: shows server rail + channel list, tap channel closes drawer
- Swipe right from edge opens drawer, swipe left on drawer closes it

- [ ] **Step 6: Commit**

```bash
git add client/src/components/layout/AppShell.tsx \
  client/src/components/layout/Sidebar.tsx
git commit -m "feat(client): integrate responsive layout with MobileDrawer at md breakpoint"
```

---

## Task 8: Final verification

- [ ] **Step 1: Run full test and build suite**

```bash
cd client && bun run test:run && bun run build
```
Expected: ALL PASS

- [ ] **Step 2: Verify no text-text-secondary/50 remaining**

```bash
grep -r "text-text-secondary/50" client/src/
```
Expected: No matches

- [ ] **Step 3: Self-review the branch diff**

```bash
git diff main...HEAD --stat
git log --oneline main..HEAD
```

Verify: 5 bugs fixed + responsive overhaul complete, new files are minimal and focused.
