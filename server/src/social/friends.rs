use axum::{
    extract::{Path, State},
    Json,
};
use uuid::Uuid;
use validator::Validate;

use super::block_cache;
use super::types::{Friend, Friendship, FriendshipStatus, SendFriendRequestBody, SocialError};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::ws::{broadcast_to_user, ServerEvent};

/// POST /api/friends/request
/// Send a friend request to another user
pub async fn send_friend_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<SendFriendRequestBody>,
) -> Result<Json<Friendship>, SocialError> {
    body.validate()
        .map_err(|e| SocialError::Validation(e.to_string()))?;

    // Find the target user by username
    let target_user: Uuid = sqlx::query_scalar!("SELECT id FROM users WHERE username = $1", body.username)
        .fetch_optional(&state.db)
        .await?
        .ok_or(SocialError::UserNotFound)?;

    let target_id = target_user;

    // Cannot friend yourself
    if target_id == auth.id {
        return Err(SocialError::SelfFriendRequest);
    }

    // Check block in either direction via Redis cache
    if let Ok(true) = block_cache::is_blocked_either_direction(&state.redis, auth.id, target_id).await {
        return Err(SocialError::Blocked);
    }

    // Check if friendship already exists (in either direction)
    let existing = sqlx::query_as::<_, Friendship>(
        r"SELECT * FROM friendships
           WHERE (requester_id = $1 AND addressee_id = $2)
              OR (requester_id = $2 AND addressee_id = $1)"
    )
    .bind(auth.id)
    .bind(target_id)
    .fetch_optional(&state.db)
    .await?;

    if let Some(friendship) = existing {
        // Check if blocked
        if friendship.status == FriendshipStatus::Blocked {
            return Err(SocialError::Blocked);
        }
        // Already friends or pending
        return Err(SocialError::AlreadyExists);
    }

    // Create new friendship request
    let friendship_id = Uuid::now_v7();
    let friendship = sqlx::query_as::<_, Friendship>(
        r"INSERT INTO friendships (id, requester_id, addressee_id, status)
           VALUES ($1, $2, $3, 'pending')
           RETURNING id, requester_id, addressee_id, status, created_at, updated_at",
    )
    .bind(friendship_id)
    .bind(auth.id)
    .bind(target_id)
    .fetch_one(&state.db)
    .await?;

    // Send WebSocket notification to addressee
    // Fetch requester's info for the notification
    let requester_info = sqlx::query!(
        "SELECT username, display_name, avatar_url FROM users WHERE id = $1",
        auth.id
    )
    .fetch_one(&state.db)
    .await?;

    let event = ServerEvent::FriendRequestReceived {
        friendship_id: friendship.id,
        from_user_id: auth.id,
        from_username: requester_info.username,
        from_display_name: requester_info.display_name,
        from_avatar_url: requester_info.avatar_url,
    };

    if let Err(e) = broadcast_to_user(&state.redis, target_id, &event).await {
        tracing::warn!("Failed to send friend request notification: {}", e);
    }

    Ok(Json(friendship))
}

