# Guild Management Modal Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add guild invite system and member management modal accessible via gear icon in sidebar.

**Architecture:** Database migration adds `guild_invites` table and `last_seen_at` to users. Backend handlers manage invite CRUD and join flow. Frontend modal with Invites and Members tabs using existing store/component patterns.

**Tech Stack:** Rust/axum (backend), Solid.js (frontend), PostgreSQL, existing tauri.ts API pattern

---

## Task 1: Database Migration

**Files:**
- Create: `server/migrations/20260114000000_guild_invites.sql`

**Step 1: Create migration file**

```sql
-- Guild Invites and Last Seen Migration
-- Adds invite system for guilds and last_seen tracking for users

-- ============================================================================
-- Guild Invites Table
-- ============================================================================

CREATE TABLE guild_invites (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    code VARCHAR(8) NOT NULL UNIQUE,
    created_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ,
    use_count INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_guild_invites_code ON guild_invites(code);
CREATE INDEX idx_guild_invites_guild ON guild_invites(guild_id);
CREATE INDEX idx_guild_invites_expires ON guild_invites(expires_at) WHERE expires_at IS NOT NULL;

-- ============================================================================
-- User Last Seen Tracking
-- ============================================================================

ALTER TABLE users ADD COLUMN last_seen_at TIMESTAMPTZ;
CREATE INDEX idx_users_last_seen ON users(last_seen_at DESC NULLS LAST);
```

**Step 2: Verify migration applies**

Run: `cd server && cargo sqlx migrate run`
Expected: Migration applies successfully

**Step 3: Commit**

```bash
git add server/migrations/20260114000000_guild_invites.sql
git commit -m "feat(db): add guild_invites table and last_seen_at column"
```

---

## Task 2: Backend Invite Types

**Files:**
- Modify: `server/src/guild/types.rs`

**Step 1: Add invite types to types.rs**

Add after line 60 (after `GuildMember` struct):

```rust
// ============================================================================
// Invite Types
// ============================================================================

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GuildInvite {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub code: String,
    pub created_by: Uuid,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub use_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateInviteRequest {
    /// Expiry duration: "30m", "1h", "1d", "7d", or "never"
    pub expires_in: String,
}

#[derive(Debug, Serialize)]
pub struct InviteResponse {
    pub id: Uuid,
    pub code: String,
    pub guild_id: Uuid,
    pub guild_name: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub use_count: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
```

**Step 2: Update GuildMember to include status**

Replace the existing `GuildMember` struct (lines 52-60) with:

```rust
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GuildMember {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub nickname: Option<String>,
    pub joined_at: chrono::DateTime<chrono::Utc>,
    pub status: String,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
}
```

**Step 3: Commit**

```bash
git add server/src/guild/types.rs
git commit -m "feat(guild): add invite and updated member types"
```

---

## Task 3: Backend Invite Handlers

**Files:**
- Create: `server/src/guild/invites.rs`

**Step 1: Create invite handlers file**

