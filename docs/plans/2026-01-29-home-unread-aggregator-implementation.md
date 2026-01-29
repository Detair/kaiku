# Home Page Unread Aggregator — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Provide a centralized "Unread Activity" module on the Home page that shows all unread messages across every guild and DM, letting users quickly see what they missed and jump to the right channel.

**Architecture:** New backend aggregate endpoint returns per-guild and per-DM unread summaries in a single API call. A new Home sidebar module (`UnreadModule`) renders the data grouped by guild/DM with click-to-navigate. The ServerRail Home button also shows a total unread badge.

**Tech Stack:** Rust (axum handler, sqlx queries), Solid.js (new module component), existing WebSocket infrastructure for live updates.

---

## Context

### Existing Infrastructure (DO NOT recreate)

| Component | Location | What it does |
|-----------|----------|--------------|
| `dm_read_state` table | `server/migrations/20260113000000_add_dm_read_state.sql` | Tracks per-user, per-DM last read timestamp |
| `channel_read_state` table | `server/migrations/20260127000000_add_channel_read_state.sql` | Tracks per-user, per-guild-channel last read timestamp |
| `POST /api/dm/:id/read` | `server/src/chat/dm.rs` | Marks DM as read |
| `POST /api/channels/:id/read` | `server/src/chat/channels.rs` | Marks guild channel as read |
| `DmRead` / `ChannelRead` events | `server/src/ws/mod.rs` | Cross-device read sync |
| `dms.ts` store | `client/src/stores/dms.ts` | DM unread counts, `getTotalUnreadCount()` |
| `channels.ts` store | `client/src/stores/channels.ts` | Guild channel unread counts, `getTotalUnreadCount()` |
| `guilds.ts` store | `client/src/stores/guilds.ts` | `getGuildUnreadCount()`, `incrementGuildUnread()`, `clearGuildUnread()` |
| `CollapsibleModule` | `client/src/components/home/modules/CollapsibleModule.tsx` | Generic collapsible wrapper for Home modules |
| `ServerRail.tsx` | `client/src/components/layout/ServerRail.tsx` | Already shows per-guild unread badges |

### What's Missing

1. **No aggregate endpoint** — Client must load each guild's channels separately to get unread counts
2. **No Home page module** — No centralized "what did I miss?" view
3. **No Home button unread badge** — ServerRail Home icon shows no total unread indicator
4. **No total unread across all guilds** — `getGuildUnreadCount()` works per-guild but no global sum

---

## Files to Modify

### Server
| File | Changes |
|------|---------|
| `server/src/api/mod.rs` | Add route for aggregate unread endpoint |
| `server/src/guild/handlers.rs` | Add `get_unread_summary()` handler |
| `server/src/guild/mod.rs` | Wire new route |

### Client
| File | Changes |
|------|---------|
| `client/src/lib/types.ts` | Add `UnreadSummary` response type |
| `client/src/lib/tauri.ts` | Add `getUnreadSummary()` API function |
| `client/src/stores/guilds.ts` | Add `loadUnreadSummary()`, `getAllGuildsUnreadCount()` |
| `client/src/components/home/modules/UnreadModule.tsx` | **NEW** — Unread activity module |
| `client/src/components/home/modules/CollapsibleModule.tsx` | Add `"unread"` to `id` union type |
| `client/src/components/home/modules/index.ts` | Export `UnreadModule` |
| `client/src/components/home/HomeRightPanel.tsx` | Add `UnreadModule` to modular sidebar |
| `client/src/components/layout/ServerRail.tsx` | Add total unread badge on Home icon |

---

## Implementation Tasks

### Task 1: Backend Aggregate Unread Endpoint

**Files:**
- Modify: `server/src/guild/handlers.rs`
- Modify: `server/src/guild/mod.rs`

**Purpose:** Single API call returns unread counts for all guilds + all DMs the user belongs to.

**Step 1: Add response types to handlers.rs**

Add after the existing `ChannelWithUnread` struct:

