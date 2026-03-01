# Phase 3: Guild Architecture & Security Implementation Plan

**Goal:** Transform from "Simple Chat" to "Multi-Server Platform" (Discord-like architecture)

**Prerequisites:** Phase 2 complete, familiar with Rust/Axum and Solid.js

---

## Executive Summary

This phase introduces the guild (server) concept, enabling users to create and join multiple servers. Each server has its own channels, members, and roles. We also add a social layer (friends, DMs) and improve security with rate limiting.

### Key Deliverables
1. **Guild Entity** - Database, API, and UI for servers
2. **Server Rail** - Left sidebar showing user's servers
3. **Friends & Status** - Social graph with friend requests
4. **Direct Messages** - 1:1 and group DMs (up to 10 people)
5. **Home View** - Dashboard with DMs, mentions, activity
6. **Guild-Scoped RBAC** - Permissions per server
7. **Rate Limiting** - Protection against spam/DoS

---

## Architecture Changes

### Database Schema Changes

```sql
-- New migration: 20240201000000_guilds.sql

-- Guilds (Servers)
CREATE TABLE guilds (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(100) NOT NULL,
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    icon_url TEXT,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_guilds_owner ON guilds(owner_id);

-- Guild Members
CREATE TABLE guild_members (
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    nickname VARCHAR(64),
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (guild_id, user_id)
);

CREATE INDEX idx_guild_members_user ON guild_members(user_id);

-- Add guild_id to existing tables
ALTER TABLE channels ADD COLUMN guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE;
ALTER TABLE roles ADD COLUMN guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE;
ALTER TABLE channel_categories ADD COLUMN guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE;

-- Guild-scoped roles
CREATE TABLE guild_member_roles (
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (guild_id, user_id, role_id)
);

-- Friendships
CREATE TYPE friendship_status AS ENUM ('pending', 'accepted', 'blocked');

CREATE TABLE friendships (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    requester_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    addressee_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status friendship_status NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (requester_id, addressee_id),
    CONSTRAINT no_self_friendship CHECK (requester_id != addressee_id)
);

CREATE INDEX idx_friendships_users ON friendships(requester_id, addressee_id);
CREATE INDEX idx_friendships_addressee ON friendships(addressee_id);

-- User status message
ALTER TABLE users ADD COLUMN status_message VARCHAR(128);
ALTER TABLE users ADD COLUMN invisible BOOLEAN NOT NULL DEFAULT FALSE;

-- DM Channels (reuse channels table with type='dm')
-- dm_participants for group DMs
CREATE TABLE dm_participants (
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (channel_id, user_id)
);

CREATE INDEX idx_dm_participants_user ON dm_participants(user_id);
```

### API Endpoints

```
# Guilds
POST   /api/guilds                    - Create guild
GET    /api/guilds                    - List user's guilds
GET    /api/guilds/:id                - Get guild details
PATCH  /api/guilds/:id                - Update guild
DELETE /api/guilds/:id                - Delete guild (owner only)
POST   /api/guilds/:id/join           - Join guild (with invite code)
POST   /api/guilds/:id/leave          - Leave guild
GET    /api/guilds/:id/members        - List guild members
GET    /api/guilds/:id/channels       - List guild channels (replaces /api/channels)

# Friends
GET    /api/friends                   - List friends
POST   /api/friends/request           - Send friend request
POST   /api/friends/:id/accept        - Accept friend request
POST   /api/friends/:id/reject        - Reject friend request
POST   /api/friends/:id/block         - Block user
DELETE /api/friends/:id               - Remove friend

# DMs
GET    /api/dm                        - List DM channels
POST   /api/dm                        - Create DM (1:1 or group)
GET    /api/dm/:id/messages           - Get DM messages
POST   /api/dm/:id/messages           - Send DM message
POST   /api/dm/:id/leave              - Leave group DM

# User Status
PATCH  /api/users/me/status           - Update status/message
```

---

## Implementation Tasks

### Task 1: Guild Database Migration
**Files to create/modify:**
- `server/migrations/20240201000000_guilds.sql` (CREATE)

**Steps:**
1. Create the migration file with guild tables
2. Add guild_id to channels, roles, channel_categories
3. Create guild_members and guild_member_roles tables
4. Create friendships table
5. Create dm_participants table
6. Run migration: `sqlx migrate run`

**Verification:**
```bash
sqlx migrate run
psql -d voicechat -c "\dt"  # Should show new tables
```

---

