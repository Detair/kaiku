# Friction-Reduction & Productivity — Design

> **Status:** Draft
> **Date:** 2026-02-01

## Overview

Streamline daily messaging interactions with four incremental improvements:

1. **Multi-line Input Upgrade** — Replace `<input>` with auto-resizing `<textarea>`
2. **Persistent Drafts** — Preserve unsent messages across channel switches
3. **Quick Message Actions** — Hover toolbar with common emoji reactions and actions
4. **Smart Input Auto-complete** — Popup suggestions for `@users` and `:emojis:`

---

## MVP Scope

- Multi-line `<textarea>` with Shift+Enter for newlines, Enter to send
- Persistent drafts with localStorage (no server sync)
- Quick-action toolbar on message hover (4 quick emojis + picker + context menu)
- Auto-complete for `@user` mentions and `:emoji:` shortcodes

**Deferred:**
- `#channel` auto-complete (lower frequency use, add in follow-up)
- `/command` auto-complete (no slash command system yet)
- Markdown keyboard shortcuts (Ctrl+B/I/K — add when `<textarea>` supports selection ranges)
- Server-synced drafts (overkill for v1, localStorage is sufficient)
- Draft indicator badges on channels (cosmetic, low priority)
- Configurable quick-emoji sets per guild

---

## 1. Multi-line Input Upgrade

### Problem

`MessageInput` uses `<input type="text">` which doesn't support newlines. Users can't write multi-paragraph messages.

### Design

Replace `<input>` with `<textarea>` that auto-resizes based on content.

#### Changes to MessageInput.tsx

**Element replacement:**
```tsx
{/* Before */}
<input type="text" ... />

{/* After */}
<textarea
  ref={textareaRef}
  value={content()}
  onInput={handleTextareaInput}
  onKeyDown={handleKeyDown}
  rows={1}
  class="flex-1 bg-transparent py-3 text-text-input placeholder-text-secondary
         focus:outline-none resize-none overflow-hidden"
  placeholder={`Message #${props.channelName}`}
  disabled={isSending()}
  style={{ "max-height": `${MAX_HEIGHT}px` }}
/>
```

**Auto-resize logic:**
```typescript
const MIN_HEIGHT = 24;  // ~1 line
const MAX_HEIGHT = 192; // ~8 lines

function autoResize(textarea: HTMLTextAreaElement) {
  textarea.style.height = "auto"; // Reset to measure scrollHeight
  const newHeight = Math.min(textarea.scrollHeight, MAX_HEIGHT);
  textarea.style.height = `${newHeight}px`;

  // Switch to scrollable when hitting max
  textarea.style.overflowY = newHeight >= MAX_HEIGHT ? "auto" : "hidden";
}
```

Use `requestAnimationFrame` to batch reflows and prevent input lag on very long messages (e.g. pasting a 500-line code block):
```typescript
let resizeScheduled = false;
function scheduleResize(textarea: HTMLTextAreaElement) {
  if (resizeScheduled) return;
  resizeScheduled = true;
  requestAnimationFrame(() => {
    autoResize(textarea);
    resizeScheduled = false;
  });
}
```

**Key handling with IME composition support:**

CJK input methods fire `compositionstart`/`compositionend` events. During composition, Enter must not send the message — it confirms the composed character.

```typescript
let isComposing = false;

// On the textarea element:
// onCompositionStart={() => isComposing = true}
// onCompositionEnd={() => isComposing = false}

const handleKeyDown = (e: KeyboardEvent) => {
  // Don't intercept during IME composition (CJK input)
  if (isComposing) return;

  // When autocomplete is active, intercept navigation keys and delegate
  // to PopupList via imperative handle (see PopupList API Contract in Section 4)
  if (autocompleteContext()) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      popupListRef?.navigateDown();
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      popupListRef?.navigateUp();
      return;
    }
    if (e.key === "Enter" || e.key === "Tab") {
      e.preventDefault();
      popupListRef?.selectCurrent();
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      setAutocompleteContext(null);
      return;
    }
  }

  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    handleSubmit(e);
  }
  // Shift+Enter: default behavior (inserts newline in textarea)
};
```

**After send, reset height:**
```typescript
setContent("");
clearDraft(props.channelId);
if (textareaRef) {
  textareaRef.style.height = "auto";
}
```

**Ref and state declarations:**
```typescript
import type { PopupListHandle } from "@/components/ui/PopupList";

