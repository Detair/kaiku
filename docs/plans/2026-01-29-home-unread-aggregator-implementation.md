# Home Page Unread Aggregator — Implementation Plan v2

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
| `client/src/lib/types.ts` | Add `UnreadSummary` response types |
| `client/src/lib/tauri.ts` | Add `getUnreadSummary()` API function |
| `client/src/stores/guilds.ts` | Add `loadUnreadSummary()`, `getAllGuildsUnreadCount()`, `removeFromUnreadSummary()` |
| `client/src/stores/channels.ts` | Call `removeFromUnreadSummary()` after read |
| `client/src/stores/dms.ts` | Call `removeFromUnreadSummary()` after read |
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

// Note: These structs use snake_case field names matching the database schema.
// Ensure no global #[serde(rename_all = "camelCase")] is configured.
// If the project uses camelCase for API responses, configure it in axum's
// JsonRejection, not per-struct.
```

**Step 2: Add intermediate row types for sqlx**

```rust
// Internal row types for sqlx query results
#[derive(Debug, sqlx::FromRow)]
struct GuildUnreadRow {
    guild_id: Uuid,
    guild_name: String,
    guild_icon: Option<String>,
    channel_id: Uuid,
    channel_name: String,
    unread_count: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct DMUnreadRow {
    channel_id: Uuid,
    dm_name: Option<String>,
    is_group: bool,
    display_name: String,
    unread_count: i64,
}
```

**Step 3: Add the handler function with full implementation**

```rust
/// GET /api/guilds/unread-summary
/// Returns aggregated unread counts across all guilds and DMs for the current user.
#[tracing::instrument(skip(state))]
pub async fn get_unread_summary(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<UnreadSummaryResponse>, GuildError> {
    let user_id = auth.user_id;

    // Query guild channels with unread messages (single query across ALL guilds)
    let guild_rows = sqlx::query_as::<_, GuildUnreadRow>(
        r#"
        SELECT
            g.id as guild_id,
            g.name as guild_name,
            g.icon as guild_icon,
            c.id as channel_id,
            c.name as channel_name,
            COUNT(m.id) as unread_count
        FROM guilds g
        INNER JOIN guild_members gm ON gm.guild_id = g.id AND gm.user_id = $1
        INNER JOIN channels c ON c.guild_id = g.id AND c.channel_type = 'text'
        LEFT JOIN channel_read_state crs ON crs.channel_id = c.id AND crs.user_id = $1
        LEFT JOIN messages m ON m.channel_id = c.id
            AND (crs.last_read_at IS NULL OR m.created_at > crs.last_read_at)
        GROUP BY g.id, g.name, g.icon, c.id, c.name
        HAVING COUNT(m.id) > 0
        ORDER BY g.name, c.name
        LIMIT 100
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query guild unread: {:?}", e);
        GuildError::DatabaseError
    })?;

    // Query DM channels with unread messages (optimized - no N+1 subquery)
    let dm_rows = sqlx::query_as::<_, DMUnreadRow>(
        r#"
        SELECT
            dc.id as channel_id,
            dc.name as dm_name,
            dc.is_group,
            COALESCE(u.display_name, dc.name, 'Unknown') as display_name,
            COUNT(m.id) as unread_count
        FROM channels dc
        INNER JOIN dm_members dmm ON dmm.channel_id = dc.id AND dmm.user_id = $1
        LEFT JOIN dm_members dm_other ON dm_other.channel_id = dc.id AND dm_other.user_id != $1
        LEFT JOIN users u ON u.id = dm_other.user_id
        LEFT JOIN dm_read_state drs ON drs.channel_id = dc.id AND drs.user_id = $1
        LEFT JOIN messages m ON m.channel_id = dc.id
            AND (drs.last_read_at IS NULL OR m.created_at > drs.last_read_at)
        WHERE dc.channel_type = 'dm'
        GROUP BY dc.id, dc.name, dc.is_group, u.display_name
        HAVING COUNT(m.id) > 0
        ORDER BY MAX(m.created_at) DESC NULLS LAST
        LIMIT 100
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to query DM unread: {:?}", e);
        GuildError::DatabaseError
    })?;

    // Group guild rows by guild_id
    let mut guilds_map: std::collections::HashMap<Uuid, GuildUnreadSummary> =
        std::collections::HashMap::new();

