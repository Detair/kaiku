# Cross-Server Favorites - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow users to pin channels from different guilds into a unified "Favorites" section in the ServerRail.

**Architecture:** Two normalized database tables (guilds + channels), REST API with Axum handlers, Tauri commands bridging to frontend, Solid.js store and components with drag-to-reorder.

**Tech Stack:** Rust/Axum (server), SQLx/PostgreSQL (database), Rust/Tauri (client backend), Solid.js/TypeScript (frontend)

**Design Document:** `docs/plans/2026-01-24-cross-server-favorites-design.md`

---

## Task 1: Database Migration

**Files:**
- Create: `server/migrations/20260124100000_create_favorites.sql`

**Step 1: Write the migration file**

```sql
-- Cross-server favorites: two normalized tables + cleanup trigger
-- Design doc: docs/plans/2026-01-24-cross-server-favorites-design.md

-- Guild ordering (one row per guild in favorites)
CREATE TABLE user_favorite_guilds (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    position INT NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, guild_id)
);

CREATE INDEX idx_user_fav_guilds ON user_favorite_guilds(user_id, position);

-- Channel favorites (position within guild)
CREATE TABLE user_favorite_channels (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    guild_id UUID NOT NULL,  -- Denormalized for query efficiency
    position INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, channel_id),
    FOREIGN KEY (user_id, guild_id) REFERENCES user_favorite_guilds(user_id, guild_id) ON DELETE CASCADE
);

CREATE INDEX idx_user_fav_channels ON user_favorite_channels(user_id, guild_id, position);

-- Auto-cleanup: Remove guild entry when last channel is unfavorited
CREATE OR REPLACE FUNCTION cleanup_empty_favorite_guilds()
RETURNS TRIGGER AS $$
BEGIN
    DELETE FROM user_favorite_guilds
    WHERE user_id = OLD.user_id
      AND guild_id = OLD.guild_id
      AND NOT EXISTS (
          SELECT 1 FROM user_favorite_channels
          WHERE user_id = OLD.user_id AND guild_id = OLD.guild_id
      );
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_cleanup_favorite_guilds
AFTER DELETE ON user_favorite_channels
FOR EACH ROW
EXECUTE FUNCTION cleanup_empty_favorite_guilds();

COMMENT ON TABLE user_favorite_guilds IS 'Guild ordering for favorites section. One row per guild that has favorited channels.';
COMMENT ON TABLE user_favorite_channels IS 'User channel favorites. Max 25 per user enforced in API.';
```

**Step 2: Run migration to verify syntax**

Run: `cd server && sqlx migrate run --database-url $DATABASE_URL`
Expected: Migration applied successfully

**Step 3: Verify tables created**

Run: `cd server && sqlx migrate info --database-url $DATABASE_URL`
Expected: Shows new migration as applied

**Step 4: Commit**

```bash
git add server/migrations/20260124100000_create_favorites.sql
git commit -m "feat(db): add favorites tables with cleanup trigger"
```

---

## Task 2: Backend Types and Error Handling

**Files:**
- Create: `server/src/api/favorites.rs`
- Modify: `server/src/api/mod.rs`

**Step 1: Create favorites.rs with types and error handling**

```rust
//! User Favorites API
//!
//! CRUD operations for user's cross-server channel favorites.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Serialize, FromRow)]
pub struct FavoriteChannelRow {
    pub channel_id: Uuid,
    pub channel_name: String,
    pub channel_type: String,
    pub guild_id: Uuid,
    pub guild_name: String,
    pub guild_icon: Option<String>,
    pub guild_position: i32,
    pub channel_position: i32,
}

#[derive(Debug, Serialize)]
pub struct FavoriteChannel {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_type: String,
    pub guild_id: String,
    pub guild_name: String,
    pub guild_icon: Option<String>,
    pub guild_position: i32,
    pub channel_position: i32,
}

impl From<FavoriteChannelRow> for FavoriteChannel {
    fn from(row: FavoriteChannelRow) -> Self {
        FavoriteChannel {
            channel_id: row.channel_id.to_string(),
            channel_name: row.channel_name,
            channel_type: row.channel_type,
            guild_id: row.guild_id.to_string(),
            guild_name: row.guild_name,
            guild_icon: row.guild_icon,
            guild_position: row.guild_position,
            channel_position: row.channel_position,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FavoritesResponse {
    pub favorites: Vec<FavoriteChannel>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct FavoriteRow {
    pub channel_id: Uuid,
    pub guild_id: Uuid,
    pub position: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct Favorite {
    pub channel_id: String,
    pub guild_id: String,
    pub guild_position: i32,
    pub channel_position: i32,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ReorderChannelsRequest {
    pub guild_id: String,
    pub channel_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReorderGuildsRequest {
    pub guild_ids: Vec<String>,
}

// ============================================================================
// Constants
// ============================================================================

const MAX_FAVORITES_PER_USER: i64 = 25;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum FavoritesError {
    #[error("Channel not found")]
    ChannelNotFound,
    #[error("Channel cannot be favorited (DM channels not allowed)")]
    InvalidChannel,
    #[error("Maximum favorites limit reached (25)")]
    LimitExceeded,
    #[error("Channel already favorited")]
    AlreadyFavorited,
    #[error("Channel is not favorited")]
    NotFavorited,
    #[error("Invalid channel IDs in reorder request")]
    InvalidChannels,
    #[error("Invalid guild IDs in reorder request")]
    InvalidGuilds,
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for FavoritesError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match &self {
            FavoritesError::ChannelNotFound => (StatusCode::NOT_FOUND, "channel_not_found", "Channel not found"),
            FavoritesError::InvalidChannel => (StatusCode::BAD_REQUEST, "invalid_channel", "DM channels cannot be favorited"),
            FavoritesError::LimitExceeded => (StatusCode::BAD_REQUEST, "limit_exceeded", "Maximum 25 favorites allowed"),
            FavoritesError::AlreadyFavorited => (StatusCode::CONFLICT, "already_favorited", "Channel already in favorites"),
            FavoritesError::NotFavorited => (StatusCode::NOT_FOUND, "favorite_not_found", "Channel is not favorited"),
            FavoritesError::InvalidChannels => (StatusCode::BAD_REQUEST, "invalid_channels", "Reorder contains invalid channel IDs"),
            FavoritesError::InvalidGuilds => (StatusCode::BAD_REQUEST, "invalid_guilds", "Reorder contains invalid guild IDs"),
            FavoritesError::Database(err) => {
                tracing::error!("Database error in favorites: {}", err);
                (StatusCode::INTERNAL_SERVER_ERROR, "database_error", "Database error")
            }
        };
        (status, Json(serde_json::json!({ "error": code, "message": message }))).into_response()
    }
}
```

