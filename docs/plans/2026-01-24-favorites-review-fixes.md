# Favorites Review Fixes - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Address all issues found in the cross-server favorites code review.

**Architecture:** Targeted fixes to existing files - transactions for atomicity, response consistency, error feedback, and comprehensive test coverage.

**Tech Stack:** Rust/Axum (server), Solid.js/TypeScript (frontend), cargo test (testing)

---

## Task 1: Wrap Reorder Handlers in Transaction

**Priority:** HIGH (prevents partial state on error)

**Files:**
- Modify: `server/src/api/favorites.rs:335-378` (reorder_channels)
- Modify: `server/src/api/favorites.rs:380-419` (reorder_guilds)

**Step 1: Update reorder_channels to use transaction**

Replace the reorder_channels handler body with:

```rust
/// PUT /api/me/favorites/reorder - Reorder channels within a guild
pub async fn reorder_channels(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<ReorderChannelsRequest>,
) -> Result<StatusCode, FavoritesError> {
    let guild_id =
        Uuid::parse_str(&request.guild_id).map_err(|_| FavoritesError::InvalidGuilds)?;

    // Start transaction for atomic reorder
    let mut tx = state.db.begin().await?;

    // Verify all channel IDs belong to user's favorites in this guild
    let existing: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT channel_id FROM user_favorite_channels WHERE user_id = $1 AND guild_id = $2",
    )
    .bind(auth_user.id)
    .bind(guild_id)
    .fetch_all(&mut *tx)
    .await?;

    let existing_ids: std::collections::HashSet<String> =
        existing.iter().map(|r| r.0.to_string()).collect();

    // Verify all provided IDs are valid
    for id in &request.channel_ids {
        if !existing_ids.contains(id) {
            return Err(FavoritesError::InvalidChannels);
        }
    }

    // Update positions within transaction
    for (position, channel_id_str) in request.channel_ids.iter().enumerate() {
        let channel_id =
            Uuid::parse_str(channel_id_str).map_err(|_| FavoritesError::InvalidChannels)?;

        sqlx::query(
            "UPDATE user_favorite_channels SET position = $3 WHERE user_id = $1 AND channel_id = $2",
        )
        .bind(auth_user.id)
        .bind(channel_id)
        .bind(position as i32)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
```

**Step 2: Update reorder_guilds to use transaction**

Replace the reorder_guilds handler body with:

```rust
/// PUT /api/me/favorites/reorder-guilds - Reorder guild groups
pub async fn reorder_guilds(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<ReorderGuildsRequest>,
) -> Result<StatusCode, FavoritesError> {
    // Start transaction for atomic reorder
    let mut tx = state.db.begin().await?;

    // Verify all guild IDs belong to user's favorites
    let existing: Vec<(Uuid,)> =
        sqlx::query_as("SELECT guild_id FROM user_favorite_guilds WHERE user_id = $1")
            .bind(auth_user.id)
            .fetch_all(&mut *tx)
            .await?;

    let existing_ids: std::collections::HashSet<String> =
        existing.iter().map(|r| r.0.to_string()).collect();

    // Verify all provided IDs are valid
    for id in &request.guild_ids {
        if !existing_ids.contains(id) {
            return Err(FavoritesError::InvalidGuilds);
        }
    }

    // Update positions within transaction
    for (position, guild_id_str) in request.guild_ids.iter().enumerate() {
        let guild_id =
            Uuid::parse_str(guild_id_str).map_err(|_| FavoritesError::InvalidGuilds)?;

        sqlx::query(
            "UPDATE user_favorite_guilds SET position = $3 WHERE user_id = $1 AND guild_id = $2",
        )
        .bind(auth_user.id)
        .bind(guild_id)
        .bind(position as i32)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
```

**Step 3: Verify compilation**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/api/favorites.rs
git commit -m "fix(api): wrap favorites reorder handlers in transaction

Prevents partial state updates if an error occurs mid-reorder.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 2: Fix Response Wrapper Mismatch

**Priority:** HIGH (API consistency between browser and Tauri)

**Files:**
- Modify: `client/src/stores/favorites.ts:101-102`

**Problem:** Server returns `{ favorites: [...] }` but Tauri returns `[...]` directly. The store expects array but browser path wraps it.

**Step 1: Fix loadFavorites to handle both response shapes**

Update the `loadFavorites` function in `client/src/stores/favorites.ts`:

```typescript
export async function loadFavorites(): Promise<void> {
  setIsLoading(true);
  try {
    const response = await apiCall<FavoriteChannel[] | { favorites: FavoriteChannel[] }>("/api/me/favorites");
    // Handle both shapes: Tauri returns array directly, browser returns { favorites: [...] }
    const data = Array.isArray(response) ? response : response.favorites;
    setFavorites(data);
  } catch (error) {
    console.error("Failed to load favorites:", error);
  } finally {
    setIsLoading(false);
  }
}
```

