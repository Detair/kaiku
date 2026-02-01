# Production-Scale Polish - Design

> **Status:** Draft
> **Date:** 2026-02-01
> **Branch:** `feature/production-polish`

## Overview

Upgrade the message list to handle 10,000+ message histories efficiently with virtual scrolling, infinite scroll pagination, and configurable memory eviction. Add a server admin dashboard for storage/memory overview. The toast notification system is already complete and requires no further work.

---

## Current State

### Message List (`MessageList.tsx`)
- Loads **50 messages** on channel open (`MESSAGE_LIMIT = 50`)
- All messages rendered in DOM simultaneously (no windowing)
- Store has `loadMessages()` with cursor-based pagination (`before` param) but **not wired to UI**
- Auto-scroll to bottom on new messages, "N new messages" floating button when scrolled up
- Message grouping via `shouldGroupWithPrevious()` (same author within 5 minutes → compact)
- No `IntersectionObserver` for scroll-to-load-more

### Toast System (`Toast.tsx`) — COMPLETE
- 4 types (info/success/warning/error), auto-dismiss, action buttons, deduplication, max 5 visible
- Already integrated in `App.tsx` and used across the codebase
- **No work needed.**

---

## Scope

### In Scope
1. **Virtual scrolling** — only render visible messages + buffer
2. **Infinite scroll (upward)** — load older messages when scrolling near top
3. **Scroll position preservation** — when prepending older messages, maintain viewport position
4. **Jump-to-bottom** — existing "new messages" button behavior preserved
5. **Variable-height items** — messages have varying heights (compact, attachments, code blocks)
6. **Configurable memory eviction** — drop messages far from viewport when too many are loaded
7. **Admin storage dashboard** — server-side overview of message volume and storage consumption

### Out of Scope
- Jump-to-message (search result linking) — deferred to Advanced Search feature
- Message caching/offline — deferred
- Lazy rendering of markdown/code blocks within individual messages

---

## Technical Approach

### Library: `@tanstack/solid-virtual`

**Why:**
- Official Solid.js adapter from TanStack (well-maintained, large community)
- Handles variable-height items with dynamic measurement
- No assumptions about item height — measures after render
- Small bundle (~3KB gzipped)
- Used successfully in production by many Solid.js apps

**Alternatives considered:**
| Library | Verdict |
|---------|---------|
| `solid-virtual` (community) | Less maintained, fewer features |
| Custom `IntersectionObserver` | Only solves pagination, not DOM reduction |
| CSS `content-visibility: auto` | Browser paints all items, just skips rendering offscreen — not enough for 10k+ |

### Architecture

```
MessageList (container, role="list")
├─ Sentinel div (top) — IntersectionObserver triggers loadMore
├─ "Beginning of conversation" marker (when hasMore = false)
├─ Virtualizer (from @tanstack/solid-virtual)
│   ├─ Overscan buffer (top, ~5 items)
│   ├─ Visible window (~20-30 items, role="listitem" each)
│   └─ Overscan buffer (bottom, ~5 items)
├─ "New messages" button (unchanged, fixed positioned)
└─ Scroll-to-bottom anchor
```

### Key Behaviors

#### 1. Initial Load
- Load 50 messages (unchanged)
- Scroll to bottom via `scrollToIndex(count - 1, { align: "end" })`
- Virtualizer renders only visible subset (~30 DOM nodes instead of 50)

#### 2. Scrolling Up — Load Older Messages
- `IntersectionObserver` on a sentinel element near the top of the list
- Observer is created inside a `createEffect` keyed on `props.channelId` (not `onMount`), so it reconnects on every channel switch
- A local `isLoadingMore` boolean (not reactive) acts as a synchronous guard against double-fires
- When sentinel enters viewport and `hasMore[channelId]` is true:
  1. Set `isLoadingMore = true`
  2. Record the index of the topmost visible message and its pixel offset within the viewport
  3. Call `loadMessages(channelId)` (existing store function, fetches 50 older)
  4. Messages prepended to store array
  5. Restore scroll position using index-based approach (see Step 4)
  6. Set `isLoadingMore = false`