```rust
#[derive(Debug, Serialize)]
pub struct GuildUnreadSummary {
    pub guild_id: Uuid,
    pub guild_name: String,
    pub guild_icon: Option<String>,
    pub total_unread: i64,
    pub channels: Vec<ChannelUnreadEntry>,
}

#[derive(Debug, Serialize)]
pub struct ChannelUnreadEntry {
    pub channel_id: Uuid,
    pub channel_name: String,
    pub unread_count: i64,
}

#[derive(Debug, Serialize)]
pub struct DMUnreadEntry {
    pub channel_id: Uuid,
    pub display_name: String,
    pub is_group: bool,
    pub unread_count: i64,
}

#[derive(Debug, Serialize)]
pub struct UnreadSummaryResponse {
    pub guilds: Vec<GuildUnreadSummary>,
    pub dms: Vec<DMUnreadEntry>,
    pub total_guild_unread: i64,
    pub total_dm_unread: i64,
}
```

**Step 2: Add the handler function**

```rust
/// GET /api/guilds/unread-summary
/// Returns aggregated unread counts across all guilds and DMs for the current user.
pub async fn get_unread_summary(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<UnreadSummaryResponse>, GuildError> {
    // 1. Get all guilds the user is a member of
    // 2. For each guild, query unread counts per text channel using batch query:
    //    SELECT c.guild_id, c.id as channel_id, c.name as channel_name,
    //           COUNT(m.id) as unread_count
    //    FROM channels c
    //    INNER JOIN guild_members gm ON gm.guild_id = c.guild_id AND gm.user_id = $1
    //    LEFT JOIN channel_read_state crs ON crs.channel_id = c.id AND crs.user_id = $1
    //    LEFT JOIN messages m ON m.channel_id = c.id
    //        AND (crs.last_read_at IS NULL OR m.created_at > crs.last_read_at)
    //    WHERE c.channel_type = 'text'
    //    GROUP BY c.guild_id, c.id, c.name
    //    HAVING COUNT(m.id) > 0
    //
    // 3. Join with guilds table to get guild names/icons
    // 4. Get DM unread counts (reuse logic from dm.rs list_dms)
    // 5. Only include entries with unread_count > 0
    // 6. Return grouped response
}
```

**Key SQL query (guild channels with unread, single query across ALL guilds):**

```sql
SELECT
    g.id as guild_id,
    g.name as guild_name,
    g.icon as guild_icon,
    c.id as channel_id,
    c.name as channel_name,
    COUNT(m.id) as "unread_count!"
FROM guilds g
INNER JOIN guild_members gm ON gm.guild_id = g.id AND gm.user_id = $1
INNER JOIN channels c ON c.guild_id = g.id AND c.channel_type = 'text'
LEFT JOIN channel_read_state crs ON crs.channel_id = c.id AND crs.user_id = $1
LEFT JOIN messages m ON m.channel_id = c.id
    AND (crs.last_read_at IS NULL OR m.created_at > crs.last_read_at)
GROUP BY g.id, g.name, g.icon, c.id, c.name
HAVING COUNT(m.id) > 0
ORDER BY g.name, c.name
```

**DM unread query (single query across ALL DMs):**

```sql
SELECT
    dc.id as channel_id,
    dc.name as dm_name,
    dc.is_group,
    COALESCE(
        (SELECT u.display_name FROM users u
         INNER JOIN dm_members dm2 ON dm2.channel_id = dc.id AND dm2.user_id = u.id
         WHERE dm2.user_id != $1 LIMIT 1),
        dc.name,
        'Unknown'
    ) as display_name,
    COUNT(m.id) as "unread_count!"
FROM channels dc
INNER JOIN dm_members dmm ON dmm.channel_id = dc.id AND dmm.user_id = $1
LEFT JOIN dm_read_state drs ON drs.channel_id = dc.id AND drs.user_id = $1
LEFT JOIN messages m ON m.channel_id = dc.id
    AND (drs.last_read_at IS NULL OR m.created_at > drs.last_read_at)
WHERE dc.channel_type = 'dm'
GROUP BY dc.id, dc.name, dc.is_group
HAVING COUNT(m.id) > 0
ORDER BY MAX(m.created_at) DESC NULLS LAST
```