**Step 2: Verify TypeScript compiles**

Run: `cd client && npx tsc --noEmit`
Expected: No type errors

**Step 3: Commit**

```bash
git add client/src/stores/favorites.ts
git commit -m "fix(client): handle both response shapes in loadFavorites

Tauri returns array directly, browser returns { favorites: [...] } wrapper.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 3: Add User Feedback for Favorite Errors

**Priority:** MEDIUM (improves UX)

**Files:**
- Modify: `client/src/stores/favorites.ts`

**Step 1: Update addFavorite to throw on error**

```typescript
export async function addFavorite(
  channelId: string,
  _guildId: string,
  guildName: string,
  guildIcon: string | null,
  channelName: string,
  channelType: "text" | "voice"
): Promise<boolean> {
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
    const message = error instanceof Error ? error.message : "Failed to add favorite";
    console.error("Failed to add favorite:", message);
    // Re-throw so caller can show toast/notification
    throw new Error(message);
  }
}
```

**Step 2: Update removeFavorite to throw on error**

```typescript
export async function removeFavorite(channelId: string): Promise<boolean> {
  try {
    await apiCall(`/api/me/favorites/${channelId}`, { method: "DELETE" });
    setFavorites((prev) => prev.filter((f) => f.channel_id !== channelId));
    return true;
  } catch (error) {
    const message = error instanceof Error ? error.message : "Failed to remove favorite";
    console.error("Failed to remove favorite:", message);
    throw new Error(message);
  }
}
```

**Step 3: Update toggleFavorite to catch and show error**

```typescript
export async function toggleFavorite(
  channelId: string,
  guildId: string,
  guildName: string,
  guildIcon: string | null,
  channelName: string,
  channelType: "text" | "voice"
): Promise<boolean> {
  try {
    if (isFavorited(channelId)) {
      await removeFavorite(channelId);
    } else {
      await addFavorite(channelId, guildId, guildName, guildIcon, channelName, channelType);
    }
    return true;
  } catch (error) {
    // Error already logged in add/remove, just return false
    return false;
  }
}
```

**Step 4: Verify TypeScript compiles**

Run: `cd client && npx tsc --noEmit`
Expected: No type errors

**Step 5: Commit**

```bash
git add client/src/stores/favorites.ts
git commit -m "fix(client): improve error handling in favorites store

Add/remove now throw errors for caller to handle. Toggle catches
and returns false for simple UI feedback.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 4: Add Rate Limiting to add_favorite

**Priority:** LOW (defense in depth)

**Files:**
- Modify: `server/src/api/mod.rs`

**Step 1: Find existing rate limit pattern**

Search for how other endpoints apply rate limiting (likely via middleware layer).

**Step 2: Apply rate limit to favorites routes**

Add rate limiting middleware to the `/api/me/favorites/{channel_id}` POST route.
Use existing rate limit helpers from `crate::ratelimit`.

Example pattern (adjust based on existing code):

```rust
.route(
    "/api/me/favorites/{channel_id}",
    axum::routing::post(favorites::add_favorite)
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .delete(favorites::remove_favorite),
)
```

**Step 3: Verify compilation**

Run: `cd server && cargo check`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add server/src/api/mod.rs
git commit -m "feat(api): add rate limiting to add_favorite endpoint

Prevents spam of favorite additions.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 5: Add Unit Tests for Favorites Handlers

**Priority:** MEDIUM (testing coverage)

**Files:**
- Create: `server/src/api/favorites_tests.rs` or add `#[cfg(test)]` module to `favorites.rs`

**Step 1: Add test module to favorites.rs**

Add at the bottom of `server/src/api/favorites.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_favorite_channel_from_row() {
        let row = FavoriteChannelRow {
            channel_id: Uuid::new_v4(),
            channel_name: "general".to_string(),
            channel_type: "text".to_string(),
            guild_id: Uuid::new_v4(),
            guild_name: "My Server".to_string(),
            guild_icon: Some("https://example.com/icon.png".to_string()),
            guild_position: 0,
            channel_position: 1,
        };

        let channel = FavoriteChannel::from(row.clone());

        assert_eq!(channel.channel_id, row.channel_id.to_string());
        assert_eq!(channel.channel_name, "general");
        assert_eq!(channel.channel_type, "text");
        assert_eq!(channel.guild_name, "My Server");
        assert_eq!(channel.guild_position, 0);
        assert_eq!(channel.channel_position, 1);
    }

    #[test]
    fn test_favorites_error_responses() {
        // Test that error codes match expected values
        let err = FavoritesError::LimitExceeded;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let err = FavoritesError::ChannelNotFound;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let err = FavoritesError::AlreadyFavorited;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn test_max_favorites_constant() {
        assert_eq!(MAX_FAVORITES_PER_USER, 25);
    }
}
```