let textareaRef: HTMLTextAreaElement | undefined;
let popupListRef: PopupListHandle | undefined;
let isComposing = false;
const [autocompleteContext, setAutocompleteContext] = createSignal<{
  type: "user" | "emoji";
  query: string;
  startPos: number;
} | null>(null);
// (was: let inputRef: HTMLInputElement | undefined)
```

---

## 2. Persistent Drafts

### Problem

`MessageInput` uses a component-local `createSignal("")` for content. Solid.js doesn't unmount/remount on prop changes — the component instance persists across channel switches — but there is no reactive effect syncing content to the new `channelId`, so the previous channel's text remains visible and the draft is effectively lost when overwritten.

### Design

Store drafts in a lightweight reactive store backed by localStorage.

#### E2EE Draft Policy

DM channels may have E2EE enabled. Storing plaintext drafts for encrypted DMs in localStorage would be a privacy leak (an attacker with localStorage access reads unencrypted drafts, defeating E2EE).

**Decision: Skip draft persistence for E2EE-enabled DMs.** The in-memory signal still works within the session, but `saveDraft` is a no-op for E2EE channels. This is the simplest secure option. Users are unlikely to notice since DM conversations are typically short exchanges, not long-form composition.

#### Store: `client/src/stores/drafts.ts`

```typescript
import { createStore, unwrap } from "solid-js/store";

const STORAGE_KEY = "vc:drafts";
const MAX_DRAFTS = 50; // Prevent unbounded growth

interface DraftEntry {
  content: string;
  updatedAt: number; // Date.now() — for LRU eviction
}

interface DraftsState {
  byChannel: Record<string, DraftEntry>;
}

// Load from localStorage on init
function loadDrafts(): Record<string, DraftEntry> {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (!stored) return {};
    const parsed = JSON.parse(stored);
    // Migration: handle old format (plain strings) if needed
    if (typeof Object.values(parsed)[0] === "string") {
      const migrated: Record<string, DraftEntry> = {};
      for (const [k, v] of Object.entries(parsed)) {
        migrated[k] = { content: v as string, updatedAt: Date.now() };
      }
      return migrated;
    }
    return parsed;
  } catch {
    return {};
  }
}

const [draftsState, setDraftsState] = createStore<DraftsState>({
  byChannel: loadDrafts(),
});

// Debounced persist (300ms) to avoid thrashing localStorage
let persistTimer: ReturnType<typeof setTimeout> | undefined;
function persistDrafts() {
  clearTimeout(persistTimer);
  persistTimer = setTimeout(() => {
    // unwrap() strips Solid proxy before cloning (structuredClone throws on proxies)
    const plain = structuredClone(unwrap(draftsState.byChannel));
    localStorage.setItem(STORAGE_KEY, JSON.stringify(plain));
  }, 300);
}

// Force-flush on app close to avoid losing drafts within the debounce window
if (typeof window !== "undefined") {
  window.addEventListener("beforeunload", () => {
    clearTimeout(persistTimer);
    const plain = structuredClone(draftsState.byChannel);
    localStorage.setItem(STORAGE_KEY, JSON.stringify(plain));
  });
}

export function getDraft(channelId: string): string {
  return draftsState.byChannel[channelId]?.content ?? "";
}

export function saveDraft(channelId: string, content: string, isE2EE: boolean = false): void {
  // Don't persist drafts for E2EE DMs (security)
  if (isE2EE) return;

  if (!content.trim()) {
    setDraftsState("byChannel", channelId, undefined!);
  } else {
    setDraftsState("byChannel", channelId, { content, updatedAt: Date.now() });
    evictIfNeeded();
  }
  persistDrafts();
}

export function clearDraft(channelId: string): void {
  setDraftsState("byChannel", channelId, undefined!);
  persistDrafts();
}