```rust
//! Guild Invite Handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{Duration, Utc};
use rand::Rng;
use uuid::Uuid;

use crate::{api::AppState, auth::AuthUser, db};
use super::handlers::GuildError;
use super::types::{CreateInviteRequest, GuildInvite, InviteResponse};

/// Generate a cryptographically random 8-character invite code
fn generate_invite_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Parse expiry string to duration
fn parse_expiry(expires_in: &str) -> Option<Duration> {
    match expires_in {
        "30m" => Some(Duration::minutes(30)),
        "1h" => Some(Duration::hours(1)),
        "1d" => Some(Duration::days(1)),
        "7d" => Some(Duration::days(7)),
        "never" => None,
        _ => Some(Duration::days(7)), // Default to 7 days
    }
}

/// List invites for a guild (owner only)
#[tracing::instrument(skip(state))]
pub async fn list_invites(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildInvite>>, GuildError> {
    // Verify ownership
    let guild = sqlx::query_as::<_, (Uuid,)>("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(GuildError::NotFound)?;

    if guild.0 != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Get active invites (not expired)
    let invites = sqlx::query_as::<_, GuildInvite>(
        r#"SELECT id, guild_id, code, created_by, expires_at, use_count, created_at
           FROM guild_invites
           WHERE guild_id = $1 AND (expires_at IS NULL OR expires_at > NOW())
           ORDER BY created_at DESC"#,
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(invites))
}

/// Create a new invite (owner only)
#[tracing::instrument(skip(state))]
pub async fn create_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<CreateInviteRequest>,
) -> Result<Json<GuildInvite>, GuildError> {
    // Verify ownership
    let guild = sqlx::query_as::<_, (Uuid,)>("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(GuildError::NotFound)?;

    if guild.0 != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Check rate limit (max 10 active invites per guild)
    let active_count: (i64,) = sqlx::query_as(
        r#"SELECT COUNT(*) FROM guild_invites
           WHERE guild_id = $1 AND (expires_at IS NULL OR expires_at > NOW())"#,
    )
    .bind(guild_id)
    .fetch_one(&state.db)
    .await?;

    if active_count.0 >= 10 {
        return Err(GuildError::Validation(
            "Maximum 10 active invites per guild".to_string(),
        ));
    }

    // Generate unique code (retry if collision)
    let mut code = generate_invite_code();
    let mut attempts = 0;
    while attempts < 5 {
        let exists: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM guild_invites WHERE code = $1")
                .bind(&code)
                .fetch_optional(&state.db)
                .await?;
        if exists.is_none() {
            break;
        }
        code = generate_invite_code();
        attempts += 1;
    }

    // Calculate expiry
    let expires_at = parse_expiry(&body.expires_in).map(|d| Utc::now() + d);

    // Insert invite
    let invite = sqlx::query_as::<_, GuildInvite>(
        r#"INSERT INTO guild_invites (guild_id, code, created_by, expires_at)
           VALUES ($1, $2, $3, $4)
           RETURNING id, guild_id, code, created_by, expires_at, use_count, created_at"#,
    )
    .bind(guild_id)
    .bind(&code)
    .bind(auth.id)
    .bind(expires_at)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(invite))
}

/// Delete/revoke an invite (owner only)
#[tracing::instrument(skip(state))]
pub async fn delete_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, code)): Path<(Uuid, String)>,
) -> Result<StatusCode, GuildError> {
    // Verify ownership
    let guild = sqlx::query_as::<_, (Uuid,)>("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(GuildError::NotFound)?;

    if guild.0 != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Delete the invite
    let result = sqlx::query("DELETE FROM guild_invites WHERE guild_id = $1 AND code = $2")
        .bind(guild_id)
        .bind(&code)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(GuildError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Join a guild via invite code (any authenticated user)
#[tracing::instrument(skip(state))]
pub async fn join_via_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(code): Path<String>,
) -> Result<Json<InviteResponse>, GuildError> {
    // Find the invite
    let invite = sqlx::query_as::<_, GuildInvite>(
        r#"SELECT id, guild_id, code, created_by, expires_at, use_count, created_at
           FROM guild_invites
           WHERE code = $1 AND (expires_at IS NULL OR expires_at > NOW())"#,
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await?
    .ok_or(GuildError::Validation("Invalid or expired invite code".to_string()))?;

    // Check if already a member
    let is_member = db::is_guild_member(&state.db, invite.guild_id, auth.id).await?;
    if is_member {
        // Already a member, just return guild info
        let guild_name: (String,) = sqlx::query_as("SELECT name FROM guilds WHERE id = $1")
            .bind(invite.guild_id)
            .fetch_one(&state.db)
            .await?;

        return Ok(Json(InviteResponse {
            id: invite.id,
            code: invite.code,
            guild_id: invite.guild_id,
            guild_name: guild_name.0,
            expires_at: invite.expires_at,
            use_count: invite.use_count,
            created_at: invite.created_at,
        }));
    }

    // Add as member
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(invite.guild_id)
        .bind(auth.id)
        .execute(&state.db)
        .await?;

    // Increment use count
    sqlx::query("UPDATE guild_invites SET use_count = use_count + 1 WHERE id = $1")
        .bind(invite.id)
        .execute(&state.db)
        .await?;

    // Get guild name for response
    let guild_name: (String,) = sqlx::query_as("SELECT name FROM guilds WHERE id = $1")
        .bind(invite.guild_id)
        .fetch_one(&state.db)
        .await?;

    Ok(Json(InviteResponse {
        id: invite.id,
        code: invite.code,
        guild_id: invite.guild_id,
        guild_name: guild_name.0,
        expires_at: invite.expires_at,
        use_count: invite.use_count + 1,
        created_at: invite.created_at,
    }))
}
```

**Step 2: Commit**

```bash
git add server/src/guild/invites.rs
git commit -m "feat(guild): add invite handlers"
```

---

## Task 4: Backend Routes and Module Updates

**Files:**
- Modify: `server/src/guild/mod.rs`
- Modify: `server/src/guild/handlers.rs`

**Step 1: Update mod.rs to include invites module and routes**

Replace entire `server/src/guild/mod.rs` with:

```rust
//! Guild (Server) Management Module
//!
//! Handles guild creation, membership, invites, and management.

pub mod handlers;
pub mod invites;
pub mod types;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::api::AppState;

/// Create the guild router with all endpoints
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::list_guilds).post(handlers::create_guild))
        .route(
            "/:id",
            get(handlers::get_guild)
                .patch(handlers::update_guild)
                .delete(handlers::delete_guild),
        )
        .route("/:id/join", post(handlers::join_guild))
        .route("/:id/leave", post(handlers::leave_guild))
        .route("/:id/members", get(handlers::list_members))
        .route("/:id/members/:user_id", delete(handlers::kick_member))
        .route("/:id/channels", get(handlers::list_channels))
        // Invite routes
        .route(
            "/:id/invites",
            get(invites::list_invites).post(invites::create_invite),
        )
        .route("/:id/invites/:code", delete(invites::delete_invite))
}

/// Create the invite join router (separate for public access pattern)
pub fn invite_router() -> Router<AppState> {
    Router::new().route("/:code/join", post(invites::join_via_invite))
}
```

**Step 2: Add kick_member handler to handlers.rs**

Add after `list_members` function (around line 346):

