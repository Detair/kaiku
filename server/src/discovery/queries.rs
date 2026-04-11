//! Database queries for the discovery module.

use sqlx::{PgConnection, PgPool, Postgres, QueryBuilder};
use uuid::Uuid;

use super::error::DiscoveryError;
use super::types::{DiscoverSort, DiscoverableGuild};

/// A row returned by the discovery browse query, including a window-function total count.
pub struct DiscoverableGuildRow {
    pub guild: DiscoverableGuild,
    pub total_count: i64,
}

/// Browse discoverable guilds with optional full-text search and tag filter.
///
/// `q` and `tags` are pre-validated by the caller. The query uses the denormalized
/// `member_count` column on `guilds` (maintained by trigger) to avoid scanning
/// `guild_members`. `COUNT(*) OVER()` provides the total count atomically with
/// the page data when the page is non-empty.
pub async fn browse_discoverable_guilds(
    pool: &PgPool,
    q: Option<&str>,
    tags: Option<&[String]>,
    sort: &DiscoverSort,
    limit: i64,
    offset: i64,
) -> Result<Vec<DiscoverableGuildRow>, DiscoveryError> {
    let has_search = q.is_some_and(|q| !q.trim().is_empty());
    let has_tags = tags.is_some_and(|t| !t.is_empty());

    let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(
        r"SELECT g.id, g.name, g.icon_url, g.banner_url, g.description, g.tags, g.created_at,
                 g.member_count::bigint,
                 COUNT(*) OVER() as total_count
          FROM guilds g
          WHERE g.discoverable = true AND g.suspended_at IS NULL",
    );

    if has_search {
        builder.push(" AND g.search_vector @@ websearch_to_tsquery('english', ");
        builder.push_bind(q.unwrap_or_default().trim().to_string());
        builder.push(")");
    }

    if has_tags {
        let lower_tags: Vec<String> = tags.unwrap().iter().map(|t| t.to_lowercase()).collect();
        builder.push(" AND g.tags && ");
        builder.push_bind(lower_tags);
    }

    match sort {
        DiscoverSort::Members => {
            builder.push(" ORDER BY g.member_count DESC, g.created_at DESC");
        }
        DiscoverSort::Newest => {
            builder.push(" ORDER BY g.created_at DESC");
        }
    }

    builder.push(" LIMIT ");
    builder.push_bind(limit);
    builder.push(" OFFSET ");
    builder.push_bind(offset);

    let rows: Vec<(
        Uuid,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Vec<String>,
        chrono::DateTime<chrono::Utc>,
        i64,
        i64,
    )> = builder.build_query_as().fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                name,
                icon_url,
                banner_url,
                description,
                tags,
                created_at,
                member_count,
                total_count,
            )| {
                DiscoverableGuildRow {
                    guild: DiscoverableGuild {
                        id,
                        name,
                        icon_url,
                        banner_url,
                        description,
                        tags,
                        member_count,
                        created_at,
                    },
                    total_count,
                }
            },
        )
        .collect())
}

/// Count the discoverable guilds matching a search/tag filter.
///
/// Used as a fallback when the page is past the end of the result set so that
/// `COUNT(*) OVER()` can no longer report a total.
pub async fn count_discoverable_guilds(
    pool: &PgPool,
    q: Option<&str>,
    tags: Option<&[String]>,
) -> Result<i64, DiscoveryError> {
    let has_search = q.is_some_and(|q| !q.trim().is_empty());
    let has_tags = tags.is_some_and(|t| !t.is_empty());

    let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(
        "SELECT COUNT(*) FROM guilds g WHERE g.discoverable = true AND g.suspended_at IS NULL",
    );
    if has_search {
        builder.push(" AND g.search_vector @@ websearch_to_tsquery('english', ");
        builder.push_bind(q.unwrap_or_default().trim().to_string());
        builder.push(")");
    }
    if has_tags {
        let lower_tags: Vec<String> = tags.unwrap().iter().map(|t| t.to_lowercase()).collect();
        builder.push(" AND g.tags && ");
        builder.push_bind(lower_tags);
    }
    let (count,): (i64,) = builder.build_query_as().fetch_one(pool).await?;
    Ok(count)
}

/// Fetch a discoverable, non-suspended guild's name. Returns `None` if no such guild.
pub async fn find_discoverable_guild_name(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Option<String>, DiscoveryError> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM guilds WHERE id = $1 AND discoverable = true AND suspended_at IS NULL",
    )
    .bind(guild_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(name,)| name))
}

/// Check whether a user has an active global ban.
pub async fn is_user_globally_banned(pool: &PgPool, user_id: Uuid) -> Result<bool, DiscoveryError> {
    let banned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM global_bans WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > NOW()))",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(banned)
}

/// Acquire a transaction-scoped advisory lock keyed on a guild ID.
///
/// Used to serialize concurrent member-join attempts so the per-guild member
/// limit check is strict under concurrency.
pub async fn acquire_guild_join_lock(
    conn: &mut PgConnection,
    guild_id: Uuid,
) -> Result<(), DiscoveryError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 53))")
        .bind(guild_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Check whether a user has an active guild-specific ban.
pub async fn is_user_guild_banned(
    conn: &mut PgConnection,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, DiscoveryError> {
    let banned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM guild_bans WHERE guild_id = $1 AND user_id = $2 AND (expires_at IS NULL OR expires_at > NOW()))",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(conn)
    .await?;
    Ok(banned)
}

/// Count members in a guild within a transaction.
pub async fn count_guild_members(
    conn: &mut PgConnection,
    guild_id: Uuid,
) -> Result<i64, DiscoveryError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM guild_members WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_one(conn)
        .await?;
    Ok(count)
}

/// Insert a guild member, ignoring conflicts. Returns the number of rows affected
/// (0 if the user was already a member, 1 on a fresh insert).
pub async fn insert_guild_member_ignore_conflict(
    conn: &mut PgConnection,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<u64, DiscoveryError> {
    let result = sqlx::query(
        "INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(guild_id)
    .bind(user_id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected())
}

/// Look up a user's `(username, display_name)` pair for broadcast purposes.
pub async fn find_user_name_pair(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<(String, String)>, DiscoveryError> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT username, display_name FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}