export function clearAllDrafts(): void {
  setDraftsState({ byChannel: {} });
  localStorage.removeItem(STORAGE_KEY);
}

function evictIfNeeded(): void {
  const entries = Object.entries(draftsState.byChannel);
  if (entries.length <= MAX_DRAFTS) return;

  // Evict oldest by updatedAt (LRU)
  const sorted = entries
    .filter(([, v]) => v != null)
    .sort(([, a], [, b]) => a.updatedAt - b.updatedAt);

  const toEvict = sorted.slice(0, entries.length - MAX_DRAFTS);
  for (const [key] of toEvict) {
    setDraftsState("byChannel", key, undefined!);
  }
}
```

#### MessageInput Changes

**New prop for context:**
```typescript
interface MessageInputProps {
  channelId: string;
  channelName: string;
  guildId?: string;       // NEW — needed for autocomplete member lookup
  isE2EE?: boolean;       // NEW — skip draft persistence for encrypted DMs
}
```

Both `Main.tsx` and `DMConversation.tsx` pass the new props:
- `Main.tsx`: `<MessageInput channelId={...} channelName={...} guildId={guildsState.activeGuildId} />`
- `DMConversation.tsx`: `<MessageInput channelId={...} channelName={...} isE2EE={isEncrypted()} />`

**Draft load/save with channel-switch handling:**
```typescript
// Track previous channel's E2EE status separately — props.isE2EE reflects
// the NEW channel when the effect fires, not the previous one
let prevIsE2EE = props.isE2EE ?? false;

// Track previous channelId to save draft before switching
createEffect((prevChannelId: string | undefined) => {
  // Save current content to previous channel before switching
  // Note: This may duplicate the last keystroke save, but ensures
  // drafts are never lost if user switches immediately after typing.
  if (prevChannelId && prevChannelId !== props.channelId) {
    const currentContent = content();
    if (currentContent.trim()) {
      saveDraft(prevChannelId, currentContent, prevIsE2EE); // Use PREVIOUS E2EE status
    } else {
      clearDraft(prevChannelId);
    }
  }

  // Load draft for new channel
  const draft = getDraft(props.channelId);
  setContent(draft);

  // Reset textarea height for new channel
  if (textareaRef) {
    requestAnimationFrame(() => autoResize(textareaRef!));
  }

  prevIsE2EE = props.isE2EE ?? false; // Capture for next switch
  return props.channelId;
});

// In handleInput: save draft
const handleInput = (value: string) => {
  setContent(value);
  saveDraft(props.channelId, value, props.isE2EE);
  // ... existing typing indicator logic
};

// In handleSubmit — clear draft ONLY on success (inside try block):
try {
  // ... send logic ...
  setContent("");
  clearDraft(props.channelId); // Only after successful send
  if (textareaRef) {
    textareaRef.style.height = "auto";
  }
} catch (err) {
  // Draft stays intact so user can retry
}
```

#### Logout Cleanup

Clear drafts on logout to prevent leaking content between accounts on shared machines. In `stores/auth.ts`'s logout function:
```typescript
import { clearAllDrafts } from "@/stores/drafts";

export function logout() {
  // ... existing logout logic ...
  clearAllDrafts();
}
```

#### Pending Files

Pending files are **not** persisted in drafts. File blobs can't be serialized to localStorage, and object URLs are session-scoped. Files are cleared on channel switch (existing behavior).

---

## 3. Quick Message Actions (Hover Toolbar)

### Problem

The only way to react to a message is right-clicking (context menu) or clicking the small `SmilePlus` icon that only appears when there are no existing reactions. There are no quick-access common reactions.

### Design

A floating toolbar that appears on message hover, positioned at the top-right corner of the message.

#### UI Layout

```
                                        ┌──────────────────────────┐
                                        │ 👍  ❤️  😂  😮  [+] [⋯] │
                                        └──────────────────────────┘
┌──────────────────────────────────────────────────────────────────────┐
│ [avatar]  Username            12:34 PM                               │
│           Message content here...                                    │
│           [👍 3] [❤️ 1]                                              │
└──────────────────────────────────────────────────────────────────────┘
```

#### Component: `MessageActions.tsx`

```
client/src/components/messages/MessageActions.tsx
```

**Props:**
```typescript
const DEFAULT_QUICK_EMOJIS = ["👍", "❤️", "😂", "😮"];

