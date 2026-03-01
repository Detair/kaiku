# Home View Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a three-column Discord-style Home view with DM sidebar, content area (Friends/DM conversation), and context-aware right panel.

**Architecture:** Server-side read state tracking with WebSocket broadcast for cross-device sync. Frontend uses dedicated DMs store with reactive UI components.

**Tech Stack:** Rust/Axum (backend), Solid.js/TypeScript (frontend), PostgreSQL (db), WebSocket (real-time)

---

## Task 1: Database Migration for dm_read_state

**Files:**
- Create: `server/migrations/20260113000000_add_dm_read_state.sql`

**Step 1: Create the migration file**

```sql
-- Add read state tracking for DM channels
CREATE TABLE dm_read_state (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    last_read_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_read_message_id UUID REFERENCES messages(id) ON DELETE SET NULL,
    PRIMARY KEY (user_id, channel_id)
);

-- Index for fast lookups by user
CREATE INDEX idx_dm_read_state_user ON dm_read_state(user_id);

-- Index for fast lookups by channel
CREATE INDEX idx_dm_read_state_channel ON dm_read_state(channel_id);
```

**Step 2: Verify migration syntax**

Run: `cd server && cargo sqlx prepare --check 2>&1 || echo "Migration not applied yet - OK"`

**Step 3: Commit**

```bash
git add server/migrations/
git commit -m "feat(db): add dm_read_state table for cross-device read sync"
```

---

## Task 2: Backend - Enhanced DM List Response

**Files:**
- Modify: `server/src/chat/dm.rs`
- Modify: `server/src/lib/types.ts` (later in frontend)

**Step 1: Add enhanced DMResponse struct with last_message and unread_count**

In `server/src/chat/dm.rs`, add after the existing `DMResponse` struct definition (around line 36):

```rust
/// Last message info for DM list preview
#[derive(Debug, Serialize)]
pub struct LastMessagePreview {
    pub id: Uuid,
    pub content: String,
    pub author_id: Uuid,
    pub author_username: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Enhanced DM response with unread count and last message
#[derive(Debug, Serialize)]
pub struct DMListResponse {
    #[serde(flatten)]
    pub channel: ChannelResponse,
    pub participants: Vec<DMParticipant>,
    pub last_message: Option<LastMessagePreview>,
    pub unread_count: i64,
}
```

**Step 2: Update list_dms handler to return enhanced response**

Replace the `list_dms` function (around line 293-309):

```rust
/// List all DM channels for the authenticated user
/// GET /api/dm
pub async fn list_dms(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<DMListResponse>>, ChannelError> {
    let channels = list_user_dms(&state.db, auth.id).await?;

    let mut responses = Vec::new();
    for channel in channels {
        let participants = get_dm_participants(&state.db, channel.id).await?;

        // Get last message
        let last_message = sqlx::query_as!(
            LastMessagePreview,
            r#"SELECT m.id, m.content, m.author_id, u.username as author_username, m.created_at
               FROM messages m
               JOIN users u ON u.id = m.author_id
               WHERE m.channel_id = $1
               ORDER BY m.created_at DESC
               LIMIT 1"#,
            channel.id
        )
        .fetch_optional(&state.db)
        .await?;

        // Get unread count
        let read_state = sqlx::query!(
            r#"SELECT last_read_at FROM dm_read_state
               WHERE user_id = $1 AND channel_id = $2"#,
            auth.id,
            channel.id
        )
        .fetch_optional(&state.db)
        .await?;

        let unread_count = if let Some(state) = read_state {
            sqlx::query!(
                r#"SELECT COUNT(*) as "count!" FROM messages
                   WHERE channel_id = $1 AND created_at > $2"#,
                channel.id,
                state.last_read_at
            )
            .fetch_one(&state.db)
            .await?
            .count
        } else {
            // No read state = all messages are unread
            sqlx::query!(
                r#"SELECT COUNT(*) as "count!" FROM messages WHERE channel_id = $1"#,
                channel.id
            )
            .fetch_one(&state.db)
            .await?
            .count
        };

        responses.push(DMListResponse {
            channel: channel.into(),
            participants,
            last_message,
            unread_count,
        });
    }

    // Sort by last message time (most recent first)
    responses.sort_by(|a, b| {
        let a_time = a.last_message.as_ref().map(|m| m.created_at);
        let b_time = b.last_message.as_ref().map(|m| m.created_at);
        b_time.cmp(&a_time)
    });

    Ok(Json(responses))
}
```

**Step 3: Build and verify**

Run: `cd server && cargo build 2>&1 | tail -20`
Expected: Build succeeds

**Step 4: Commit**

```bash
git add server/src/chat/dm.rs
git commit -m "feat(api): enhance GET /api/dm with last_message and unread_count"
```

---

## Task 3: Backend - Mark DM as Read Endpoint

**Files:**
- Modify: `server/src/chat/dm.rs`
- Modify: `server/src/chat/mod.rs`

