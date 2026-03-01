# Production-Scale Polish Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Upgrade the virtualizer to TanStack, expand virtualization to member list / DM sidebar / search results, and audit toast usage for consistency.

**Architecture:** Replace the custom 110-line virtualizer wrapper with `@tanstack/solid-virtual` (already installed). The TanStack virtualizer provides ResizeObserver-based dynamic sizing, proper scroll restoration, and battle-tested edge case handling. Each long list component gets virtualization added inline using the same wrapper. Toast audit is a cleanup pass with no architectural changes.

**Tech Stack:** @tanstack/solid-virtual v3.13.18, Solid.js, vitest, UnoCSS

---

### Task 1: Write virtualizer wrapper tests

**Files:**
- Create: `client/src/lib/__tests__/virtualizer.test.ts`

**Step 1: Write failing tests for the virtualizer API**

```typescript
import { describe, it, expect, vi, beforeEach } from "vitest";
import { createVirtualizer } from "../virtualizer";

function makeScrollElement(height = 500, scrollTop = 0): HTMLDivElement {
  const el = document.createElement("div");
  Object.defineProperties(el, {
    clientHeight: { value: height, configurable: true },
    scrollTop: { value: scrollTop, writable: true, configurable: true },
    scrollTo: { value: vi.fn(), configurable: true },
  });
  return el;
}

describe("createVirtualizer", () => {
  it("returns empty items when count is 0", () => {
    const v = createVirtualizer({
      count: 0,
      getScrollElement: () => makeScrollElement(),
      estimateSize: () => 50,
      overscan: 0,
    });
    expect(v.getVirtualItems()).toEqual([]);
    expect(v.getTotalSize()).toBe(0);
  });

  it("returns all items when they fit in viewport", () => {
    const el = makeScrollElement(500);
    const v = createVirtualizer({
      count: 5,
      getScrollElement: () => el,
      estimateSize: () => 50,
      overscan: 0,
    });
    const items = v.getVirtualItems();
    expect(items.length).toBe(5);
    expect(v.getTotalSize()).toBe(250);
  });

  it("applies overscan correctly", () => {
    const el = makeScrollElement(100, 200);
    const v = createVirtualizer({
      count: 100,
      getScrollElement: () => el,
      estimateSize: () => 50,
      overscan: 2,
    });
    const items = v.getVirtualItems();
    // Viewport at 200-300 covers items 4-5, overscan adds 2 each side
    expect(items[0].index).toBeLessThanOrEqual(2);
  });

  it("scrollToIndex calls scrollTo on the element", () => {
    const el = makeScrollElement(200);
    const v = createVirtualizer({
      count: 50,
      getScrollElement: () => el,
      estimateSize: () => 50,
      overscan: 0,
    });
    v.scrollToIndex(10, { align: "start" });
    expect(el.scrollTo).toHaveBeenCalled();
  });

  it("measureElement accepts an element without throwing", () => {
    const el = makeScrollElement();
    const v = createVirtualizer({
      count: 10,
      getScrollElement: () => el,
      estimateSize: () => 50,
      overscan: 0,
    });
    const item = document.createElement("div");
    item.setAttribute("data-index", "0");
    expect(() => v.measureElement(item)).not.toThrow();
  });

  it("getTotalSize reflects all items", () => {
    const v = createVirtualizer({
      count: 20,
      getScrollElement: () => makeScrollElement(),
      estimateSize: (i) => (i < 10 ? 50 : 100),
      overscan: 0,
    });
    expect(v.getTotalSize()).toBe(10 * 50 + 10 * 100);
  });
});
```

**Step 2: Run tests to verify they fail**

Run: `cd client && bun run test:run -- --reporter=verbose src/lib/__tests__/virtualizer.test.ts`
Expected: Tests should pass against the current custom implementation (this is a characterization test).

**Step 3: Commit**

```bash
git add client/src/lib/__tests__/virtualizer.test.ts
git commit -m "test(client): add virtualizer characterization tests"
```

---

### Task 2: Replace virtualizer internals with TanStack

**Files:**
- Modify: `client/src/lib/virtualizer.ts`

**Step 1: Rewrite virtualizer.ts to wrap @tanstack/solid-virtual**

Replace the entire file with:

```typescript
import {
  createVirtualizer as createTanStackVirtualizer,
  type VirtualItem,
} from "@tanstack/solid-virtual";

export type { VirtualItem };

interface VirtualizerOptions {
  count: number;
  getScrollElement: () => HTMLElement | null;
  estimateSize: (index: number) => number;
  overscan?: number;
}

interface ScrollToIndexOptions {
  align?: "start" | "center" | "end" | "auto";
  behavior?: ScrollBehavior;
}

export function createVirtualizer(options: VirtualizerOptions) {
  const virtualizer = createTanStackVirtualizer({
    get count() {
      return options.count;
    },
    getScrollElement: options.getScrollElement,
    estimateSize: options.estimateSize,
    overscan: options.overscan ?? 0,
  });

  return {
    getVirtualItems: (): VirtualItem[] => virtualizer.getVirtualItems(),
    getTotalSize: (): number => virtualizer.getTotalSize(),
    getScrollElement: options.getScrollElement,
    scrollToIndex: (index: number, scrollOptions: ScrollToIndexOptions = {}) => {
      virtualizer.scrollToIndex(index, scrollOptions);
    },
    measureElement: (node: Element | null | undefined) => {
      if (node) {
        virtualizer.measureElement(node as HTMLElement);
      }
    },
  };
}
```

**Step 2: Run the virtualizer tests**

Run: `cd client && bun run test:run -- --reporter=verbose src/lib/__tests__/virtualizer.test.ts`
Expected: All tests pass. TanStack's API matches the facade shape.

**Step 3: Run the full test suite to check for regressions**

Run: `cd client && bun run test:run`
Expected: All existing tests pass.

**Step 4: Commit**

```bash
git add client/src/lib/virtualizer.ts
git commit -m "refactor(client): replace custom virtualizer with @tanstack/solid-virtual"
```

---

### Task 3: Update MessageList to use TanStack measureElement

**Files:**
- Modify: `client/src/components/messages/MessageList.tsx`

The MessageList already calls `virtualizer.measureElement(el)` on each item's ref callback (line 354). With TanStack, this now actually works — ResizeObserver will track real sizes. No code change needed for the ref wiring.

However, TanStack's `getVirtualItems()` is reactive in Solid — it returns a reactive accessor. We need to verify that the `<For each={virtualizer.getVirtualItems()}>` pattern works correctly. Since our wrapper calls `virtualizer.getVirtualItems()` (the TanStack method), Solid's tracking should pick up changes.

**Step 1: Verify MessageList renders correctly**

Run: `cd client && bun run test:run`
Expected: All tests pass. No changes needed to MessageList.tsx — the facade API is identical.

**Step 2: Manual verification (if dev server available)**

Open a channel with messages. Scroll up/down. Verify:
- Messages render correctly with absolute positioning
- Scrolling is smooth
- Loading more messages (infinite scroll) preserves scroll position
- New message indicator works

**Step 3: Commit (only if changes were needed)**

If any adjustments are needed:
```bash
git add client/src/components/messages/MessageList.tsx
git commit -m "fix(client): adjust MessageList for TanStack virtualizer"
```

---

### Task 4: Virtualize MembersTab

**Files:**
- Modify: `client/src/components/guilds/MembersTab.tsx`

**Step 1: Add virtualizer to MembersTab**

The member list currently renders all items with `<For each={filteredMembers()}>` inside a `<div class="space-y-1">` (line 138-236). Replace with virtualized rendering.

Member rows have fixed height (~80px: avatar 40px + padding + info lines). Add to imports:

```typescript
import { createVirtualizer } from "@/lib/virtualizer";
```

Add a container ref and virtualizer inside the component (after the `filteredMembers` memo):

```typescript
let membersContainerRef: HTMLDivElement | undefined;

const virtualizer = createVirtualizer({
  get count() { return filteredMembers().length; },
  getScrollElement: () => membersContainerRef ?? null,
  estimateSize: () => 80,
  overscan: 5,
});
```

Replace the members list section (lines 129-236). The outer container needs a ref and fixed height. Replace:

```tsx
<div class="space-y-1">
  <For each={filteredMembers()}>
    {(member) => {
      // ... existing member rendering
    }}
  </For>
</div>
```

With:

```tsx
<div
  ref={membersContainerRef}
  class="flex-1 overflow-y-auto"
  style={{ "max-height": "calc(100vh - 200px)" }}
>
  <div style={{ height: `${virtualizer.getTotalSize()}px`, position: "relative" }}>
    <For each={virtualizer.getVirtualItems()}>
      {(virtualItem) => {
        const member = () => filteredMembers()[virtualItem.index];
        return (
          <div
            data-index={virtualItem.index}
            ref={(el) => virtualizer.measureElement(el)}
            style={{
              position: "absolute",
              top: `${virtualItem.start}px`,
              width: "100%",
            }}
          >
            <Show when={member()}>
              {/* Existing member row JSX — move the existing <div class="flex items-center gap-3 p-3 ..."> here unchanged */}
            </Show>
          </div>
        );
      }}
    </For>
  </div>
</div>
```