```rust
/// Kick a member from guild (owner only)
#[tracing::instrument(skip(state))]
pub async fn kick_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, GuildError> {
    // Verify ownership
    let owner_check: Option<(Uuid,)> =
        sqlx::query_as("SELECT owner_id FROM guilds WHERE id = $1")
            .bind(guild_id)
            .fetch_optional(&state.db)
            .await?;

    let owner_id = owner_check.ok_or(GuildError::NotFound)?.0;

    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Cannot kick yourself (owner)
    if user_id == auth.id {
        return Err(GuildError::Validation(
            "Cannot kick yourself from the guild".to_string(),
        ));
    }

    // Remove membership
    let result = sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(user_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(GuildError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}
```

**Step 3: Update list_members query to include status and last_seen**

Replace the `list_members` function (around line 317-346) with:

```rust
/// List guild members
#[tracing::instrument(skip(state))]
pub async fn list_members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildMember>>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let members = sqlx::query_as::<_, GuildMember>(
        r#"SELECT
            u.id as user_id,
            u.username,
            u.display_name,
            u.avatar_url,
            gm.nickname,
            gm.joined_at,
            u.status::text as status,
            u.last_seen_at
           FROM guild_members gm
           INNER JOIN users u ON gm.user_id = u.id
           WHERE gm.guild_id = $1
           ORDER BY gm.joined_at"#,
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(members))
}
```

**Step 4: Commit**

```bash
git add server/src/guild/mod.rs server/src/guild/handlers.rs
git commit -m "feat(guild): add invite routes and kick_member handler"
```

---

## Task 5: Register Invite Router in API

**Files:**
- Modify: `server/src/api/mod.rs`

**Step 1: Add invite router to protected routes**

In `create_router` function (around line 70-81), add the invite router:

```rust
    // Protected routes that require authentication
    let protected_routes = Router::new()
        .nest("/api/channels", chat::channels_router())
        .nest("/api/messages", chat::messages_router())
        .nest("/api/guilds", guild::router())
        .nest("/api/invites", guild::invite_router())  // ADD THIS LINE
        .nest("/api", social::router())
        .nest("/api/dm", chat::dm_router())
        .nest("/api/dm", voice::call_handlers::call_router())
        .nest("/api/voice", voice::router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));
```

**Step 2: Build and verify**

Run: `cd server && cargo build`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add server/src/api/mod.rs
git commit -m "feat(api): register invite router"
```

---

## Task 6: Frontend Types

**Files:**
- Modify: `client/src/lib/types.ts`

**Step 1: Add invite types**

Add after `GuildMember` interface (around line 43):

```typescript
export interface GuildInvite {
  id: string;
  guild_id: string;
  code: string;
  created_by: string;
  expires_at: string | null;
  use_count: number;
  created_at: string;
}

export interface InviteResponse {
  id: string;
  code: string;
  guild_id: string;
  guild_name: string;
  expires_at: string | null;
  use_count: number;
  created_at: string;
}

export type InviteExpiry = "30m" | "1h" | "1d" | "7d" | "never";
```

**Step 2: Update GuildMember to include status and last_seen**

Replace existing `GuildMember` interface with:

```typescript
export interface GuildMember {
  user_id: string;
  username: string;
  display_name: string;
  avatar_url: string | null;
  nickname: string | null;
  joined_at: string;
  status: "online" | "idle" | "offline";
  last_seen_at: string | null;
}
```

**Step 3: Commit**

```bash
git add client/src/lib/types.ts
git commit -m "feat(types): add invite types and update GuildMember"
```

---

## Task 7: Frontend API Functions

**Files:**
- Modify: `client/src/lib/tauri.ts`

**Step 1: Add invite API functions**

Add the following imports to the type imports at the top (around line 7-18):

```typescript
import type {
  User,
  Channel,
  Message,
  AppSettings,
  Guild,
  GuildMember,
  GuildInvite,      // ADD
  InviteResponse,   // ADD
  InviteExpiry,     // ADD
  Friend,
  Friendship,
  DMChannel,
  DMListItem,
} from "./types";
```

Update the re-export line (around line 21):

```typescript
export type { User, Channel, Message, AppSettings, Guild, GuildMember, GuildInvite, InviteResponse, InviteExpiry, Friend, Friendship, DMChannel, DMListItem };
```

**Step 2: Add invite API functions at end of file**

```typescript
// ============================================================================
// Guild Invite Functions
// ============================================================================

/**
 * Get invites for a guild (owner only)
 */
export async function getGuildInvites(guildId: string): Promise<GuildInvite[]> {
  return apiCall<GuildInvite[]>(`/api/guilds/${guildId}/invites`, { method: "GET" });
}

/**
 * Create a new invite for a guild
 */
export async function createGuildInvite(
  guildId: string,
  expiresIn: InviteExpiry = "7d"
): Promise<GuildInvite> {
  return apiCall<GuildInvite>(`/api/guilds/${guildId}/invites`, {
    method: "POST",
    body: JSON.stringify({ expires_in: expiresIn }),
  });
}

/**
 * Delete/revoke an invite
 */
export async function deleteGuildInvite(guildId: string, code: string): Promise<void> {
  await apiCall<void>(`/api/guilds/${guildId}/invites/${code}`, { method: "DELETE" });
}

/**
 * Join a guild via invite code
 */
export async function joinViaInvite(code: string): Promise<InviteResponse> {
  return apiCall<InviteResponse>(`/api/invites/${code}/join`, { method: "POST" });
}