**Step 2: Build to verify types compile**

Run: `cd server && cargo check`
Expected: Compiles without errors (handlers not yet implemented)

**Step 3: Commit**

```bash
git add server/src/api/favorites.rs
git commit -m "feat(api): add favorites types and error handling"
```

---

## Task 3: Backend Handlers - List and Add

**Files:**
- Modify: `server/src/api/favorites.rs`

**Step 1: Add list_favorites handler**

Add to `server/src/api/favorites.rs`:

```rust
// ============================================================================
// Handlers
// ============================================================================

/// GET /api/me/favorites - List user's favorite channels
pub async fn list_favorites(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<FavoritesResponse>, FavoritesError> {
    let rows = sqlx::query_as::<_, FavoriteChannelRow>(r#"
        SELECT
            fc.channel_id,
            c.name as channel_name,
            c.channel_type,
            fc.guild_id,
            g.name as guild_name,
            g.icon_url as guild_icon,
            fg.position as guild_position,
            fc.position as channel_position
        FROM user_favorite_channels fc
        JOIN user_favorite_guilds fg ON fg.user_id = fc.user_id AND fg.guild_id = fc.guild_id
        JOIN channels c ON c.id = fc.channel_id
        JOIN guilds g ON g.id = fc.guild_id
        JOIN guild_members gm ON gm.guild_id = fc.guild_id AND gm.user_id = fc.user_id
        WHERE fc.user_id = $1
        ORDER BY fg.position ASC, fc.position ASC
    "#)
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;

    let favorites: Vec<FavoriteChannel> = rows.into_iter().map(FavoriteChannel::from).collect();
    Ok(Json(FavoritesResponse { favorites }))
}
```

**Step 2: Add add_favorite handler**

Add to `server/src/api/favorites.rs`:

```rust
/// POST /api/me/favorites/:channel_id - Add channel to favorites
pub async fn add_favorite(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<Favorite>, FavoritesError> {
    // 1. Check limit (max 25)
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM user_favorite_channels WHERE user_id = $1"
    )
    .bind(auth_user.id)
    .fetch_one(&state.db)
    .await?;

    if count.0 >= MAX_FAVORITES_PER_USER {
        return Err(FavoritesError::LimitExceeded);
    }

    // 2. Verify channel exists and get guild_id
    let channel = sqlx::query_as::<_, (Uuid, Option<Uuid>)>(
        "SELECT id, guild_id FROM channels WHERE id = $1"
    )
    .bind(channel_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(FavoritesError::ChannelNotFound)?;

    let guild_id = channel.1.ok_or(FavoritesError::InvalidChannel)?;

    // 3. Verify user has access to guild
    let is_member = sqlx::query(
        "SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2"
    )
    .bind(guild_id)
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?
    .is_some();

    if !is_member {
        return Err(FavoritesError::ChannelNotFound);  // Don't leak existence
    }

    // 4. Transaction for atomic insert
    let mut tx = state.db.begin().await?;

    // 5. Insert guild entry (ON CONFLICT for race condition)
    sqlx::query(r#"
        INSERT INTO user_favorite_guilds (user_id, guild_id, position)
        SELECT $1, $2, COALESCE((SELECT MAX(position) + 1 FROM user_favorite_guilds WHERE user_id = $1), 0)
        ON CONFLICT (user_id, guild_id) DO NOTHING
    "#)
    .bind(auth_user.id)
    .bind(guild_id)
    .execute(&mut *tx)
    .await?;

    // 6. Insert channel entry
    let result = sqlx::query_as::<_, FavoriteRow>(r#"
        INSERT INTO user_favorite_channels (user_id, channel_id, guild_id, position)
        VALUES ($1, $2, $3, COALESCE((SELECT MAX(position) + 1 FROM user_favorite_channels WHERE user_id = $1 AND guild_id = $3), 0))
        RETURNING channel_id, guild_id, position, created_at
    "#)
    .bind(auth_user.id)
    .bind(channel_id)
    .bind(guild_id)
    .fetch_one(&mut *tx)
    .await;

    let favorite = match result {
        Ok(row) => row,
        Err(sqlx::Error::Database(ref db_err)) if db_err.is_unique_violation() => {
            return Err(FavoritesError::AlreadyFavorited);
        }
        Err(e) => return Err(FavoritesError::Database(e)),
    };

    // 7. Get guild_position for response
    let guild_pos: (i32,) = sqlx::query_as(
        "SELECT position FROM user_favorite_guilds WHERE user_id = $1 AND guild_id = $2"
    )
    .bind(auth_user.id)
    .bind(guild_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(Favorite {
        channel_id: favorite.channel_id.to_string(),
        guild_id: favorite.guild_id.to_string(),
        guild_position: guild_pos.0,
        channel_position: favorite.position,
        created_at: favorite.created_at.to_rfc3339(),
    }))
}
```

