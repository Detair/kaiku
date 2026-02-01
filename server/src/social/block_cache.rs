//! Block Cache
//!
//! Redis SET-based cache for user blocking relationships.
//! Each user has a Redis SET `blocks:{user_id}` containing UUIDs they've blocked.

use std::collections::HashSet;

use fred::prelude::*;
use sqlx::PgPool;
use uuid::Uuid;

/// Redis key for a user's blocked list.
fn blocked_key(user_id: Uuid) -> String {
    format!("blocks:{user_id}")
}

/// Load the set of users blocked by `user_id` from DB into Redis and return it.
pub async fn load_blocked_users(
    db: &PgPool,
    redis: &Client,
    user_id: Uuid,
) -> Result<HashSet<Uuid>, anyhow::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT addressee_id FROM friendships WHERE requester_id = $1 AND status = 'blocked'",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    let key = blocked_key(user_id);
    // Clear existing set and repopulate
    let _: () = redis.del(&key).await?;

    let ids: HashSet<Uuid> = rows.into_iter().map(|(id,)| id).collect();

    if !ids.is_empty() {
        let members: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let _: () = redis.sadd(&key, members).await?;
        // Expire after 1 hour (will be refreshed on next WS connect)
        let _: () = redis.expire(&key, 3600, None).await?;
    }

    Ok(ids)
}

/// Load the set of users who have blocked `user_id` from DB into Redis and return it.
pub async fn load_blocked_by(
    db: &PgPool,
    redis: &Client,
    user_id: Uuid,
) -> Result<HashSet<Uuid>, anyhow::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT requester_id FROM friendships WHERE addressee_id = $1 AND status = 'blocked'",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    let key = format!("blocked_by:{user_id}");
    let _: () = redis.del(&key).await?;

    let ids: HashSet<Uuid> = rows.into_iter().map(|(id,)| id).collect();

    if !ids.is_empty() {
        let members: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let _: () = redis.sadd(&key, members).await?;
        let _: () = redis.expire(&key, 3600, None).await?;
    }

    Ok(ids)
}

/// Check if either user has blocked the other (using Redis SETs).
pub async fn is_blocked_either_direction(
    redis: &Client,
    user_a: Uuid,
    user_b: Uuid,
) -> Result<bool, anyhow::Error> {
    let a_blocked_b: bool = redis
        .sismember(blocked_key(user_a), user_b.to_string())
        .await?;
    if a_blocked_b {
        return Ok(true);
    }

    let b_blocked_a: bool = redis
        .sismember(blocked_key(user_b), user_a.to_string())
        .await?;
    Ok(b_blocked_a)
}

/// Add a block relationship to cache.
pub async fn add_block(redis: &Client, blocker: Uuid, target: Uuid) -> Result<(), anyhow::Error> {
    let _: () = redis
        .sadd(blocked_key(blocker), target.to_string())
        .await?;
    let _: () = redis
        .sadd(format!("blocked_by:{target}"), blocker.to_string())
        .await?;
    Ok(())
}

/// Remove a block relationship from cache.
pub async fn remove_block(
    redis: &Client,
    blocker: Uuid,
    target: Uuid,
) -> Result<(), anyhow::Error> {
    let _: () = redis
        .srem(blocked_key(blocker), target.to_string())
        .await?;
    let _: () = redis
        .srem(format!("blocked_by:{target}"), blocker.to_string())
        .await?;
    Ok(())
}