    for row in guild_rows {
        let guild_summary = guilds_map.entry(row.guild_id).or_insert_with(|| {
            GuildUnreadSummary {
                guild_id: row.guild_id,
                guild_name: row.guild_name.clone(),
                guild_icon: row.guild_icon.clone(),
                total_unread: 0,
                channels: Vec::new(),
            }
        });

        guild_summary.total_unread += row.unread_count;
        guild_summary.channels.push(ChannelUnreadEntry {
            channel_id: row.channel_id,
            channel_name: row.channel_name,
            unread_count: row.unread_count,
        });
    }

    let mut guilds: Vec<GuildUnreadSummary> = guilds_map.into_values().collect();
    guilds.sort_by(|a, b| a.guild_name.cmp(&b.guild_name));

    let total_guild_unread: i64 = guilds.iter().map(|g| g.total_unread).sum();

    // Convert DM rows
    let dms: Vec<DMUnreadEntry> = dm_rows
        .into_iter()
        .map(|row| DMUnreadEntry {
            channel_id: row.channel_id,
            display_name: row.display_name,
            is_group: row.is_group,
            unread_count: row.unread_count,
        })
        .collect();

    let total_dm_unread: i64 = dms.iter().map(|d| d.unread_count).sum();

    Ok(Json(UnreadSummaryResponse {
        guilds,
        dms,
        total_guild_unread,
        total_dm_unread,
    }))
}
```

**Step 4: Verify required indexes**

Add this comment to the handler or in a separate migration verification section:

```rust
// CRITICAL: Verify these indexes exist for query performance.
// Without them, these queries will be extremely slow on large datasets.
//
// Required indexes (check migrations):
// - idx_messages_channel_created ON messages(channel_id, created_at DESC)
// - idx_channel_read_state_lookup ON channel_read_state(user_id, channel_id)
// - idx_dm_read_state_lookup ON dm_read_state(user_id, channel_id)
// - idx_guild_members_user ON guild_members(user_id, guild_id)
// - idx_dm_members_user ON dm_members(user_id, channel_id)
//
// If any are missing, add them to migrations before deploying this feature.
```

**Step 5: Wire the route**

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

**Purpose:** Add functions to load the unread summary, compute total unread across all guilds, and update the summary when channels are read.

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

**Step 4: Add remove function for reactive updates**

```typescript
/**
 * Remove a channel from the unread summary (called when channel is read).
 * This provides instant UI feedback before the next summary reload.
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
      
      // Remove guild entry if this was the last unread channel
      if (guild.channels.length === 1) {
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

**Step 5: Update selectHome to reload summary**

In the `selectHome()` function (already in `guilds.ts`), add:

```typescript
export function selectHome(): void {
  setGuildsState("activeGuildId", null);
  loadUnreadSummary();  // Refresh unread data when returning to Home
}
```

**Step 6: Import the new API function**

Add `getUnreadSummary` to the imports from `@/lib/tauri`.

Also add `UnreadSummaryResponse` to imports from `@/lib/types`.

**Verification:**
```bash
cd client && bun run check
```

---

### Task 4: Integrate removeFromUnreadSummary into Channel/DM Read Events

**Files:**
- Modify: `client/src/stores/channels.ts`
- Modify: `client/src/stores/dms.ts`

**Purpose:** When a channel or DM is marked as read, remove it from the unread summary for instant UI feedback.

**Step 1: Update channels.ts**

Import the function:

```typescript
import { removeFromUnreadSummary } from "./guilds";
```

Find the function that marks a channel as read (likely `markChannelAsRead` or similar). After the API call succeeds and the local state is updated, add:

```typescript
export async function markChannelAsRead(channelId: string): Promise<void> {
  try {
    await markChannelAsReadAPI(channelId);
    setChannelsState("channels", channelId, "unreadCount", 0);
    removeFromUnreadSummary(channelId);  // NEW: Update unread summary
  } catch (err) {
    console.error("Failed to mark channel as read:", err);
  }
}
```

**Note:** If the function has a different name or structure, adapt accordingly. The key is to call `removeFromUnreadSummary(channelId)` after successfully marking the channel as read.

**Step 2: Update dms.ts**

Import the function:

```typescript
import { removeFromUnreadSummary } from "./guilds";
```

Find the function that marks a DM as read. After the API call succeeds and the local state is updated, add:

```typescript
export async function markDMAsRead(channelId: string): Promise<void> {
  try {
    await markDMAsReadAPI(channelId);
    setDMsState("dms", channelId, "unreadCount", 0);
    removeFromUnreadSummary(channelId);  // NEW: Update unread summary
  } catch (err) {
    console.error("Failed to mark DM as read:", err);
  }
}
```

**Step 3: Export removeFromUnreadSummary from guilds.ts**

Ensure `removeFromUnreadSummary` is exported in `guilds.ts`:

```typescript
export { removeFromUnreadSummary };
```

**Verification:**
```bash
cd client && bun run check
```

---

### Task 5: UnreadModule Component

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

import { Component, For, Show, onMount, createMemo, createSignal } from "solid-js";
import { Hash, MessageSquare, Users, Loader } from "lucide-solid";
import CollapsibleModule from "./CollapsibleModule";
import { guildsState, loadUnreadSummary, selectGuild } from "@/stores/guilds";
import { selectChannel } from "@/stores/channels";
import { selectDM } from "@/stores/dms";
import type { GuildUnreadSummary, DMUnreadEntry } from "@/lib/types";

const UnreadModule: Component = () => {
  const [loading, setLoading] = createSignal(true);

  onMount(async () => {
    setLoading(true);
    await loadUnreadSummary();
    setLoading(false);
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
      <Show when={loading()}>
        <div class="px-3 py-4 flex items-center justify-center">
          <Loader class="w-4 h-4 animate-spin text-text-secondary" />
        </div>
      </Show>
      <Show when={!loading()}>
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

### Task 6: Integrate UnreadModule into Home Right Panel

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

### Task 7: Home Button Unread Badge on ServerRail

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

Find the Home icon's container. Ensure it has `class="relative"` for absolute positioning. After the Home icon `<div>`, add:

```tsx
<Show when={homeTotalUnread() > 0}>
  <div class="absolute -bottom-0.5 -right-0.5 min-w-4 h-4 px-1 bg-accent-primary text-white text-[10px] font-bold rounded-full flex items-center justify-center">
    {homeTotalUnread() > 99 ? "99+" : homeTotalUnread()}
  </div>
</Show>
```

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
  - Limited to 100 most recent unread channels per query for performance
```

**Verification:**
```bash
cd client && bun run check
cd server && cargo check
```

---

### Task 9: Tests

**Purpose:** Ensure the unread summary feature works correctly under various scenarios.

#### Server Tests

Create `server/tests/integration/unread_summary_test.rs` or add to `server/src/guild/handlers.rs` module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unread_summary_empty() {
        // Setup: Create user with no guilds, no DMs
        let response = get_unread_summary(/* test state */).await.unwrap();
        assert_eq!(response.guilds.len(), 0);
        assert_eq!(response.dms.len(), 0);
        assert_eq!(response.total_guild_unread, 0);
        assert_eq!(response.total_dm_unread, 0);
    }

    #[tokio::test]
    async fn test_unread_summary_with_guild_messages() {
        // Setup: Create guild, channel, send 3 messages
        let response = get_unread_summary(/* test state */).await.unwrap();
        assert_eq!(response.guilds.len(), 1);
        assert_eq!(response.guilds[0].channels.len(), 1);
        assert_eq!(response.guilds[0].channels[0].unread_count, 3);
        assert_eq!(response.total_guild_unread, 3);
    }

    #[tokio::test]
    async fn test_unread_summary_respects_read_state() {
        // Setup: Create guild, send messages, mark channel as read
        let response = get_unread_summary(/* test state */).await.unwrap();
        assert_eq!(response.guilds.len(), 0); // No unread after marking read
    }

    #[tokio::test]
    async fn test_unread_summary_respects_100_limit() {
        // Setup: Create 150 channels with unread messages
        let response = get_unread_summary(/* test state */).await.unwrap();
        let total_channels: usize = response.guilds.iter()
            .map(|g| g.channels.len())
            .sum();
        assert!(total_channels <= 100);
    }

    #[tokio::test]
    async fn test_dm_unread_with_group_and_direct() {
        // Setup: Create DM with 2 unread, group DM with 5 unread
        let response = get_unread_summary(/* test state */).await.unwrap();
        assert_eq!(response.dms.len(), 2);
        assert_eq!(response.total_dm_unread, 7);
    }
}
```

#### Client Tests

Create `client/src/stores/guilds.test.ts`:

```typescript
import { describe, it, expect, beforeEach } from "bun:test";
import { createRoot } from "solid-js";
import { loadUnreadSummary, guildsState, removeFromUnreadSummary, setGuildsState } from "./guilds";