**Step 1: Add mark_as_read handler in dm.rs**

Add at the end of `server/src/chat/dm.rs`:

```rust
/// Mark DM as read request body
#[derive(Debug, Deserialize)]
pub struct MarkAsReadRequest {
    pub last_read_message_id: Option<Uuid>,
}

/// Mark DM as read response
#[derive(Debug, Serialize)]
pub struct MarkAsReadResponse {
    pub channel_id: Uuid,
    pub last_read_at: chrono::DateTime<chrono::Utc>,
    pub last_read_message_id: Option<Uuid>,
    pub unread_count: i64,
}

/// Mark a DM channel as read
/// POST /api/dm/:id/read
pub async fn mark_as_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(body): Json<MarkAsReadRequest>,
) -> Result<Json<MarkAsReadResponse>, ChannelError> {
    // Verify channel exists and user is a participant
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChannelError::NotFound)?;

    if channel.channel_type != ChannelType::Dm {
        return Err(ChannelError::NotFound);
    }

    let is_participant = sqlx::query!(
        "SELECT EXISTS(SELECT 1 FROM dm_participants WHERE channel_id = $1 AND user_id = $2) as \"exists!\"",
        channel_id,
        auth.id
    )
    .fetch_one(&state.db)
    .await?
    .exists;

    if !is_participant {
        return Err(ChannelError::Forbidden);
    }

    let now = chrono::Utc::now();

    // Upsert read state
    sqlx::query!(
        r#"INSERT INTO dm_read_state (user_id, channel_id, last_read_at, last_read_message_id)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (user_id, channel_id)
           DO UPDATE SET last_read_at = $3, last_read_message_id = $4"#,
        auth.id,
        channel_id,
        now,
        body.last_read_message_id
    )
    .execute(&state.db)
    .await?;

    // TODO: Broadcast dm_read event to all user's WebSocket sessions

    Ok(Json(MarkAsReadResponse {
        channel_id,
        last_read_at: now,
        last_read_message_id: body.last_read_message_id,
        unread_count: 0,
    }))
}
```

**Step 2: Add route in dm_router**

In `server/src/chat/mod.rs`, update `dm_router`:

```rust
/// Create DM (Direct Message) router.
pub fn dm_router() -> Router<AppState> {
    Router::new()
        .route("/", get(dm::list_dms).post(dm::create_dm))
        .route("/:id", get(dm::get_dm))
        .route("/:id/leave", post(dm::leave_dm))
        .route("/:id/read", post(dm::mark_as_read))
}
```

**Step 3: Build and verify**

Run: `cd server && cargo build 2>&1 | tail -10`
Expected: Build succeeds

**Step 4: Commit**

```bash
git add server/src/chat/
git commit -m "feat(api): add POST /api/dm/:id/read endpoint for marking DMs as read"
```

---

## Task 4: Frontend - DMs Store

**Files:**
- Create: `client/src/stores/dms.ts`
- Modify: `client/src/lib/types.ts`

**Step 1: Add DM types to types.ts**

In `client/src/lib/types.ts`, add after the existing DMChannel interface:

```typescript
// Enhanced DM types for Home view

export interface LastMessagePreview {
  id: string;
  content: string;
  author_id: string;
  author_username: string;
  created_at: string;
}

export interface DMListItem {
  id: string;
  name: string;
  channel_type: ChannelType;
  category_id: string | null;
  guild_id: string | null;
  topic: string | null;
  user_limit: number | null;
  position: number;
  created_at: string;
  participants: DMParticipant[];
  last_message: LastMessagePreview | null;
  unread_count: number;
}
```

**Step 2: Create DMs store**

Create `client/src/stores/dms.ts`:

