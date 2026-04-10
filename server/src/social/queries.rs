//! Database queries for the social module.

use sqlx::PgPool;
use uuid::Uuid;

use super::error::SocialError;
use super::types::{Friend, Friendship};

// ============================================================================
// User lookups
// ============================================================================

/// Find a user's id by username. Returns `None` if no such user.
pub async fn find_user_id_by_username(
    pool: &PgPool,
    username: &str,
) -> Result<Option<Uuid>, SocialError> {
    let id = sqlx::query_scalar!("SELECT id FROM users WHERE username = $1", username)
        .fetch_optional(pool)
        .await?;
    Ok(id)
}

/// Check whether a user with the given id exists.
pub async fn user_exists(pool: &PgPool, user_id: Uuid) -> Result<bool, SocialError> {
    let exists = sqlx::query_scalar!("SELECT id FROM users WHERE id = $1", user_id)
        .fetch_optional(pool)
        .await?
        .is_some();
    Ok(exists)
}

/// Public profile fields used to enrich friend-event WebSocket payloads.
pub struct UserProfileSnippet {
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

/// Fetch a user's `(username, display_name, avatar_url)` for friend-event broadcasts.
pub async fn find_user_profile_snippet(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<UserProfileSnippet, SocialError> {
    let row = sqlx::query!(
        "SELECT username, display_name, avatar_url FROM users WHERE id = $1",
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(UserProfileSnippet {
        username: row.username,
        display_name: row.display_name,
        avatar_url: row.avatar_url,
    })
}

// ============================================================================
// Friendship CRUD
// ============================================================================

/// Find an existing friendship between two users in either direction.
pub async fn find_friendship_between(
    pool: &PgPool,
    user_a: Uuid,
    user_b: Uuid,
) -> Result<Option<Friendship>, SocialError> {
    let friendship = sqlx::query_as::<_, Friendship>(
        r"SELECT * FROM friendships
           WHERE (requester_id = $1 AND addressee_id = $2)
              OR (requester_id = $2 AND addressee_id = $1)",
    )
    .bind(user_a)
    .bind(user_b)
    .fetch_optional(pool)
    .await?;
    Ok(friendship)
}

/// Insert a new pending friendship from `requester_id` to `addressee_id`.
pub async fn insert_pending_friendship(
    pool: &PgPool,
    friendship_id: Uuid,
    requester_id: Uuid,
    addressee_id: Uuid,
) -> Result<Friendship, SocialError> {
    let friendship = sqlx::query_as::<_, Friendship>(
        r"INSERT INTO friendships (id, requester_id, addressee_id, status)
           VALUES ($1, $2, $3, 'pending')
           RETURNING id, requester_id, addressee_id, status, created_at, updated_at",
    )
    .bind(friendship_id)
    .bind(requester_id)
    .bind(addressee_id)
    .fetch_one(pool)
    .await?;
    Ok(friendship)
}

/// Find a friendship by id.
pub async fn find_friendship_by_id(
    pool: &PgPool,
    friendship_id: Uuid,
) -> Result<Option<Friendship>, SocialError> {
    let friendship = sqlx::query_as::<_, Friendship>(
        r"SELECT id, requester_id, addressee_id, status as status, created_at, updated_at
           FROM friendships
           WHERE id = $1",
    )
    .bind(friendship_id)
    .fetch_optional(pool)
    .await?;
    Ok(friendship)
}

/// Update a friendship's status to `accepted`.
pub async fn accept_friendship(
    pool: &PgPool,
    friendship_id: Uuid,
) -> Result<Friendship, SocialError> {
    let updated = sqlx::query_as::<_, Friendship>(
        r"UPDATE friendships
           SET status = 'accepted', updated_at = NOW()
           WHERE id = $1
           RETURNING id, requester_id, addressee_id, status, created_at, updated_at",
    )
    .bind(friendship_id)
    .fetch_one(pool)
    .await?;
    Ok(updated)
}

/// Delete a friendship by id.
pub async fn delete_friendship(
    pool: &PgPool,
    friendship_id: Uuid,
) -> Result<(), SocialError> {
    sqlx::query!("DELETE FROM friendships WHERE id = $1", friendship_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Update a friendship's status to `blocked`.
pub async fn set_friendship_blocked(
    pool: &PgPool,
    friendship_id: Uuid,
) -> Result<Friendship, SocialError> {
    let updated = sqlx::query_as::<_, Friendship>(
        r"UPDATE friendships
           SET status = 'blocked', updated_at = NOW()
           WHERE id = $1
           RETURNING id, requester_id, addressee_id, status, created_at, updated_at",
    )
    .bind(friendship_id)
    .fetch_one(pool)
    .await?;
    Ok(updated)
}

/// Insert a new blocked friendship from `requester_id` (the blocker) to
/// `addressee_id` (the blocked user).
pub async fn insert_blocked_friendship(
    pool: &PgPool,
    friendship_id: Uuid,
    requester_id: Uuid,
    addressee_id: Uuid,
) -> Result<Friendship, SocialError> {
    let friendship = sqlx::query_as::<_, Friendship>(
        r"INSERT INTO friendships (id, requester_id, addressee_id, status)
           VALUES ($1, $2, $3, 'blocked')
           RETURNING id, requester_id, addressee_id, status, created_at, updated_at",
    )
    .bind(friendship_id)
    .bind(requester_id)
    .bind(addressee_id)
    .fetch_one(pool)
    .await?;
    Ok(friendship)
}

/// Find the blocked friendship row where `blocker_id` is the requester and
/// `target_id` is the addressee.
pub async fn find_blocked_friendship(
    pool: &PgPool,
    blocker_id: Uuid,
    target_id: Uuid,
) -> Result<Option<Friendship>, SocialError> {
    let friendship = sqlx::query_as::<_, Friendship>(
        r"SELECT * FROM friendships
           WHERE requester_id = $1 AND addressee_id = $2 AND status = 'blocked'",
    )
    .bind(blocker_id)
    .bind(target_id)
    .fetch_optional(pool)
    .await?;
    Ok(friendship)
}

// ============================================================================
// Friend listings
// ============================================================================

/// List all accepted friends for `user_id`, joined with the friend's user row.
pub async fn list_accepted_friends(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<Friend>, SocialError> {
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
            f.created_at,
            NULL::text as direction
           FROM friendships f
           JOIN users u ON u.id = CASE
               WHEN f.requester_id = $1 THEN f.addressee_id
               ELSE f.requester_id
           END
           WHERE (f.requester_id = $1 OR f.addressee_id = $1)
             AND f.status = 'accepted'
           ORDER BY u.username ASC"#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(friends)
}

/// List pending friend requests for `user_id` (both incoming and outgoing).
pub async fn list_pending_friend_requests(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<Friend>, SocialError> {
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
            f.created_at,
            CASE
                WHEN f.requester_id = $1 THEN 'outgoing'
                ELSE 'incoming'
            END as direction
           FROM friendships f
           JOIN users u ON u.id = CASE
               WHEN f.requester_id = $1 THEN f.addressee_id
               ELSE f.requester_id
           END
           WHERE (f.requester_id = $1 OR f.addressee_id = $1)
             AND f.status = 'pending'
           ORDER BY f.created_at DESC"#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(pending)
}

/// List users blocked by `user_id` (where the user is the requester of a
/// `blocked` friendship row).
pub async fn list_blocked_users(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<Friend>, SocialError> {
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
            f.created_at,
            NULL::text as direction
           FROM friendships f
           JOIN users u ON u.id = f.addressee_id
           WHERE f.requester_id = $1
             AND f.status = 'blocked'
           ORDER BY u.username ASC"#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(blocked)
}

// ============================================================================
// Block cache lookups (id-only, no joins)
// ============================================================================

/// List `addressee_id`s for all `blocked` friendships where `user_id` is the
/// requester. Used by the Redis block cache to populate the per-user blocked
/// set.
pub async fn list_blocked_addressee_ids(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT addressee_id FROM friendships WHERE requester_id = $1 AND status = 'blocked'",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// List `requester_id`s for all `blocked` friendships where `user_id` is the
/// addressee. Used by the Redis block cache to populate the per-user
/// `blocked_by` set.
pub async fn list_requester_ids_blocking(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT requester_id FROM friendships WHERE addressee_id = $1 AND status = 'blocked'",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}