/**
 * Kick a member from a guild (owner only)
 */
export async function kickGuildMember(guildId: string, userId: string): Promise<void> {
  await apiCall<void>(`/api/guilds/${guildId}/members/${userId}`, { method: "DELETE" });
}
```

**Step 3: Commit**

```bash
git add client/src/lib/tauri.ts
git commit -m "feat(api): add invite and kick API functions"
```

---

## Task 8: Frontend Guild Store Updates

**Files:**
- Modify: `client/src/stores/guilds.ts`

**Step 1: Add invite state to store**

Update the `GuildStoreState` interface (around line 14-28):

```typescript
interface GuildStoreState {
  // All guilds the user is a member of
  guilds: Guild[];
  // Currently active/selected guild ID
  activeGuildId: string | null;
  // Members of the active guild
  members: Record<string, GuildMember[]>;
  // Invites for guilds (owner only)
  invites: Record<string, GuildInvite[]>;
  // Channels of the active guild
  guildChannels: Record<string, Channel[]>;
  // Loading states
  isLoading: boolean;
  isMembersLoading: boolean;
  isInvitesLoading: boolean;
  // Error state
  error: string | null;
}
```

Update the initial state (around line 31-39):

```typescript
const [guildsState, setGuildsState] = createStore<GuildStoreState>({
  guilds: [],
  activeGuildId: null,
  members: {},
  invites: {},
  guildChannels: {},
  isLoading: false,
  isMembersLoading: false,
  isInvitesLoading: false,
  error: null,
});
```

**Step 2: Add import for GuildInvite type**

Update import at top:

```typescript
import type { Guild, GuildMember, GuildInvite, Channel } from "@/lib/types";
```

**Step 3: Add invite functions**

Add after `leaveGuild` function (around line 241):

```typescript
/**
 * Load invites for a guild (owner only)
 */
export async function loadGuildInvites(guildId: string): Promise<void> {
  setGuildsState({ isInvitesLoading: true });

  try {
    const invites = await tauri.getGuildInvites(guildId);
    setGuildsState("invites", guildId, invites);
    setGuildsState({ isInvitesLoading: false });
  } catch (err) {
    console.error("Failed to load guild invites:", err);
    setGuildsState({ isInvitesLoading: false });
  }
}

/**
 * Create a new invite
 */
export async function createInvite(
  guildId: string,
  expiresIn: tauri.InviteExpiry = "7d"
): Promise<tauri.GuildInvite> {
  const invite = await tauri.createGuildInvite(guildId, expiresIn);
  setGuildsState("invites", guildId, (prev) => [invite, ...(prev || [])]);
  return invite;
}

/**
 * Delete an invite
 */
export async function deleteInvite(guildId: string, code: string): Promise<void> {
  await tauri.deleteGuildInvite(guildId, code);
  setGuildsState("invites", guildId, (prev) =>
    (prev || []).filter((i) => i.code !== code)
  );
}

/**
 * Join a guild via invite code
 */
export async function joinViaInviteCode(code: string): Promise<void> {
  const response = await tauri.joinViaInvite(code);
  await loadGuilds(); // Reload guilds to include the new one
  await selectGuild(response.guild_id);
}

/**
 * Kick a member from a guild
 */
export async function kickMember(guildId: string, userId: string): Promise<void> {
  await tauri.kickGuildMember(guildId, userId);
  setGuildsState("members", guildId, (prev) =>
    (prev || []).filter((m) => m.user_id !== userId)
  );
}

/**
 * Get invites for a guild
 */
export function getGuildInvites(guildId: string): tauri.GuildInvite[] {
  return guildsState.invites[guildId] || [];
}

/**
 * Check if current user is guild owner
 */
export function isGuildOwner(guildId: string, userId: string): boolean {
  const guild = guildsState.guilds.find((g) => g.id === guildId);
  return guild?.owner_id === userId;
}
```

**Step 4: Commit**

```bash
git add client/src/stores/guilds.ts
git commit -m "feat(store): add invite state and functions"
```

---

## Task 9: Frontend GuildSettingsModal Component

**Files:**
- Create: `client/src/components/guilds/GuildSettingsModal.tsx`

**Step 1: Create the modal component**

```typescript
/**
 * GuildSettingsModal - Guild management modal with tabs
 *
 * Provides invite management (owner only) and member list.
 */