#### 3. New Message Arrives (Bottom)
- If user is at bottom → auto-scroll to new message (unchanged)
- If user is scrolled up → increment "new messages" counter (unchanged)
- Virtualizer handles the new item automatically

#### 4. Variable Heights
- `@tanstack/solid-virtual` measures each item after initial render using `ResizeObserver`
- Provide a smart `estimateSize` callback that checks the message content (see Step 2)
- Measured heights cached per message ID for stable scrolling

#### 5. Stick-to-Bottom
- Track `isAtBottom` via scroll position (existing logic)
- After virtualizer updates, if was at bottom, scroll to new total height
- Use `scrollToIndex(messages.length - 1, { align: "end" })` from virtualizer API

---

## Implementation Plan

### Step 1: Add Dependency
```bash
bun add @tanstack/solid-virtual
```

### Step 2: Refactor `MessageList.tsx`

Replace the `<For>` loop with a virtualizer.

**Virtualizer setup:**

```tsx
import { createVirtualizer } from "@tanstack/solid-virtual";

const virtualizer = createVirtualizer({
  get count() { return messagesWithCompact().length; },
  getScrollElement: () => containerRef ?? null,
  estimateSize: (index) => {
    const item = messagesWithCompact()[index];
    if (!item) return 96;
    const msg = item.message;

    let estimate = item.isCompact ? 48 : 96;

    // Images are tall (~320px from max-h-80)
    const hasImage = msg.attachments?.some(a =>
      a.content_type?.startsWith("image/")
    );
    if (hasImage) estimate = 400;

    // Code blocks add height
    if (msg.content.includes("```")) estimate = Math.max(estimate, 200);

    // Reactions add ~36px
    if (msg.reactions && msg.reactions.length > 0) estimate += 36;

    return estimate;
  },
  overscan: 5,
  measureElement: (el) => el.getBoundingClientRect().height,
});
```

The `estimateSize` doesn't need to be exact. It just needs to be close enough that the virtualizer doesn't visibly jump when the real measurement comes in. Getting within ~50% of the actual height is sufficient.

**Render structure:**

```tsx
<div
  ref={containerRef}
  class="flex-1 overflow-y-auto relative"
  role="list"
  aria-label="Messages"
>
  {/* Total height spacer */}
  <div style={{ height: `${virtualizer.getTotalSize()}px`, position: "relative" }}>
    <For each={virtualizer.getVirtualItems()}>
      {(virtualItem) => {
        const item = messagesWithCompact()[virtualItem.index];
        return (
          <div
            role="listitem"
            data-index={virtualItem.index}
            ref={(el) => virtualizer.measureElement(el)}
            style={{
              position: "absolute",
              top: `${virtualItem.start}px`,
              width: "100%",
            }}
          >
            <MessageItem
              message={item.message}
              compact={item.isCompact}
              guildId={props.guildId}
            />
          </div>
        );
      }}
    </For>
  </div>
</div>
```

### Step 3: Add Infinite Scroll (Upward Pagination)

The observer must be **re-created on every channel switch**, not just on mount. `MessageList` stays mounted when navigating between channels — it reacts via `createEffect` on `props.channelId`. If the observer is only set up in `onMount`, it will reference stale channel state after switching.

```tsx
let sentinelRef: HTMLDivElement | undefined;
let isLoadingMore = false; // synchronous guard, not reactive