```typescript
/**
 * DMs Store
 *
 * Manages DM channels state for Home view.
 */

import { createStore } from "solid-js/store";
import type { DMListItem, Message } from "@/lib/types";
import * as tauri from "@/lib/tauri";

interface DMsStoreState {
  dms: DMListItem[];
  selectedDMId: string | null;
  isShowingFriends: boolean;
  typingUsers: Record<string, string[]>;
  isLoading: boolean;
  error: string | null;
}

const [dmsState, setDmsState] = createStore<DMsStoreState>({
  dms: [],
  selectedDMId: null,
  isShowingFriends: true,
  typingUsers: {},
  isLoading: false,
  error: null,
});

/**
 * Load all DMs for the current user
 */
export async function loadDMs(): Promise<void> {
  setDmsState({ isLoading: true, error: null });

  try {
    const dms = await tauri.getDMList();
    setDmsState({ dms, isLoading: false });
  } catch (err) {
    console.error("Failed to load DMs:", err);
    setDmsState({
      error: err instanceof Error ? err.message : "Failed to load DMs",
      isLoading: false,
    });
  }
}

/**
 * Select a DM to view
 */
export function selectDM(channelId: string): void {
  setDmsState({
    selectedDMId: channelId,
    isShowingFriends: false,
  });
}

/**
 * Switch to Friends tab
 */
export function selectFriendsTab(): void {
  setDmsState({
    selectedDMId: null,
    isShowingFriends: true,
  });
}

/**
 * Update last message for a DM (from WebSocket)
 */
export function updateDMLastMessage(channelId: string, message: Message): void {
  const dmIndex = dmsState.dms.findIndex((d) => d.id === channelId);
  if (dmIndex === -1) return;

  setDmsState("dms", dmIndex, {
    last_message: {
      id: message.id,
      content: message.content,
      author_id: message.author.id,
      author_username: message.author.username,
      created_at: message.created_at,
    },
    unread_count: dmsState.dms[dmIndex].unread_count + 1,
  });

  // Re-sort DMs by last message time
  const sortedDMs = [...dmsState.dms].sort((a, b) => {
    const aTime = a.last_message?.created_at || a.created_at;
    const bTime = b.last_message?.created_at || b.created_at;
    return new Date(bTime).getTime() - new Date(aTime).getTime();
  });
  setDmsState({ dms: sortedDMs });
}

/**
 * Mark DM as read (called when viewing a DM)
 */
export async function markDMAsRead(channelId: string): Promise<void> {
  const dm = dmsState.dms.find((d) => d.id === channelId);
  if (!dm || dm.unread_count === 0) return;

  try {
    await tauri.markDMAsRead(channelId, dm.last_message?.id);

    const dmIndex = dmsState.dms.findIndex((d) => d.id === channelId);
    if (dmIndex !== -1) {
      setDmsState("dms", dmIndex, "unread_count", 0);
    }
  } catch (err) {
    console.error("Failed to mark DM as read:", err);
  }
}

/**
 * Handle dm_read event from WebSocket (cross-device sync)
 */
export function handleDMReadEvent(channelId: string): void {
  const dmIndex = dmsState.dms.findIndex((d) => d.id === channelId);
  if (dmIndex !== -1) {
    setDmsState("dms", dmIndex, "unread_count", 0);
  }
}

/**
 * Get the currently selected DM
 */
export function getSelectedDM(): DMListItem | null {
  if (!dmsState.selectedDMId) return null;
  return dmsState.dms.find((d) => d.id === dmsState.selectedDMId) || null;
}

/**
 * Get total unread count across all DMs
 */
export function getTotalUnreadCount(): number {
  return dmsState.dms.reduce((sum, dm) => sum + dm.unread_count, 0);
}

export { dmsState };
```

**Step 3: Add API function to tauri.ts**

In `client/src/lib/tauri.ts`, add after the existing DM functions:

```typescript
export async function getDMList(): Promise<DMListItem[]> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_dm_list");
  }

  return httpRequest<DMListItem[]>("GET", "/api/dm");
}

export async function markDMAsRead(
  channelId: string,
  lastReadMessageId?: string
): Promise<void> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("mark_dm_as_read", { channelId, lastReadMessageId });
  }

  await httpRequest<void>("POST", `/api/dm/${channelId}/read`, {
    last_read_message_id: lastReadMessageId,
  });
}
```

Also add to the imports at the top:
```typescript
import type {
  // ... existing imports
  DMListItem,
} from "./types";

export type { /* ... existing */ DMListItem };
```

**Step 4: Build and verify**

Run: `cd client && bun run build 2>&1 | tail -10`
Expected: Build succeeds

**Step 5: Commit**

```bash
git add client/src/stores/dms.ts client/src/lib/types.ts client/src/lib/tauri.ts
git commit -m "feat(client): add DMs store with cross-device read sync support"
```

---

## Task 5: Frontend - DMItem Component

**Files:**
- Create: `client/src/components/home/DMItem.tsx`

**Step 1: Create DMItem component**

Create `client/src/components/home/DMItem.tsx`:

```typescript
/**
 * DMItem Component
 *
 * Displays a single DM in the sidebar list.
 */

import { Component, Show } from "solid-js";
import type { DMListItem } from "@/lib/types";
import { dmsState, selectDM } from "@/stores/dms";

interface DMItemProps {
  dm: DMListItem;
}

const DMItem: Component<DMItemProps> = (props) => {
  const isSelected = () => dmsState.selectedDMId === props.dm.id;

  // Get the other participant(s) for display
  const displayName = () => {
    if (props.dm.participants.length === 1) {
      return props.dm.participants[0].display_name;
    }
    return props.dm.name || props.dm.participants.map(p => p.display_name).join(", ");
  };

  const isGroupDM = () => props.dm.participants.length > 1;

  // Get online status for 1:1 DMs
  const isOnline = () => {
    if (isGroupDM()) return false;
    // TODO: Check presence store for online status
    return false;
  };

  const formatTimestamp = (dateStr: string) => {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffMins < 1) return "now";
    if (diffMins < 60) return `${diffMins}m`;
    if (diffHours < 24) return `${diffHours}h`;
    if (diffDays < 7) return `${diffDays}d`;
    return date.toLocaleDateString();
  };

  const lastMessagePreview = () => {
    const msg = props.dm.last_message;
    if (!msg) return "No messages yet";

    const prefix = isGroupDM() ? `${msg.author_username}: ` : "";
    const content = msg.content.length > 30
      ? msg.content.substring(0, 30) + "..."
      : msg.content;
    return prefix + content;
  };

  return (
    <button
      onClick={() => selectDM(props.dm.id)}
      class="w-full flex items-start gap-3 p-2 rounded-lg transition-colors text-left"
      classList={{
        "bg-white/10": isSelected(),
        "hover:bg-white/5": !isSelected(),
      }}
    >
      {/* Avatar */}
      <div class="relative flex-shrink-0">
        <Show
          when={isGroupDM()}
          fallback={
            <div class="w-10 h-10 rounded-full bg-accent-primary flex items-center justify-center">
              <span class="text-sm font-semibold text-surface-base">
                {props.dm.participants[0]?.display_name?.charAt(0).toUpperCase() || "?"}
              </span>
            </div>
          }
        >
          <div class="w-10 h-10 rounded-full bg-surface-layer2 flex items-center justify-center">
            <svg class="w-5 h-5 text-text-secondary" fill="currentColor" viewBox="0 0 20 20">
              <path d="M13 6a3 3 0 11-6 0 3 3 0 016 0zM18 8a2 2 0 11-4 0 2 2 0 014 0zM14 15a4 4 0 00-8 0v3h8v-3zM6 8a2 2 0 11-4 0 2 2 0 014 0zM16 18v-3a5.972 5.972 0 00-.75-2.906A3.005 3.005 0 0119 15v3h-3zM4.75 12.094A5.973 5.973 0 004 15v3H1v-3a3 3 0 013.75-2.906z" />
            </svg>
          </div>
        </Show>

        {/* Online indicator for 1:1 DMs */}
        <Show when={!isGroupDM() && isOnline()}>
          <div class="absolute bottom-0 right-0 w-3 h-3 bg-green-500 border-2 border-surface-base rounded-full" />
        </Show>
      </div>

      {/* Content */}
      <div class="flex-1 min-w-0">
        <div class="flex items-center justify-between gap-2">
          <span class="font-medium text-text-primary truncate">
            {displayName()}
          </span>
          <Show when={props.dm.last_message}>
            <span class="text-xs text-text-secondary flex-shrink-0">
              {formatTimestamp(props.dm.last_message!.created_at)}
            </span>
          </Show>
        </div>

        <div class="flex items-center gap-2">
          <span class="text-sm text-text-secondary truncate flex-1">
            {lastMessagePreview()}
          </span>

          {/* Unread badge */}
          <Show when={props.dm.unread_count > 0}>
            <span class="flex-shrink-0 min-w-5 h-5 px-1.5 bg-accent-primary text-surface-base text-xs font-bold rounded-full flex items-center justify-center">
              {props.dm.unread_count > 99 ? "99+" : props.dm.unread_count}
            </span>
          </Show>
        </div>
      </div>
    </button>
  );
};

export default DMItem;
```

**Step 2: Build and verify**

Run: `cd client && bun run build 2>&1 | tail -10`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add client/src/components/home/DMItem.tsx
git commit -m "feat(client): add DMItem component with unread badges and previews"
```

---

## Task 6: Frontend - DMSidebar Component

**Files:**
- Create: `client/src/components/home/DMSidebar.tsx`

**Step 1: Create DMSidebar component**

Create `client/src/components/home/DMSidebar.tsx`:

```typescript
/**
 * DMSidebar Component
 *
 * Left column of Home view with Friends tab and DM list.
 */

import { Component, For, Show, createSignal, onMount } from "solid-js";
import { Users, Plus } from "lucide-solid";
import { dmsState, loadDMs, selectFriendsTab } from "@/stores/dms";
import DMItem from "./DMItem";
import NewMessageModal from "./NewMessageModal";