interface MessageActionsProps {
  message: Message;
  guildId?: string;
  quickEmojis?: string[];  // Customizable, defaults to DEFAULT_QUICK_EMOJIS
  onReaction: (emoji: string) => void;
  // Future: onReply, onEdit, onPin
}
```

**Behavior:**
- Appears on parent `group-hover` (CSS-only, no JS state)
- Position: `absolute top-0 right-4 -translate-y-1/2`
- Background: `bg-surface-layer2 border border-white/10 rounded-lg shadow-xl`
- Quick emojis: `👍`, `❤️`, `😂`, `😮` (defaults, accepted as prop for future guild customization)
- `[+]` button opens the existing `PositionedEmojiPicker`
- `[⋯]` button opens the existing context menu (same items as right-click)
- Click a quick emoji → calls `addReaction(channelId, messageId, emoji)`
- If user already reacted with that emoji, click removes it (toggle behavior matching `ReactionBar`)

**Z-index:** `z-20` — above message content and any stacking contexts, below modals/pickers (z-9999).

#### Integration in MessageItem.tsx

Remove the entire `<Show when={!hasReactions()}>` block (lines 360-381) which contains the standalone SmilePlus button. Replace with the always-present toolbar:

```tsx
{/* Hover action toolbar — always present, replaces standalone SmilePlus */}
<div class="absolute top-0 right-4 -translate-y-1/2 opacity-0 group-hover:opacity-100 transition-opacity z-20">
  <MessageActions
    message={props.message}
    guildId={props.guildId}
    onReaction={handleAddReaction}
  />
</div>
```

The message's outer `<div>` needs `relative` added for absolute positioning:
```tsx
<div
  onContextMenu={handleContextMenu}
  class={`group relative flex gap-4 px-4 py-0.5 hover:bg-white/3 transition-colors ${
    props.compact ? "mt-0" : "mt-4"
  }`}
>
```

#### Edge Cases

- **First message in list:** Toolbar may clip above the `overflow-y: auto` message list container. Known issue for MVP — acceptable because the message background highlight on hover provides enough visual feedback. Fix in follow-up with `floating-ui` if user feedback warrants it.
- **Mobile/touch:** Not applicable (Tauri desktop only). Touch targets are not a concern.
- **Keyboard navigation:** Not in MVP. Toolbar is mouse-only.

---

## 4. Smart Input Auto-complete

### Problem

Typing `@username` or `:emoji_name:` doesn't offer suggestions. Users must remember exact names.

### Design

A popup that appears above the input when a trigger character is typed, showing filtered suggestions.

#### Triggers

| Prefix | Source | Max results |
|--------|--------|-------------|
| `@` | Guild members (`getGuildMembers(guildId)`) or DM participants | 8 |
| `:` | Emoji search (`searchEmojis()` from `@/lib/emojiData` + guild custom emojis) | 8 |

#### Generic Popup List Base

Extract a reusable `PopupList` component that handles keyboard navigation and floating-ui positioning. This avoids duplicating navigation logic when `#channel` and `/command` triggers are added later. `CommandPalette.tsx` already has similar patterns — future refactoring can share this base.

```
client/src/components/ui/PopupList.tsx
```

```typescript
interface PopupListProps<T> {
  items: T[];
  anchorEl: HTMLElement;
  renderItem: (item: T, isSelected: boolean) => JSXElement;
  onSelect: (item: T) => void;
  onClose: () => void;
  maxVisible?: number; // Default 8
  ref?: (handle: PopupListHandle) => void; // Imperative handle for parent
}

// Imperative API — parent (MessageInput) calls these from its keydown handler
export interface PopupListHandle {
  navigateDown: () => void;
  navigateUp: () => void;
  selectCurrent: () => void;
}
```