createEffect(on(
  () => props.channelId,
  () => {
    if (!sentinelRef || !containerRef) return;

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (
          entry.isIntersecting &&
          hasMoreMessages(props.channelId) &&
          !loading() &&
          !isLoadingMore
        ) {
          triggerLoadMore();
        }
      },
      { root: containerRef, rootMargin: "200px 0px 0px 0px" }
    );
    observer.observe(sentinelRef);
    onCleanup(() => observer.disconnect());
  }
));
```

The `isLoadingMore` flag is set synchronously (not via store) because the store's reactive `loadingChannels` may not propagate fast enough to prevent a second `IntersectionObserver` fire in the same tick.

### Step 4: Scroll Position Preservation on Prepend

**The problem:** When 50 older messages are prepended, every existing message shifts its index by 50. Without adjustment, the viewport jumps to show completely different content.

**Why pixel-based restoration is wrong here:** In a virtualized list, `scrollHeight` is calculated from *estimated* sizes for items that haven't been rendered yet. When you prepend 50 unmeasured messages, the `scrollHeight` change reflects guesses, not real heights. As items get measured, the viewport drifts.

**The fix — work in index-space, not pixel-space:**

```tsx
async function triggerLoadMore() {
  isLoadingMore = true;

  // 1. Remember what the user is looking at right now:
  //    "Which message is at the top of the viewport, and how many
  //     pixels of it are scrolled past?"
  const topItem = virtualizer.getVirtualItems()[0];
  const topIndex = topItem?.index ?? 0;
  const topOffset = (containerRef?.scrollTop ?? 0) - (topItem?.start ?? 0);

  // 2. Load 50 older messages (prepended to the array)
  const prevCount = messagesWithCompact().length;
  await loadMessages(props.channelId);
  const addedCount = messagesWithCompact().length - prevCount;

  // 3. The message that was at index N is now at index N + addedCount.
  //    Tell the virtualizer to scroll there.
  if (addedCount > 0) {
    virtualizer.scrollToIndex(topIndex + addedCount, { align: "start" });

    // Fine-adjust by the pixel offset so the user sees the exact same
    // slice of the message they were reading before.
    requestAnimationFrame(() => {
      if (containerRef) {
        containerRef.scrollTop += topOffset;
      }
    });
  }

  isLoadingMore = false;
}
```

In plain English: before loading, we note "the user is looking at message #5, scrolled 30px into it." After loading 50 older messages, that same message is now #55. We scroll to #55 and fine-adjust by 30px. The user sees no jump.

### Step 5: Preserve Existing Behaviors
- `isAtBottom` detection → use virtualizer scroll offset + container height vs total size
- "New messages" button → `scrollToIndex(count - 1, { align: "end", behavior: "smooth" })`
- Initial scroll to bottom → `scrollToIndex(count - 1, { align: "end" })`
- "New messages" button uses `fixed` positioning — unaffected by absolute-positioned virtual items

### Step 6: Loading Indicator

Show a spinner at the top when fetching older messages:

```tsx
<Show when={loading() && messages().length > 0}>
  <div class="flex justify-center py-4 sticky top-0 z-10">
    <Loader2 class="w-5 h-5 text-text-secondary animate-spin" />
  </div>