**Step 2: Run tests**

Run: `cd server && cargo test favorites`
Expected: All tests pass

**Step 3: Commit**

```bash
git add server/src/api/favorites.rs
git commit -m "test(api): add unit tests for favorites types and errors

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 6: Add Integration Tests for Favorites API

**Priority:** MEDIUM (testing coverage)

**Files:**
- Create: `server/tests/favorites_integration_test.rs`

**Step 1: Create integration test file**

Create `server/tests/favorites_integration_test.rs`:

```rust
//! Integration tests for favorites API endpoints.

use axum::http::StatusCode;
use serde_json::json;

mod common;
use common::TestApp;

#[tokio::test]
async fn test_list_favorites_empty() {
    let app = TestApp::new().await;
    let user = app.create_test_user().await;

    let response = app
        .get("/api/me/favorites")
        .auth(&user.token)
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response.json().await;
    assert_eq!(body["favorites"], json!([]));
}

#[tokio::test]
async fn test_add_favorite_success() {
    let app = TestApp::new().await;
    let user = app.create_test_user().await;
    let guild = app.create_test_guild(&user).await;
    let channel = app.create_test_channel(&guild, "general", "text").await;

    let response = app
        .post(&format!("/api/me/favorites/{}", channel.id))
        .auth(&user.token)
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response.json().await;
    assert_eq!(body["channel_id"], channel.id.to_string());
}

#[tokio::test]
async fn test_add_favorite_limit_exceeded() {
    let app = TestApp::new().await;
    let user = app.create_test_user().await;
    let guild = app.create_test_guild(&user).await;

    // Add 25 favorites
    for i in 0..25 {
        let channel = app.create_test_channel(&guild, &format!("channel-{}", i), "text").await;
        app.post(&format!("/api/me/favorites/{}", channel.id))
            .auth(&user.token)
            .send()
            .await;
    }

    // 26th should fail
    let channel = app.create_test_channel(&guild, "channel-26", "text").await;
    let response = app
        .post(&format!("/api/me/favorites/{}", channel.id))
        .auth(&user.token)
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = response.json().await;
    assert_eq!(body["error"], "limit_exceeded");
}

#[tokio::test]
async fn test_remove_favorite_success() {
    let app = TestApp::new().await;
    let user = app.create_test_user().await;
    let guild = app.create_test_guild(&user).await;
    let channel = app.create_test_channel(&guild, "general", "text").await;

    // Add first
    app.post(&format!("/api/me/favorites/{}", channel.id))
        .auth(&user.token)
        .send()
        .await;

    // Then remove
    let response = app
        .delete(&format!("/api/me/favorites/{}", channel.id))
        .auth(&user.token)
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_add_favorite_non_member_guild() {
    let app = TestApp::new().await;
    let user1 = app.create_test_user().await;
    let user2 = app.create_test_user().await;
    let guild = app.create_test_guild(&user1).await;
    let channel = app.create_test_channel(&guild, "general", "text").await;

    // User2 is not a member of the guild
    let response = app
        .post(&format!("/api/me/favorites/{}", channel.id))
        .auth(&user2.token)
        .send()
        .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
```

**Step 2: Run integration tests**

Run: `cd server && cargo test --test favorites_integration_test`
Expected: All tests pass (may need to adapt based on existing test infrastructure)

**Step 3: Commit**

```bash
git add server/tests/favorites_integration_test.rs
git commit -m "test(api): add integration tests for favorites endpoints

Tests: list empty, add success, limit exceeded, remove, non-member access.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 7: Clean Up Unused Parameter

**Priority:** LOW (code quality)

**Files:**
- Modify: `client/src/stores/favorites.ts:137-144`

**Step 1: Remove unused _guildId parameter**

The `_guildId` parameter in `addFavorite` is not used after the API call (server derives it from channel). However, keeping it maintains a consistent interface for `toggleFavorite`.

**Decision:** Keep as-is. The underscore prefix correctly indicates intentional non-use. No change needed.

---

## Summary

| Task | Priority | Description | Files |
|------|----------|-------------|-------|
| 1 | HIGH | Transaction for reorder | `server/src/api/favorites.rs` |
| 2 | HIGH | Fix response wrapper mismatch | `client/src/stores/favorites.ts` |
| 3 | MEDIUM | Add error feedback | `client/src/stores/favorites.ts` |
| 4 | LOW | Rate limiting | `server/src/api/mod.rs` |
| 5 | MEDIUM | Unit tests | `server/src/api/favorites.rs` |
| 6 | MEDIUM | Integration tests | `server/tests/favorites_integration_test.rs` |
| 7 | LOW | Cleanup unused param | N/A (keep as-is) |

---

## Verification

After all tasks:

```bash
# Server
cd server && cargo check && cargo test

# Client
cd client && npx tsc --noEmit && bun run build
```