**Internal state:**
```typescript
const ITEM_HEIGHT = 40; // px — consistent with existing list item heights
const [selectedIndex, setSelectedIndex] = createSignal(0);

// Reset selection when items change
createEffect(() => {
  items(); // track
  setSelectedIndex(0);
});

// Expose imperative handle to parent
props.ref?.({
  navigateDown: () => setSelectedIndex((i) => Math.min(i + 1, props.items.length - 1)),
  navigateUp: () => setSelectedIndex((i) => Math.max(i - 1, 0)),
  selectCurrent: () => {
    const item = props.items[selectedIndex()];
    if (item) props.onSelect(item);
  },
});
```

**Scroll behavior:**

When `items.length > maxVisible`, the container has a fixed height (`maxVisible * ITEM_HEIGHT`) with `overflow-y: auto`. Arrow key navigation auto-scrolls the selected item into view:

```typescript
createEffect(() => {
  const index = selectedIndex();
  const listEl = listRef;
  if (!listEl) return;

  const itemTop = index * ITEM_HEIGHT;
  const itemBottom = itemTop + ITEM_HEIGHT;

  if (itemTop < listEl.scrollTop) {
    listEl.scrollTop = itemTop;
  } else if (itemBottom > listEl.scrollTop + listEl.clientHeight) {
    listEl.scrollTop = itemBottom - listEl.clientHeight;
  }
});
```

**Empty state:** PopupList does not render when `items` is empty. This is the caller's responsibility — `AutocompletePopup` wraps PopupList in `<Show when={filteredItems().length > 0}>`.

**Positioning & rendering:**
- Positioned above the anchor using `@floating-ui/dom` with `flip` + `shift` + `offset` middleware (same pattern as `PositionedEmojiPicker.tsx`)
- Rendered in a Solid.js `Portal` (prevents parent container clipping)
- Click-outside detection closes the popup
- Smooth fade-in transition (150ms, matching existing picker animation)
- Z-index: `z-9999` (same as `PositionedEmojiPicker`)

**Keyboard delegation:** PopupList does NOT attach its own `keydown` listener. The parent component (MessageInput) intercepts keyboard events in its `handleKeyDown`, calls `preventDefault()`, and delegates via the imperative `PopupListHandle` ref. This avoids event listener conflicts and keeps keyboard ownership in one place.

#### Component: `AutocompletePopup.tsx`

Thin wrapper around `PopupList` that provides data source and item renderer for each trigger type.

```
client/src/components/messages/AutocompletePopup.tsx
```

**Props:**
```typescript
interface AutocompletePopupProps {
  query: string;
  type: "user" | "emoji";
  anchorEl: HTMLElement;
  onSelect: (value: string, display: string) => void;
  onClose: () => void;
  guildId?: string;
  dmParticipants?: DMParticipant[]; // For DM channels
}
```

**Filtering logic (createMemo inside AutocompletePopup):**
```typescript
const filteredItems = createMemo(() => {
  const q = props.query.toLowerCase();
  if (props.type === "user") {
    const source = props.guildId
      ? getGuildMembers(props.guildId)
      : (props.dmParticipants ?? []);
    // Prefix matches first, then contains matches, online users prioritized
    const matches = source.filter((u) =>
      u.username.toLowerCase().includes(q) ||
      u.display_name.toLowerCase().includes(q) ||
      u.nickname?.toLowerCase().includes(q)
    );
    matches.sort((a, b) => {
      // Prefix match > contains match
      const aPrefix = a.username.toLowerCase().startsWith(q) || a.display_name.toLowerCase().startsWith(q);
      const bPrefix = b.username.toLowerCase().startsWith(q) || b.display_name.toLowerCase().startsWith(q);
      if (aPrefix !== bPrefix) return aPrefix ? -1 : 1;
      // Online > offline
      if (a.status !== b.status) return a.status === "online" ? -1 : 1;
      return 0;
    });
    return matches.slice(0, 8);
  } else {
    // Standard Unicode emojis
    const standard = searchEmojis(q).map((char) => ({ type: "standard" as const, char, name: char }));
    // Guild custom emojis
    const custom = props.guildId
      ? getGuildEmojis(props.guildId)
          .filter((e) => e.name.toLowerCase().includes(q))
          .map((e) => ({ type: "custom" as const, char: `:${e.name}:`, name: e.name, imageUrl: e.image_url }))
      : [];
    return [...custom, ...standard].slice(0, 8);
  }
});
```

