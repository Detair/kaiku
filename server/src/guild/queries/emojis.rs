//! Guild custom emoji queries.
//!
//! Functions return `Result<T, EmojiError>` so handlers can use `?` directly.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::super::emojis::EmojiError;
use super::super::types::GuildEmoji;

/// Check whether a user is a member of a guild.
pub async fn is_guild_member(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2)",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(result.0)
}

/// List all emojis in a guild ordered newest first.
pub async fn list_guild_emojis(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Vec<GuildEmoji>, EmojiError> {
    let emojis = sqlx::query_as::<_, GuildEmoji>(
        r"
        SELECT * FROM guild_emojis
        WHERE guild_id = $1
        ORDER BY created_at DESC
        ",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await?;
    Ok(emojis)
}

/// Fetch a single emoji by `(guild_id, emoji_id)`.
pub async fn fetch_guild_emoji(
    pool: &PgPool,
    guild_id: Uuid,
    emoji_id: Uuid,
) -> Result<GuildEmoji, EmojiError> {
    sqlx::query_as::<_, GuildEmoji>(
        r"
        SELECT * FROM guild_emojis
        WHERE id = $1 AND guild_id = $2
        ",
    )
    .bind(emoji_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await?
    .ok_or(EmojiError::EmojiNotFound)
}

/// Count emojis in a guild on a `&PgPool` (fast-path before any uploads).
pub async fn count_guild_emojis(pool: &PgPool, guild_id: Uuid) -> Result<i64, EmojiError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM guild_emojis WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Acquire the per-guild advisory lock used to serialize emoji creation
/// (seed 59).
pub async fn lock_emoji_create(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
) -> Result<(), EmojiError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 59))")
        .bind(guild_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Count emojis in a guild inside a transaction (used under the emoji-create
/// advisory lock).
pub async fn count_guild_emojis_tx(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
) -> Result<i64, EmojiError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM guild_emojis WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(count)
}

/// Insert a new emoji row inside a transaction.
pub async fn insert_guild_emoji(
    tx: &mut Transaction<'_, Postgres>,
    emoji_id: Uuid,
    guild_id: Uuid,
    name: &str,
    image_url: &str,
    animated: bool,
    uploaded_by: Uuid,
) -> Result<GuildEmoji, EmojiError> {
    let emoji = sqlx::query_as::<_, GuildEmoji>(
        r"INSERT INTO guild_emojis (id, guild_id, name, image_url, animated, uploaded_by)
          VALUES ($1, $2, $3, $4, $5, $6)
          RETURNING *",
    )
    .bind(emoji_id)
    .bind(guild_id)
    .bind(name)
    .bind(image_url)
    .bind(animated)
    .bind(uploaded_by)
    .fetch_one(&mut **tx)
    .await?;
    Ok(emoji)
}

/// Compensation delete after a failed S3 upload.
pub async fn delete_guild_emoji_compensation(
    pool: &PgPool,
    emoji_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM guild_emojis WHERE id = $1")
        .bind(emoji_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Update an emoji's name.
pub async fn update_guild_emoji(
    pool: &PgPool,
    name: &str,
    emoji_id: Uuid,
    guild_id: Uuid,
) -> Result<GuildEmoji, EmojiError> {
    let updated = sqlx::query_as::<_, GuildEmoji>(
        r"
        UPDATE guild_emojis
        SET name = $1
        WHERE id = $2 AND guild_id = $3
        RETURNING *
        ",
    )
    .bind(name)
    .bind(emoji_id)
    .bind(guild_id)
    .fetch_one(pool)
    .await?;
    Ok(updated)
}

/// Delete an emoji row.
pub async fn delete_guild_emoji(pool: &PgPool, emoji_id: Uuid) -> Result<(), EmojiError> {
    sqlx::query("DELETE FROM guild_emojis WHERE id = $1")
        .bind(emoji_id)
        .execute(pool)
        .await?;
    Ok(())
}
