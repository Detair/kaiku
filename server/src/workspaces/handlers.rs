//! Workspace HTTP Handlers
//!
//! 9 endpoints for personal workspace CRUD, entry management, and reordering.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;
use validator::Validate;

use super::error::WorkspaceError;
use super::types::{
    AddEntryRequest, CreateWorkspaceRequest, ReorderEntriesRequest, ReorderWorkspacesRequest,
    UpdateWorkspaceRequest, WorkspaceDetailResponse, WorkspaceEntryResponse, WorkspaceEntryRow,
    WorkspaceListItem, WorkspaceListRow, WorkspaceResponse, WorkspaceRow,
    MAX_WORKSPACE_ICON_LENGTH,
};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::ws::{broadcast_to_user, ServerEvent};

// ============================================================================
// Workspace CRUD
// ============================================================================

/// Create a new workspace.
///
/// POST /api/me/workspaces
#[utoipa::path(
    post,
    path = "/api/me/workspaces",
    tag = "workspaces",
    request_body = CreateWorkspaceRequest,
    responses(
        (status = 201, body = WorkspaceResponse),
        (status = 400, description = "Invalid name"),
        (status = 403, description = "Workspace limit exceeded"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn create_workspace(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<CreateWorkspaceRequest>,
) -> Result<(StatusCode, Json<WorkspaceResponse>), WorkspaceError> {
    let request = CreateWorkspaceRequest {
        name: request.name.trim().to_string(),
        icon: request.icon,
    };
    request
        .validate()
        .map_err(|e| WorkspaceError::Validation(e.to_string()))?;

    if let Some(icon) = request.icon.as_ref() {
        if icon.chars().count() > MAX_WORKSPACE_ICON_LENGTH {
            return Err(WorkspaceError::Validation(format!(
                "Icon must be at most {MAX_WORKSPACE_ICON_LENGTH} characters"
            )));
        }
    }

    let mut tx = state.db.begin().await?;

    // Advisory lock: serialize workspace creation per user to enforce strict limits under
    // concurrency. Seed 41 prevents collision with other lock sites (see seed registry in
    // server/src/db/mod.rs).
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 41))")
        .bind(auth_user.id)
        .execute(&mut *tx)
        .await?;

    // Atomic count check + insert inside lock.
    let row = sqlx::query_as::<_, WorkspaceRow>(
        r"
        INSERT INTO workspaces (owner_user_id, name, icon, sort_order)
        SELECT $1, $2, $3, COALESCE((SELECT MAX(sort_order) + 1 FROM workspaces WHERE owner_user_id = $1), 0)
        WHERE (SELECT COUNT(*) FROM workspaces WHERE owner_user_id = $1) < $4
        RETURNING id, owner_user_id, name, icon, sort_order, created_at, updated_at
        ",
    )
    .bind(auth_user.id)
    .bind(&request.name)
    .bind(&request.icon)
    .bind(state.config.max_workspaces_per_user)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(WorkspaceError::WorkspaceLimitExceeded)?;

    tx.commit().await?;

    let response = WorkspaceResponse::from(row);

    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth_user.id,
        &ServerEvent::WorkspaceCreated {
            workspace: serde_json::to_value(&response).unwrap_or_default(),
        },
    )
    .await
    {
        tracing::warn!("Failed to broadcast WorkspaceCreated event: {}", e);
    }

    Ok((StatusCode::CREATED, Json(response)))
}

/// List user's workspaces with entry counts.
///
/// GET /api/me/workspaces
#[utoipa::path(
    get,
    path = "/api/me/workspaces",
    tag = "workspaces",
    responses(
        (status = 200, body = Vec<WorkspaceListItem>),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn list_workspaces(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<Vec<WorkspaceListItem>>, WorkspaceError> {
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
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter().map(WorkspaceListItem::from).collect(),
    ))
}

