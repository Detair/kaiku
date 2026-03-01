# Production-Scale Polish Design

> **Status:** Complete
> **Supersedes:** `2026-02-15-phase-5-production-polish-design.md` (draft)

## Goal

Upgrade the message list virtualizer to TanStack, expand virtualization to other long lists, and audit toast usage for consistency across the codebase.

## Current State

- **MessageList** already has virtualization via a custom 110-line wrapper (`lib/virtualizer.ts`), infinite scroll with IntersectionObserver, and memory eviction (2000 max, keep 500 around viewport).
- **Toast system** has deduplication by ID, max 5 visible, auto-dismiss, action buttons, ARIA attributes. Used in 36 files (177 occurrences).
- `@tanstack/solid-virtual` v3.13.18 is installed but unused.
- The custom virtualizer's `measureElement` is a stub — no ResizeObserver, so dynamic content (images loading, code blocks expanding) can cause layout drift.
- Other long lists (member list, DM sidebar, search results) render all items without virtualization.

## Design

### Part A: Virtualizer Upgrade

Replace `client/src/lib/virtualizer.ts` internals with `@tanstack/solid-virtual` while preserving the existing facade API.

**Changes:**
- `createVirtualizer()` wraps TanStack's `createVirtualizer` with the same options interface (count, estimateSize, overscan, getScrollElement).
- `measureElement` becomes real — uses ResizeObserver to track actual rendered sizes, fixing layout drift for dynamic content.
- `scrollToIndex` delegates to TanStack's implementation with align/behavior options.

**Migration:** `MessageList.tsx` keeps its existing structure. The virtualizer returns the same shape (`virtualItems`, `totalSize`, `scrollToIndex`, `measureElement`), so the component swaps its import with minimal changes.

The memory eviction policy (MAX_MESSAGES_PER_CHANNEL=2000, keep 500 around viewport) stays in `MessageList.tsx` — independent of the virtualizer.

### Part B: Expand Virtualization to Other Lists

Three lists get virtualization:

| List | Component | Row height | Notes |
|------|-----------|-----------|-------|
| Member list | `MemberList.tsx` | Fixed | Guild sidebar, can reach thousands |
| DM conversations | DM sidebar component | Fixed | Home view, typically dozens but can reach 100+ |
| Search results | `SearchPanel.tsx` | Variable | Paginated, content length varies like messages |

All three use the upgraded `lib/virtualizer.ts` facade. Member list and DM list use fixed heights (simpler). Search results use dynamic sizing (like messages).

No new components — each existing component gets virtualization added inline.

### Part C: Toast Audit & Consistency

No architectural changes to the toast system. This is an audit/cleanup pass:

- **Audit all `showToast` calls** — verify correct type usage, fix inconsistent wording, add missing error toasts on failed API calls.
- **Document conventions** — add a comment block in `Toast.tsx` establishing patterns:
  - `error`: API failures, permission denials. Duration: 8s.
  - `success`: User-initiated actions that complete. Duration: 3s.
  - `info`: Background events (bot timeout, reconnect). Duration: 5s (default).
  - `warning`: Degraded state, approaching limits. Duration: 5s.
- **Deduplicate IDs** — ensure callers pass stable IDs for repeatable toasts (WebSocket reconnect, rate limits) to prevent toast spam.
- **Complete rendering tests** — fill in the skipped component tests: toast appears in DOM, auto-dismisses, action button fires, max 5 visible in UI.

## Success Criteria

- 10,000+ messages in a channel: smooth scrolling, no layout drift on image load.
- Member list with 1,000+ members: instant scroll, no jank.
- All `showToast` calls follow documented conventions.
- Toast rendering tests passing (no skipped tests).

## Out of Scope

- Server-side changes (no API modifications needed).
- Cursor-based pagination changes (current offset pagination works fine).
- New toast UI/UX features (the current system is sufficient).
