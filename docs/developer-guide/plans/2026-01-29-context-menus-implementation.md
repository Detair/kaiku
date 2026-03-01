# Advanced Browser Context Menus — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a global right-click context menu system with menus for messages, channels, and users.

**Architecture:** A single `ContextMenu` provider renders a Portal-based floating menu at cursor position. Each consumer calls `showContextMenu(event, items)` from its `onContextMenu` handler. Menu items are conditional based on permissions and ownership.

**Tech Stack:** Solid.js (signals, Portal, `onCleanup`), lucide-solid icons, UnoCSS utility classes, existing permission system (`hasPermission` from `permissionConstants.ts`).

---

## Task 1: ContextMenu Provider Component

**Files:**
- Create: `client/src/components/ui/ContextMenu.tsx`
- Modify: `client/src/App.tsx`

### Step 1: Create `ContextMenu.tsx`

Create the file at `client/src/components/ui/ContextMenu.tsx` with the following:

```typescript
/**
 * Context Menu System
 *
 * Global right-click context menu with Portal rendering.
 * Usage: call showContextMenu(event, items) from any onContextMenu handler.
 */

import { Component, For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { Portal } from "solid-js/web";

export interface ContextMenuItem {
  label: string;
  icon?: Component<{ class?: string }>;
  action: () => void;
  danger?: boolean;
  disabled?: boolean;
}

export interface ContextMenuSeparator {
  separator: true;
}

export type ContextMenuEntry = ContextMenuItem | ContextMenuSeparator;

function isSeparator(entry: ContextMenuEntry): entry is ContextMenuSeparator {
  return "separator" in entry && entry.separator === true;
}

// --- Global state ---

interface MenuState {
  visible: boolean;
  x: number;
  y: number;
  items: ContextMenuEntry[];
}

const [menuState, setMenuState] = createSignal<MenuState>({
  visible: false,
  x: 0,
  y: 0,
  items: [],
});

/**
 * Show a context menu at the cursor position.
 * Call this from any onContextMenu handler.
 */
export function showContextMenu(event: MouseEvent, items: ContextMenuEntry[]): void {
  event.preventDefault();
  event.stopPropagation();

  // Filter out disabled items? No — show them greyed out.
  // Filter out empty sections (separators at start/end, double separators).
  const cleaned = cleanItems(items);
  if (cleaned.length === 0) return;

  // Calculate position, flipping if near viewport edge
  const MENU_WIDTH = 220;
  const MENU_ITEM_HEIGHT = 36;
  const SEPARATOR_HEIGHT = 9;
  const PADDING = 8;

  const estimatedHeight = cleaned.reduce((h, entry) =>
    h + (isSeparator(entry) ? SEPARATOR_HEIGHT : MENU_ITEM_HEIGHT), PADDING * 2);

  let x = event.clientX;
  let y = event.clientY;

  if (x + MENU_WIDTH > window.innerWidth) {
    x = window.innerWidth - MENU_WIDTH - 8;
  }
  if (y + estimatedHeight > window.innerHeight) {
    y = window.innerHeight - estimatedHeight - 8;
  }

  setMenuState({ visible: true, x, y, items: cleaned });
}

/**
 * Hide the context menu.
 */
export function hideContextMenu(): void {
  setMenuState({ visible: false, x: 0, y: 0, items: [] });
}

/** Remove leading/trailing/double separators */
function cleanItems(items: ContextMenuEntry[]): ContextMenuEntry[] {
  const result: ContextMenuEntry[] = [];
  for (const item of items) {
    if (isSeparator(item)) {
      // Skip if first item or previous was also a separator
      if (result.length === 0 || isSeparator(result[result.length - 1])) continue;
    }
    result.push(item);
  }
  // Remove trailing separator
  if (result.length > 0 && isSeparator(result[result.length - 1])) {
    result.pop();
  }
  return result;
}

/**
 * ContextMenuContainer — mount once in App.tsx alongside ToastContainer.
 */
export const ContextMenuContainer: Component = () => {
  let menuRef: HTMLDivElement | undefined;

  const handleClickOutside = (e: MouseEvent) => {
    if (menuState().visible && menuRef && !menuRef.contains(e.target as Node)) {
      hideContextMenu();
    }
  };

  const handleEscape = (e: KeyboardEvent) => {
    if (e.key === "Escape" && menuState().visible) {
      hideContextMenu();
    }
  };

  const handleScroll = () => {
    if (menuState().visible) {
      hideContextMenu();
    }
  };

  onMount(() => {
    document.addEventListener("click", handleClickOutside, true);
    document.addEventListener("keydown", handleEscape);
    document.addEventListener("scroll", handleScroll, true);
    // Also close on another context menu
    document.addEventListener("contextmenu", () => {
      // Will be replaced by new showContextMenu call if applicable
    });
  });

  onCleanup(() => {
    document.removeEventListener("click", handleClickOutside, true);
    document.removeEventListener("keydown", handleEscape);
    document.removeEventListener("scroll", handleScroll, true);
  });

  const handleItemClick = (item: ContextMenuItem) => {
    if (item.disabled) return;
    hideContextMenu();
    item.action();
  };

  return (
    <Portal>
      <Show when={menuState().visible}>
        <div
          ref={menuRef}
          class="fixed z-[9999] min-w-[200px] max-w-[280px] py-1.5 bg-surface-base border border-white/10 rounded-lg shadow-xl"
          style={{
            left: `${menuState().x}px`,
            top: `${menuState().y}px`,
          }}
          role="menu"
          aria-label="Context menu"
        >
          <For each={menuState().items}>
            {(entry) => (
              <Show
                when={!isSeparator(entry)}
                fallback={
                  <div class="my-1 mx-2 border-t border-white/10" role="separator" />
                }
              >
                {(() => {
                  const item = entry as ContextMenuItem;
                  return (
                    <button
                      class="w-full flex items-center gap-2.5 px-3 py-1.5 text-sm text-left transition-colors"
                      classList={{
                        "text-accent-error hover:bg-accent-error/10": !!item.danger && !item.disabled,
                        "text-text-primary hover:bg-white/5": !item.danger && !item.disabled,
                        "text-text-muted cursor-not-allowed": !!item.disabled,
                      }}
                      onClick={() => handleItemClick(item)}
                      disabled={item.disabled}
                      role="menuitem"
                    >
                      <Show when={item.icon}>
                        {(Icon) => <Icon() class="w-4 h-4 flex-shrink-0" />}
                      </Show>
                      <span class="truncate">{item.label}</span>
                    </button>
                  );
                })()}
              </Show>
            )}
          </For>
        </div>
      </Show>
    </Portal>
  );
};
```