</Show>
```

When `hasMore` is false and user has scrolled to the top, show a "Beginning of conversation" marker.

### Step 7: Memory Eviction (Configurable)

When a channel accumulates too many loaded messages, drop the ones farthest from the viewport to keep memory bounded.

**Client-side config (synced via user preferences):**

```typescript
// Default: 2000 messages per channel before eviction kicks in
const MAX_MESSAGES_PER_CHANNEL = 2000;
// Keep this many messages around the viewport when evicting
const EVICTION_KEEP_WINDOW = 500;
```

**How it works:**
1. After each successful `loadMessages`, check if `messages.length > MAX_MESSAGES_PER_CHANNEL`
2. If yes, find the current viewport center index
3. Keep messages within `EVICTION_KEEP_WINDOW / 2` of the viewport center
4. Drop messages outside that window
5. Set `hasMore = true` for the evicted direction (so scrolling back triggers a re-fetch)

**Timing constraint:** Eviction must NOT run in the same tick as scroll restoration (Step 4). After `triggerLoadMore` completes its `requestAnimationFrame` callback for scroll adjustment, only then check and run eviction. Use a second `requestAnimationFrame` or `setTimeout(0)` after the scroll restore finishes. If both run simultaneously, the virtualizer recalculates positions mid-scroll-restore and causes a visible jump.

**Not during initial load:** Eviction only applies after pagination, not during `loadInitialMessages`. The initial 50 messages are always below the threshold.

**When messages are evicted from the top:** The user scrolls down, old history at the top is dropped. If they scroll back up, the `IntersectionObserver` triggers a fresh fetch — the server returns those messages again.

**When messages are evicted from the bottom:** The user scrolls up through history, recent messages at the bottom are dropped. When they scroll back down or click "new messages", a fresh load restores them.

**Why not evict at the store level automatically?** The virtualizer already only renders ~30 DOM nodes regardless of array size. Eviction is about capping *memory* (JS heap for message objects + attachment metadata), not DOM pressure. For most users this never triggers — 2000 messages at ~1KB each is only ~2MB. The setting exists for admins who want to enforce tighter limits on low-memory devices.

**Server admin configurable default:** The server can push a recommended `max_messages_per_channel` value via the existing server config endpoint. Individual users can override it upward or downward in their settings.

### Step 8: Admin Storage Dashboard

Add a "Storage" section to the existing Admin Dashboard with an overview of message volume and space consumption, so admins can plan capacity.

**Backend — new API endpoint:**

```
GET /api/admin/storage/overview
```

Response:
```json
{
  "total_messages": 847293,
  "total_attachments": 12847,
  "total_attachment_size_bytes": 5368709120,
  "channels": [
    {
      "channel_id": "...",
      "channel_name": "general",
      "guild_name": "My Server",
      "message_count": 45230,
      "attachment_count": 892,
      "total_attachment_size_bytes": 524288000,
      "oldest_message": "2025-06-15T...",
      "newest_message": "2026-02-01T..."
    }
  ],
  "guilds": [
    {
      "guild_id": "...",
      "guild_name": "My Server",
      "total_messages": 128000,
      "total_attachment_size_bytes": 2147483648,
      "member_count": 350,
      "custom_emoji_count": 45
    }
  ]
}
```

**Database queries:**
```sql
-- Message counts per channel (paginated, top 100 by volume)
SELECT c.id, c.name, g.name as guild_name,
       COUNT(m.id) as message_count,
       MIN(m.created_at) as oldest_message,
       MAX(m.created_at) as newest_message
FROM channels c
LEFT JOIN messages m ON m.channel_id = c.id
LEFT JOIN guilds g ON c.guild_id = g.id
GROUP BY c.id, c.name, g.name
ORDER BY message_count DESC
LIMIT 100 OFFSET $1;

-- Attachment sizes per channel (table: file_attachments, column: size_bytes)
SELECT m.channel_id,
       COUNT(fa.id) as attachment_count,
       COALESCE(SUM(fa.size_bytes), 0) as total_size