**Step 3: Build to verify handlers compile**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/api/favorites.rs
git commit -m "feat(api): add list and add favorite handlers"
```

---

## Task 4: Backend Handlers - Remove and Reorder

**Files:**
- Modify: `server/src/api/favorites.rs`

**Step 1: Add remove_favorite handler**

Add to `server/src/api/favorites.rs`:

```rust
/// DELETE /api/me/favorites/:channel_id - Remove channel from favorites
pub async fn remove_favorite(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<StatusCode, FavoritesError> {
    let result = sqlx::query(
        "DELETE FROM user_favorite_channels WHERE user_id = $1 AND channel_id = $2"
    )
    .bind(auth_user.id)
    .bind(channel_id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(FavoritesError::NotFavorited);
    }

    // Trigger handles guild cleanup automatically
    Ok(StatusCode::NO_CONTENT)
}
```

**Step 2: Add reorder_channels handler**

Add to `server/src/api/favorites.rs`:

```rust
/// PUT /api/me/favorites/reorder - Reorder channels within a guild
pub async fn reorder_channels(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<ReorderChannelsRequest>,
) -> Result<StatusCode, FavoritesError> {
    let guild_id = Uuid::parse_str(&request.guild_id)
        .map_err(|_| FavoritesError::InvalidGuilds)?;

    // Verify all channel IDs belong to user's favorites in this guild
    let existing: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT channel_id FROM user_favorite_channels WHERE user_id = $1 AND guild_id = $2"
    )
    .bind(auth_user.id)
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    let existing_ids: std::collections::HashSet<String> = existing
        .iter()
        .map(|r| r.0.to_string())
        .collect();

    // Verify all provided IDs are valid
    for id in &request.channel_ids {
        if !existing_ids.contains(id) {
            return Err(FavoritesError::InvalidChannels);
        }
    }

    // Update positions
    for (position, channel_id_str) in request.channel_ids.iter().enumerate() {
        let channel_id = Uuid::parse_str(channel_id_str)
            .map_err(|_| FavoritesError::InvalidChannels)?;

        sqlx::query(
            "UPDATE user_favorite_channels SET position = $3 WHERE user_id = $1 AND channel_id = $2"
        )
        .bind(auth_user.id)
        .bind(channel_id)
        .bind(position as i32)
        .execute(&state.db)
        .await?;
    }

    Ok(StatusCode::NO_CONTENT)
}
```

**Step 3: Add reorder_guilds handler**

Add to `server/src/api/favorites.rs`:

```rust
/// PUT /api/me/favorites/reorder-guilds - Reorder guild groups
pub async fn reorder_guilds(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<ReorderGuildsRequest>,
) -> Result<StatusCode, FavoritesError> {
    // Verify all guild IDs belong to user's favorites
    let existing: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT guild_id FROM user_favorite_guilds WHERE user_id = $1"
    )
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;

    let existing_ids: std::collections::HashSet<String> = existing
        .iter()
        .map(|r| r.0.to_string())
        .collect();

    // Verify all provided IDs are valid
    for id in &request.guild_ids {
        if !existing_ids.contains(id) {
            return Err(FavoritesError::InvalidGuilds);
        }
    }

    // Update positions
    for (position, guild_id_str) in request.guild_ids.iter().enumerate() {
        let guild_id = Uuid::parse_str(guild_id_str)
            .map_err(|_| FavoritesError::InvalidGuilds)?;

        sqlx::query(
            "UPDATE user_favorite_guilds SET position = $3 WHERE user_id = $1 AND guild_id = $2"
        )
        .bind(auth_user.id)
        .bind(guild_id)
        .bind(position as i32)
        .execute(&state.db)
        .await?;
    }

    Ok(StatusCode::NO_CONTENT)
}
```

**Step 4: Build to verify handlers compile**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 5: Commit**

```bash
git add server/src/api/favorites.rs
git commit -m "feat(api): add remove and reorder favorite handlers"
```

---

## Task 5: Register API Routes

**Files:**
- Modify: `server/src/api/mod.rs`

**Step 1: Add favorites module declaration**

Add to top of `server/src/api/mod.rs`:

```rust
pub mod favorites;
```

**Step 2: Add routes in create_router function**

Add to `api_routes` in `create_router`:

```rust
.route("/api/me/favorites", get(favorites::list_favorites))
.route("/api/me/favorites/{channel_id}",
    axum::routing::post(favorites::add_favorite)
        .delete(favorites::remove_favorite))