**Step 3: Wire the route**

In `server/src/guild/mod.rs`, add the route:

```rust
.route("/unread-summary", get(handlers::get_unread_summary))
```

This should be added to the `/api/guilds` router (not nested under `/:guild_id`).

**Verification:**
```bash
cd server && cargo check
```

---

### Task 2: Client Types and API Function

**Files:**
- Modify: `client/src/lib/types.ts`
- Modify: `client/src/lib/tauri.ts`

**Step 1: Add types to types.ts**

Add after the existing `ChannelWithUnread` type:

```typescript
export interface ChannelUnreadEntry {
  channel_id: string;
  channel_name: string;
  unread_count: number;
}

export interface GuildUnreadSummary {
  guild_id: string;
  guild_name: string;
  guild_icon: string | null;
  total_unread: number;
  channels: ChannelUnreadEntry[];
}

export interface DMUnreadEntry {
  channel_id: string;
  display_name: string;
  is_group: boolean;
  unread_count: number;
}

export interface UnreadSummaryResponse {
  guilds: GuildUnreadSummary[];
  dms: DMUnreadEntry[];
  total_guild_unread: number;
  total_dm_unread: number;
}
```

**Step 2: Add API function to tauri.ts**

Add after `markChannelAsRead`:

```typescript
export async function getUnreadSummary(): Promise<UnreadSummaryResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_unread_summary");
  }
  return httpRequest<UnreadSummaryResponse>("GET", "/api/guilds/unread-summary");
}
```

**Verification:**
```bash
cd client && bun run check
```

---

### Task 3: Guilds Store — Aggregate Unread State

**Files:**
- Modify: `client/src/stores/guilds.ts`

**Purpose:** Add functions to load the unread summary and compute total unread across all guilds.

**Step 1: Add state field**

Add to `GuildsState` interface:

```typescript
unreadSummary: UnreadSummaryResponse | null;
```

Initialize to `null` in the `createStore` call.

**Step 2: Add load function**

```typescript
/**
 * Load aggregate unread summary across all guilds and DMs.
 * Called when entering the Home view.
 */
export async function loadUnreadSummary(): Promise<void> {
  try {
    const summary = await getUnreadSummary();
    setGuildsState("unreadSummary", summary);

    // Also update per-guild unread counts for ServerRail badges
    for (const guild of summary.guilds) {
      setGuildsState("guildUnreadCounts", guild.guild_id, guild.total_unread);
    }
  } catch (err) {
    console.error("Failed to load unread summary:", err);
  }
}
```

**Step 3: Add computed total**

```typescript
/**
 * Get total unread count across ALL guilds (for Home button badge).
 */
export function getAllGuildsUnreadCount(): number {
  let total = 0;
  for (const guildId in guildsState.guildUnreadCounts) {
    total += guildsState.guildUnreadCounts[guildId] ?? 0;
  }
  return total;
}
```

**Step 4: Import the new API function**

Add `getUnreadSummary` to the imports from `@/lib/tauri`.

**Verification:**
```bash
cd client && bun run check
```

---

### Task 4: UnreadModule Component

**Files:**
- Create: `client/src/components/home/modules/UnreadModule.tsx`
- Modify: `client/src/components/home/modules/CollapsibleModule.tsx`
- Modify: `client/src/components/home/modules/index.ts`

**Step 1: Update CollapsibleModule id type**

In `CollapsibleModule.tsx`, change the `id` prop type:

```typescript
id: "activeNow" | "pending" | "pins" | "unread";
```

**Step 2: Create UnreadModule.tsx**