FROM file_attachments fa
JOIN messages m ON fa.message_id = m.id
GROUP BY m.channel_id;
```

**Note:** `guild_emojis` has no `size_bytes` column — emoji images are referenced by URL. Emoji counts are available but sizes are not queryable from the database. Use `custom_emoji_count` only.

**Auth:** Requires `Extension<ElevatedAdmin>` guard (same pattern as existing admin endpoints in `admin_handlers.rs`). Non-admin requests get 403.

**Frontend — Admin Dashboard tab:**
- Summary cards: total messages, total storage, average per guild
- Table: channels sorted by message count (descending), with storage bars
- Guild breakdown with member count for context
- Refresh button (these queries can be slow on large instances)
- Consider caching results in Valkey with 5-minute TTL for large instances

---

## Verified Integration Points

These were checked against the actual codebase and are confirmed safe:

| Component | Location | Risk | Notes |
|-----------|----------|------|-------|
| **MessageItem CSS** | `MessageItem.tsx` | None | Uses flexbox, `group` hover — all self-contained, works with absolute positioning |
| **Message grouping** | `utils.ts:83-98` | None | Pure data comparison on array indices, no DOM dependency |
| **Context menus** | `ContextMenu.tsx` | None | Portaled with `fixed` positioning from mouse coordinates |
| **Emoji picker** | `PositionedEmojiPicker.tsx` | None | Uses `@floating-ui/dom` + `Portal` + `fixed` position — handles absolutely-positioned anchors correctly |
| **Reactions** | `ReactionBar.tsx` | None | Same floating-ui pattern as emoji picker |
| **TypingIndicator** | `Main.tsx:74-81` | None | Placed *outside* MessageList as a sibling — not inside the scroll container |
| **"New messages" button** | `MessageList.tsx:201` | None | Uses `fixed` positioning — floats above everything regardless of virtualizer |

---

## Known Behavior Changes

### Spoiler state resets on scroll

`MessageItem.tsx:68-87` attaches spoiler click handlers in `onMount`. In a virtualized list, items unmount when scrolled off-screen and remount when scrolled back. This means a spoiler the user clicked to reveal will re-hide if they scroll away and come back.

**Accepted trade-off for now.** If this causes complaints, the fix is a `Set<messageId>` signal tracking which spoilers the user has revealed, checked in `MessageItem` on mount.

### E2EE decryption adds latency to pagination

When loading 50 older messages, `decryptMessages()` runs before they're added to the store. This adds 50-200ms on top of the network fetch. Users see the loading spinner during this time. There is **no flicker** of encrypted placeholder text — decryption finishes before the messages become visible.

---

## Edge Cases

| Scenario | Handling |
|----------|----------|
| Channel switch | `createEffect` on `channelId` resets virtualizer, reconnects observer, clears scroll state, loads initial 50 |
| Message deleted | Remove from array, virtualizer auto-adjusts |
| Message edited | Update in-place, height may change → `measureElement` re-measures |
| Very long messages (code blocks) | Dynamic measurement handles variable heights |
| Empty channel | Existing empty state (unchanged) |
| Rapid scroll to top | `isLoadingMore` synchronous guard + store `loadingChannels` guard prevents duplicate fetches |
| Network error during pagination | Show error toast, keep existing messages, user can retry by scrolling up again |
| Memory eviction then scroll back | `hasMore` set to true for evicted direction, `IntersectionObserver` triggers re-fetch |

---

## Open Questions — Resolved by Testing

### Will a WebSocket append + pagination prepend race?

If a new message arrives via WebSocket (`addMessage` appends to the end) while `loadMessages` is prepending older messages to the front, both write to `messagesState.byChannel[channelId]`. Both read `existing` and then create a new array.

The actual race is in `addMessage` (`messages.ts:234-246`), not `loadMessages`. `addMessage` reads `existing` on line 236, then `await`s decryption on line 244, then writes on line 245 using the stale `existing`. If `loadMessages` completes its write between `addMessage`'s read and write, the prepended messages are lost. (`loadMessages` already re-reads at line 162 after its own await, so it's not the problem.)

**Plan:** Write a targeted test before implementation:

```typescript
// test: concurrent-store-mutations.test.ts
//
// 1. Set up a channel with 50 messages in the store
// 2. Start loadMessages() (it's async — awaits network + decrypt)
// 3. While loadMessages is in-flight, call addMessage() with a new message
// 4. After both complete, verify:
//    - All 50 original messages are present
//    - The 50 older messages are prepended
//    - The 1 new message is appended
//    - No messages are lost
//
// If messages are lost, the fix is to re-read `existing` after the await
// in addMessage (move the read from line 236 to after line 244).
```

If the test shows lost messages, the fix is small: in `addMessage`, move the `const existing = messagesState.byChannel[channelId]` read to *after* the `await decryptMessageIfNeeded(message)` call, so it always sees the latest store state before writing.

### Channel switch will flash empty state (known, fix planned)

When switching channels, the message count goes from (say) 500 → 0 → 50. The user will see a brief empty/loading state.

**In plain language:** Imagine you're in `#general` with 500 messages loaded. You click `#random`. Three things happen:
1. The store clears `#general` messages (count drops to 0) — this is synchronous
2. The store starts loading `#random` messages — this is async (network + decrypt)
3. 50 `#random` messages arrive and count becomes 50