Keep all existing member rendering logic (avatar, status dot, role badges, etc.) inside the `<Show when={member()}>` block — just wrap it with the virtual positioning div.

**Step 2: Run tests**

Run: `cd client && bun run test:run`
Expected: All tests pass.

**Step 3: Commit**

```bash
git add client/src/components/guilds/MembersTab.tsx
git commit -m "feat(client): virtualize guild member list for large guilds"
```

---

### Task 5: Virtualize HomeSidebar DM list

**Files:**
- Modify: `client/src/components/home/HomeSidebar.tsx`

**Step 1: Add virtualizer to HomeSidebar**

The DM list currently renders with `<For each={sortedDMs()}>` (line 129). DM items have fixed height (~56px). Add import and virtualizer:

```typescript
import { createVirtualizer } from "@/lib/virtualizer";
```

Add inside the component:

```typescript
let dmListRef: HTMLDivElement | undefined;

const virtualizer = createVirtualizer({
  get count() { return sortedDMs().length; },
  getScrollElement: () => dmListRef ?? null,
  estimateSize: () => 56,
  overscan: 3,
});
```

Replace the DM list container (line 105, `<div class="flex-1 overflow-y-auto px-2 pb-2 space-y-0.5">`):

```tsx
<div ref={dmListRef} class="flex-1 overflow-y-auto px-2 pb-2">
  <Show when={showDMs()}>
    <Show
      when={!dmsState.isLoading}
      fallback={/* existing loading fallback */}
    >
      <Show
        when={dmsState.dms.length > 0}
        fallback={/* existing empty fallback */}
      >
        <div style={{ height: `${virtualizer.getTotalSize()}px`, position: "relative" }}>
          <For each={virtualizer.getVirtualItems()}>
            {(virtualItem) => {
              const dm = () => sortedDMs()[virtualItem.index];
              return (
                <div
                  data-index={virtualItem.index}
                  ref={(el) => virtualizer.measureElement(el)}
                  style={{
                    position: "absolute",
                    top: `${virtualItem.start}px`,
                    width: "100%",
                  }}
                >
                  <Show when={dm()}>
                    <DMItem dm={dm()!} />
                  </Show>
                </div>
              );
            }}
          </For>
        </div>
      </Show>
    </Show>
  </Show>
</div>
```

**Step 2: Run tests**

Run: `cd client && bun run test:run`
Expected: All tests pass.

**Step 3: Commit**

```bash
git add client/src/components/home/HomeSidebar.tsx
git commit -m "feat(client): virtualize DM conversation list in home sidebar"
```

---

### Task 6: Virtualize SearchPanel results

**Files:**
- Modify: `client/src/components/search/SearchPanel.tsx`

**Step 1: Add virtualizer to SearchPanel**

Search results have variable height (like messages — content length varies). Add import:

```typescript
import { createVirtualizer } from "@/lib/virtualizer";
```

Add a container ref and virtualizer inside the component:

```typescript
let resultsContainerRef: HTMLDivElement | undefined;

const virtualizer = createVirtualizer({
  get count() { return searchState.results.length; },
  getScrollElement: () => resultsContainerRef ?? null,
  estimateSize: () => 100, // average result height
  overscan: 5,
});
```

Replace the results list section (around line 253, the `<div class="flex-1 overflow-y-auto">`) — add the ref to the scroll container, and replace `<For each={searchState.results}>` with virtualized rendering:

```tsx
<div ref={resultsContainerRef} class="flex-1 overflow-y-auto">
  {/* Keep existing Loading/Empty/Hint/Error states as-is */}

  <Show when={searchState.results.length > 0}>
    <div style={{ height: `${virtualizer.getTotalSize()}px`, position: "relative" }}>
      <For each={virtualizer.getVirtualItems()}>
        {(virtualItem) => {
          const result = () => searchState.results[virtualItem.index];
          return (
            <div
              data-index={virtualItem.index}
              ref={(el) => virtualizer.measureElement(el)}
              style={{
                position: "absolute",
                top: `${virtualItem.start}px`,
                width: "100%",
              }}
            >
              <Show when={result()}>
                {/* Move existing result button JSX here unchanged */}
              </Show>
            </div>
          );
        }}
      </For>
    </div>

    {/* Keep "Load More" button below the virtualized container */}
    <Show when={hasMore()}>
      {/* existing load more button */}
    </Show>
  </Show>
</div>
```