.route("/api/me/favorites/reorder", put(favorites::reorder_channels))
.route("/api/me/favorites/reorder-guilds", put(favorites::reorder_guilds))
```

**Step 3: Add routing import**

Add `post` to the routing import if not present:

```rust
use axum::{
    extract::DefaultBodyLimit, extract::State, middleware::from_fn, middleware::from_fn_with_state,
    routing::{get, post, put}, Json, Router,
};
```

**Step 4: Build and test**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 5: Run tests**

Run: `cd server && cargo test`
Expected: All tests pass

**Step 6: Commit**

```bash
git add server/src/api/mod.rs server/src/api/favorites.rs
git commit -m "feat(api): register favorites routes"
```

---

## Task 6: Frontend Types

**Files:**
- Modify: `client/src/lib/types.ts`

**Step 1: Add favorites types**

Add to `client/src/lib/types.ts` (near the Pins section):

```typescript
// Favorites Types
// ============================================================================

export interface FavoriteChannel {
  channel_id: string;
  channel_name: string;
  channel_type: "text" | "voice";
  guild_id: string;
  guild_name: string;
  guild_icon: string | null;
  guild_position: number;
  channel_position: number;
}

export interface FavoritesResponse {
  favorites: FavoriteChannel[];
}

export interface Favorite {
  channel_id: string;
  guild_id: string;
  guild_position: number;
  channel_position: number;
  created_at: string;
}

export interface ReorderChannelsRequest {
  guild_id: string;
  channel_ids: string[];
}

export interface ReorderGuildsRequest {
  guild_ids: string[];
}
```

**Step 2: Verify types compile**

Run: `cd client && bun run check`
Expected: No type errors

**Step 3: Commit**

```bash
git add client/src/lib/types.ts
git commit -m "feat(client): add favorites types"
```

---

## Task 7: Tauri Commands

**Files:**
- Create: `client/src-tauri/src/commands/favorites.rs`
- Modify: `client/src-tauri/src/commands/mod.rs`

**Step 1: Create favorites.rs with all commands**

Create `client/src-tauri/src/commands/favorites.rs`:

```rust
//! Favorites Tauri Commands
//!
//! CRUD operations for cross-server channel favorites.

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tracing::{debug, error};

use crate::AppState;

/// A favorite channel.
#[derive(Debug, Serialize, Deserialize)]
pub struct FavoriteChannel {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_type: String,
    pub guild_id: String,
    pub guild_name: String,
    pub guild_icon: Option<String>,
    pub guild_position: i32,
    pub channel_position: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FavoritesResponse {
    pub favorites: Vec<FavoriteChannel>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Favorite {
    pub channel_id: String,
    pub guild_id: String,
    pub guild_position: i32,
    pub channel_position: i32,
    pub created_at: String,
}

/// Fetch all favorites for the current user.
#[command]
pub async fn fetch_favorites(state: State<'_, AppState>) -> Result<Vec<FavoriteChannel>, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Fetching favorites from server");

    let response = state
        .http
        .get(format!("{server_url}/api/me/favorites"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to fetch favorites: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to fetch favorites: {}", status);
        return Err(format!("Failed to fetch favorites: {status}"));
    }

    let data: FavoritesResponse = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Fetched {} favorites", data.favorites.len());
    Ok(data.favorites)
}

/// Add a channel to favorites.
#[command]
pub async fn add_favorite(
    state: State<'_, AppState>,
    channel_id: String,
) -> Result<Favorite, String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Adding favorite: channel_id={}", channel_id);

    let response = state
        .http
        .post(format!("{server_url}/api/me/favorites/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to add favorite: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        error!("Failed to add favorite: {} - {}", status, body);
        return Err(format!("Failed to add favorite: {status}"));
    }

    let favorite: Favorite = response
        .json()
        .await
        .map_err(|e| format!("Invalid response: {e}"))?;

    debug!("Favorite added: channel_id={}", favorite.channel_id);
    Ok(favorite)
}

/// Remove a channel from favorites.
#[command]
pub async fn remove_favorite(
    state: State<'_, AppState>,
    channel_id: String,
) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Removing favorite: channel_id={}", channel_id);

    let response = state
        .http
        .delete(format!("{server_url}/api/me/favorites/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to remove favorite: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to remove favorite: {}", status);
        return Err(format!("Failed to remove favorite: {status}"));
    }

    debug!("Favorite removed: channel_id={}", channel_id);
    Ok(())
}

/// Reorder channels within a guild.
#[command]
pub async fn reorder_favorite_channels(
    state: State<'_, AppState>,
    guild_id: String,
    channel_ids: Vec<String>,
) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Reordering {} favorite channels in guild {}", channel_ids.len(), guild_id);

    let response = state
        .http
        .put(format!("{server_url}/api/me/favorites/reorder"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "guild_id": guild_id, "channel_ids": channel_ids }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to reorder favorites: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to reorder favorites: {}", status);
        return Err(format!("Failed to reorder favorites: {status}"));
    }

    debug!("Favorites reordered successfully");
    Ok(())
}

/// Reorder guild groups.
#[command]
pub async fn reorder_favorite_guilds(
    state: State<'_, AppState>,
    guild_ids: Vec<String>,
) -> Result<(), String> {
    let (server_url, token) = {
        let auth = state.auth.read().await;
        (auth.server_url.clone(), auth.access_token.clone())
    };

    let server_url = server_url.ok_or("Not authenticated")?;
    let token = token.ok_or("Not authenticated")?;

    debug!("Reordering {} favorite guilds", guild_ids.len());

    let response = state
        .http
        .put(format!("{server_url}/api/me/favorites/reorder-guilds"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "guild_ids": guild_ids }))
        .send()
        .await
        .map_err(|e| {
            error!("Failed to reorder favorite guilds: {}", e);
            format!("Connection failed: {e}")
        })?;