import { Component, createSignal, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { X, Link, Users } from "lucide-solid";
import { guildsState, isGuildOwner } from "@/stores/guilds";
import { authState } from "@/stores/auth";
import InvitesTab from "./InvitesTab";
import MembersTab from "./MembersTab";

interface GuildSettingsModalProps {
  guildId: string;
  onClose: () => void;
}

type TabId = "invites" | "members";

const GuildSettingsModal: Component<GuildSettingsModalProps> = (props) => {
  const guild = () => guildsState.guilds.find((g) => g.id === props.guildId);
  const isOwner = () => isGuildOwner(props.guildId, authState.user?.id || "");

  // Default to members tab for non-owners
  const [activeTab, setActiveTab] = createSignal<TabId>(isOwner() ? "invites" : "members");

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      props.onClose();
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      props.onClose();
    }
  };

  return (
    <Portal>
      <div
        class="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50"
        onClick={handleBackdropClick}
        onKeyDown={handleKeyDown}
        tabIndex={-1}
      >
        <div
          class="border border-white/10 rounded-2xl w-[600px] max-h-[80vh] flex flex-col shadow-2xl"
          style="background-color: var(--color-surface-base)"
        >
          {/* Header */}
          <div class="flex items-center justify-between px-6 py-4 border-b border-white/10">
            <div class="flex items-center gap-3">
              <div class="w-10 h-10 rounded-xl bg-accent-primary/20 flex items-center justify-center">
                <span class="text-lg font-bold text-accent-primary">
                  {guild()?.name.charAt(0).toUpperCase()}
                </span>
              </div>
              <div>
                <h2 class="text-lg font-bold text-text-primary">{guild()?.name}</h2>
                <p class="text-sm text-text-secondary">Server Settings</p>
              </div>
            </div>
            <button
              onClick={props.onClose}
              class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            >
              <X class="w-5 h-5" />
            </button>
          </div>

          {/* Tabs */}
          <div class="flex border-b border-white/10">
            <Show when={isOwner()}>
              <button
                onClick={() => setActiveTab("invites")}
                class="flex items-center gap-2 px-6 py-3 font-medium transition-colors"
                classList={{
                  "text-accent-primary border-b-2 border-accent-primary": activeTab() === "invites",
                  "text-text-secondary hover:text-text-primary": activeTab() !== "invites",
                }}
              >
                <Link class="w-4 h-4" />
                Invites
              </button>
            </Show>
            <button
              onClick={() => setActiveTab("members")}
              class="flex items-center gap-2 px-6 py-3 font-medium transition-colors"
              classList={{
                "text-accent-primary border-b-2 border-accent-primary": activeTab() === "members",
                "text-text-secondary hover:text-text-primary": activeTab() !== "members",
              }}
            >
              <Users class="w-4 h-4" />
              Members
            </button>
          </div>

          {/* Content */}
          <div class="flex-1 overflow-y-auto">
            <Show when={activeTab() === "invites" && isOwner()}>
              <InvitesTab guildId={props.guildId} />
            </Show>
            <Show when={activeTab() === "members"}>
              <MembersTab guildId={props.guildId} isOwner={isOwner()} />
            </Show>
          </div>
        </div>
      </div>
    </Portal>
  );
};

export default GuildSettingsModal;
```

**Step 2: Commit**

```bash
git add client/src/components/guilds/GuildSettingsModal.tsx
git commit -m "feat(ui): add GuildSettingsModal component"
```

---

## Task 10: Frontend InvitesTab Component

**Files:**
- Create: `client/src/components/guilds/InvitesTab.tsx`

**Step 1: Create the invites tab component**

```typescript
/**
 * InvitesTab - Invite management for guild owners
 */

import { Component, createSignal, For, Show, onMount } from "solid-js";
import { Copy, Trash2, Plus } from "lucide-solid";
import {
  guildsState,
  loadGuildInvites,
  createInvite,
  deleteInvite,
  getGuildInvites,
} from "@/stores/guilds";
import type { InviteExpiry } from "@/lib/types";

interface InvitesTabProps {
  guildId: string;
}

const EXPIRY_OPTIONS: { value: InviteExpiry; label: string }[] = [
  { value: "30m", label: "30 minutes" },
  { value: "1h", label: "1 hour" },
  { value: "1d", label: "1 day" },
  { value: "7d", label: "7 days" },
  { value: "never", label: "Never" },
];