```tsx
/**
 * UnreadModule - Shows unread message summary across all guilds and DMs.
 * Allows quick navigation to channels with unread messages.
 */

import { Component, For, Show, onMount, createMemo } from "solid-js";
import { Hash, MessageSquare, Users } from "lucide-solid";
import CollapsibleModule from "./CollapsibleModule";
import { guildsState, loadUnreadSummary, selectGuild } from "@/stores/guilds";
import { selectChannel } from "@/stores/channels";
import { selectDM } from "@/stores/dms";
import type { GuildUnreadSummary, DMUnreadEntry } from "@/lib/types";

const UnreadModule: Component = () => {
  onMount(() => {
    loadUnreadSummary();
  });

  const summary = () => guildsState.unreadSummary;

  const totalUnread = createMemo(() => {
    const s = summary();
    if (!s) return 0;
    return s.total_guild_unread + s.total_dm_unread;
  });

  const hasUnread = () => totalUnread() > 0;

  const handleGuildChannelClick = (guildId: string, channelId: string) => {
    selectGuild(guildId);
    selectChannel(channelId);
  };

  const handleDMClick = (channelId: string) => {
    selectDM(channelId);
  };

  return (
    <CollapsibleModule id="unread" title="Unread" badge={totalUnread()}>
      <Show
        when={hasUnread()}
        fallback={
          <div class="px-3 py-4 text-sm text-text-secondary text-center">
            All caught up!
          </div>
        }
      >
        <div class="space-y-3 px-1">
          {/* Guild unread sections */}
          <For each={summary()?.guilds ?? []}>
            {(guild: GuildUnreadSummary) => (
              <div>
                <div class="flex items-center gap-2 px-2 py-1">
                  <Show
                    when={guild.guild_icon}
                    fallback={
                      <div class="w-4 h-4 rounded bg-accent-primary/20 flex items-center justify-center">
                        <span class="text-[8px] font-bold text-accent-primary">
                          {guild.guild_name.charAt(0).toUpperCase()}
                        </span>
                      </div>
                    }
                  >
                    <img
                      src={guild.guild_icon!}
                      alt={guild.guild_name}
                      class="w-4 h-4 rounded object-cover"
                    />
                  </Show>
                  <span class="text-xs font-semibold text-text-secondary uppercase tracking-wide truncate">
                    {guild.guild_name}
                  </span>
                  <span class="ml-auto text-xs text-text-secondary">
                    {guild.total_unread}
                  </span>
                </div>
                <For each={guild.channels}>
                  {(channel) => (
                    <button
                      type="button"
                      class="w-full flex items-center gap-2 px-3 py-1 rounded hover:bg-white/5 transition-colors text-left group"
                      onClick={() => handleGuildChannelClick(guild.guild_id, channel.channel_id)}
                    >
                      <Hash class="w-3.5 h-3.5 text-text-secondary flex-shrink-0" />
                      <span class="text-sm text-text-primary truncate flex-1">
                        {channel.channel_name}
                      </span>
                      <span class="min-w-5 h-5 px-1.5 bg-accent-primary text-surface-base text-xs font-bold rounded-full flex items-center justify-center flex-shrink-0">
                        {channel.unread_count > 99 ? "99+" : channel.unread_count}
                      </span>
                    </button>
                  )}
                </For>
              </div>
            )}
          </For>

          {/* DM unread section */}
          <Show when={(summary()?.dms?.length ?? 0) > 0}>
            <div>
              <div class="flex items-center gap-2 px-2 py-1">
                <MessageSquare class="w-4 h-4 text-text-secondary" />
                <span class="text-xs font-semibold text-text-secondary uppercase tracking-wide">
                  Direct Messages
                </span>
                <span class="ml-auto text-xs text-text-secondary">
                  {summary()?.total_dm_unread}
                </span>
              </div>
              <For each={summary()?.dms ?? []}>
                {(dm: DMUnreadEntry) => (
                  <button
                    type="button"
                    class="w-full flex items-center gap-2 px-3 py-1 rounded hover:bg-white/5 transition-colors text-left group"
                    onClick={() => handleDMClick(dm.channel_id)}
                  >
                    <Show
                      when={dm.is_group}
                      fallback={
                        <div class="w-4 h-4 rounded-full bg-accent-primary/20 flex items-center justify-center flex-shrink-0">
                          <span class="text-[8px] font-bold text-accent-primary">
                            {dm.display_name.charAt(0).toUpperCase()}
                          </span>
                        </div>
                      }
                    >
                      <Users class="w-3.5 h-3.5 text-text-secondary flex-shrink-0" />
                    </Show>
                    <span class="text-sm text-text-primary truncate flex-1">
                      {dm.display_name}
                    </span>
                    <span class="min-w-5 h-5 px-1.5 bg-accent-primary text-surface-base text-xs font-bold rounded-full flex items-center justify-center flex-shrink-0">
                      {dm.unread_count > 99 ? "99+" : dm.unread_count}
                    </span>
                  </button>
                )}
              </For>
            </div>
          </Show>
        </div>
      </Show>
    </CollapsibleModule>
  );
};

export default UnreadModule;
```

