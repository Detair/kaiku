//! Guild resource limit queries.
//!
//! Reusable count queries used by enforcement checks and the usage stats endpoint.
//! Functions take `&PgPool` and return `Result<i64, sqlx::Error>` so callers can
//! map the error into module-specific variants.

use sqlx::PgPool;
use uuid::Uuid;

/// Count guilds owned by a user.
pub async fn count_user_owned_guilds(pool: &PgPool, user_id: Uuid) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM guilds WHERE owner_id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Get the member count for a guild (uses denormalized `member_count` column).
pub async fn get_member_count(pool: &PgPool, guild_id: Uuid) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT member_count::bigint FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Count channels in a guild.
pub async fn count_guild_channels(pool: &PgPool, guild_id: Uuid) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM channels WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Count roles in a guild (includes `@everyone`).
pub async fn count_guild_roles(pool: &PgPool, guild_id: Uuid) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM guild_roles WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Count custom emojis in a guild.
pub async fn count_guild_emojis(pool: &PgPool, guild_id: Uuid) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM guild_emojis WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Count bot installations in a guild.
pub async fn count_guild_bots(pool: &PgPool, guild_id: Uuid) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM guild_bot_installations WHERE guild_id = $1")
            .bind(guild_id)
            .fetch_one(pool)
            .await?;
    Ok(count)
}