const InvitesTab: Component<InvitesTabProps> = (props) => {
  const [expiresIn, setExpiresIn] = createSignal<InviteExpiry>("7d");
  const [isCreating, setIsCreating] = createSignal(false);
  const [copiedCode, setCopiedCode] = createSignal<string | null>(null);
  const [deletingCode, setDeletingCode] = createSignal<string | null>(null);

  onMount(() => {
    loadGuildInvites(props.guildId);
  });

  const invites = () => getGuildInvites(props.guildId);

  const handleCreate = async () => {
    setIsCreating(true);
    try {
      await createInvite(props.guildId, expiresIn());
    } catch (err) {
      console.error("Failed to create invite:", err);
    } finally {
      setIsCreating(false);
    }
  };

  const handleCopy = async (code: string) => {
    const url = `${window.location.origin}/invite/${code}`;
    await navigator.clipboard.writeText(url);
    setCopiedCode(code);
    setTimeout(() => setCopiedCode(null), 2000);
  };

  const handleDelete = async (code: string) => {
    if (deletingCode() === code) {
      // Confirmed, delete it
      try {
        await deleteInvite(props.guildId, code);
      } catch (err) {
        console.error("Failed to delete invite:", err);
      }
      setDeletingCode(null);
    } else {
      // First click, show confirmation
      setDeletingCode(code);
      setTimeout(() => setDeletingCode(null), 3000);
    }
  };

  const formatExpiry = (expiresAt: string | null): string => {
    if (!expiresAt) return "Never expires";
    const expires = new Date(expiresAt);
    const now = new Date();
    const diff = expires.getTime() - now.getTime();

    if (diff <= 0) return "Expired";

    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(diff / 3600000);
    const days = Math.floor(diff / 86400000);

    if (days > 0) return `Expires in ${days} day${days > 1 ? "s" : ""}`;
    if (hours > 0) return `Expires in ${hours} hour${hours > 1 ? "s" : ""}`;
    return `Expires in ${minutes} minute${minutes > 1 ? "s" : ""}`;
  };

  return (
    <div class="p-6">
      {/* Create Invite */}
      <div class="p-4 rounded-xl border border-white/10" style="background-color: var(--color-surface-layer1)">
        <h3 class="text-sm font-semibold text-text-primary mb-3">Create New Invite</h3>
        <div class="flex items-center gap-3">
          <div class="flex-1">
            <label class="text-xs text-text-secondary mb-1 block">Expires after</label>
            <select
              value={expiresIn()}
              onChange={(e) => setExpiresIn(e.currentTarget.value as InviteExpiry)}
              class="w-full px-3 py-2 rounded-lg border border-white/10 text-text-input"
              style="background-color: var(--color-surface-layer2)"
            >
              <For each={EXPIRY_OPTIONS}>
                {(opt) => <option value={opt.value}>{opt.label}</option>}
              </For>
            </select>
          </div>
          <button
            onClick={handleCreate}
            disabled={isCreating()}
            class="flex items-center gap-2 px-4 py-2 bg-accent-primary text-white rounded-lg font-medium hover:opacity-90 disabled:opacity-50 mt-5"
          >
            <Plus class="w-4 h-4" />
            {isCreating() ? "Creating..." : "Create"}
          </button>
        </div>
      </div>

      {/* Active Invites */}
      <div class="mt-6">
        <h3 class="text-sm font-semibold text-text-primary mb-3">
          Active Invites ({invites().length})
        </h3>

        <Show
          when={invites().length > 0}
          fallback={
            <div class="text-center py-8 text-text-secondary">
              No active invites. Create one to let people join!
            </div>
          }
        >
          <div class="space-y-2">
            <For each={invites()}>
              {(invite) => (
                <div
                  class="flex items-center justify-between p-3 rounded-lg border border-white/5"
                  style="background-color: var(--color-surface-layer1)"
                >
                  <div class="flex-1 min-w-0">
                    <code class="text-sm text-accent-primary font-mono truncate block">
                      {window.location.origin}/invite/{invite.code}
                    </code>
                    <div class="text-xs text-text-secondary mt-1">
                      {formatExpiry(invite.expires_at)} &bull; {invite.use_count} use{invite.use_count !== 1 ? "s" : ""}
                    </div>
                  </div>
                  <div class="flex items-center gap-2 ml-3">
                    <button
                      onClick={() => handleCopy(invite.code)}
                      class="p-2 text-text-secondary hover:text-accent-primary hover:bg-white/10 rounded-lg transition-colors"
                      title="Copy invite link"
                    >
                      <Show when={copiedCode() === invite.code} fallback={<Copy class="w-4 h-4" />}>
                        <span class="text-xs text-accent-primary">Copied!</span>
                      </Show>
                    </button>
                    <button
                      onClick={() => handleDelete(invite.code)}
                      class="p-2 rounded-lg transition-colors"
                      classList={{
                        "bg-accent-danger text-white": deletingCode() === invite.code,
                        "text-text-secondary hover:text-accent-danger hover:bg-white/10": deletingCode() !== invite.code,
                      }}
                      title={deletingCode() === invite.code ? "Click again to confirm" : "Delete invite"}
                    >
                      <Show
                        when={deletingCode() === invite.code}
                        fallback={<Trash2 class="w-4 h-4" />}
                      >
                        <span class="text-xs">Confirm?</span>
                      </Show>
                    </button>
                  </div>
                </div>
              )}
            </For>
          </div>
        </Show>
      </div>

      {/* Loading state */}
      <Show when={guildsState.isInvitesLoading}>
        <div class="text-center py-4 text-text-secondary">Loading invites...</div>
      </Show>
    </div>
  );
};

export default InvitesTab;
```

**Step 2: Commit**

```bash
git add client/src/components/guilds/InvitesTab.tsx
git commit -m "feat(ui): add InvitesTab component"
```

---

## Task 11: Frontend MembersTab Component

**Files:**
- Create: `client/src/components/guilds/MembersTab.tsx`

**Step 1: Create the members tab component**

```typescript
/**
 * MembersTab - Member list with search and kick functionality
 */

import { Component, createSignal, createMemo, For, Show, onMount } from "solid-js";
import { Search, Crown, X } from "lucide-solid";
import { guildsState, loadGuildMembers, getGuildMembers, kickMember } from "@/stores/guilds";
import type { GuildMember } from "@/lib/types";

interface MembersTabProps {
  guildId: string;
  isOwner: boolean;
}