/// Get workspace with all entries (including guild/channel metadata).
///
/// GET /api/me/workspaces/{id}
#[utoipa::path(
    get,
    path = "/api/me/workspaces/{id}",
    tag = "workspaces",
    params(("id" = Uuid, Path, description = "Workspace ID")),
    responses(
        (status = 200, body = WorkspaceDetailResponse),
        (status = 404, description = "Workspace not found"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn get_workspace(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<WorkspaceDetailResponse>, WorkspaceError> {
    let workspace = sqlx::query_as::<_, WorkspaceRow>(
        "SELECT id, owner_user_id, name, icon, sort_order, created_at, updated_at FROM workspaces WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(workspace_id)
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(WorkspaceError::NotFound)?;

    let entries = sqlx::query_as::<_, WorkspaceEntryRow>(
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
    .fetch_all(&state.db)
    .await?;

    Ok(Json(WorkspaceDetailResponse {
        workspace: WorkspaceResponse::from(workspace),
        entries: entries
            .into_iter()
            .map(WorkspaceEntryResponse::from)
            .collect(),
    }))
}

/// Update workspace name and/or icon.
///
/// PATCH /api/me/workspaces/{id}
#[utoipa::path(
    patch,
    path = "/api/me/workspaces/{id}",
    tag = "workspaces",
    params(("id" = Uuid, Path, description = "Workspace ID")),
    request_body = UpdateWorkspaceRequest,
    responses(
        (status = 200, body = WorkspaceResponse),
        (status = 400, description = "Invalid name"),
        (status = 404, description = "Workspace not found"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn update_workspace(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<UpdateWorkspaceRequest>,
) -> Result<Json<WorkspaceResponse>, WorkspaceError> {
    // Trim name first (whitespace-only → empty → rejected by validator's min=1)
    let request = UpdateWorkspaceRequest {
        name: request.name.as_deref().map(str::trim).map(String::from),
        icon: request.icon,
    };
    request
        .validate()
        .map_err(|e| WorkspaceError::Validation(e.to_string()))?;

    if let Some(Some(icon)) = request.icon.as_ref() {
        if icon.chars().count() > MAX_WORKSPACE_ICON_LENGTH {
            return Err(WorkspaceError::Validation(format!(
                "Icon must be at most {MAX_WORKSPACE_ICON_LENGTH} characters"
            )));
        }
    }

    // icon: None = no change, Some(None) = clear, Some(Some(val)) = set
    let should_update_icon = request.icon.is_some();
    let new_icon_value: Option<&str> = request.icon.as_ref().and_then(|v| v.as_deref());

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
    .bind(auth_user.id)
    .bind(&request.name)
    .bind(should_update_icon)
    .bind(new_icon_value)
    .fetch_optional(&state.db)
    .await?
    .ok_or(WorkspaceError::NotFound)?;

    let response = WorkspaceResponse::from(row);

    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth_user.id,
        &ServerEvent::WorkspaceUpdated {
            workspace: serde_json::to_value(&response).unwrap_or_default(),
        },
    )
    .await
    {
        tracing::warn!("Failed to broadcast WorkspaceUpdated event: {}", e);
    }

    Ok(Json(response))
}

/// Delete a workspace (entries cascade).
///
/// DELETE /api/me/workspaces/{id}
#[utoipa::path(
    delete,
    path = "/api/me/workspaces/{id}",
    tag = "workspaces",
    params(("id" = Uuid, Path, description = "Workspace ID")),
    responses(
        (status = 204, description = "Workspace deleted"),
        (status = 404, description = "Workspace not found"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn delete_workspace(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> Result<StatusCode, WorkspaceError> {
    let result = sqlx::query("DELETE FROM workspaces WHERE id = $1 AND owner_user_id = $2")
        .bind(workspace_id)
        .bind(auth_user.id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(WorkspaceError::NotFound);
    }

    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth_user.id,
        &ServerEvent::WorkspaceDeleted { workspace_id },
    )
    .await
    {
        tracing::warn!("Failed to broadcast WorkspaceDeleted event: {}", e);
    }

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Entry Management
// ============================================================================

/// Add a channel to a workspace.
///
/// POST /api/me/workspaces/{id}/entries
#[utoipa::path(
    post,
    path = "/api/me/workspaces/{id}/entries",
    tag = "workspaces",
    params(("id" = Uuid, Path, description = "Workspace ID")),
    request_body = AddEntryRequest,
    responses(
        (status = 201, body = WorkspaceEntryResponse),
        (status = 403, description = "Entry limit exceeded"),
        (status = 404, description = "Workspace or channel not found"),
        (status = 409, description = "Channel already in workspace"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn add_entry(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<AddEntryRequest>,
) -> Result<(StatusCode, Json<WorkspaceEntryResponse>), WorkspaceError> {
    // 1. Verify workspace ownership
    let workspace_exists =
        sqlx::query("SELECT 1 FROM workspaces WHERE id = $1 AND owner_user_id = $2")
            .bind(workspace_id)
            .bind(auth_user.id)
            .fetch_optional(&state.db)
            .await?
            .is_some();

    if !workspace_exists {
        return Err(WorkspaceError::NotFound);
    }

    // 2. Verify channel belongs to the claimed guild
    let channel_guild: Option<(Option<Uuid>,)> =
        sqlx::query_as("SELECT guild_id FROM channels WHERE id = $1")
            .bind(request.channel_id)
            .fetch_optional(&state.db)
            .await?;

    match channel_guild {
        Some((Some(gid),)) if gid == request.guild_id => {}
        _ => return Err(WorkspaceError::ChannelNotFound),
    }

    // 3. Verify guild membership
    let is_member = sqlx::query("SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(request.guild_id)
        .bind(auth_user.id)
        .fetch_optional(&state.db)
        .await?
        .is_some();

    if !is_member {
        return Err(WorkspaceError::ChannelNotFound);
    }

    // 4. Verify channel access (VIEW_CHANNEL permission)
    crate::permissions::require_channel_access(&state.db, auth_user.id, request.channel_id)
        .await
        .map_err(|_| WorkspaceError::ChannelNotFound)?;

    let mut tx = state.db.begin().await?;

    // Advisory lock: serialize entry creation per workspace to enforce strict limits under
    // concurrency. Seed 43 prevents collision with other lock sites (see seed registry in
    // server/src/db/mod.rs).
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 43))")
        .bind(workspace_id)
        .execute(&mut *tx)
        .await?;

    // 5. Atomic insert with entry limit check inside lock.
    let result = sqlx::query_as::<_, (Uuid,)>(
        r"
        INSERT INTO workspace_entries (workspace_id, guild_id, channel_id, position)
        SELECT $1, $2, $3, COALESCE((SELECT MAX(position) + 1 FROM workspace_entries WHERE workspace_id = $1), 0)
        WHERE (SELECT COUNT(*) FROM workspace_entries WHERE workspace_id = $1) < $4
        RETURNING id
        ",
    )
    .bind(workspace_id)
    .bind(request.guild_id)
    .bind(request.channel_id)
    .bind(state.config.max_entries_per_workspace)
    .fetch_optional(&mut *tx)
    .await;

    let entry_id = match result {
        Ok(Some((id,))) => {
            tx.commit().await?;
            id
        }
        Ok(None) => return Err(WorkspaceError::EntryLimitExceeded),
        Err(sqlx::Error::Database(ref db_err)) if db_err.is_unique_violation() => {
            return Err(WorkspaceError::DuplicateEntry);
        }
        Err(e) => return Err(WorkspaceError::Database(e)),
    };

    // 6. Fetch guild/channel names for the response
    let entry_row = sqlx::query_as::<_, WorkspaceEntryRow>(
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
    .fetch_one(&state.db)
    .await?;

    let response = WorkspaceEntryResponse::from(entry_row);

    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth_user.id,
        &ServerEvent::WorkspaceEntryAdded {
            workspace_id,
            entry: serde_json::to_value(&response).unwrap_or_default(),
        },
    )
    .await
    {
        tracing::warn!("Failed to broadcast WorkspaceEntryAdded event: {}", e);
    }

    Ok((StatusCode::CREATED, Json(response)))
}

/// Remove an entry from a workspace.
///
/// `DELETE /api/me/workspaces/{id}/entries/{entry_id}`
#[utoipa::path(
    delete,
    path = "/api/me/workspaces/{id}/entries/{entry_id}",
    tag = "workspaces",
    params(
        ("id" = Uuid, Path, description = "Workspace ID"),
        ("entry_id" = Uuid, Path, description = "Entry ID"),
    ),
    responses(
        (status = 204, description = "Entry removed"),
        (status = 404, description = "Entry not found"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn remove_entry(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path((workspace_id, entry_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, WorkspaceError> {
    // Delete entry only if workspace is owned by user
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
    .bind(auth_user.id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(WorkspaceError::EntryNotFound);
    }

    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth_user.id,
        &ServerEvent::WorkspaceEntryRemoved {
            workspace_id,
            entry_id,
        },
    )
    .await
    {
        tracing::warn!("Failed to broadcast WorkspaceEntryRemoved event: {}", e);
    }

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Reordering
// ============================================================================

/// Reorder entries within a workspace.
///
/// PATCH /api/me/workspaces/{id}/reorder
#[utoipa::path(
    patch,
    path = "/api/me/workspaces/{id}/reorder",
    tag = "workspaces",
    params(("id" = Uuid, Path, description = "Workspace ID")),
    request_body = ReorderEntriesRequest,
    responses(
        (status = 204, description = "Entries reordered"),
        (status = 400, description = "Invalid entry IDs"),
        (status = 404, description = "Workspace not found"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn reorder_entries(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(request): Json<ReorderEntriesRequest>,
) -> Result<StatusCode, WorkspaceError> {
    if request.entry_ids.len() > state.config.max_entries_per_workspace as usize {
        return Err(WorkspaceError::Validation(format!(
            "entry_ids must contain at most {} items",
            state.config.max_entries_per_workspace
        )));
    }

    // Verify workspace ownership
    let workspace_exists =
        sqlx::query("SELECT 1 FROM workspaces WHERE id = $1 AND owner_user_id = $2")
            .bind(workspace_id)
            .bind(auth_user.id)
            .fetch_optional(&state.db)
            .await?
            .is_some();

    if !workspace_exists {
        return Err(WorkspaceError::NotFound);
    }

    let mut tx = state.db.begin().await?;

    // Verify all entry IDs belong to this workspace AND cover the full set
    let (matched_count, total_count): (i64, i64) = sqlx::query_as(
        r"
        SELECT
            COUNT(*) FILTER (WHERE id = ANY($2)),
            COUNT(*)
        FROM workspace_entries WHERE workspace_id = $1
        ",
    )
    .bind(workspace_id)
    .bind(&request.entry_ids)
    .fetch_one(&mut *tx)
    .await?;

    if matched_count != request.entry_ids.len() as i64 || total_count != matched_count {
        return Err(WorkspaceError::InvalidEntries);
    }

    // Update positions (trigger handles updated_at)
    for (position, entry_id) in request.entry_ids.iter().enumerate() {
        sqlx::query(
            "UPDATE workspace_entries SET position = $1 WHERE id = $2 AND workspace_id = $3",
        )
        .bind(position as i32)
        .bind(entry_id)
        .bind(workspace_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let entries: Vec<serde_json::Value> = request
        .entry_ids
        .iter()
        .enumerate()
        .map(|(pos, id)| serde_json::json!({ "id": id, "position": pos }))
        .collect();

    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth_user.id,
        &ServerEvent::WorkspaceEntriesReordered {
            workspace_id,
            entries,
        },
    )
    .await
    {
        tracing::warn!("Failed to broadcast WorkspaceEntriesReordered event: {}", e);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Reorder workspaces.
///
/// POST /api/me/workspaces/reorder
#[utoipa::path(
    post,
    path = "/api/me/workspaces/reorder",
    tag = "workspaces",
    request_body = ReorderWorkspacesRequest,
    responses(
        (status = 204, description = "Workspaces reordered"),
        (status = 400, description = "Invalid workspace IDs"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn reorder_workspaces(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<ReorderWorkspacesRequest>,
) -> Result<StatusCode, WorkspaceError> {
    if request.workspace_ids.len() > state.config.max_workspaces_per_user as usize {
        return Err(WorkspaceError::Validation(format!(
            "workspace_ids must contain at most {} items",
            state.config.max_workspaces_per_user
        )));
    }

    let mut tx = state.db.begin().await?;

    // Verify all workspace IDs belong to this user AND cover the full set
    let (matched_count, total_count): (i64, i64) = sqlx::query_as(
        r"
        SELECT
            COUNT(*) FILTER (WHERE id = ANY($2)),
            COUNT(*)
        FROM workspaces WHERE owner_user_id = $1
        ",
    )
    .bind(auth_user.id)
    .bind(&request.workspace_ids)
    .fetch_one(&mut *tx)
    .await?;

    if matched_count != request.workspace_ids.len() as i64 || total_count != matched_count {
        return Err(WorkspaceError::InvalidWorkspaces);
    }

    // Update sort_order (trigger handles updated_at)
    for (sort_order, workspace_id) in request.workspace_ids.iter().enumerate() {
        sqlx::query("UPDATE workspaces SET sort_order = $1 WHERE id = $2 AND owner_user_id = $3")
            .bind(sort_order as i32)
            .bind(workspace_id)
            .bind(auth_user.id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    let workspaces: Vec<serde_json::Value> = request
        .workspace_ids
        .iter()
        .enumerate()
        .map(|(order, id)| serde_json::json!({ "id": id, "sort_order": order }))
        .collect();

    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth_user.id,
        &ServerEvent::WorkspaceReordered { workspaces },
    )
    .await
    {
        tracing::warn!("Failed to broadcast WorkspaceReordered event: {}", e);
    }

    Ok(StatusCode::NO_CONTENT)
}