    if !response.status().is_success() {
        let status = response.status();
        error!("Failed to reorder favorite guilds: {}", status);
        return Err(format!("Failed to reorder favorite guilds: {status}"));
    }

    debug!("Favorite guilds reordered successfully");
    Ok(())
}
```

**Step 2: Register module in mod.rs**

Add to `client/src-tauri/src/commands/mod.rs`:

```rust
pub mod favorites;
```

**Step 3: Register commands in main.rs or lib.rs**

Find where commands are registered (likely `lib.rs` or `main.rs`) and add:

```rust
favorites::fetch_favorites,
favorites::add_favorite,
favorites::remove_favorite,
favorites::reorder_favorite_channels,
favorites::reorder_favorite_guilds,
```

**Step 4: Build to verify**

Run: `cd client && cargo build --manifest-path src-tauri/Cargo.toml`
Expected: Compiles without errors

**Step 5: Commit**

```bash
git add client/src-tauri/src/commands/favorites.rs client/src-tauri/src/commands/mod.rs
git commit -m "feat(client): add favorites tauri commands"
```

---

## Task 8: Frontend Store

**Files:**
- Create: `client/src/stores/favorites.ts`

**Step 1: Create favorites store**

Create `client/src/stores/favorites.ts`:

```typescript
/**
 * Favorites Store
 *
 * Manages user's cross-server channel favorites.
 */

import { createSignal, createMemo } from "solid-js";
import type { FavoriteChannel, Favorite } from "@/lib/types";

// ============================================================================
// State
// ============================================================================

const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

const [favorites, setFavorites] = createSignal<FavoriteChannel[]>([]);
const [isLoading, setIsLoading] = createSignal(false);

// ============================================================================
// Computed
// ============================================================================

/**
 * Favorites grouped by guild, sorted by guild_position then channel_position.
 */
export const favoritesByGuild = createMemo(() => {
  const grouped = new Map<string, { guild: { id: string; name: string; icon: string | null }; channels: FavoriteChannel[] }>();

  for (const fav of favorites()) {
    if (!grouped.has(fav.guild_id)) {
      grouped.set(fav.guild_id, {
        guild: { id: fav.guild_id, name: fav.guild_name, icon: fav.guild_icon },
        channels: [],
      });
    }
    grouped.get(fav.guild_id)!.channels.push(fav);
  }

  // Sort channels within each guild by channel_position
  for (const group of grouped.values()) {
    group.channels.sort((a, b) => a.channel_position - b.channel_position);
  }

  // Convert to array and sort by guild_position
  return Array.from(grouped.values()).sort((a, b) => {
    const posA = a.channels[0]?.guild_position ?? 0;
    const posB = b.channels[0]?.guild_position ?? 0;
    return posA - posB;
  });
});

/**
 * Check if a channel is favorited.
 */
export function isFavorited(channelId: string): boolean {
  return favorites().some((f) => f.channel_id === channelId);
}

// ============================================================================
// API Calls
// ============================================================================