**Empty state guard:** AutocompletePopup handles the empty case — when `filteredItems().length === 0`, PopupList is not rendered. The popup disappears when the query matches nothing and reappears when results return.

```tsx
<Show when={filteredItems().length > 0}>
  <PopupList
    items={filteredItems()}
    anchorEl={props.anchorEl}
    renderItem={renderItem}
    onSelect={handleSelect}
    onClose={props.onClose}
    ref={setPopupListRef}  // Forward handle to MessageInput
  />
</Show>
```

**Ref forwarding:** AutocompletePopup receives the `PopupListHandle` from PopupList and exposes it to MessageInput. MessageInput holds `let popupListRef: PopupListHandle | undefined` and passes a setter through AutocompletePopup.

**Item rendering:**
- **Users:** Avatar (16px) + display_name + `@username` in muted text
- **Standard emojis:** Unicode character (24px) + `:shortcode:` name
- **Custom guild emojis:** `<img>` (24px) + `:name:` — custom emojis have `imageUrl` instead of a Unicode character

#### Query Extraction Logic (in MessageInput)

Trigger characters must be preceded by whitespace, start-of-string, or newline to avoid false positives from URLs (`https://...`) or timestamps (`12:30`):

```typescript
function getAutocompleteContext(text: string, cursorPos: number): {
  type: "user" | "emoji";
  query: string;
  startPos: number;
} | null {
  const beforeCursor = text.slice(0, cursorPos);

  // @user — trigger at word boundary
  const atMatch = beforeCursor.match(/(?:^|\s)@(\w*)$/);
  if (atMatch) {
    // startPos points to the @ character, not the preceding whitespace
    const fullMatchLen = atMatch[0].length;
    const triggerLen = atMatch[1].length + 1; // +1 for @
    return {
      type: "user",
      query: atMatch[1],
      startPos: cursorPos - triggerLen,
    };
  }

  // :emoji: — min 2 chars after colon, trigger at word boundary
  const colonMatch = beforeCursor.match(/(?:^|\s):(\w{2,})$/);
  if (colonMatch) {
    const triggerLen = colonMatch[1].length + 1; // +1 for :
    return {
      type: "emoji",
      query: colonMatch[1],
      startPos: cursorPos - triggerLen,
    };
  }

  return null;
}
```

**Cursor position tracking:**
```typescript
const handleTextareaInput = (e: InputEvent & { currentTarget: HTMLTextAreaElement }) => {
  const value = e.currentTarget.value;
  const cursorPos = e.currentTarget.selectionStart ?? value.length;

  setContent(value);
  saveDraft(props.channelId, value, props.isE2EE);
  scheduleResize(e.currentTarget);

  // Update autocomplete context
  const ctx = getAutocompleteContext(value, cursorPos);
  setAutocompleteContext(ctx);

  // ... existing typing indicator logic
};
```

#### Insertion Logic

On select, replace the trigger + query with the result, and persist as draft:
```typescript
function insertCompletion(
  text: string,
  startPos: number,
  cursorPos: number,
  replacement: string
): { newText: string; newCursorPos: number } {
  const before = text.slice(0, startPos);
  const after = text.slice(cursorPos);
  const newText = before + replacement + after;
  return { newText, newCursorPos: before.length + replacement.length };
}

// After insertion:
const { newText, newCursorPos } = insertCompletion(...);
setContent(newText);
saveDraft(props.channelId, newText, props.isE2EE);
setAutocompleteContext(null);

// Restore cursor position after Solid's reactive DOM flush
// queueMicrotask runs after the current microtask queue (including Solid's sync flush)
// but before the next paint frame — more reliable than requestAnimationFrame
queueMicrotask(() => {
  textareaRef?.setSelectionRange(newCursorPos, newCursorPos);
  textareaRef?.focus();
});
```

**User mention flow:**
1. User types `@da` in message input
2. Popup appears with members whose `username` or `display_name` starts with or contains `"da"` (case-insensitive)
3. User selects "Daniel" → text replaced: `@da` becomes `@Daniel ` (with trailing space)
4. On send, server already parses `@username` for `mention_type` — no backend changes needed

