//! Database queries for the workspaces module.

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::error::WorkspaceError;
use super::types::{WorkspaceEntryRow, WorkspaceListRow, WorkspaceRow};

// ============================================================================
// Workspace CRUD
// ============================================================================

/// Acquire a per-user advisory lock to serialize workspace creation.
///
/// Seed 41 prevents collisions with other lock sites — see the seed registry
/// in `server/src/db/mod.rs`.
pub async fn acquire_user_workspace_lock(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<(), WorkspaceError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 41))")
        .bind(user_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Atomically insert a new workspace iff the user is below `max_workspaces`.
///
/// Returns `None` if the limit would be exceeded so the caller can map it to
/// `WorkspaceLimitExceeded`. Must be called inside a transaction holding the
/// per-user advisory lock.
pub async fn insert_workspace_if_under_limit(
    conn: &mut PgConnection,
    owner_user_id: Uuid,
    name: &str,
    icon: Option<&str>,
    max_workspaces: i64,
) -> Result<Option<WorkspaceRow>, WorkspaceError> {
    let row = sqlx::query_as::<_, WorkspaceRow>(
        r"
        INSERT INTO workspaces (owner_user_id, name, icon, sort_order)
        SELECT $1, $2, $3, COALESCE((SELECT MAX(sort_order) + 1 FROM workspaces WHERE owner_user_id = $1), 0)
        WHERE (SELECT COUNT(*) FROM workspaces WHERE owner_user_id = $1) < $4
        RETURNING id, owner_user_id, name, icon, sort_order, created_at, updated_at
        ",
    )
    .bind(owner_user_id)
    .bind(name)
    .bind(icon)
    .bind(max_workspaces)
    .fetch_optional(conn)
    .await?;
    Ok(row)
}

/// List a user's workspaces with the count of entries in each.
pub async fn list_user_workspaces(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<WorkspaceListRow>, WorkspaceError> {
    let rows = sqlx::query_as::<_, WorkspaceListRow>(
        r"
        SELECT w.id, w.name, w.icon, w.sort_order,
               COUNT(we.id) AS entry_count,
               w.created_at, w.updated_at
        FROM workspaces w
        LEFT JOIN workspace_entries we ON we.workspace_id = w.id
        WHERE w.owner_user_id = $1
        GROUP BY w.id
        ORDER BY w.sort_order, w.created_at
        ",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Find a workspace owned by `user_id`. Returns `None` if it does not exist
/// or is owned by someone else.
pub async fn find_workspace_for_owner(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<Option<WorkspaceRow>, WorkspaceError> {
    let row = sqlx::query_as::<_, WorkspaceRow>(
        "SELECT id, owner_user_id, name, icon, sort_order, created_at, updated_at FROM workspaces WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// List the entries of a workspace, joined with guild and channel metadata.
pub async fn list_workspace_entries(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<WorkspaceEntryRow>, WorkspaceError> {
    let rows = sqlx::query_as::<_, WorkspaceEntryRow>(
        r"
        SELECT we.id, we.workspace_id, we.guild_id, we.channel_id, we.position,
               g.name AS guild_name, g.icon_url AS guild_icon,
               c.name AS channel_name, c.channel_type AS channel_type,
               we.created_at
        FROM workspace_entries we
        JOIN guilds g ON g.id = we.guild_id
        JOIN channels c ON c.id = we.channel_id
        WHERE we.workspace_id = $1
        ORDER BY we.position, we.created_at
        ",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Update name and/or icon on a workspace owned by `user_id`. The icon update
/// is conditional on `should_update_icon` so callers can distinguish "leave
/// unchanged" from "clear to NULL".
pub async fn update_workspace_for_owner(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    name: Option<&str>,
    should_update_icon: bool,
    new_icon: Option<&str>,
) -> Result<Option<WorkspaceRow>, WorkspaceError> {
    let row = sqlx::query_as::<_, WorkspaceRow>(
        r"
        UPDATE workspaces
        SET name = COALESCE($3, name),
            icon = CASE WHEN $4 THEN $5 ELSE icon END
        WHERE id = $1 AND owner_user_id = $2
        RETURNING id, owner_user_id, name, icon, sort_order, created_at, updated_at
        ",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(name)
    .bind(should_update_icon)
    .bind(new_icon)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Delete a workspace owned by `user_id`. Returns `true` if a row was deleted.
pub async fn delete_workspace_for_owner(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<bool, WorkspaceError> {
    let result = sqlx::query("DELETE FROM workspaces WHERE id = $1 AND owner_user_id = $2")
        .bind(workspace_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ============================================================================
// Entry management
// ============================================================================

/// Check whether a workspace is owned by `user_id`.
pub async fn workspace_owned_by(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<bool, WorkspaceError> {
    let exists = sqlx::query("SELECT 1 FROM workspaces WHERE id = $1 AND owner_user_id = $2")
        .bind(workspace_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .is_some();
    Ok(exists)
}

/// Look up the `guild_id` for a channel, returning `None` if the channel does
/// not exist. The outer `Option` is the row presence; the inner option is
/// `NULL` for DM channels that have no guild.
pub async fn find_channel_guild_id(
    pool: &PgPool,
    channel_id: Uuid,
) -> Result<Option<Option<Uuid>>, WorkspaceError> {
    let row: Option<(Option<Uuid>,)> = sqlx::query_as("SELECT guild_id FROM channels WHERE id = $1")
        .bind(channel_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(gid,)| gid))
}

/// Check whether a user is a member of a guild.
pub async fn is_guild_member(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, WorkspaceError> {
    let exists = sqlx::query("SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .is_some();
    Ok(exists)
}

/// Acquire a per-workspace advisory lock to serialize entry creation.
///
/// Seed 43 prevents collisions with other lock sites — see the seed registry
/// in `server/src/db/mod.rs`.
pub async fn acquire_workspace_entry_lock(
    conn: &mut PgConnection,
    workspace_id: Uuid,
) -> Result<(), WorkspaceError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 43))")
        .bind(workspace_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Outcome of attempting to insert a workspace entry under the per-workspace
/// limit and unique constraint.
pub enum InsertEntryOutcome {
    /// Successfully inserted; returns the new entry id.
    Inserted(Uuid),
    /// The workspace is at the maximum entry count.
    LimitExceeded,
    /// The channel is already part of this workspace.
    DuplicateEntry,
}

/// Atomically insert a workspace entry, enforcing the per-workspace cap and
/// uniqueness constraint. Must be called inside a transaction holding the
/// workspace entry advisory lock.
pub async fn insert_workspace_entry_if_under_limit(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    guild_id: Uuid,
    channel_id: Uuid,
    max_entries: i64,
) -> Result<InsertEntryOutcome, WorkspaceError> {
    let result = sqlx::query_as::<_, (Uuid,)>(
        r"
        INSERT INTO workspace_entries (workspace_id, guild_id, channel_id, position)
        SELECT $1, $2, $3, COALESCE((SELECT MAX(position) + 1 FROM workspace_entries WHERE workspace_id = $1), 0)
        WHERE (SELECT COUNT(*) FROM workspace_entries WHERE workspace_id = $1) < $4
        RETURNING id
        ",
    )
    .bind(workspace_id)
    .bind(guild_id)
    .bind(channel_id)
    .bind(max_entries)
    .fetch_optional(conn)
    .await;

    match result {
        Ok(Some((id,))) => Ok(InsertEntryOutcome::Inserted(id)),
        Ok(None) => Ok(InsertEntryOutcome::LimitExceeded),
        Err(sqlx::Error::Database(ref db_err)) if db_err.is_unique_violation() => {
            Ok(InsertEntryOutcome::DuplicateEntry)
        }
        Err(e) => Err(WorkspaceError::Database(e)),
    }
}

/// Fetch a single workspace entry joined with guild and channel metadata.
pub async fn find_workspace_entry_with_metadata(
    pool: &PgPool,
    entry_id: Uuid,
) -> Result<WorkspaceEntryRow, WorkspaceError> {
    let row = sqlx::query_as::<_, WorkspaceEntryRow>(
        r"
        SELECT we.id, we.workspace_id, we.guild_id, we.channel_id, we.position,
               g.name AS guild_name, g.icon_url AS guild_icon,
               c.name AS channel_name, c.channel_type AS channel_type,
               we.created_at
        FROM workspace_entries we
        JOIN guilds g ON g.id = we.guild_id
        JOIN channels c ON c.id = we.channel_id
        WHERE we.id = $1
        ",
    )
    .bind(entry_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Delete a workspace entry, but only if the workspace is owned by `user_id`.
/// Returns `true` if a row was actually deleted.
pub async fn delete_workspace_entry_for_owner(
    pool: &PgPool,
    workspace_id: Uuid,
    entry_id: Uuid,
    user_id: Uuid,
) -> Result<bool, WorkspaceError> {
    let result = sqlx::query(
        r"
        DELETE FROM workspace_entries we
        USING workspaces w
        WHERE we.id = $1
          AND we.workspace_id = $2
          AND w.id = we.workspace_id
          AND w.owner_user_id = $3
        ",
    )
    .bind(entry_id)
    .bind(workspace_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

// ============================================================================
// Reordering
// ============================================================================

/// Count how many of `entry_ids` belong to `workspace_id`, and the total
/// number of entries the workspace contains. Used to validate that a reorder
/// request covers exactly the workspace's entries.
pub async fn count_workspace_entries_for_reorder(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    entry_ids: &[Uuid],
) -> Result<(i64, i64), WorkspaceError> {
    let row: (i64, i64) = sqlx::query_as(
        r"
        SELECT
            COUNT(*) FILTER (WHERE id = ANY($2)),
            COUNT(*)
        FROM workspace_entries WHERE workspace_id = $1
        ",
    )
    .bind(workspace_id)
    .bind(entry_ids)
    .fetch_one(conn)
    .await?;
    Ok(row)
}

/// Update the position of a single workspace entry.
pub async fn update_workspace_entry_position(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    entry_id: Uuid,
    position: i32,
) -> Result<(), WorkspaceError> {
    sqlx::query("UPDATE workspace_entries SET position = $1 WHERE id = $2 AND workspace_id = $3")
        .bind(position)
        .bind(entry_id)
        .bind(workspace_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Count how many of `workspace_ids` belong to `user_id`, and the total
/// number of workspaces the user owns. Used to validate that a reorder
/// request covers exactly the user's workspaces.
pub async fn count_user_workspaces_for_reorder(
    conn: &mut PgConnection,
    user_id: Uuid,
    workspace_ids: &[Uuid],
) -> Result<(i64, i64), WorkspaceError> {
    let row: (i64, i64) = sqlx::query_as(
        r"
        SELECT
            COUNT(*) FILTER (WHERE id = ANY($2)),
            COUNT(*)
        FROM workspaces WHERE owner_user_id = $1
        ",
    )
    .bind(user_id)
    .bind(workspace_ids)
    .fetch_one(conn)
    .await?;
    Ok(row)
}

/// Update the `sort_order` of a single workspace owned by `user_id`.
pub async fn update_workspace_sort_order(
    conn: &mut PgConnection,
    workspace_id: Uuid,
    user_id: Uuid,
    sort_order: i32,
) -> Result<(), WorkspaceError> {
    sqlx::query("UPDATE workspaces SET sort_order = $1 WHERE id = $2 AND owner_user_id = $3")
        .bind(sort_order)
        .bind(workspace_id)
        .bind(user_id)
        .execute(conn)
        .await?;
    Ok(())
}