**Step 2: Run tests**

Run: `cd client && bun run test:run`
Expected: All tests pass.

**Step 3: Commit**

```bash
git add client/src/components/search/SearchPanel.tsx
git commit -m "feat(client): virtualize search results for large result sets"
```

---

### Task 7: Document toast conventions

**Files:**
- Modify: `client/src/components/ui/Toast.tsx`

**Step 1: Add convention documentation to Toast.tsx**

Add a documentation block after the existing module docstring (after line 6):

```typescript
/**
 * Toast Notification System
 *
 * Provides a simple toast notification system for displaying
 * temporary messages to the user.
 *
 * ## Usage Conventions
 *
 * | Type    | Use for                                      | Duration |
 * |---------|----------------------------------------------|----------|
 * | error   | API failures, permission denials, fatal       | 8000ms   |
 * | success | User-initiated actions that complete          | 3000ms   |
 * | info    | Background events (reconnect, bot timeout)    | 5000ms   |
 * | warning | Degraded state, approaching limits            | 5000ms   |
 *
 * ## Deduplication
 *
 * Pass a stable `id` for toasts that can fire repeatedly (e.g. WebSocket
 * reconnect, rate limit warnings) to prevent toast spam. Example:
 *
 *   showToast({ type: "warning", title: "Rate limited", id: "rate-limit" })
 *
 * ## Default Durations
 *
 * If no `duration` is specified, toasts auto-dismiss after 5000ms.
 * Use `duration: 0` for persistent toasts that require manual dismissal.
 * Prefer explicit durations matching the convention table above.
 */
```

Replace the existing docstring at the top of the file (lines 1-6) with this expanded version.

**Step 2: Run tests to verify no breakage**

Run: `cd client && bun run test:run -- --reporter=verbose src/components/ui/__tests__/Toast.test.tsx`
Expected: All existing tests pass (documentation-only change).

**Step 3: Commit**

```bash
git add client/src/components/ui/Toast.tsx
git commit -m "docs(client): add toast usage conventions to Toast.tsx"
```

---

### Task 8: Audit and fix toast usages

**Files:**
- Modify: Multiple files across `client/src/` (stores, components)

**Step 1: Search for all showToast calls and audit**

Run: `cd client && grep -rn "showToast" src/ --include="*.ts" --include="*.tsx" | grep -v "__tests__" | grep -v "node_modules"`

For each call, check:
1. **Type correctness**: API failures should be `error`, not `warning` or `info`
2. **Duration**: Error toasts should use `duration: 8000`, success toasts `duration: 3000`
3. **Deduplication**: Repeatable toasts (WebSocket events, rate limits) must have a stable `id`
4. **Wording consistency**: Use "Failed to X" for errors, "X successful" for success

Common patterns to fix:
- Missing `id` on WebSocket reconnect/disconnect toasts
- Missing `id` on rate limit toasts
- Error toasts using default 5s duration instead of 8s
- Success toasts using default 5s duration instead of 3s

**Step 2: Apply fixes**

Fix each file according to the conventions. Example patterns:

```typescript
// BEFORE: missing duration, missing id
showToast({ type: "error", title: "Failed to send message" });

// AFTER: explicit duration, stable id for deduplication
showToast({ type: "error", title: "Failed to send message", duration: 8000 });

// BEFORE: WebSocket reconnect without dedup id
showToast({ type: "info", title: "Reconnecting..." });

// AFTER: stable id prevents toast spam on flaky connections
showToast({ type: "info", title: "Reconnecting...", id: "ws-reconnect" });
```

**Step 3: Run tests**

Run: `cd client && bun run test:run`
Expected: All tests pass.

**Step 4: Commit**

```bash
git add -u client/src/
git commit -m "fix(client): standardize toast types, durations, and dedup IDs"
```

---

### Task 9: Complete toast rendering tests

**Files:**
- Modify: `client/src/components/ui/__tests__/Toast.test.tsx`

**Step 1: Fix the skipped auto-dismiss tests**

The tests use `vi.useFakeTimers()` but the `showToast` function uses `window.setTimeout`. The fix is to import and read the `toasts` signal to verify state, and ensure fake timers are set up before any toast calls.

Unskip and fix the timer tests. The key insight: we need to import the `toasts` signal to verify toast state. Add to imports:

```typescript
import { showToast, dismissToast, dismissAllToasts, toasts } from "../Toast";
```

This requires exporting `toasts` from Toast.tsx. Add this export in Toast.tsx (after line 37):

```typescript
// Exported for testing only — do not use in components
export { toasts };
```