const MembersTab: Component<MembersTabProps> = (props) => {
  const [search, setSearch] = createSignal("");
  const [kickingId, setKickingId] = createSignal<string | null>(null);

  onMount(() => {
    loadGuildMembers(props.guildId);
  });

  const guild = () => guildsState.guilds.find((g) => g.id === props.guildId);
  const members = () => getGuildMembers(props.guildId);

  const filteredMembers = createMemo(() => {
    const query = search().toLowerCase().trim();
    if (!query) return members();
    return members().filter(
      (m) =>
        m.display_name.toLowerCase().includes(query) ||
        m.username.toLowerCase().includes(query)
    );
  });

  const handleKick = async (userId: string) => {
    if (kickingId() === userId) {
      // Confirmed, kick them
      try {
        await kickMember(props.guildId, userId);
      } catch (err) {
        console.error("Failed to kick member:", err);
      }
      setKickingId(null);
    } else {
      // First click, show confirmation
      setKickingId(userId);
      setTimeout(() => setKickingId(null), 3000);
    }
  };

  const formatLastSeen = (member: GuildMember): string => {
    if (member.status === "online") return "Online";
    if (member.status === "idle") return "Idle";
    if (!member.last_seen_at) return "Never";

    const lastSeen = new Date(member.last_seen_at);
    const now = new Date();
    const diff = now.getTime() - lastSeen.getTime();

    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(diff / 3600000);
    const days = Math.floor(diff / 86400000);

    if (minutes < 60) return `${minutes} min${minutes !== 1 ? "s" : ""} ago`;
    if (hours < 24) return `${hours} hour${hours !== 1 ? "s" : ""} ago`;
    if (days < 7) return `${days} day${days !== 1 ? "s" : ""} ago`;
    return lastSeen.toLocaleDateString();
  };

  const getStatusColor = (status: string): string => {
    switch (status) {
      case "online": return "#22c55e"; // green
      case "idle": return "#eab308"; // yellow
      default: return "#6b7280"; // gray
    }
  };

  const formatJoinDate = (date: string): string => {
    return new Date(date).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
    });
  };

  return (
    <div class="p-6">
      {/* Search */}
      <div class="relative mb-4">
        <Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-text-secondary" />
        <input
          type="text"
          value={search()}
          onInput={(e) => setSearch(e.currentTarget.value)}
          placeholder="Search members..."
          class="w-full pl-10 pr-4 py-2 rounded-lg border border-white/10 text-text-input placeholder-text-secondary"
          style="background-color: var(--color-surface-layer1)"
        />
      </div>

      {/* Member Count */}
      <div class="text-sm text-text-secondary mb-3">
        {filteredMembers().length} member{filteredMembers().length !== 1 ? "s" : ""}
        {search() && ` matching "${search()}"`}
      </div>

      {/* Members List */}
      <Show
        when={filteredMembers().length > 0}
        fallback={
          <div class="text-center py-8 text-text-secondary">
            {search() ? "No members match your search" : "You're the only one here. Invite some friends!"}
          </div>
        }
      >
        <div class="space-y-1">
          <For each={filteredMembers()}>
            {(member) => {
              const isGuildOwner = member.user_id === guild()?.owner_id;
              const canKick = props.isOwner && !isGuildOwner;

              return (
                <div
                  class="flex items-center gap-3 p-3 rounded-lg hover:bg-white/5 transition-colors group"
                >
                  {/* Avatar with status indicator */}
                  <div class="relative">
                    <div class="w-10 h-10 rounded-full bg-accent-primary/20 flex items-center justify-center">
                      <Show
                        when={member.avatar_url}
                        fallback={
                          <span class="text-sm font-semibold text-accent-primary">
                            {member.display_name.charAt(0).toUpperCase()}
                          </span>
                        }
                      >
                        <img
                          src={member.avatar_url!}
                          alt={member.display_name}
                          class="w-10 h-10 rounded-full object-cover"
                        />
                      </Show>
                    </div>
                    {/* Status dot */}
                    <div
                      class="absolute -bottom-0.5 -right-0.5 w-3.5 h-3.5 rounded-full border-2"
                      style={{
                        "background-color": getStatusColor(member.status),
                        "border-color": "var(--color-surface-base)",
                      }}
                    />
                  </div>

                  {/* Member info */}
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="font-medium text-text-primary truncate">
                        {member.nickname || member.display_name}
                      </span>
                      <Show when={isGuildOwner}>
                        <Crown class="w-4 h-4 text-yellow-500" title="Server Owner" />
                      </Show>
                    </div>
                    <div class="text-sm text-text-secondary">
                      @{member.username}
                    </div>
                    <div class="text-xs text-text-secondary mt-0.5">
                      Joined {formatJoinDate(member.joined_at)} &bull; {formatLastSeen(member)}
                    </div>
                  </div>

                  {/* Kick button */}
                  <Show when={canKick}>
                    <button
                      onClick={() => handleKick(member.user_id)}
                      class="p-2 rounded-lg transition-all opacity-0 group-hover:opacity-100"
                      classList={{
                        "bg-accent-danger text-white": kickingId() === member.user_id,
                        "text-text-secondary hover:text-accent-danger hover:bg-white/10": kickingId() !== member.user_id,
                      }}
                      title={kickingId() === member.user_id ? "Click to confirm" : "Kick member"}
                    >
                      <Show
                        when={kickingId() === member.user_id}
                        fallback={<X class="w-4 h-4" />}
                      >
                        <span class="text-xs px-1">Confirm?</span>
                      </Show>
                    </button>
                  </Show>
                </div>
              );
            }}
          </For>
        </div>
      </Show>

      {/* Loading state */}
      <Show when={guildsState.isMembersLoading}>
        <div class="text-center py-4 text-text-secondary">Loading members...</div>
      </Show>
    </div>
  );
};