### Task 2: Guild Backend (Rust)
**Files to create/modify:**
- `server/src/guild/mod.rs` (CREATE)
- `server/src/guild/handlers.rs` (CREATE)
- `server/src/guild/types.rs` (CREATE)
- `server/src/db/queries.rs` (MODIFY - add guild queries)
- `server/src/api/mod.rs` (MODIFY - add guild routes)
- `server/src/lib.rs` (MODIFY - add guild module)

**Guild Module Structure:**
```rust
// server/src/guild/mod.rs
pub mod handlers;
pub mod types;

use axum::{routing::{get, post, patch, delete}, Router};
use crate::api::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::list_guilds).post(handlers::create_guild))
        .route("/:id", get(handlers::get_guild).patch(handlers::update_guild).delete(handlers::delete_guild))
        .route("/:id/join", post(handlers::join_guild))
        .route("/:id/leave", post(handlers::leave_guild))
        .route("/:id/members", get(handlers::list_members))
        .route("/:id/channels", get(handlers::list_channels))
}

// server/src/guild/types.rs
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct Guild {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub icon_url: Option<String>,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateGuildRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateGuildRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub icon_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GuildMember {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub nickname: Option<String>,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}
```

**Handlers Template:**
```rust
// server/src/guild/handlers.rs
use axum::{extract::{Path, State}, Json};
use uuid::Uuid;
use crate::{api::AppState, auth::AuthUser, db};
use super::types::*;

pub async fn create_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateGuildRequest>,
) -> Result<Json<Guild>, GuildError> {
    // 1. Validate name (2-100 chars)
    // 2. Insert guild with owner_id = auth.id
    // 3. Add owner as member
    // 4. Create default @everyone role
    // 5. Return guild
}

pub async fn list_guilds(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Guild>>, GuildError> {
    // Return all guilds where user is a member
}

// ... other handlers
```

**Verification:**
```bash
cargo build
cargo test guild
```

---

### Task 3: Modify Channels for Guild Scope
**Files to modify:**
- `server/src/chat/channels.rs`
- `server/src/chat/messages.rs`
- `server/src/db/queries.rs`

**Changes:**
1. Add `guild_id` parameter to channel creation
2. Filter channels by `guild_id` in list endpoint
3. Verify guild membership before channel operations
4. Update WebSocket events to include `guild_id`

**Key Query Changes:**
```rust
// Get channels for a guild
pub async fn get_guild_channels(pool: &PgPool, guild_id: Uuid) -> sqlx::Result<Vec<Channel>> {
    sqlx::query_as!(
        Channel,
        r#"SELECT id, name, channel_type as "channel_type: _", topic, guild_id, created_at
           FROM channels WHERE guild_id = $1 ORDER BY position"#,
        guild_id
    )
    .fetch_all(pool)
    .await
}

// Verify user is guild member
pub async fn is_guild_member(pool: &PgPool, guild_id: Uuid, user_id: Uuid) -> sqlx::Result<bool> {
    let result: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2)"
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(result.0)
}
```

---

### Task 4: Friends Backend
**Files to create:**
- `server/src/social/mod.rs` (CREATE)
- `server/src/social/friends.rs` (CREATE)
- `server/src/social/types.rs` (CREATE)

**Friend Request Flow:**
```
User A sends request -> status: pending
User B accepts -> status: accepted (both are now friends)
User B rejects -> row deleted
User B blocks -> status: blocked (A can't send more requests)
```

**API Handlers:**
```rust
// POST /api/friends/request
pub async fn send_friend_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<FriendRequestBody>,
) -> Result<Json<Friendship>, SocialError> {
    // 1. Check if friendship exists (any direction)
    // 2. Check if blocked
    // 3. Insert pending friendship
    // 4. Send WebSocket notification to addressee
}

// POST /api/friends/:id/accept
pub async fn accept_friend_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(friendship_id): Path<Uuid>,
) -> Result<(), SocialError> {
    // 1. Verify auth.id is addressee
    // 2. Update status to accepted
    // 3. Send WebSocket notification to requester
}
```

---

### Task 5: DM Channels Backend
**Files to modify:**
- `server/src/chat/channels.rs`
- `server/src/chat/messages.rs`

**DM Logic:**
- 1:1 DM: Create channel with type='dm', add both users to dm_participants
- Group DM: Create channel with type='dm', name = participant names, max 10 users
- DMs have no guild_id (guild_id = NULL)

