use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;
use validator::Validate;

use super::block_cache;
use super::error::SocialError;
use super::queries;
use super::types::{Friend, Friendship, FriendshipStatus, SendFriendRequestBody};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::ws::{broadcast_to_user, ServerEvent};

/// POST /api/friends/request
/// Send a friend request to another user
#[utoipa::path(
    post,
    path = "/api/friends/request",
    tag = "social",
    request_body = SendFriendRequestBody,
    responses(
        (status = 200, description = "Friend request sent"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn send_friend_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<SendFriendRequestBody>,
) -> Result<Json<Friendship>, SocialError> {
    body.validate()
        .map_err(|e| SocialError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Find the target user by username
    let target_id = queries::find_user_id_by_username(&state.db, &body.username)
        .await?
        .ok_or(SocialError::UserNotFound)?;

    // Cannot friend yourself
    if target_id == auth.id {
        return Err(SocialError::SelfFriendRequest);
    }

    // Check block in either direction via Redis cache
    match block_cache::is_blocked_either_direction(&state.redis, auth.id, target_id).await {
        Ok(true) => return Err(SocialError::Blocked),
        Ok(false) => {}
        Err(e) => {
            tracing::warn!(
                error = %e,
                user_id = %auth.id,
                %target_id,
                fail_open = state.config.block_check_fail_open,
                "Redis block check failed, using failsafe policy"
            );
            if !state.config.block_check_fail_open {
                return Err(SocialError::Blocked);
            }
        }
    }

    // Check if friendship already exists (in either direction)
    if let Some(friendship) = queries::find_friendship_between(&state.db, auth.id, target_id).await?
    {
        // Check if blocked
        if friendship.status == FriendshipStatus::Blocked {
            return Err(SocialError::Blocked);
        }
        // Already friends or pending
        return Err(SocialError::AlreadyExists);
    }

    // Create new friendship request
    let friendship_id = Uuid::now_v7();
    let friendship =
        queries::insert_pending_friendship(&state.db, friendship_id, auth.id, target_id).await?;

    // Send WebSocket notification to addressee
    // Fetch requester's info for the notification
    let requester_info = queries::find_user_profile_snippet(&state.db, auth.id).await?;

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
#[utoipa::path(
    get,
    path = "/api/friends",
    tag = "social",
    responses(
        (status = 200, body = Vec<Friend>),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn list_friends(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Friend>>, SocialError> {
    let friends = queries::list_accepted_friends(&state.db, auth.id).await?;
    Ok(Json(friends))
}

/// GET /api/friends/pending
/// List pending friend requests (both sent and received)
#[utoipa::path(
    get,
    path = "/api/friends/pending",
    tag = "social",
    responses(
        (status = 200, description = "Pending friend requests"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn list_pending_requests(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Friend>>, SocialError> {
    let pending = queries::list_pending_friend_requests(&state.db, auth.id).await?;
    Ok(Json(pending))
}

/// GET /api/friends/blocked
/// List blocked users
#[utoipa::path(
    get,
    path = "/api/friends/blocked",
    tag = "social",
    responses(
        (status = 200, description = "Blocked users"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn list_blocked(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Friend>>, SocialError> {
    let blocked = queries::list_blocked_users(&state.db, auth.id).await?;
    Ok(Json(blocked))
}

/// POST /api/friends/:id/accept
/// Accept a friend request
#[utoipa::path(
    post,
    path = "/api/friends/{id}/accept",
    tag = "social",
    params(("id" = Uuid, Path, description = "Friendship ID")),
    responses(
        (status = 200, description = "Friend request accepted"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn accept_friend_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(friendship_id): Path<Uuid>,
) -> Result<Json<Friendship>, SocialError> {
    // Verify that auth.id is the addressee of this friendship
    let friendship = queries::find_friendship_by_id(&state.db, friendship_id)
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
    let updated = queries::accept_friendship(&state.db, friendship_id).await?;

    // Send WebSocket notification to the original requester
    // Fetch accepter's info for the notification
    let accepter_info = queries::find_user_profile_snippet(&state.db, auth.id).await?;

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
#[utoipa::path(
    post,
    path = "/api/friends/{id}/reject",
    tag = "social",
    params(("id" = Uuid, Path, description = "Friendship ID")),
    responses(
        (status = 200, description = "Friend request rejected"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn reject_friend_request(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(friendship_id): Path<Uuid>,
) -> Result<Json<()>, SocialError> {
    // Verify that auth.id is a participant in this friendship
    let friendship = queries::find_friendship_by_id(&state.db, friendship_id)
        .await?
        .ok_or(SocialError::FriendshipNotFound)?;

    // Either party can reject/cancel a pending request
    if friendship.requester_id != auth.id && friendship.addressee_id != auth.id {
        return Err(SocialError::Unauthorized);
    }

    // Only pending requests can be rejected/cancelled
    if friendship.status != FriendshipStatus::Pending {
        return Err(SocialError::Unauthorized);
    }

    // Delete the friendship
    queries::delete_friendship(&state.db, friendship_id).await?;

    // Notify the other party so their UI updates in real time
    let other_user_id = if friendship.requester_id == auth.id {
        friendship.addressee_id
    } else {
        friendship.requester_id
    };

    let event = ServerEvent::FriendRequestRejected { friendship_id };

    if let Err(e) = broadcast_to_user(&state.redis, other_user_id, &event).await {
        tracing::warn!("Failed to send friend request rejected notification: {}", e);
    }

    Ok(Json(()))
}

/// POST /api/friends/:id/block
/// Block a user
#[utoipa::path(
    post,
    path = "/api/friends/{id}/block",
    tag = "social",
    params(("id" = Uuid, Path, description = "User ID")),
    responses(
        (status = 200, description = "User blocked"),
    ),
    security(("bearer_auth" = [])),
)]
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
    if !queries::user_exists(&state.db, user_id).await? {
        return Err(SocialError::UserNotFound);
    }

    // Check if friendship already exists
    let existing = queries::find_friendship_between(&state.db, auth.id, user_id).await?;

    let result = if let Some(friendship) = existing {
        // If we're the requester, update status to blocked
        if friendship.requester_id == auth.id {
            queries::set_friendship_blocked(&state.db, friendship.id).await?
        } else {
            // If they're the requester, delete and create new blocked entry
            queries::delete_friendship(&state.db, friendship.id).await?;

            let friendship_id = Uuid::now_v7();
            queries::insert_blocked_friendship(&state.db, friendship_id, auth.id, user_id).await?
        }
    } else {
        // Create new blocked friendship
        let friendship_id = Uuid::now_v7();
        queries::insert_blocked_friendship(&state.db, friendship_id, auth.id, user_id).await?
    };

    // Update Redis block cache
    if let Err(e) = block_cache::add_block(&state.redis, auth.id, user_id).await {
        tracing::warn!("Failed to update block cache: {}", e);
    }

    // Broadcast UserBlocked to all of the blocker's sessions
    let event = ServerEvent::UserBlocked { user_id };
    if let Err(e) = broadcast_to_user(&state.redis, auth.id, &event).await {
        tracing::warn!("Failed to broadcast UserBlocked event: {}", e);
    }

    Ok(Json(result))
}

/// DELETE /api/friends/:id/block
/// Unblock a user
#[utoipa::path(
    delete,
    path = "/api/friends/{id}/block",
    tag = "social",
    params(("id" = Uuid, Path, description = "User ID")),
    responses(
        (status = 200, description = "User unblocked"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn unblock_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<()>, SocialError> {
    // Find the blocked friendship where we are the blocker
    let friendship = queries::find_blocked_friendship(&state.db, auth.id, user_id)
        .await?
        .ok_or(SocialError::FriendshipNotFound)?;

    // Delete the blocked friendship row
    queries::delete_friendship(&state.db, friendship.id).await?;

    // Update Redis block cache
    if let Err(e) = block_cache::remove_block(&state.redis, auth.id, user_id).await {
        tracing::warn!("Failed to update block cache: {}", e);
    }

    // Broadcast UserUnblocked to all of the blocker's sessions
    let event = ServerEvent::UserUnblocked { user_id };
    if let Err(e) = broadcast_to_user(&state.redis, auth.id, &event).await {
        tracing::warn!("Failed to broadcast UserUnblocked event: {}", e);
    }

    Ok(Json(()))
}

/// DELETE /api/friends/:id
/// Remove a friend (delete friendship)
#[utoipa::path(
    delete,
    path = "/api/friends/{id}",
    tag = "social",
    params(("id" = Uuid, Path, description = "Friendship ID")),
    responses(
        (status = 200, description = "Friend removed"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn remove_friend(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(friendship_id): Path<Uuid>,
) -> Result<Json<()>, SocialError> {
    // Verify that auth.id is part of this friendship
    let friendship = queries::find_friendship_by_id(&state.db, friendship_id)
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
    queries::delete_friendship(&state.db, friendship_id).await?;

    Ok(Json(()))
}