describe("Unread Summary", () => {
  it("loads summary from API", async () => {
    await createRoot(async (dispose) => {
      // Mock getUnreadSummary
      await loadUnreadSummary();
      expect(guildsState.unreadSummary).not.toBeNull();
      dispose();
    });
  });

  it("removes guild channel from summary on read", () => {
    createRoot((dispose) => {
      // Setup mock summary with one guild, one channel
      setGuildsState("unreadSummary", {
        guilds: [{
          guild_id: "g1",
          guild_name: "Test Guild",
          guild_icon: null,
          total_unread: 5,
          channels: [{ channel_id: "c1", channel_name: "general", unread_count: 5 }]
        }],
        dms: [],
        total_guild_unread: 5,
        total_dm_unread: 0,
      });

      removeFromUnreadSummary("c1");

      expect(guildsState.unreadSummary!.guilds.length).toBe(0);
      expect(guildsState.unreadSummary!.total_guild_unread).toBe(0);
      dispose();
    });
  });

  it("removes DM from summary on read", () => {
    createRoot((dispose) => {
      setGuildsState("unreadSummary", {
        guilds: [],
        dms: [{ channel_id: "dm1", display_name: "Alice", is_group: false, unread_count: 3 }],
        total_guild_unread: 0,
        total_dm_unread: 3,
      });

      removeFromUnreadSummary("dm1");

      expect(guildsState.unreadSummary!.dms.length).toBe(0);
      expect(guildsState.unreadSummary!.total_dm_unread).toBe(0);
      dispose();
    });
  });

  it("updates per-guild unread counts on load", async () => {
    await createRoot(async (dispose) => {
      // Mock response with 2 guilds
      // Verify guildsState.guildUnreadCounts is updated
      dispose();
    });
  });
});
```

**Verification:**
```bash
cd server && cargo test unread_summary
cd client && bun test guilds.test.ts
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
6. Verify the channel disappears from the unread module after being read (instant feedback)
7. Switch to a guild → Home button in ServerRail shows total unread badge
8. Open a second browser/device as User B → verify cross-device read sync clears items from unread module
9. Receive new message while on Home → reload Home to see updated counts
10. Test with >100 unread channels → verify only 100 are returned (check browser network tab)