const DMSidebar: Component = () => {
  const [showNewMessage, setShowNewMessage] = createSignal(false);

  onMount(() => {
    loadDMs();
  });

  return (
    <aside class="w-60 flex flex-col bg-surface-layer1 border-r border-white/5">
      {/* Friends Tab */}
      <button
        onClick={() => selectFriendsTab()}
        class="flex items-center gap-3 px-3 py-2 mx-2 mt-2 rounded-lg transition-colors"
        classList={{
          "bg-white/10": dmsState.isShowingFriends,
          "hover:bg-white/5": !dmsState.isShowingFriends,
        }}
      >
        <Users class="w-5 h-5 text-text-secondary" />
        <span class="font-medium text-text-primary">Friends</span>
      </button>

      {/* Separator */}
      <div class="mx-3 my-2 border-t border-white/10" />

      {/* Direct Messages Header */}
      <div class="flex items-center justify-between px-3 py-1">
        <span class="text-xs font-semibold text-text-secondary uppercase tracking-wide">
          Direct Messages
        </span>
        <button
          onClick={() => setShowNewMessage(true)}
          class="p-1 rounded hover:bg-white/10 transition-colors"
          title="New Message"
        >
          <Plus class="w-4 h-4 text-text-secondary" />
        </button>
      </div>

      {/* DM List */}
      <div class="flex-1 overflow-y-auto px-2 py-1 space-y-0.5">
        <Show
          when={!dmsState.isLoading}
          fallback={
            <div class="flex items-center justify-center py-8">
              <span class="text-text-secondary text-sm">Loading...</span>
            </div>
          }
        >
          <Show
            when={dmsState.dms.length > 0}
            fallback={
              <div class="text-center py-8 px-4">
                <p class="text-text-secondary text-sm">No conversations yet</p>
                <button
                  onClick={() => setShowNewMessage(true)}
                  class="mt-2 text-accent-primary text-sm hover:underline"
                >
                  Start a conversation
                </button>
              </div>
            }
          >
            <For each={dmsState.dms}>
              {(dm) => <DMItem dm={dm} />}
            </For>
          </Show>
        </Show>
      </div>

      {/* New Message Modal */}
      <Show when={showNewMessage()}>
        <NewMessageModal onClose={() => setShowNewMessage(false)} />
      </Show>
    </aside>
  );
};

export default DMSidebar;
```

**Step 2: Build and verify**

Run: `cd client && bun run build 2>&1 | tail -10`
Expected: Build fails (NewMessageModal not yet created - expected)

**Step 3: Commit partial progress**

```bash
git add client/src/components/home/DMSidebar.tsx
git commit -m "feat(client): add DMSidebar component with Friends tab and DM list"
```

---

## Task 7: Frontend - NewMessageModal Component

**Files:**
- Create: `client/src/components/home/NewMessageModal.tsx`

**Step 1: Create NewMessageModal component**

Create `client/src/components/home/NewMessageModal.tsx`:

```typescript
/**
 * NewMessageModal Component
 *
 * Modal for creating a new DM conversation.
 */

import { Component, createSignal, For, Show, createMemo } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Search, Check } from "lucide-solid";
import { friendsState, loadFriends } from "@/stores/friends";
import { dmsState, loadDMs, selectDM } from "@/stores/dms";
import * as tauri from "@/lib/tauri";
import type { Friend } from "@/lib/types";

interface NewMessageModalProps {
  onClose: () => void;
}