**Step 3: Export from index.ts**

Add to `client/src/components/home/modules/index.ts`:

```typescript
export { default as UnreadModule } from "./UnreadModule";
```

**Verification:**
```bash
cd client && bun run check
```

---

### Task 5: Integrate UnreadModule into Home Right Panel

**Files:**
- Modify: `client/src/components/home/HomeRightPanel.tsx`

**Step 1: Import UnreadModule**

Add to imports:

```typescript
import { UnreadModule } from "./modules";
```

**Step 2: Add UnreadModule as FIRST module**

Place `<UnreadModule />` before `<ActiveNowModule />` in the modular sidebar section (rendered when `isShowingFriends` is true):

```tsx
<UnreadModule />
<ActiveNowModule />
<PendingModule />
<PinsModule />
```

**Rationale:** Unread messages are the highest-priority information when returning to Home.

**Verification:**
```bash
cd client && bun run check
```

---

### Task 6: Home Button Unread Badge on ServerRail

**Files:**
- Modify: `client/src/components/layout/ServerRail.tsx`

**Purpose:** Show a total unread badge on the Home icon in the server rail when the user is NOT on the Home page.

**Step 1: Import required functions**

Add to imports from `@/stores/guilds`:

```typescript
import { guildsState, selectHome, selectGuild, getGuildUnreadCount, getAllGuildsUnreadCount } from "@/stores/guilds";
```

Add DM unread import:

```typescript
import { getTotalUnreadCount as getDMTotalUnread } from "@/stores/dms";
```

**Step 2: Add computed total**

Inside the component, add:

```typescript
const homeTotalUnread = () => {
  // Only show badge when NOT on Home page
  if (guildsState.activeGuildId === null) return 0;
  return getAllGuildsUnreadCount() + getDMTotalUnread();
};
```

**Step 3: Add badge to Home icon**

After the Home icon `<div>`, add (similar pattern to guild unread badges):

```tsx
<Show when={homeTotalUnread() > 0}>
  <div class="absolute -bottom-0.5 -right-0.5 min-w-4 h-4 px-1 bg-accent-primary text-white text-[10px] font-bold rounded-full flex items-center justify-center">
    {homeTotalUnread() > 99 ? "99+" : homeTotalUnread()}
  </div>
</Show>
```

Make sure the Home icon's parent container has `class="relative"` for absolute positioning to work.

**Verification:**
```bash
cd client && bun run check
```

---

### Task 7: Live Updates via WebSocket

**Files:**
- Modify: `client/src/stores/guilds.ts`

**Purpose:** When a new message arrives (WebSocket `message_new` event) or a channel is read (`channel_read` / `dm_read` event), update the unread summary reactively.

**Step 1: Invalidate summary on message events**

The existing WebSocket handlers already call `incrementGuildUnread()` and `incrementUnreadCount()`. The `unreadSummary` in the store becomes stale but the per-guild counts stay current via the existing increment logic.