```rust
// Create or get existing 1:1 DM
pub async fn get_or_create_dm(
    pool: &PgPool,
    user1_id: Uuid,
    user2_id: Uuid,
) -> sqlx::Result<Channel> {
    // Check for existing DM between these two users
    let existing = sqlx::query_as!(
        Channel,
        r#"SELECT c.* FROM channels c
           JOIN dm_participants p1 ON c.id = p1.channel_id AND p1.user_id = $1
           JOIN dm_participants p2 ON c.id = p2.channel_id AND p2.user_id = $2
           WHERE c.channel_type = 'dm' AND c.guild_id IS NULL
           AND (SELECT COUNT(*) FROM dm_participants WHERE channel_id = c.id) = 2"#,
        user1_id, user2_id
    ).fetch_optional(pool).await?;

    if let Some(dm) = existing {
        return Ok(dm);
    }

    // Create new DM channel
    // ... insert and return
}
```

---

### Task 6: Frontend Guild Store
**Files to modify:**
- `client/src/stores/guilds.ts` (already exists as skeleton)
- `client/src/lib/tauri.ts` (add guild API calls)
- `client/src/lib/types.ts` (add Guild types)

**Complete the guilds.ts store:**
```typescript
// Add to tauri.ts
export async function getGuilds(): Promise<Guild[]> {
  return httpRequest<Guild[]>("GET", "/api/guilds");
}

export async function createGuild(name: string, description?: string): Promise<Guild> {
  return httpRequest<Guild>("POST", "/api/guilds", { name, description });
}

export async function getGuildChannels(guildId: string): Promise<Channel[]> {
  return httpRequest<Channel[]>("GET", `/api/guilds/${guildId}/channels`);
}

// Update guilds.ts loadGuilds()
export async function loadGuilds(): Promise<void> {
  setGuildsState({ isLoading: true, error: null });
  try {
    const guilds = await getGuilds();
    setGuildsState({ guilds, isLoading: false });
  } catch (err) {
    setGuildsState({ error: err.message, isLoading: false });
  }
}
```

---

### Task 7: Server Rail UI Component
**Files to create:**
- `client/src/components/layout/ServerRail.tsx` (CREATE)
- `client/src/components/layout/ServerIcon.tsx` (CREATE)

**ServerRail Component:**
```tsx
// ServerRail.tsx - Vertical list of guild icons on the left
const ServerRail: Component = () => {
  const [showCreateModal, setShowCreateModal] = createSignal(false);

  return (
    <div class="w-[72px] bg-surface-base flex flex-col items-center py-3 gap-2">
      {/* Home button (DMs/mentions) */}
      <ServerIcon
        icon={<Home />}
        active={guildsState.activeGuildId === null}
        onClick={selectHome}
        tooltip="Home"
      />

      <div class="w-8 h-0.5 bg-white/10 rounded-full" />

      {/* Guild list */}
      <For each={guildsState.guilds}>
        {(guild) => (
          <ServerIcon
            icon={guild.icon_url ? <img src={guild.icon_url} /> : guild.name[0]}
            active={guildsState.activeGuildId === guild.id}
            onClick={() => selectGuild(guild.id)}
            tooltip={guild.name}
          />
        )}
      </For>

      {/* Add server button */}
      <ServerIcon
        icon={<Plus />}
        onClick={() => setShowCreateModal(true)}
        tooltip="Add a Server"
        variant="action"
      />
    </div>
  );
};
```

---

### Task 8: Context Switching Logic
**Files to modify:**
- `client/src/stores/guilds.ts`
- `client/src/stores/channels.ts`
- `client/src/stores/voice.ts`
- `client/src/components/layout/AppShell.tsx`

**Context Switch Flow:**
1. User clicks guild icon in ServerRail
2. `selectGuild(guildId)` is called
3. Store updates `activeGuildId`
4. `channelsStore` reloads channels for new guild
5. If in voice channel from different guild, disconnect
6. UI updates to show guild's channels

```typescript
// guilds.ts - Enhanced selectGuild
export async function selectGuild(guildId: string): Promise<void> {
  const previousGuildId = guildsState.activeGuildId;
  setGuildsState({ activeGuildId: guildId });

  // Load channels for new guild
  await loadChannelsForGuild(guildId);

  // Check if we need to disconnect from voice
  const currentVoiceChannel = voiceState.channelId;
  if (currentVoiceChannel) {
    const channel = channelsState.channels.find(c => c.id === currentVoiceChannel);
    if (channel && channel.guild_id !== guildId) {
      await leaveVoice();
    }
  }
}
```

---