Between steps 1 and 3, the virtualizer sees 0 items and renders the empty/loading state. This happens today too (without the virtualizer), but adding virtual scrolling makes it more noticeable because the virtualizer resets its scroll position and measurements.

**Fix (apply during Step 2):** Change `loadInitialMessages` to load first, then swap — don't clear the old channel's messages until the new channel's messages are ready. In practice: remove the `setMessagesState("byChannel", channelId, [])` call on line 191, and let `loadMessages` overwrite instead of prepend when it detects a channel change (or use a separate "replace" code path).

---

## Performance Budget

| Metric | Target | Current |
|--------|--------|---------|
| DOM nodes (1000 messages loaded) | ~60-80 message elements | 1000 (all rendered) |
| Scroll jank (FPS) | 60 FPS sustained | 60 FPS (only 50 items) |
| Memory per channel | ~2MB for 1000 messages in store | ~200KB for 50 |
| Initial render | <100ms | ~50ms |
| Pagination fetch | <200ms (network) | N/A |
| Eviction threshold | Configurable, default 2000 msgs | N/A |

---

## Testing Plan

### Unit Tests
- Virtualizer renders correct number of items for given container size
- Smart `estimateSize` returns ~400 for image messages, ~200 for code blocks, ~48/96 for compact/full
- Scroll-to-bottom works after new message
- Infinite scroll triggers `loadMessages` when sentinel visible
- Eviction drops messages outside the keep window and re-enables `hasMore`

### Concurrency Test (new, resolves open question)
- Simulate concurrent `loadMessages` + `addMessage` on the same channel
- Verify no messages are lost after both operations complete

### Integration Tests
- Load channel → 50 messages displayed
- Scroll to top → older messages loaded, scroll position preserved
- Receive new message while scrolled up → counter shows, click scrolls to bottom
- Channel switch resets scroll and messages (no empty flash)
- `hasMore = false` → no more fetch attempts, "beginning of conversation" shown
- Load 2000+ messages → eviction triggers, scroll back re-fetches

### Manual Testing
- Load a channel with 500+ messages, scroll through entire history
- Verify no visible flicker or jump during pagination
- Verify messages with images/code blocks measure correctly
- Test on low-end hardware (throttle CPU in DevTools)
- Cross-message text selection (may degrade with absolute positioning)
- Screen reader navigation through virtualized messages

### Admin Dashboard Tests
- `/api/admin/storage/overview` returns correct counts
- Non-admin gets 403
- Large instance performance (query execution time)

---

## Risks & Mitigations

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Scroll position jumps on prepend | Medium | Index-based restoration (Step 4), not pixel-based |
| `ResizeObserver` loop warnings | Low | Use `requestAnimationFrame` batching for measurements |
| Memory growth with many loaded messages | Medium | Configurable eviction (Step 7), default 2000 per channel |
| `@tanstack/solid-virtual` Solid.js compat issues | Low | Library has official Solid adapter; fall back to vanilla `@tanstack/virtual-core` if needed |
| Store mutation races (WebSocket + pagination) | Low | Test first (see Open Questions), fix if needed by re-reading store after await |
| Channel switch empty flash | Certain | Fix during Step 2: load new channel first, then swap (see Open Questions) |
| Spoiler state reset | Low | Accepted trade-off, document for users. Fix later if complaints. |
| Cross-message text selection | Low | Test early. If broken with `position: absolute`, try `translateY` transforms instead |
| Admin storage query slow on large instances | Medium | Cache in Valkey with 5-minute TTL |

---

## Roadmap Update

After implementation, update `ROADMAP.md` Phase 5:
- Mark "Production-Scale Polish" → "Virtualized Message Lists" as complete
- Mark "Production-Scale Polish" → "Admin Storage Dashboard" as complete
- Note "Global Toast Notification Service" was already complete (Phase 4 era)
- Update completion percentage