export default MembersTab;
```

**Step 2: Commit**

```bash
git add client/src/components/guilds/MembersTab.tsx
git commit -m "feat(ui): add MembersTab component"
```

---

## Task 12: Wire Up Modal in Sidebar

**Files:**
- Modify: `client/src/components/layout/Sidebar.tsx`

**Step 1: Update Sidebar to show gear icon and modal**

Replace entire `client/src/components/layout/Sidebar.tsx` with:

```typescript
/**
 * Sidebar - Context Navigation
 *
 * Middle-left panel containing:
 * - Server/Guild header with settings gear
 * - Search bar
 * - Channel list
 * - User panel at bottom
 */

import { Component, createSignal, onMount, Show } from "solid-js";
import { ChevronDown, Settings } from "lucide-solid";
import { loadChannels } from "@/stores/channels";
import { guildsState, getActiveGuild } from "@/stores/guilds";
import ChannelList from "@/components/channels/ChannelList";
import UserPanel from "./UserPanel";
import GuildSettingsModal from "@/components/guilds/GuildSettingsModal";

const Sidebar: Component = () => {
  const [showGuildSettings, setShowGuildSettings] = createSignal(false);

  // Load channels when sidebar mounts
  onMount(() => {
    loadChannels();
  });

  const activeGuild = () => getActiveGuild();

  return (
    <aside class="w-[240px] flex flex-col bg-surface-layer2 z-10 transition-all duration-300">
      {/* Server Header with Settings */}
      <header class="h-12 px-4 flex items-center justify-between border-b border-white/5 group">
        <div class="flex items-center gap-2 flex-1 min-w-0 cursor-pointer hover:bg-surface-highlight rounded-lg -ml-2 px-2 py-1">
          <h1 class="font-bold text-lg text-text-primary truncate">
            {activeGuild()?.name || "VoiceChat"}
          </h1>
          <ChevronDown class="w-4 h-4 text-text-secondary flex-shrink-0 transition-transform duration-200 group-hover:rotate-180" />
        </div>

        {/* Settings gear - only show when in a guild */}
        <Show when={activeGuild()}>
          <button
            onClick={() => setShowGuildSettings(true)}
            class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded-lg transition-colors"
            title="Server Settings"
          >
            <Settings class="w-4 h-4" />
          </button>
        </Show>
      </header>

      {/* Search Bar */}
      <div class="px-3 py-2">
        <input
          type="text"
          placeholder="Search..."
          class="w-full px-3 py-2 rounded-xl text-sm text-text-input placeholder:text-text-secondary/50 outline-none focus:ring-2 focus:ring-accent-primary/30 border border-white/5"
          style="background-color: var(--color-surface-base)"
        />
      </div>

      {/* Channel List */}
      <ChannelList />

      {/* User Panel (Bottom) */}
      <UserPanel />

      {/* Guild Settings Modal */}
      <Show when={showGuildSettings() && activeGuild()}>
        <GuildSettingsModal
          guildId={activeGuild()!.id}
          onClose={() => setShowGuildSettings(false)}
        />
      </Show>
    </aside>
  );
};

export default Sidebar;
```

**Step 2: Verify build**

Run: `cd client && bun run build`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add client/src/components/layout/Sidebar.tsx
git commit -m "feat(ui): add guild settings gear icon in sidebar"
```

---

## Task 13: Final Build and Test

**Step 1: Run server migration**

```bash
cd server && cargo sqlx migrate run
```

**Step 2: Build server**

```bash
cd server && cargo build --release
```

**Step 3: Build client**

```bash
cd client && bun run build
```

**Step 4: Start server and test**

```bash
cd server && ../target/release/vc-server
```

**Step 5: Manual testing checklist**

- [ ] Open a guild
- [ ] Click gear icon in sidebar header
- [ ] Modal opens with Invites tab (if owner)
- [ ] Create an invite with different expiry options
- [ ] Copy invite URL
- [ ] Delete an invite (with confirmation)
- [ ] Switch to Members tab
- [ ] Search members by name
- [ ] See last online status for each member
- [ ] Kick a member (with confirmation) - not the owner
- [ ] Non-owner sees only Members tab

**Step 6: Final commit**

```bash
git add -A
git commit -m "feat: complete guild management modal implementation"
```

---

## Summary

| Task | Description | Files |
|------|-------------|-------|
| 1 | Database migration | `migrations/20260114000000_guild_invites.sql` |
| 2 | Backend invite types | `server/src/guild/types.rs` |
| 3 | Backend invite handlers | `server/src/guild/invites.rs` |
| 4 | Backend routes | `server/src/guild/mod.rs`, `handlers.rs` |
| 5 | Register invite router | `server/src/api/mod.rs` |
| 6 | Frontend types | `client/src/lib/types.ts` |
| 7 | Frontend API functions | `client/src/lib/tauri.ts` |
| 8 | Frontend store updates | `client/src/stores/guilds.ts` |
| 9 | GuildSettingsModal | `client/src/components/guilds/GuildSettingsModal.tsx` |
| 10 | InvitesTab | `client/src/components/guilds/InvitesTab.tsx` |
| 11 | MembersTab | `client/src/components/guilds/MembersTab.tsx` |
| 12 | Sidebar integration | `client/src/components/layout/Sidebar.tsx` |
| 13 | Build and test | - |