### Step 2: Run TypeScript check

```bash
cd client && bunx tsc --noEmit
```

Expected: No errors related to ContextMenu.tsx.

### Step 3: Mount in App.tsx

In `client/src/App.tsx`, add the import and render `<ContextMenuContainer />` inside the `Layout` component, after `<ToastContainer />`:

```typescript
// Add import at top:
import { ContextMenuContainer } from "./components/ui/ContextMenu";

// In Layout component, after <ToastContainer />:
<ContextMenuContainer />
```

The Layout component should become:
```tsx
const Layout: Component<ParentProps> = (props) => {
  onMount(async () => {
    await initTheme();
  });

  return (
    <div class="h-screen bg-background-tertiary text-text-primary">
      {props.children}
      <ToastContainer />
      <ContextMenuContainer />
    </div>
  );
};
```

### Step 4: Run TypeScript check

```bash
cd client && bunx tsc --noEmit
```

Expected: No errors.

### Step 5: Commit

```bash
git add client/src/components/ui/ContextMenu.tsx client/src/App.tsx
git commit -m "feat(client): add global context menu system with Portal rendering"
```

---

## Task 2: Message Context Menu

**Files:**
- Modify: `client/src/components/messages/MessageItem.tsx`
- Read (context): `client/src/lib/types.ts` (Message type), `client/src/lib/permissionConstants.ts`, `client/src/stores/auth.ts`

### Step 1: Read MessageItem.tsx fully

Read the full file to understand its structure: props, JSX tree, existing hover actions, and how it accesses message data.

### Step 2: Add onContextMenu handler

In `MessageItem.tsx`:

1. Add imports at the top:
```typescript
import { showContextMenu, type ContextMenuEntry } from "@/components/ui/ContextMenu";
import { Copy, Reply, Pencil, Trash2, Pin, Quote, Link, Hash } from "lucide-solid";
import { currentUser } from "@/stores/auth";
```

2. Inside the `MessageItem` component, add a function that builds the menu items:

```typescript
const buildContextMenu = (e: MouseEvent) => {
  const msg = props.message;
  const me = currentUser();
  const isOwn = me?.id === msg.author.id;

  const items: ContextMenuEntry[] = [
    {
      label: "Copy Text",
      icon: Copy,
      action: () => navigator.clipboard.writeText(msg.content),
    },
    {
      label: "Copy Message Link",
      icon: Link,
      action: () => navigator.clipboard.writeText(`${window.location.origin}/messages/${msg.id}`),
    },
    {
      label: "Copy ID",
      icon: Hash,
      action: () => navigator.clipboard.writeText(msg.id),
    },
  ];

  // Separator before destructive/privileged actions
  if (isOwn) {
    items.push(
      { separator: true },
      {
        label: "Edit Message",
        icon: Pencil,
        action: () => {
          // TODO: Trigger edit mode on this message
          console.log("Edit message:", msg.id);
        },
      },
      {
        label: "Delete Message",
        icon: Trash2,
        danger: true,
        action: () => {
          // TODO: Trigger delete confirmation
          console.log("Delete message:", msg.id);
        },
      },
    );
  }

  showContextMenu(e, items);
};
```

3. Add `onContextMenu={buildContextMenu}` to the outermost `<div>` of the message item (the one with `class="group ..."` or similar). Find the root element in the component's JSX and add the handler.

### Step 3: Run TypeScript check

```bash
cd client && bunx tsc --noEmit
```

Expected: No errors.

### Step 4: Manual test

Right-click a message in the chat. Context menu should appear with Copy Text, Copy Message Link, Copy ID. For own messages, should also show Edit and Delete options.

### Step 5: Commit

```bash
git add client/src/components/messages/MessageItem.tsx
git commit -m "feat(client): add right-click context menu for messages"
```

---

## Task 3: Channel Context Menu

**Files:**
- Modify: `client/src/components/channels/ChannelItem.tsx`
- Read (context): `client/src/stores/sound.ts` (mute functions), `client/src/stores/favorites.ts`

### Step 1: Read ChannelItem.tsx fully

Read the full file to understand its structure, props, existing buttons (favorites, settings, mute indicator).

### Step 2: Add onContextMenu handler

In `ChannelItem.tsx`:

1. Add imports:
```typescript
import { showContextMenu, type ContextMenuEntry } from "@/components/ui/ContextMenu";
import { CheckCheck, BellOff, Bell, Settings, Copy, Star, StarOff } from "lucide-solid";
```

2. Inside the `ChannelItem` component, add:

```typescript
const buildContextMenu = (e: MouseEvent) => {
  const ch = props.channel;
  const muted = isChannelMuted(ch.id);
  const favorited = props.guildId ? isFavorited(ch.id) : false;

  const items: ContextMenuEntry[] = [];

  // Mark as Read (only for text channels with unreads)
  if (ch.channel_type === "text" && ch.unread_count > 0) {
    items.push({
      label: "Mark as Read",
      icon: CheckCheck,
      action: () => {
        // TODO: call markChannelAsRead
        console.log("Mark as read:", ch.id);
      },
    });
  }

  // Mute/Unmute
  items.push({
    label: muted ? "Unmute Channel" : "Mute Channel",
    icon: muted ? Bell : BellOff,
    action: () => {
      // Toggle mute via sound store
      // TODO: call toggleChannelMute
      console.log("Toggle mute:", ch.id);
    },
  });

  // Favorites
  if (props.guildId) {
    items.push({
      label: favorited ? "Remove from Favorites" : "Add to Favorites",
      icon: favorited ? StarOff : Star,
      action: () => {
        toggleFavorite(ch.id, props.guildId!, props.guildName ?? "", props.guildIcon ?? null);
      },
    });
  }

  // Separator
  items.push({ separator: true });

  // Edit Channel (if settings callback exists = user has permission)
  if (props.onSettings) {
    items.push({
      label: "Edit Channel",
      icon: Settings,
      action: () => props.onSettings!(),
    });
  }

  // Copy ID
  items.push({
    label: "Copy Channel ID",
    icon: Copy,
    action: () => navigator.clipboard.writeText(ch.id),
  });

  showContextMenu(e, items);
};
```

3. Add `onContextMenu={buildContextMenu}` to the outermost clickable element of the channel item (the `<button>` or `<div>` wrapper).