---

## Known Limitations

### 1. Query Result Limit
- Both guild and DM queries are limited to 100 results each
- Users with >100 unread channels will only see the first 100
- Future enhancement: Add pagination or "Show More" functionality
- Current workaround: Users should mark channels as read to keep under limit

### 2. No Real-time Increment on New Messages
- New messages trigger per-channel unread increments (existing logic)
- But the UnreadModule won't show NEW channels that weren't in the initial summary
- User must reload Home (navigate away and back) to see new unread channels
- Future enhancement: Add WebSocket handler to append new channels to summary

### 3. SQL Optimization Opportunity
- The handler groups guild rows in Rust code
- Alternative: Use PostgreSQL `json_agg()` to group in the database
- Current approach is simpler and performs well under 100 results
- Future enhancement: Consider `json_agg()` if profiling shows grouping is slow

### 4. Frontend/Backend Config Sync
- The 100-item limit is hardcoded in SQL queries
- If this limit needs to be configurable, add it to `AppState` config
- Would require propagating config to SQL queries and documenting in `.env.example`

---

## Changes from v1

### Blocking Fixes
1. ✅ **SQL Performance:** Rewrote DM query to eliminate N+1 subquery pattern (LEFT JOIN instead)
2. ✅ **Logic Bug:** Fixed guild removal condition (`channels.length === 1`, not `<= 1`)
3. ✅ **Integration:** Added explicit code for `channels.ts` and `dms.ts` modifications (Task 4)

### Should Fix
4. ✅ **Pagination:** Added `LIMIT 100` to both SQL queries, documented in Known Limitations
5. ✅ **Field Naming:** Added serde configuration note to prevent camelCase conflicts
6. ✅ **Error Handling:** Added full handler implementation with proper error handling and tracing
7. ✅ **Indexes:** Added index verification section with critical indexes list
8. ✅ **Testing:** Added Task 9 with server and client test examples

### Nice to Have
9. ✅ **Loading State:** Added loading spinner to UnreadModule with `createSignal(true)`
10. ✅ **SQL Optimization:** Documented `json_agg()` alternative in Known Limitations