**Emoji shortcode flow:**
1. User types `:thu` in message input
2. Popup appears with emojis matching `"thu"` (e.g., 👍 thumbsup, 👎 thumbsdown)
3. User selects 👍 → text replaced: `:thu` becomes `👍` (Unicode character)
4. For custom guild emojis: `:thu` becomes `:custom_emoji_name:` (server resolves on render)

#### Data Sources

**Users (guilds):** `getGuildMembers(guildId)` from `stores/guilds.ts` — already loaded when entering a guild. `GuildMember` has `user_id`, `username`, `display_name`, `avatar_url`, `nickname`, `status`. Filter on `username`, `display_name`, and `nickname`.

**Users (DMs):** `DMParticipant[]` from the DM channel data. `DMConversation.tsx` already has `dm().participants`. Pass as `dmParticipants` prop to `MessageInput`, which forwards to `AutocompletePopup`.

**Emojis:** Use `searchEmojis(query)` from `@/lib/emojiData` for standard Unicode emojis (returns `string[]`). For guild custom emojis, use `getGuildEmojis(guildId)` from `stores/emoji.ts` and filter on `name`.

#### Filtering

- **Users:** Case-insensitive match against `username`, `display_name`, and `nickname`. Prioritize prefix matches over contains matches. Sort online users first.
- **Emojis:** `searchEmojis(query)` from `@/lib/emojiData` handles standard emoji search. Guild custom emojis filtered separately by name substring match.

---

## Component Dependency Map

```
MessageInput.tsx (modified)
├── stores/drafts.ts (NEW)
├── AutocompletePopup.tsx (NEW)
│   ├── ui/PopupList.tsx (NEW — generic keyboard nav + floating-ui)
│   ├── stores/guilds.ts (read: getGuildMembers)
│   ├── lib/emojiData.ts (read: searchEmojis)
│   ├── stores/emoji.ts (read: getGuildEmojis)
│   └── @floating-ui/dom (existing dep)
└── <textarea> (replaces <input>)

MessageItem.tsx (modified)
├── MessageActions.tsx (NEW)
│   ├── PositionedEmojiPicker (existing)
│   ├── tauri.ts (addReaction/removeReaction)
│   └── ContextMenu (existing: showContextMenu)
└── Remove standalone SmilePlus button + <Show when={!hasReactions()}> block

MessageInputProps (extended)
├── guildId?: string (NEW — for autocomplete)
├── isE2EE?: boolean (NEW — draft security)
└── dmParticipants?: DMParticipant[] (NEW — DM autocomplete)
```

---

## Build Sequence

Each step is independently shippable and testable:

### Step 1: Multi-line Input
- Replace `<input>` with `<textarea>` in `MessageInput.tsx`
- Add auto-resize logic with `requestAnimationFrame` batching
- Add IME composition handling (`compositionstart`/`compositionend`)
- Verify Enter sends, Shift+Enter inserts newline
- **Files:** `MessageInput.tsx`
- **Risk:** Low — isolated change, no new components

### Step 2: Persistent Drafts
- Create `stores/drafts.ts` with timestamped entries and LRU eviction
- Add `guildId`, `isE2EE` props to `MessageInputProps`
- Wire into `MessageInput.tsx`:
  - `createEffect` with previous channelId tracking
  - Save on input, clear on successful send only
  - Skip persistence for E2EE DMs
- Add `beforeunload` flush and logout cleanup (`clearAllDrafts`)
- Update `Main.tsx` and `DMConversation.tsx` to pass new props
- **Files:** `stores/drafts.ts` (new), `MessageInput.tsx`, `Main.tsx`, `DMConversation.tsx`, `stores/auth.ts`
- **Risk:** Low — additive, no breaking changes
- **Note:** Step 4 (autocomplete) must call `saveDraft` after `insertCompletion` — document as wiring requirement

### Step 3: Quick Message Actions
- Create `MessageActions.tsx` with quick emoji buttons, picker trigger, context menu trigger
- Integrate into `MessageItem.tsx`:
  - Add `relative` to message outer div
  - Remove entire `<Show when={!hasReactions()}>` block (SmilePlus)
  - Add `MessageActions` in absolute-positioned wrapper
