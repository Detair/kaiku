# Cross-Server Favorites - Design

> **Status:** Approved
> **Date:** 2026-01-24

## Overview

Allow users to pin channels from different guilds into a unified "Favorites" section in the ServerRail for quick cross-server navigation.

---

## MVP Scope

- Favorites section in ServerRail (expandable, grouped by guild)
- Star icon + right-click context menu to toggle favorites
- Drag-to-reorder channels within guild groups
- Drag-to-reorder guild groups
- Max 25 favorites per user
- Favorited channels remain visible in guild channel list (with star indicator)

**Deferred:**
- Unread indicators on favorited channels (requires unread tracking integration)

---

## UI Design

### ServerRail Layout

```
[Home]
[─────────────]
[⭐ Favorites ▼]  ← Expandable section
  ├─ Guild A        ← Mini header (draggable)
  │   ├─ #general   ← Channel (draggable within group)
  │   └─ #dev
  ├─ Guild B
  │   └─ 🔊 voice
[─────────────]
[Guild icons...]
```

### Channel List (Guild View)

- Favorited channels show filled star icon (★)
- Non-favorited channels show outline star on hover (☆)
- Click star to toggle favorite status
- Right-click → "Add to Favorites" / "Remove from Favorites"

### Interactions

| Action | Result |
|--------|--------|
| Click star on channel | Toggle favorite status |
| Right-click channel | Context menu with favorite option |
| Click favorited channel in sidebar | Navigate to guild + select channel |
| Drag channel in favorites | Reorder within guild group |
| Drag guild header in favorites | Reorder guild groups |

---

## Data Model

### Tables

```sql
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
```

### Cascade Behavior

| Event | Result |
|-------|--------|
| User deleted | All favorites removed (CASCADE) |
| Guild deleted | Guild entry + channel entries removed (CASCADE) |
| Channel deleted | Channel entry removed (CASCADE) → trigger cleans guild if empty |
| Last channel unfavorited from guild | Trigger removes guild entry |

---

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/me/favorites` | List user's favorite channels |
| POST | `/api/me/favorites/:channel_id` | Add channel to favorites |
| DELETE | `/api/me/favorites/:channel_id` | Remove from favorites |
| PUT | `/api/me/favorites/reorder` | Reorder channels within a guild |
| PUT | `/api/me/favorites/reorder-guilds` | Reorder guild groups |

### Request/Response Types

```typescript
// GET /api/me/favorites response
interface FavoritesResponse {
  favorites: FavoriteChannel[];
}

interface FavoriteChannel {
  channel_id: string;
  channel_name: string;
  channel_type: "text" | "voice";
  guild_id: string;
  guild_name: string;
  guild_icon: string | null;
  guild_position: number;
  channel_position: number;
}

// POST /api/me/favorites/:channel_id response
interface Favorite {
  channel_id: string;
  guild_id: string;
  guild_position: number;
  channel_position: number;
  created_at: string;
}

// PUT /api/me/favorites/reorder request
interface ReorderChannelsRequest {
  guild_id: string;
  channel_ids: string[];  // New order
}

// PUT /api/me/favorites/reorder-guilds request
interface ReorderGuildsRequest {
  guild_ids: string[];  // New order
}
```

### Error Responses

| Code | Error | Description |
|------|-------|-------------|
| 400 | `limit_exceeded` | Max 25 favorites reached |
| 400 | `invalid_channel` | DM channels cannot be favorited |
| 400 | `invalid_channels` | Reorder contains invalid channel IDs |
| 400 | `invalid_guilds` | Reorder contains invalid guild IDs |
| 404 | `channel_not_found` | Channel does not exist |
| 404 | `favorite_not_found` | Channel is not favorited |
| 409 | `already_favorited` | Channel already in favorites |

---

## Implementation Details

### Add Favorite (with all safeguards)

```rust
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

    if count.0 >= 25 {
        return Err(FavoritesError::LimitExceeded);
    }

    // 2. Verify channel exists and get guild_id
    let channel = sqlx::query_as::<_, ChannelRow>(
        "SELECT id, guild_id FROM channels WHERE id = $1"
    )
    .bind(channel_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(FavoritesError::ChannelNotFound)?;

    let guild_id = channel.guild_id.ok_or(FavoritesError::InvalidChannel)?;

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
    let favorite = sqlx::query_as::<_, FavoriteRow>(r#"
        INSERT INTO user_favorite_channels (user_id, channel_id, guild_id, position)
        VALUES ($1, $2, $3, COALESCE((SELECT MAX(position) + 1 FROM user_favorite_channels WHERE user_id = $1 AND guild_id = $3), 0))
        RETURNING user_id, channel_id, guild_id, position, created_at
    "#)
    .bind(auth_user.id)
    .bind(channel_id)
    .bind(guild_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
            FavoritesError::AlreadyFavorited
        }
        _ => FavoritesError::Database(e),
    })?;

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
        channel_id: favorite.channel_id,
        guild_id: favorite.guild_id,
        guild_position: guild_pos.0,
        channel_position: favorite.position,
        created_at: favorite.created_at,
    }))
}
```

### List Favorites (with access filtering)

```rust
pub async fn list_favorites(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<FavoritesResponse>, FavoritesError> {
    let favorites = sqlx::query_as::<_, FavoriteWithChannel>(r#"
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

    Ok(Json(FavoritesResponse { favorites }))
}
```

---

## Files Summary

### Backend
| File | Changes |
|------|---------|
| `server/migrations/YYYYMMDD_create_favorites.sql` | Create tables, indexes, trigger |
| `server/src/api/favorites.rs` | CRUD handlers |
| `server/src/api/mod.rs` | Register routes |

### Frontend
| File | Changes |
|------|---------|
| `client/src/lib/types.ts` | Favorite types |
| `client/src/stores/favorites.ts` | Favorites state management |
| `client/src-tauri/src/commands/favorites.rs` | Tauri commands |
| `client/src/components/layout/ServerRail.tsx` | Add Favorites section |
| `client/src/components/layout/FavoritesSection.tsx` | New component |
| `client/src/components/channels/ChannelItem.tsx` | Add star icon |
| `client/src/components/channels/ChannelContextMenu.tsx` | Add favorite option |

---

## Limits

- Max 25 favorites per user (enforced at API level)
- No limit on guilds (naturally capped by channel limit)

---

## Deferred (Future PRs)

- **Unread indicators** - Show badge count and bold highlight for unread messages
- **Keyboard navigation** - Arrow keys to navigate favorites
- **Drag from guild to favorites** - Drag channel from guild list directly to favorites section

---

## Testing

1. **Add favorite** - Star a channel, verify appears in favorites section
2. **Remove favorite** - Unstar, verify removed from favorites
3. **Limit enforcement** - Try to add 26th favorite, verify error
4. **Reorder channels** - Drag channels within guild, verify order persists
5. **Reorder guilds** - Drag guild headers, verify order persists
6. **Navigate** - Click favorited channel, verify navigates to guild + channel
7. **Guild cleanup** - Remove all channels from a guild's favorites, verify guild header disappears
8. **Permission loss** - Remove user from guild, verify favorites from that guild don't appear
9. **Channel deletion** - Delete a favorited channel, verify removed from favorites
10. **Concurrent add** - Two tabs add first channel from same guild simultaneously, verify no errors