### Step 3: Run TypeScript check

```bash
cd client && bunx tsc --noEmit
```

Expected: No errors.

### Step 4: Manual test

Right-click a channel in the sidebar. Menu should show relevant items (Mark as Read only when unread, Mute/Unmute toggle, Favorites toggle, Edit only with permission, Copy ID always).

### Step 5: Commit

```bash
git add client/src/components/channels/ChannelItem.tsx
git commit -m "feat(client): add right-click context menu for channels"
```

---

## Task 4: User Context Menu

**Files:**
- Modify: `client/src/components/guilds/MembersTab.tsx` (or wherever the member list renders individual members)
- Read (context): `client/src/lib/tauri.ts` (friend functions), `client/src/stores/auth.ts`

### Step 1: Find user list components

Search for components that render user avatars/names in member lists, DM participant lists, and message author areas. Key locations:
- Guild member list (`MembersTab.tsx` or a `MemberItem` subcomponent)
- Message author avatar/name (already handled partially in MessageItem — can extend)

Read the relevant file(s) to understand the rendering structure.

### Step 2: Create a reusable user context menu builder

Create a utility function (can live in `ContextMenu.tsx` or a new `client/src/lib/contextMenuBuilders.ts`) to build user menu items:

```typescript
import { showContextMenu, type ContextMenuEntry } from "@/components/ui/ContextMenu";
import { User, MessageSquare, UserPlus, UserMinus, Ban, Copy } from "lucide-solid";
import { currentUser } from "@/stores/auth";

export function showUserContextMenu(
  event: MouseEvent,
  user: { id: string; username: string; display_name?: string },
): void {
  const me = currentUser();
  const isSelf = me?.id === user.id;

  const items: ContextMenuEntry[] = [
    {
      label: "View Profile",
      icon: User,
      action: () => {
        // TODO: open profile modal/panel
        console.log("View profile:", user.id);
      },
    },
  ];

  if (!isSelf) {
    items.push(
      {
        label: "Send Message",
        icon: MessageSquare,
        action: () => {
          // TODO: navigate to or create DM with this user
          console.log("Send message to:", user.id);
        },
      },
      { separator: true },
      {
        label: "Add Friend",
        icon: UserPlus,
        action: () => {
          // TODO: send friend request
          console.log("Add friend:", user.username);
        },
      },
      { separator: true },
      {
        label: "Block",
        icon: Ban,
        danger: true,
        action: () => {
          // TODO: block user
          console.log("Block:", user.id);
        },
      },
    );
  }

  items.push(
    { separator: true },
    {
      label: "Copy User ID",
      icon: Copy,
      action: () => navigator.clipboard.writeText(user.id),
    },
  );

  showContextMenu(event, items);
}
```

### Step 3: Integrate into MembersTab

In the guild member list component, add `onContextMenu` to each member row:

```tsx
onContextMenu={(e) => showUserContextMenu(e, { id: member.user_id, username: member.username, display_name: member.display_name })}
```

### Step 4: Integrate into MessageItem author

In `MessageItem.tsx`, add `onContextMenu` to the author name/avatar area (only the author section, not the whole message — the whole message already has the message context menu):

```tsx
// On the author avatar or name span:
onContextMenu={(e) => {
  e.stopPropagation(); // Prevent message context menu
  showUserContextMenu(e, { id: msg.author.id, username: msg.author.username, display_name: msg.author.display_name });
}}
```

### Step 5: Run TypeScript check

```bash
cd client && bunx tsc --noEmit
```

Expected: No errors.

### Step 6: Manual test

- Right-click a user in the guild member list → should show View Profile, Send Message, Add Friend, Block, Copy ID.
- Right-click a message author name → should show user menu (not message menu).
- Right-click own name → should NOT show Send Message, Add Friend, Block.

### Step 7: Commit

```bash
git add client/src/lib/contextMenuBuilders.ts client/src/components/guilds/MembersTab.tsx client/src/components/messages/MessageItem.tsx
git commit -m "feat(client): add right-click context menu for users"
```

---

## Task 5: Keyboard Navigation

**Files:**
- Modify: `client/src/components/ui/ContextMenu.tsx`

### Step 1: Add keyboard navigation

In `ContextMenuContainer`, add arrow key, Enter, and Home/End support:

1. Track a `focusedIndex` signal (default `-1`, meaning no focus).
2. On `ArrowDown`: increment index (skip separators and disabled items, wrap around).
3. On `ArrowUp`: decrement index (skip separators and disabled items, wrap around).
4. On `Enter` or `Space`: execute focused item's action.
5. On `Home`: focus first non-separator, non-disabled item.
6. On `End`: focus last non-separator, non-disabled item.
7. Apply a `bg-white/10` highlight class to the focused item.
8. Reset `focusedIndex` to `-1` when menu opens or closes.

Add these handlers to the existing `handleEscape` keydown listener:

```typescript
const handleKeyDown = (e: KeyboardEvent) => {
  const state = menuState();
  if (!state.visible) return;

  if (e.key === "Escape") {
    hideContextMenu();
    return;
  }

  const actionableIndices = state.items
    .map((item, i) => ({ item, i }))
    .filter(({ item }) => !isSeparator(item) && !(item as ContextMenuItem).disabled)
    .map(({ i }) => i);

  if (actionableIndices.length === 0) return;

  if (e.key === "ArrowDown") {
    e.preventDefault();
    const currentIdx = actionableIndices.indexOf(focusedIndex());
    const nextIdx = currentIdx < actionableIndices.length - 1 ? currentIdx + 1 : 0;
    setFocusedIndex(actionableIndices[nextIdx]);
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    const currentIdx = actionableIndices.indexOf(focusedIndex());
    const nextIdx = currentIdx > 0 ? currentIdx - 1 : actionableIndices.length - 1;
    setFocusedIndex(actionableIndices[nextIdx]);
  } else if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    const idx = focusedIndex();
    if (idx >= 0 && idx < state.items.length) {
      const item = state.items[idx];
      if (!isSeparator(item)) {
        handleItemClick(item as ContextMenuItem);
      }
    }
  } else if (e.key === "Home") {
    e.preventDefault();
    if (actionableIndices.length > 0) setFocusedIndex(actionableIndices[0]);
  } else if (e.key === "End") {
    e.preventDefault();
    if (actionableIndices.length > 0) setFocusedIndex(actionableIndices[actionableIndices.length - 1]);
  }
};
```

Apply focused styling to menu items:
```tsx
classList={{
  // ... existing classes ...
  "bg-white/10": focusedIndex() === index,
}}
```

### Step 2: Run TypeScript check

```bash
cd client && bunx tsc --noEmit
```

### Step 3: Manual test

- Open a context menu, press ArrowDown/ArrowUp to navigate.
- Enter selects the focused item.
- Escape closes the menu.
- Separators and disabled items are skipped.

### Step 4: Commit

```bash
git add client/src/components/ui/ContextMenu.tsx
git commit -m "feat(client): add keyboard navigation to context menus"
```

---

## Task 6: CHANGELOG Update

**Files:**
- Modify: `CHANGELOG.md`

### Step 1: Add changelog entry

Under `## [Unreleased]` → `### Added`, add:

```markdown
- Right-click context menus for messages, channels, and users
  - Message menu: Copy Text, Copy Link, Copy ID, Edit (own), Delete (own)
  - Channel menu: Mark as Read, Mute/Unmute, Add to Favorites, Edit, Copy ID
  - User menu: View Profile, Send Message, Add Friend, Block, Copy ID
  - Keyboard navigation with arrow keys, Enter, and Escape
```

### Step 2: Commit

```bash
git add CHANGELOG.md
git commit -m "docs: add context menus to changelog"
```

---

## Verification

### TypeScript
```bash
cd client && bunx tsc --noEmit
```

### Manual Testing Checklist
1. Right-click a message → menu appears at cursor with correct items
2. Right-click own message → shows Edit and Delete options
3. Right-click other user's message → no Edit/Delete
4. Right-click a channel → shows Mark as Read (only if unread), Mute, Favorites, Copy ID
5. Right-click a channel with manage permission → shows Edit Channel
6. Right-click a user in member list → shows profile/message/friend/block options
7. Right-click own user → no Send Message/Add Friend/Block
8. Right-click message author name → shows user menu, not message menu
9. Menu flips when near viewport edge (bottom-right corner)
10. Click outside closes menu
11. Escape closes menu
12. Scroll closes menu
13. Arrow keys navigate items (skip separators and disabled)
14. Enter activates focused item
15. Another right-click replaces current menu