- **Files:** `MessageActions.tsx` (new), `MessageItem.tsx`
- **Risk:** Medium — CSS positioning on first/last messages, need to test scrolled views

### Step 4: Auto-complete
- Create `PopupList.tsx` (generic keyboard nav + floating-ui positioning)
- Create `AutocompletePopup.tsx` (user/emoji data sources + item renderers)
- Add `dmParticipants` prop to `MessageInputProps`
- Add `getAutocompleteContext()` and `insertCompletion()` to `MessageInput.tsx`
- Wire keyboard intercept in `handleKeyDown` (ArrowUp/Down/Enter/Tab/Escape)
- Update `DMConversation.tsx` to pass `dmParticipants`
- **Files:** `PopupList.tsx` (new), `AutocompletePopup.tsx` (new), `MessageInput.tsx`, `DMConversation.tsx`
- **Risk:** Medium — cursor position management in textarea, floating-ui positioning above input

---

## File Inventory

| File | Action | Description |
|------|--------|-------------|
| `client/src/stores/drafts.ts` | **Create** | Draft persistence store with LRU eviction |
| `client/src/components/ui/PopupList.tsx` | **Create** | Generic keyboard-navigable popup list |
| `client/src/components/messages/MessageActions.tsx` | **Create** | Hover action toolbar |
| `client/src/components/messages/AutocompletePopup.tsx` | **Create** | Auto-complete popup (user/emoji) |
| `client/src/components/messages/MessageInput.tsx` | **Modify** | textarea, drafts, autocomplete, IME |
| `client/src/components/messages/MessageItem.tsx` | **Modify** | Add MessageActions, remove SmilePlus |
| `client/src/views/Main.tsx` | **Modify** | Pass `guildId` to MessageInput |
| `client/src/components/home/DMConversation.tsx` | **Modify** | Pass `isE2EE`, `dmParticipants` to MessageInput |
| `client/src/stores/auth.ts` | **Modify** | Call `clearAllDrafts()` on logout |

**No backend changes required.** All features are client-only. The reaction API, member list API, and emoji data already exist.

---

## Testing Strategy

### Unit Tests
- `drafts.ts`: save/load/clear/eviction logic, E2EE skip, `structuredClone` serialization
- `getAutocompleteContext()`: trigger detection with various cursor positions, false-positive rejection (URLs, timestamps, mid-word colons)
- `insertCompletion()`: text replacement correctness, cursor position accuracy
- `autoResize()`: height calculation with mock textarea

### Integration Tests (Manual)
- Type in channel A, switch to B, switch back → draft restored
- Type in E2EE DM, switch away, switch back → draft NOT restored (in-memory only)
- Hover message → toolbar appears, click 👍 → reaction added, click again → removed
- Type `@us` → popup shows members, arrow-select + Enter → mention inserted
- Type `:thumbs` → popup shows 👍👎, select → emoji inserted
- Type `12:30` → no popup (false positive prevention)
- Type `https://example.com` → no popup
- Shift+Enter inserts newline, auto-resize grows, Enter sends
- CJK IME: Enter confirms character composition, does not send message
- Close app while typing → reopen → draft preserved (beforeunload flush)
- Logout → login as different user → no stale drafts

---

## Performance Considerations

- **Drafts:** Debounced localStorage writes (300ms) with `beforeunload` flush — no impact on typing latency. `unwrap()` + `structuredClone` to strip Solid proxy before `JSON.stringify`.
- **Auto-complete:** Filtering happens on existing in-memory arrays (members, emojis) — sub-millisecond. Guild members already loaded on guild entry.
- **Hover toolbar:** Pure CSS visibility toggle (opacity/pointer-events) — no JS on hover.
- **Auto-resize:** Single `scrollHeight` read per frame (batched via `requestAnimationFrame`) — negligible even for long messages.
- **Eviction:** LRU sort only runs when `MAX_DRAFTS` (50) is exceeded — rare, O(n log n) on 50 items is instant.