async function apiCall<T>(
  endpoint: string,
  options?: RequestInit
): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    const method = options?.method || "GET";
    const body = options?.body ? JSON.parse(options.body as string) : undefined;

    switch (method) {
      case "GET":
        return invoke("fetch_favorites") as Promise<T>;
      case "POST": {
        const channelId = endpoint.split("/").pop();
        return invoke("add_favorite", { channelId }) as Promise<T>;
      }
      case "DELETE": {
        const channelId = endpoint.split("/").pop();
        return invoke("remove_favorite", { channelId }) as Promise<T>;
      }
      case "PUT":
        if (endpoint.includes("reorder-guilds")) {
          return invoke("reorder_favorite_guilds", { guildIds: body.guild_ids }) as Promise<T>;
        }
        return invoke("reorder_favorite_channels", { guildId: body.guild_id, channelIds: body.channel_ids }) as Promise<T>;
      default:
        throw new Error(`Unknown method: ${method}`);
    }
  }

  // HTTP fallback for browser
  const token = localStorage.getItem("vc:token");
  const baseUrl = import.meta.env.VITE_API_URL || "";

  const response = await fetch(`${baseUrl}${endpoint}`, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${token}`,
      ...options?.headers,
    },
  });

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: "unknown" }));
    throw new Error(error.error || `API error: ${response.status}`);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return response.json();
}

// ============================================================================
// Actions
// ============================================================================

export async function loadFavorites(): Promise<void> {
  setIsLoading(true);
  try {
    const data = await apiCall<FavoriteChannel[]>("/api/me/favorites");
    setFavorites(data);
  } catch (error) {
    console.error("Failed to load favorites:", error);
  } finally {
    setIsLoading(false);
  }
}

export async function addFavorite(channelId: string, guildId: string, guildName: string, guildIcon: string | null, channelName: string, channelType: "text" | "voice"): Promise<boolean> {
  try {
    const result = await apiCall<Favorite>(`/api/me/favorites/${channelId}`, {
      method: "POST",
    });

    // Add to local state
    setFavorites((prev) => [
      ...prev,
      {
        channel_id: result.channel_id,
        channel_name: channelName,
        channel_type: channelType,
        guild_id: result.guild_id,
        guild_name: guildName,
        guild_icon: guildIcon,
        guild_position: result.guild_position,
        channel_position: result.channel_position,
      },
    ]);
    return true;
  } catch (error) {
    console.error("Failed to add favorite:", error);
    return false;
  }
}

export async function removeFavorite(channelId: string): Promise<boolean> {
  try {
    await apiCall(`/api/me/favorites/${channelId}`, { method: "DELETE" });
    setFavorites((prev) => prev.filter((f) => f.channel_id !== channelId));
    return true;
  } catch (error) {
    console.error("Failed to remove favorite:", error);
    return false;
  }
}

export async function toggleFavorite(
  channelId: string,
  guildId: string,
  guildName: string,
  guildIcon: string | null,
  channelName: string,
  channelType: "text" | "voice"
): Promise<boolean> {
  if (isFavorited(channelId)) {
    return removeFavorite(channelId);
  }
  return addFavorite(channelId, guildId, guildName, guildIcon, channelName, channelType);
}

export async function reorderChannels(guildId: string, channelIds: string[]): Promise<boolean> {
  try {
    await apiCall("/api/me/favorites/reorder", {
      method: "PUT",
      body: JSON.stringify({ guild_id: guildId, channel_ids: channelIds }),
    });

    // Update local state positions
    setFavorites((prev) => {
      return prev.map((f) => {
        if (f.guild_id === guildId) {
          const newPos = channelIds.indexOf(f.channel_id);
          return newPos >= 0 ? { ...f, channel_position: newPos } : f;
        }
        return f;
      });
    });
    return true;
  } catch (error) {
    console.error("Failed to reorder channels:", error);
    return false;
  }
}

export async function reorderGuilds(guildIds: string[]): Promise<boolean> {
  try {
    await apiCall("/api/me/favorites/reorder-guilds", {
      method: "PUT",
      body: JSON.stringify({ guild_ids: guildIds }),
    });

    // Update local state positions
    setFavorites((prev) => {
      return prev.map((f) => {
        const newPos = guildIds.indexOf(f.guild_id);
        return newPos >= 0 ? { ...f, guild_position: newPos } : f;
      });
    });
    return true;
  } catch (error) {
    console.error("Failed to reorder guilds:", error);
    return false;
  }
}

// ============================================================================
// Selectors
// ============================================================================

export { favorites, isLoading };
```

**Step 2: Verify store compiles**

Run: `cd client && bun run check`
Expected: No type errors

**Step 3: Commit**

```bash
git add client/src/stores/favorites.ts
git commit -m "feat(client): add favorites store"
```

---

## Task 9: FavoritesSection Component

**Files:**
- Create: `client/src/components/layout/FavoritesSection.tsx`

**Step 1: Create FavoritesSection component**

Create `client/src/components/layout/FavoritesSection.tsx`:

```tsx
/**
 * FavoritesSection - Expandable Favorites in ServerRail
 *
 * Displays user's favorited channels grouped by guild.
 * Features:
 * - Expandable/collapsible section
 * - Guild headers with icons
 * - Channel items with navigation
 * - Visual feedback for active channel
 */

import { Component, For, Show, createSignal } from "solid-js";
import { Star, ChevronDown, ChevronRight, Hash, Volume2 } from "lucide-solid";
import { favoritesByGuild, isLoading } from "@/stores/favorites";
import { selectGuild, selectChannel, guildsState } from "@/stores/guilds";

const FavoritesSection: Component = () => {
  const [isExpanded, setIsExpanded] = createSignal(true);

  const handleChannelClick = (guildId: string, channelId: string) => {
    // Navigate to guild and select channel
    selectGuild(guildId);
    selectChannel(channelId);
  };

  const isActiveChannel = (channelId: string) => {
    return guildsState.activeChannelId === channelId;
  };

  return (
    <Show when={favoritesByGuild().length > 0}>
      <div class="w-full">
        {/* Header */}
        <button
          class="w-full flex items-center gap-2 px-3 py-2 text-xs font-semibold text-text-secondary hover:text-text-primary transition-colors"
          onClick={() => setIsExpanded((prev) => !prev)}
        >
          <Star class="w-3.5 h-3.5 text-yellow-400" />
          <span>Favorites</span>
          <span class="ml-auto">
            <Show when={isExpanded()} fallback={<ChevronRight class="w-3.5 h-3.5" />}>
              <ChevronDown class="w-3.5 h-3.5" />
            </Show>
          </span>
        </button>

        {/* Content */}
        <Show when={isExpanded()}>
          <div class="px-2 pb-2 space-y-2">
            <Show when={isLoading()}>
              <div class="px-2 py-1 text-xs text-text-muted">Loading...</div>
            </Show>

            <For each={favoritesByGuild()}>
              {(group) => (
                <div class="space-y-0.5">
                  {/* Guild Header */}
                  <div class="flex items-center gap-2 px-2 py-1">
                    <Show
                      when={group.guild.icon}
                      fallback={
                        <div class="w-4 h-4 rounded bg-surface-layer2 flex items-center justify-center text-[8px] font-semibold text-text-secondary">
                          {group.guild.name.slice(0, 2).toUpperCase()}
                        </div>
                      }
                    >
                      <img
                        src={group.guild.icon!}
                        alt={group.guild.name}
                        class="w-4 h-4 rounded object-cover"
                      />
                    </Show>
                    <span class="text-xs font-medium text-text-secondary truncate">
                      {group.guild.name}
                    </span>
                  </div>

                  {/* Channels */}
                  <For each={group.channels}>
                    {(channel) => (
                      <button
                        class="w-full flex items-center gap-2 px-2 py-1 rounded text-xs transition-colors"
                        classList={{
                          "bg-surface-highlight text-text-primary": isActiveChannel(channel.channel_id),
                          "text-text-secondary hover:text-text-primary hover:bg-white/5": !isActiveChannel(channel.channel_id),
                        }}
                        onClick={() => handleChannelClick(channel.guild_id, channel.channel_id)}
                      >
                        <Show
                          when={channel.channel_type === "voice"}
                          fallback={<Hash class="w-3.5 h-3.5 shrink-0" />}
                        >
                          <Volume2 class="w-3.5 h-3.5 shrink-0" />
                        </Show>
                        <span class="truncate">{channel.channel_name}</span>
                      </button>
                    )}
                  </For>
                </div>
              )}
            </For>
          </div>
        </Show>
      </div>
    </Show>
  );
};

export default FavoritesSection;
```

**Step 2: Verify component compiles**

Run: `cd client && bun run check`
Expected: No type errors

**Step 3: Commit**

```bash
git add client/src/components/layout/FavoritesSection.tsx
git commit -m "feat(client): add FavoritesSection component"
```

---

## Task 10: Integrate FavoritesSection into ServerRail

**Files:**
- Modify: `client/src/components/layout/ServerRail.tsx`

**Step 1: Import FavoritesSection and loadFavorites**

Add to imports:

```tsx
import FavoritesSection from "./FavoritesSection";
import { loadFavorites } from "@/stores/favorites";
import { onMount } from "solid-js";
```

**Step 2: Load favorites on mount**

Add inside the component:

```tsx
onMount(() => {
  loadFavorites();
});
```

**Step 3: Add FavoritesSection after the home icon separator**

After the first separator (after line 76), add:

```tsx
{/* Favorites Section */}
<FavoritesSection />

{/* Separator (only show if favorites exist) */}
<Show when={favoritesByGuild().length > 0}>
  <div class="w-8 h-0.5 bg-white/10 rounded-full my-1" />
</Show>
```

**Step 4: Import Show and favoritesByGuild**

Update imports:

```tsx
import { Component, createSignal, For, Show, onMount } from "solid-js";
import { favoritesByGuild } from "@/stores/favorites";
```

**Step 5: Verify changes compile**

Run: `cd client && bun run check`
Expected: No type errors

**Step 6: Build client**

Run: `cd client && bun run build`
Expected: Build succeeds

**Step 7: Commit**

```bash
git add client/src/components/layout/ServerRail.tsx
git commit -m "feat(client): integrate FavoritesSection into ServerRail"
```

---

## Task 11: Add Star Icon to ChannelItem

**Files:**
- Modify: `client/src/components/channels/ChannelItem.tsx`

**Step 1: Import favorites functions**

Add to imports:

```tsx
import { Star } from "lucide-solid";
import { isFavorited, toggleFavorite } from "@/stores/favorites";
import { guildsState } from "@/stores/guilds";
```

**Step 2: Add props for guild info**

Update interface:

```tsx
interface ChannelItemProps {
  channel: Channel;
  isSelected: boolean;
  onClick: () => void;
  onSettings?: () => void;
  guildId?: string;
  guildName?: string;
  guildIcon?: string | null;
}
```

**Step 3: Add star button handler**

Add inside component:

```tsx
const handleToggleFavorite = async (e: MouseEvent) => {
  e.stopPropagation();
  if (!props.guildId || !props.guildName) return;

  await toggleFavorite(
    props.channel.id,
    props.guildId,
    props.guildName,
    props.guildIcon ?? null,
    props.channel.name,
    props.channel.channel_type as "text" | "voice"
  );
};

const channelIsFavorited = () => isFavorited(props.channel.id);
```

**Step 4: Add star icon to channel row**

Add before the settings button (around line 133), inside the main button:

```tsx
{/* Favorite star - shown on hover or when favorited */}
<Show when={props.guildId}>
  <button
    class="p-0.5 rounded transition-all duration-200"
    classList={{
      "text-yellow-400": channelIsFavorited(),
      "text-text-secondary hover:text-yellow-400 opacity-0 group-hover:opacity-100": !channelIsFavorited(),
    }}
    onClick={handleToggleFavorite}
    title={channelIsFavorited() ? "Remove from favorites" : "Add to favorites"}
  >
    <Star
      class="w-3.5 h-3.5"
      fill={channelIsFavorited() ? "currentColor" : "none"}
    />
  </button>
</Show>
```

**Step 5: Update ChannelList to pass guild info**

This will be done when ChannelList is updated to pass the required props.

**Step 6: Verify changes compile**

Run: `cd client && bun run check`
Expected: No type errors

**Step 7: Commit**

```bash
git add client/src/components/channels/ChannelItem.tsx
git commit -m "feat(client): add star icon to ChannelItem for favorites"
```

---

## Task 12: Update ChannelList to Pass Guild Info

**Files:**
- Modify: `client/src/components/channels/ChannelList.tsx`

**Step 1: Read ChannelList to understand current structure**

Read the file to understand how to add guild props.

**Step 2: Pass guild info to ChannelItem**

Update ChannelItem usage to include:

```tsx
guildId={guildsState.activeGuildId ?? undefined}
guildName={currentGuild()?.name}
guildIcon={currentGuild()?.icon_url}
```

**Step 3: Ensure currentGuild is available**

Add if not present:

```tsx
const currentGuild = () => guildsState.guilds.find(g => g.id === guildsState.activeGuildId);
```

**Step 4: Verify changes compile**

Run: `cd client && bun run check`
Expected: No type errors

**Step 5: Commit**

```bash
git add client/src/components/channels/ChannelList.tsx
git commit -m "feat(client): pass guild info to ChannelItem for favorites"
```

---

## Task 13: Add Context Menu Option

**Files:**
- Modify or create context menu component

**Step 1: Find existing context menu**

Search for context menu in the codebase.

**Step 2: Add "Add to Favorites" / "Remove from Favorites" option**

Add menu item that calls toggleFavorite.

**Step 3: Verify changes compile**

Run: `cd client && bun run check`
Expected: No type errors

**Step 4: Commit**

```bash
git add [modified files]
git commit -m "feat(client): add favorites option to channel context menu"
```

---

## Task 14: Integration Testing

**Step 1: Run server tests**

Run: `cd server && cargo test`
Expected: All tests pass

**Step 2: Run client type check**

Run: `cd client && bun run check`
Expected: No type errors

**Step 3: Build everything**

Run: `cd server && cargo build && cd ../client && bun run build`
Expected: Both build successfully

**Step 4: Manual testing checklist**

- [ ] Add favorite - Star a channel, verify appears in favorites section
- [ ] Remove favorite - Unstar, verify removed from favorites
- [ ] Limit enforcement - Try to add 26th favorite, verify error
- [ ] Navigate - Click favorited channel, verify navigates to guild + channel
- [ ] Guild cleanup - Remove all channels from a guild's favorites, verify guild header disappears
- [ ] Star indicator - Favorited channels show filled star in channel list

**Step 5: Commit any fixes**

---

## Task 15: Update CHANGELOG

**Files:**
- Modify: `CHANGELOG.md`

**Step 1: Add entry under [Unreleased]**

```markdown
### Added
- Cross-server favorites: pin channels from different guilds into a unified Favorites section
  - Star icon on channels to toggle favorites
  - Right-click context menu option
  - Expandable favorites section in ServerRail grouped by guild
  - Maximum 25 favorites per user
```

**Step 2: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: add cross-server favorites to changelog"
```

---

## Summary

| Task | Description | Files |
|------|-------------|-------|
| 1 | Database migration | `server/migrations/20260124100000_create_favorites.sql` |
| 2 | Backend types | `server/src/api/favorites.rs` |
| 3 | List/Add handlers | `server/src/api/favorites.rs` |
| 4 | Remove/Reorder handlers | `server/src/api/favorites.rs` |
| 5 | Register routes | `server/src/api/mod.rs` |
| 6 | Frontend types | `client/src/lib/types.ts` |
| 7 | Tauri commands | `client/src-tauri/src/commands/favorites.rs` |
| 8 | Frontend store | `client/src/stores/favorites.ts` |
| 9 | FavoritesSection | `client/src/components/layout/FavoritesSection.tsx` |
| 10 | ServerRail integration | `client/src/components/layout/ServerRail.tsx` |
| 11 | ChannelItem star | `client/src/components/channels/ChannelItem.tsx` |
| 12 | ChannelList props | `client/src/components/channels/ChannelList.tsx` |
| 13 | Context menu | TBD |
| 14 | Testing | - |
| 15 | Changelog | `CHANGELOG.md` |

---

## Verification

After all tasks:

```bash
# Server
cd server && cargo check && cargo test

# Client
cd client && bun run check && bun run build
```