Add a function to invalidate the summary cache:

```typescript
/**
 * Mark the unread summary as stale (will reload on next Home visit).
 */
export function invalidateUnreadSummary(): void {
  setGuildsState("unreadSummary", null);
}
```

**Step 2: Reload summary when entering Home**

In the `selectHome()` function (already in `guilds.ts`), add:

```typescript
export function selectHome(): void {
  setGuildsState("activeGuildId", null);
  loadUnreadSummary();  // Refresh unread data when returning to Home
}
```

**Step 3: Handle read events**

When `channel_read` or `dm_read` events fire, the existing handlers already clear per-channel unread counts. The UnreadModule will reactively update because it depends on `guildsState.unreadSummary`. However, the summary won't reflect the read event until reloaded.

For instant feedback, update the summary in-place when a read event occurs:

```typescript
/**
 * Remove a channel from the unread summary (called when channel is read).
 */
export function removeFromUnreadSummary(channelId: string): void {
  const summary = guildsState.unreadSummary;
  if (!summary) return;

  // Check guild channels
  for (let gi = 0; gi < summary.guilds.length; gi++) {
    const guild = summary.guilds[gi];
    const ci = guild.channels.findIndex(c => c.channel_id === channelId);
    if (ci !== -1) {
      const count = guild.channels[ci].unread_count;
      setGuildsState("unreadSummary", "guilds", gi, "channels", (chs) =>
        chs.filter(c => c.channel_id !== channelId)
      );
      setGuildsState("unreadSummary", "guilds", gi, "total_unread", (prev) => prev - count);
      setGuildsState("unreadSummary", "total_guild_unread", (prev) => prev - count);
      // Remove guild entry if no more unread channels
      if (guild.channels.length <= 1) {
        setGuildsState("unreadSummary", "guilds", (gs) =>
          gs.filter(g => g.guild_id !== guild.guild_id)
        );
      }
      return;
    }
  }

  // Check DMs
  const di = summary.dms.findIndex(d => d.channel_id === channelId);
  if (di !== -1) {
    const count = summary.dms[di].unread_count;
    setGuildsState("unreadSummary", "dms", (dms) =>
      dms.filter(d => d.channel_id !== channelId)
    );
    setGuildsState("unreadSummary", "total_dm_unread", (prev) => prev - count);
  }
}
```

Call `removeFromUnreadSummary(channelId)` from the existing `handleChannelReadEvent()` in `channels.ts` and `handleDMReadEvent()` in `dms.ts`.

**Verification:**
```bash
cd client && bun run check
```

---

### Task 8: CHANGELOG Update

**Files:**
- Modify: `CHANGELOG.md`

Add under `### Added` in the `[Unreleased]` section:

```markdown
- Home Page Unread Aggregator
  - Centralized "Unread" module on Home page showing all unread messages across guilds and DMs
  - Click any channel or DM to navigate directly to unread messages
  - Unread badge on Home button in server rail showing total unread count
  - Real-time updates when messages arrive or channels are read
  - Aggregate unread API endpoint (`GET /api/guilds/unread-summary`)
```

**Verification:**
```bash
cd client && bun run check
cd server && cargo check
```

---

## Verification

### Server
```bash
cd server && cargo check && cargo test
```

### Client
```bash
cd client && bun run check
```

### Manual Testing
1. Send messages to User B in multiple guild channels and DMs
2. User B navigates to Home → Unread module shows all unread channels grouped by guild, plus DM section
3. Verify badge counts match actual unread messages
4. Click a guild channel in the module → navigates to that guild and channel
5. Click a DM in the module → opens the DM conversation
6. Verify the channel disappears from the unread module after being read (1s delay)
7. Switch to a guild → Home button in ServerRail shows total unread badge
8. Open a second browser/device as User B → verify cross-device read sync clears items from unread module
9. Receive new message while on Home → unread module badge count increments