const NewMessageModal: Component<NewMessageModalProps> = (props) => {
  const [search, setSearch] = createSignal("");
  const [selectedIds, setSelectedIds] = createSignal<string[]>([]);
  const [isCreating, setIsCreating] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  // Load friends if not already loaded
  if (friendsState.friends.length === 0) {
    loadFriends();
  }

  const filteredFriends = createMemo(() => {
    const searchLower = search().toLowerCase();
    if (!searchLower) return friendsState.friends;
    return friendsState.friends.filter(
      (f) =>
        f.username.toLowerCase().includes(searchLower) ||
        f.display_name.toLowerCase().includes(searchLower)
    );
  });

  const toggleFriend = (userId: string) => {
    setSelectedIds((prev) =>
      prev.includes(userId)
        ? prev.filter((id) => id !== userId)
        : [...prev, userId]
    );
  };

  const handleCreate = async () => {
    if (selectedIds().length === 0) return;

    setIsCreating(true);
    setError(null);

    try {
      const dm = await tauri.createDM(selectedIds());
      await loadDMs();
      selectDM(dm.channel.id);
      props.onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create conversation");
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
        onClick={props.onClose}
      >
        <div
          class="bg-surface-base border border-white/10 rounded-2xl w-full max-w-md flex flex-col max-h-[80vh]"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div class="flex items-center justify-between p-4 border-b border-white/10">
            <h2 class="text-lg font-bold text-text-primary">New Message</h2>
            <button
              onClick={props.onClose}
              class="p-1 rounded hover:bg-white/10 transition-colors"
            >
              <X class="w-5 h-5 text-text-secondary" />
            </button>
          </div>

          {/* Search */}
          <div class="p-4 border-b border-white/10">
            <div class="relative">
              <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
              <input
                type="text"
                value={search()}
                onInput={(e) => setSearch(e.currentTarget.value)}
                placeholder="Search friends..."
                class="w-full pl-9 pr-4 py-2 bg-surface-layer1 border border-white/10 rounded-lg text-text-primary placeholder-text-secondary focus:outline-none focus:border-accent-primary"
              />
            </div>
            <Show when={selectedIds().length > 0}>
              <p class="mt-2 text-sm text-text-secondary">
                {selectedIds().length} friend{selectedIds().length > 1 ? "s" : ""} selected
                {selectedIds().length > 1 && " (Group DM)"}
              </p>
            </Show>
          </div>

          {/* Friends List */}
          <div class="flex-1 overflow-y-auto p-2">
            <Show
              when={filteredFriends().length > 0}
              fallback={
                <div class="text-center py-8 text-text-secondary">
                  {search() ? "No friends found" : "No friends to message"}
                </div>
              }
            >
              <For each={filteredFriends()}>
                {(friend) => (
                  <FriendSelectItem
                    friend={friend}
                    selected={selectedIds().includes(friend.user_id)}
                    onToggle={() => toggleFriend(friend.user_id)}
                  />
                )}
              </For>
            </Show>
          </div>

          {/* Error */}
          <Show when={error()}>
            <div class="mx-4 mb-2 p-3 bg-red-500/10 border border-red-500/20 rounded-lg text-red-400 text-sm">
              {error()}
            </div>
          </Show>

          {/* Footer */}
          <div class="p-4 border-t border-white/10">
            <button
              onClick={handleCreate}
              disabled={selectedIds().length === 0 || isCreating()}
              class="w-full py-2 bg-accent-primary text-surface-base rounded-lg font-medium hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {isCreating()
                ? "Creating..."
                : selectedIds().length > 1
                ? "Create Group DM"
                : "Create DM"}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};

interface FriendSelectItemProps {
  friend: Friend;
  selected: boolean;
  onToggle: () => void;
}

const FriendSelectItem: Component<FriendSelectItemProps> = (props) => {
  return (
    <button
      onClick={props.onToggle}
      class="w-full flex items-center gap-3 p-2 rounded-lg hover:bg-white/5 transition-colors"
    >
      {/* Checkbox */}
      <div
        class="w-5 h-5 rounded border-2 flex items-center justify-center transition-colors"
        classList={{
          "border-accent-primary bg-accent-primary": props.selected,
          "border-white/30": !props.selected,
        }}
      >
        <Show when={props.selected}>
          <Check class="w-3 h-3 text-surface-base" />
        </Show>
      </div>

      {/* Avatar */}
      <div class="w-8 h-8 rounded-full bg-accent-primary flex items-center justify-center">
        <span class="text-xs font-semibold text-surface-base">
          {props.friend.display_name.charAt(0).toUpperCase()}
        </span>
      </div>

      {/* Name */}
      <div class="flex-1 text-left">
        <div class="font-medium text-text-primary">{props.friend.display_name}</div>
        <div class="text-sm text-text-secondary">@{props.friend.username}</div>
      </div>
    </button>
  );
};

export default NewMessageModal;
```

**Step 2: Build and verify**

Run: `cd client && bun run build 2>&1 | tail -10`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add client/src/components/home/NewMessageModal.tsx
git commit -m "feat(client): add NewMessageModal for creating DM conversations"
```

---

## Task 8: Frontend - HomeView Container

**Files:**
- Create: `client/src/components/home/HomeView.tsx`
- Create: `client/src/components/home/DMConversation.tsx`
- Create: `client/src/components/home/index.ts`

**Step 1: Create DMConversation component**

Create `client/src/components/home/DMConversation.tsx`:

```typescript
/**
 * DMConversation Component
 *
 * Displays a DM conversation in the Home view.
 */

import { Component, Show, onMount, onCleanup, createEffect } from "solid-js";
import { dmsState, getSelectedDM, markDMAsRead } from "@/stores/dms";
import MessageList from "@/components/messages/MessageList";
import MessageInput from "@/components/messages/MessageInput";
import TypingIndicator from "@/components/messages/TypingIndicator";

const DMConversation: Component = () => {
  const dm = () => getSelectedDM();

  // Mark as read when viewing
  createEffect(() => {
    const currentDM = dm();
    if (currentDM && currentDM.unread_count > 0) {
      // Debounce: wait 1 second before marking as read
      const timer = setTimeout(() => {
        markDMAsRead(currentDM.id);
      }, 1000);
      onCleanup(() => clearTimeout(timer));
    }
  });

  const displayName = () => {
    const currentDM = dm();
    if (!currentDM) return "";
    if (currentDM.participants.length === 1) {
      return currentDM.participants[0].display_name;
    }
    return currentDM.name || currentDM.participants.map(p => p.display_name).join(", ");
  };

  const isGroupDM = () => {
    const currentDM = dm();
    return currentDM ? currentDM.participants.length > 1 : false;
  };

  return (
    <Show
      when={dm()}
      fallback={
        <div class="flex-1 flex items-center justify-center bg-surface-layer1">
          <p class="text-text-secondary">Select a conversation</p>
        </div>
      }
    >
      <div class="flex-1 flex flex-col bg-surface-layer1">
        {/* Header */}
        <header class="h-12 px-4 flex items-center gap-3 border-b border-white/5 bg-surface-layer1 shadow-sm">
          <Show
            when={isGroupDM()}
            fallback={
              <div class="w-8 h-8 rounded-full bg-accent-primary flex items-center justify-center">
                <span class="text-sm font-semibold text-surface-base">
                  {dm()?.participants[0]?.display_name?.charAt(0).toUpperCase()}
                </span>
              </div>
            }
          >
            <div class="w-8 h-8 rounded-full bg-surface-layer2 flex items-center justify-center">
              <svg class="w-4 h-4 text-text-secondary" fill="currentColor" viewBox="0 0 20 20">
                <path d="M13 6a3 3 0 11-6 0 3 3 0 016 0zM18 8a2 2 0 11-4 0 2 2 0 014 0zM14 15a4 4 0 00-8 0v3h8v-3z" />
              </svg>
            </div>
          </Show>
          <span class="font-semibold text-text-primary">{displayName()}</span>
          <Show when={isGroupDM()}>
            <span class="text-sm text-text-secondary">
              {dm()?.participants.length} members
            </span>
          </Show>
        </header>

        {/* Messages */}
        <MessageList channelId={dm()!.id} />

        {/* Typing Indicator */}
        <TypingIndicator channelId={dm()!.id} />

        {/* Message Input */}
        <MessageInput channelId={dm()!.id} channelName={displayName()} />
      </div>
    </Show>
  );
};

export default DMConversation;
```

**Step 2: Create HomeView container**

Create `client/src/components/home/HomeView.tsx`:

```typescript
/**
 * HomeView Component
 *
 * Three-column layout for Home view (when no guild selected).
 */

import { Component, Show } from "solid-js";
import { dmsState } from "@/stores/dms";
import { FriendsList } from "@/components/social";
import DMSidebar from "./DMSidebar";
import DMConversation from "./DMConversation";
import HomeRightPanel from "./HomeRightPanel";

const HomeView: Component = () => {
  return (
    <div class="flex-1 flex h-full">
      {/* Left: DM Sidebar */}
      <DMSidebar />

      {/* Middle: Content (Friends or DM Conversation) */}
      <div class="flex-1 flex flex-col">
        <Show when={dmsState.isShowingFriends} fallback={<DMConversation />}>
          <FriendsList />
        </Show>
      </div>

      {/* Right: Context Panel (hidden on smaller screens) */}
      <HomeRightPanel />
    </div>
  );
};

export default HomeView;
```

**Step 3: Create placeholder HomeRightPanel**

Create `client/src/components/home/HomeRightPanel.tsx`:

```typescript
/**
 * HomeRightPanel Component
 *
 * Context-aware right panel for Home view.
 * Shows user profile for 1:1 DM, participants for group DM.
 */

import { Component, Show } from "solid-js";
import { dmsState, getSelectedDM } from "@/stores/dms";
import { getOnlineFriends } from "@/stores/friends";

const HomeRightPanel: Component = () => {
  const dm = () => getSelectedDM();
  const isGroupDM = () => dm()?.participants && dm()!.participants.length > 1;

  // Hide on smaller screens
  return (
    <aside class="hidden xl:flex w-60 flex-col bg-surface-layer1 border-l border-white/5">
      <Show
        when={!dmsState.isShowingFriends && dm()}
        fallback={
          // Friends view - show online count
          <div class="p-4">
            <div class="text-sm text-text-secondary">
              Online — {getOnlineFriends().length}
            </div>
          </div>
        }
      >
        <Show
          when={isGroupDM()}
          fallback={
            // 1:1 DM - show user profile
            <div class="p-4">
              <div class="flex flex-col items-center">
                <div class="w-20 h-20 rounded-full bg-accent-primary flex items-center justify-center mb-3">
                  <span class="text-2xl font-bold text-surface-base">
                    {dm()?.participants[0]?.display_name?.charAt(0).toUpperCase()}
                  </span>
                </div>
                <h3 class="text-lg font-semibold text-text-primary">
                  {dm()?.participants[0]?.display_name}
                </h3>
                <p class="text-sm text-text-secondary">
                  @{dm()?.participants[0]?.username}
                </p>
              </div>
            </div>
          }
        >
          {/* Group DM - show participants */}
          <div class="p-4">
            <h3 class="text-sm font-semibold text-text-secondary uppercase tracking-wide mb-3">
              Members — {dm()?.participants.length}
            </h3>
            <div class="space-y-2">
              {dm()?.participants.map((p) => (
                <div class="flex items-center gap-2">
                  <div class="w-8 h-8 rounded-full bg-accent-primary flex items-center justify-center">
                    <span class="text-xs font-semibold text-surface-base">
                      {p.display_name.charAt(0).toUpperCase()}
                    </span>
                  </div>
                  <span class="text-sm text-text-primary">{p.display_name}</span>
                </div>
              ))}
            </div>
          </div>
        </Show>
      </Show>
    </aside>
  );
};

export default HomeRightPanel;
```

**Step 4: Create barrel export**

Create `client/src/components/home/index.ts`:

```typescript
export { default as HomeView } from "./HomeView";
export { default as DMSidebar } from "./DMSidebar";
export { default as DMItem } from "./DMItem";
export { default as DMConversation } from "./DMConversation";
export { default as HomeRightPanel } from "./HomeRightPanel";
export { default as NewMessageModal } from "./NewMessageModal";
```

**Step 5: Build and verify**

Run: `cd client && bun run build 2>&1 | tail -10`
Expected: Build succeeds

**Step 6: Commit**

```bash
git add client/src/components/home/
git commit -m "feat(client): add HomeView container with DMConversation and right panel"
```

---

## Task 9: Frontend - Integrate HomeView into Main

**Files:**
- Modify: `client/src/views/Main.tsx`

**Step 1: Replace FriendsList with HomeView**

Update `client/src/views/Main.tsx` to use HomeView:

```typescript
/**
 * Main View - Primary Application Interface
 */

import { Component, Show, onMount } from "solid-js";
import { Hash, Volume2 } from "lucide-solid";
import AppShell from "@/components/layout/AppShell";
import CommandPalette from "@/components/layout/CommandPalette";
import MessageList from "@/components/messages/MessageList";
import MessageInput from "@/components/messages/MessageInput";
import TypingIndicator from "@/components/messages/TypingIndicator";
import { HomeView } from "@/components/home";
import { selectedChannel } from "@/stores/channels";
import { loadGuilds, guildsState } from "@/stores/guilds";

const Main: Component = () => {
  const channel = selectedChannel;

  onMount(() => {
    loadGuilds();
  });

  return (
    <>
      <CommandPalette />

      <AppShell showServerRail={true}>
        <Show
          when={guildsState.activeGuildId !== null}
          fallback={<HomeView />}
        >
          <Show
            when={channel()}
            fallback={
              <div class="flex-1 flex items-center justify-center bg-surface-layer1">
                <div class="text-center text-text-secondary">
                  <Hash class="w-12 h-12 mx-auto mb-4 opacity-30" />
                  <p class="text-lg font-medium">Select a channel to start chatting</p>
                  <p class="text-sm mt-2 opacity-60">Or press Ctrl+K to search</p>
                </div>
              </div>
            }
          >
            <header class="h-12 px-4 flex items-center border-b border-white/5 bg-surface-layer1 shadow-sm">
              <Show
                when={channel()?.channel_type === "voice"}
                fallback={<Hash class="w-5 h-5 text-text-secondary mr-2" />}
              >
                <Volume2 class="w-5 h-5 text-text-secondary mr-2" />
              </Show>
              <span class="font-semibold text-text-primary">{channel()?.name}</span>
              <Show when={channel()?.topic}>
                <div class="ml-4 pl-4 border-l border-white/10 text-text-secondary text-sm truncate">
                  {channel()?.topic}
                </div>
              </Show>
            </header>

            <MessageList channelId={channel()!.id} />
            <TypingIndicator channelId={channel()!.id} />
            <MessageInput channelId={channel()!.id} channelName={channel()!.name} />
          </Show>
        </Show>
      </AppShell>
    </>
  );
};

export default Main;
```

**Step 2: Build and verify**

Run: `cd client && bun run build 2>&1 | tail -10`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add client/src/views/Main.tsx
git commit -m "feat(client): integrate HomeView as default when no guild selected"
```

---

## Task 10: Integration Testing

**Step 1: Start the server**

Run: `cd server && cargo run`

**Step 2: Start the client dev server**

Run: `cd client && bun run dev`

**Step 3: Manual testing checklist**

Test in browser at http://localhost:5173:

1. [ ] Login and see Home view by default (Friends tab selected)
2. [ ] Click Home icon in ServerRail → shows Friends list
3. [ ] Click "+" in DM sidebar → NewMessageModal opens
4. [ ] Select a friend → Create DM → DM appears in list
5. [ ] Click DM → conversation loads in middle column
6. [ ] Send message → appears in conversation
7. [ ] Unread badge shows on DM when new message arrives
8. [ ] Click guild in ServerRail → switches to guild view
9. [ ] Click Home again → returns to Home view with DM list

**Step 4: Final commit**

```bash
git add -A
git commit -m "feat: complete Home view implementation with DM list and conversations"
```

---

## Files Summary

### Created (10 files):
- `server/migrations/20260113000000_add_dm_read_state.sql`
- `client/src/stores/dms.ts`
- `client/src/components/home/DMItem.tsx`
- `client/src/components/home/DMSidebar.tsx`
- `client/src/components/home/NewMessageModal.tsx`
- `client/src/components/home/DMConversation.tsx`
- `client/src/components/home/HomeView.tsx`
- `client/src/components/home/HomeRightPanel.tsx`
- `client/src/components/home/index.ts`

### Modified (4 files):
- `server/src/chat/dm.rs` - Enhanced list_dms, added mark_as_read
- `server/src/chat/mod.rs` - Added route for mark_as_read
- `client/src/lib/types.ts` - Added DMListItem, LastMessagePreview
- `client/src/lib/tauri.ts` - Added getDMList, markDMAsRead
- `client/src/views/Main.tsx` - Replaced FriendsList with HomeView