Then fix the skipped tests by reading `toasts()` to verify state:

```typescript
it("auto-dismisses after default duration (5s)", () => {
  vi.useFakeTimers();
  showToast({ type: "info", title: "Auto-dismiss" });
  expect(toasts().length).toBe(1);

  vi.advanceTimersByTime(5000);
  expect(toasts().length).toBe(0);

  vi.useRealTimers();
});

it("respects custom duration", () => {
  vi.useFakeTimers();
  showToast({ type: "info", title: "Custom duration", duration: 3000 });
  expect(toasts().length).toBe(1);

  vi.advanceTimersByTime(3000);
  expect(toasts().length).toBe(0);

  vi.useRealTimers();
});

it("cleans up timeouts for auto-dismissed toasts", () => {
  vi.useFakeTimers();
  for (let i = 0; i < 6; i++) {
    showToast({ type: "info", title: `Toast ${i + 1}`, duration: 5000 });
  }
  // Only 5 visible, oldest auto-evicted
  expect(toasts().length).toBe(5);
  vi.useRealTimers();
});

it("cleans up timeout when manually dismissed", () => {
  vi.useFakeTimers();
  const id = showToast({ type: "info", title: "Manual dismiss", duration: 5000 });
  expect(toasts().length).toBe(1);

  dismissToast(id);
  expect(toasts().length).toBe(0);

  // Advancing timers should not throw
  vi.advanceTimersByTime(5000);
  expect(toasts().length).toBe(0);
  vi.useRealTimers();
});

it("cleans up all timeouts on dismissAll", () => {
  vi.useFakeTimers();
  showToast({ type: "info", title: "Toast 1", duration: 5000 });
  showToast({ type: "info", title: "Toast 2", duration: 5000 });
  expect(toasts().length).toBe(2);

  dismissAllToasts();
  expect(toasts().length).toBe(0);

  vi.advanceTimersByTime(5000);
  expect(toasts().length).toBe(0);
  vi.useRealTimers();
});
```

Also add assertions to existing tests that currently just check IDs:

```typescript
it("enforces maximum of 5 visible toasts", () => {
  vi.useFakeTimers();
  for (let i = 0; i < 6; i++) {
    showToast({ type: "info", title: `Toast ${i + 1}`, duration: 0 });
  }
  expect(toasts().length).toBe(5);
  vi.useRealTimers();
});

it("dismisses a specific toast by ID", () => {
  const id1 = showToast({ type: "info", title: "Toast 1", duration: 0 });
  const id2 = showToast({ type: "info", title: "Toast 2", duration: 0 });
  expect(toasts().length).toBe(2);

  dismissToast(id1);
  expect(toasts().length).toBe(1);
  expect(toasts()[0].id).toBe(id2);
});

it("dismisses all active toasts", () => {
  showToast({ type: "info", title: "Toast 1", duration: 0 });
  showToast({ type: "info", title: "Toast 2", duration: 0 });
  showToast({ type: "info", title: "Toast 3", duration: 0 });
  expect(toasts().length).toBe(3);

  dismissAllToasts();
  expect(toasts().length).toBe(0);
});
```

**Step 2: Run tests to verify all pass**

Run: `cd client && bun run test:run -- --reporter=verbose src/components/ui/__tests__/Toast.test.tsx`
Expected: All tests pass, no skipped tests.

**Step 3: Commit**

```bash
git add client/src/components/ui/Toast.tsx client/src/components/ui/__tests__/Toast.test.tsx
git commit -m "test(client): complete toast auto-dismiss and state verification tests"
```

---

### Task 10: Update CHANGELOG and roadmap

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `docs/project/roadmap.md`

**Step 1: Add changelog entries**

Under `[Unreleased]` in CHANGELOG.md, add:

```markdown
### Changed
- Upgraded message list virtualizer to @tanstack/solid-virtual for proper dynamic sizing and ResizeObserver support
- Virtualized guild member list, DM conversation sidebar, and search results for better performance with large datasets
- Standardized toast notification types and durations across the application

### Fixed
- Message list layout drift when images load or code blocks expand (now uses real element measurement)
```

**Step 2: Update roadmap**

In `docs/project/roadmap.md`, mark the Production-Scale Polish item as complete:

Change the item from `[ ]` to `[x]` and add ✅ with completion details.

Update the Phase 5 completion counter (should increment by 1, from 8/16 to 9/16, and update percentage).

**Step 3: Commit**

```bash
git add CHANGELOG.md docs/project/roadmap.md
git commit -m "docs(client): update changelog and roadmap for production-scale polish"
```