### Task 9: Friends UI
**Files to create:**
- `client/src/components/social/FriendsList.tsx` (CREATE)
- `client/src/components/social/FriendRequest.tsx` (CREATE)
- `client/src/components/social/AddFriend.tsx` (CREATE)
- `client/src/stores/friends.ts` (CREATE)

**Friends View (shown in Home):**
```tsx
const FriendsList: Component = () => {
  const [tab, setTab] = createSignal<"online" | "all" | "pending" | "blocked">("online");

  return (
    <div class="flex-1 flex flex-col">
      {/* Tab bar */}
      <div class="flex gap-4 px-4 py-3 border-b border-white/10">
        <button onClick={() => setTab("online")}>Online</button>
        <button onClick={() => setTab("all")}>All</button>
        <button onClick={() => setTab("pending")}>Pending</button>
        <button onClick={() => setTab("blocked")}>Blocked</button>
        <button class="ml-auto btn-primary">Add Friend</button>
      </div>

      {/* Friend list */}
      <div class="flex-1 overflow-y-auto">
        <For each={filteredFriends()}>
          {(friend) => <FriendItem friend={friend} />}
        </For>
      </div>
    </div>
  );
};
```

---

### Task 10: Home View
**Files to create:**
- `client/src/components/home/HomeView.tsx` (CREATE)
- `client/src/components/home/DMList.tsx` (CREATE)

**Home Layout:**
```
+------------------+------------------------+
|   DM List        |   FriendsList or       |
|   - User 1       |   DM Messages          |
|   - Group DM     |                        |
|   - User 2       |                        |
+------------------+------------------------+
```

---

### Task 11: Rate Limiting
**Files to modify:**
- `server/Cargo.toml` (add tower-governor)
- `server/src/api/mod.rs` (add rate limit middleware)

**Implementation:**
```rust
// Cargo.toml
tower-governor = "0.3"

// api/mod.rs
use tower_governor::{GovernorConfig, GovernorLayer};

pub fn create_router(state: AppState) -> Router {
    // Rate limit: 100 requests per minute per IP
    let governor_conf = GovernorConfig::default();
    let governor_limiter = governor_conf.limiter().clone();

    Router::new()
        // ... routes ...
        .layer(GovernorLayer { config: governor_conf })
}
```

---

## Task Execution Order

Execute tasks in this order to minimize conflicts:

1. **Task 1:** Database Migration (foundation)
2. **Task 2:** Guild Backend (API)
3. **Task 3:** Channel Guild Scope (modify existing)
4. **Task 6:** Frontend Guild Store (client foundation)
5. **Task 7:** Server Rail UI
6. **Task 8:** Context Switching
7. **Task 4:** Friends Backend
8. **Task 5:** DM Backend
9. **Task 9:** Friends UI
10. **Task 10:** Home View
11. **Task 11:** Rate Limiting

---

## Verification Checklist

After each task, verify:

- [ ] `cargo build` succeeds
- [ ] `cargo test` passes
- [ ] `bun run build` succeeds (client)
- [ ] Manual testing in browser

Final verification:
- [ ] Can create a guild
- [ ] Can see guilds in Server Rail
- [ ] Clicking guild shows its channels
- [ ] Can switch between guilds
- [ ] Voice disconnects when switching guilds
- [ ] Can send friend request
- [ ] Can accept/reject friend request
- [ ] Can start DM with friend
- [ ] Can create group DM
- [ ] Home view shows DMs and friends
- [ ] Rate limiting blocks excessive requests

---

## File Summary

### New Files (Create)
```
server/migrations/20240201000000_guilds.sql
server/src/guild/mod.rs
server/src/guild/handlers.rs
server/src/guild/types.rs
server/src/social/mod.rs
server/src/social/friends.rs
server/src/social/types.rs
client/src/components/layout/ServerRail.tsx
client/src/components/layout/ServerIcon.tsx
client/src/components/social/FriendsList.tsx
client/src/components/social/FriendRequest.tsx
client/src/components/social/AddFriend.tsx
client/src/components/home/HomeView.tsx
client/src/components/home/DMList.tsx
client/src/stores/friends.ts
```

### Modified Files
```
server/src/lib.rs
server/src/api/mod.rs
server/src/chat/channels.rs
server/src/chat/messages.rs
server/src/db/queries.rs
server/Cargo.toml
client/src/stores/guilds.ts
client/src/stores/channels.ts
client/src/stores/voice.ts
client/src/lib/tauri.ts
client/src/lib/types.ts
client/src/components/layout/AppShell.tsx
```