/// GET /api/friends
/// List all friends (accepted friendships)
pub async fn list_friends(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Friend>>, SocialError> {
    let friends = sqlx::query_as::<_, Friend>(
        r#"SELECT
            CASE
                WHEN f.requester_id = $1 THEN f.addressee_id
                ELSE f.requester_id
            END as user_id,
            u.username,
            u.display_name,
            u.avatar_url,
            u.status_message,
            false as is_online,
            f.id as friendship_id,
            f.status as "friendship_status",
            f.created_at
           FROM friendships f
           JOIN users u ON u.id = CASE
               WHEN f.requester_id = $1 THEN f.addressee_id
               ELSE f.requester_id
           END
           WHERE (f.requester_id = $1 OR f.addressee_id = $1)
             AND f.status = 'accepted'
           ORDER BY u.username ASC"#,
    )
    .bind(auth.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(friends))
}

/// GET /api/friends/pending
/// List pending friend requests (both sent and received)
pub async fn list_pending_requests(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Friend>>, SocialError> {
    let pending = sqlx::query_as::<_, Friend>(
        r#"SELECT
            CASE
                WHEN f.requester_id = $1 THEN f.addressee_id
                ELSE f.requester_id
            END as user_id,
            u.username,
            u.display_name,
            u.avatar_url,
            u.status_message,
            false as is_online,
            f.id as friendship_id,
            f.status as "friendship_status",
            f.created_at
           FROM friendships f
           JOIN users u ON u.id = CASE
               WHEN f.requester_id = $1 THEN f.addressee_id
               ELSE f.requester_id
           END
           WHERE (f.requester_id = $1 OR f.addressee_id = $1)
             AND f.status = 'pending'
           ORDER BY f.created_at DESC"#,
    )
    .bind(auth.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(pending))
}

/// GET /api/friends/blocked
/// List blocked users
pub async fn list_blocked(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Friend>>, SocialError> {
    let blocked = sqlx::query_as::<_, Friend>(
        r#"SELECT
            f.addressee_id as user_id,
            u.username,
            u.display_name,
            u.avatar_url,
            u.status_message,
            false as is_online,
            f.id as friendship_id,
            f.status as "friendship_status",
            f.created_at
           FROM friendships f
           JOIN users u ON u.id = f.addressee_id
           WHERE f.requester_id = $1
             AND f.status = 'blocked'
           ORDER BY u.username ASC"#,
    )
    .bind(auth.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(blocked))
}

/// POST /api/friends/:id/accept
/// Accept a friend request
pub async fn accept_friend_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(friendship_id): Path<Uuid>,
) -> Result<Json<Friendship>, SocialError> {
    // Verify that auth.id is the addressee of this friendship
    let friendship = sqlx::query_as::<_, Friendship>(
        r"SELECT id, requester_id, addressee_id, status as status, created_at, updated_at
           FROM friendships
           WHERE id = $1",
    )
    .bind(friendship_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(SocialError::FriendshipNotFound)?;

    // Only addressee can accept
    if friendship.addressee_id != auth.id {
        return Err(SocialError::Unauthorized);
    }

    // Only pending requests can be accepted
    if friendship.status != FriendshipStatus::Pending {
        return Err(SocialError::Unauthorized);
    }

    // Update status to accepted
    let updated = sqlx::query_as::<_, Friendship>(
        r"UPDATE friendships
           SET status = 'accepted', updated_at = NOW()
           WHERE id = $1
           RETURNING id, requester_id, addressee_id, status, created_at, updated_at",
    )
    .bind(friendship_id)
    .fetch_one(&state.db)
    .await?;

    // Send WebSocket notification to the original requester
    // Fetch accepter's info for the notification
    let accepter_info = sqlx::query!(
        "SELECT username, display_name, avatar_url FROM users WHERE id = $1",
        auth.id
    )
    .fetch_one(&state.db)
    .await?;

    let event = ServerEvent::FriendRequestAccepted {
        friendship_id: updated.id,
        user_id: auth.id,
        username: accepter_info.username,
        display_name: accepter_info.display_name,
        avatar_url: accepter_info.avatar_url,
    };

    if let Err(e) = broadcast_to_user(&state.redis, friendship.requester_id, &event).await {
        tracing::warn!("Failed to send friend request accepted notification: {}", e);
    }

    Ok(Json(updated))
}

/// POST /api/friends/:id/reject
/// Reject a friend request (deletes the friendship)
pub async fn reject_friend_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(friendship_id): Path<Uuid>,
) -> Result<Json<()>, SocialError> {
    // Verify that auth.id is the addressee of this friendship
    let friendship = sqlx::query_as::<_, Friendship>(
        "SELECT * FROM friendships WHERE id = $1"
    )
    .bind(friendship_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(SocialError::FriendshipNotFound)?;

    // Only addressee can reject
    if friendship.addressee_id != auth.id {
        return Err(SocialError::Unauthorized);
    }

    // Only pending requests can be rejected
    if friendship.status != FriendshipStatus::Pending {
        return Err(SocialError::Unauthorized);
    }

    // Delete the friendship
    sqlx::query!("DELETE FROM friendships WHERE id = $1", friendship_id)
        .execute(&state.db)
        .await?;

    Ok(Json(()))
}

/// POST /api/friends/:id/block
/// Block a user
pub async fn block_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<Friendship>, SocialError> {
    // Cannot block yourself
    if user_id == auth.id {
        return Err(SocialError::SelfFriendRequest);
    }

    // Check if user exists
    let target_exists: bool = sqlx::query_scalar!("SELECT id FROM users WHERE id = $1", user_id)
        .fetch_optional(&state.db)
        .await?
        .is_some();

    if !target_exists {
        return Err(SocialError::UserNotFound);
    }

    // Check if friendship already exists
    let existing = sqlx::query_as::<_, Friendship>(
        r"SELECT * FROM friendships
           WHERE (requester_id = $1 AND addressee_id = $2)
              OR (requester_id = $2 AND addressee_id = $1)"
    )
    .bind(auth.id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    let result = if let Some(friendship) = existing {
        // If we're the requester, update status to blocked
        if friendship.requester_id == auth.id {
            sqlx::query_as::<_, Friendship>(
                r"UPDATE friendships
                   SET status = 'blocked', updated_at = NOW()
                   WHERE id = $1
                   RETURNING id, requester_id, addressee_id, status, created_at, updated_at",
            )
            .bind(friendship.id)
            .fetch_one(&state.db)
            .await?
        } else {
            // If they're the requester, delete and create new blocked entry
            sqlx::query!("DELETE FROM friendships WHERE id = $1", friendship.id)
                .execute(&state.db)
                .await?;

            let friendship_id = Uuid::now_v7();
            sqlx::query_as::<_, Friendship>(
                r"INSERT INTO friendships (id, requester_id, addressee_id, status)
                   VALUES ($1, $2, $3, 'blocked')
                   RETURNING id, requester_id, addressee_id, status, created_at, updated_at",
            )
            .bind(friendship_id)
            .bind(auth.id)
            .bind(user_id)
            .fetch_one(&state.db)
            .await?
        }
    } else {
        // Create new blocked friendship
        let friendship_id = Uuid::now_v7();
        sqlx::query_as::<_, Friendship>(
            r"INSERT INTO friendships (id, requester_id, addressee_id, status)
               VALUES ($1, $2, $3, 'blocked')
               RETURNING id, requester_id, addressee_id, status, created_at, updated_at",
        )
        .bind(friendship_id)
        .bind(auth.id)
        .bind(user_id)
        .fetch_one(&state.db)
        .await?
    };

    // Update Redis block cache
    if let Err(e) = block_cache::add_block(&state.redis, auth.id, user_id).await {
        tracing::warn!("Failed to update block cache: {}", e);
    }

    // Broadcast UserBlocked to all of the blocker's sessions
    let event = ServerEvent::UserBlocked {
        user_id,
    };
    if let Err(e) = broadcast_to_user(&state.redis, auth.id, &event).await {
        tracing::warn!("Failed to broadcast UserBlocked event: {}", e);
    }

    Ok(Json(result))
}

/// DELETE /api/friends/:id/block
/// Unblock a user
pub async fn unblock_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<()>, SocialError> {
    // Find the blocked friendship where we are the blocker
    let friendship = sqlx::query_as::<_, Friendship>(
        r"SELECT * FROM friendships
           WHERE requester_id = $1 AND addressee_id = $2 AND status = 'blocked'"
    )
    .bind(auth.id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(SocialError::FriendshipNotFound)?;

    // Delete the blocked friendship row
    sqlx::query!("DELETE FROM friendships WHERE id = $1", friendship.id)
        .execute(&state.db)
        .await?;

    // Update Redis block cache
    if let Err(e) = block_cache::remove_block(&state.redis, auth.id, user_id).await {
        tracing::warn!("Failed to update block cache: {}", e);
    }

    // Broadcast UserUnblocked to all of the blocker's sessions
    let event = ServerEvent::UserUnblocked {
        user_id,
    };
    if let Err(e) = broadcast_to_user(&state.redis, auth.id, &event).await {
        tracing::warn!("Failed to broadcast UserUnblocked event: {}", e);
    }

    Ok(Json(()))
}

/// DELETE /api/friends/:id
/// Remove a friend (delete friendship)
pub async fn remove_friend(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(friendship_id): Path<Uuid>,
) -> Result<Json<()>, SocialError> {
    // Verify that auth.id is part of this friendship
    let friendship = sqlx::query_as::<_, Friendship>(
        "SELECT * FROM friendships WHERE id = $1"
    )
    .bind(friendship_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(SocialError::FriendshipNotFound)?;

    // Only participants can remove
    if friendship.requester_id != auth.id && friendship.addressee_id != auth.id {
        return Err(SocialError::Unauthorized);
    }

    // Only accepted friendships can be removed this way
    if friendship.status != FriendshipStatus::Accepted {
        return Err(SocialError::Unauthorized);
    }

    // Delete the friendship
    sqlx::query!("DELETE FROM friendships WHERE id = $1", friendship_id)
        .execute(&state.db)
        .await?;

    Ok(Json(()))
}
