//! Workspace HTTP Handlers
//!
//! 9 endpoints for personal workspace CRUD, entry management, and reordering.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;
use validator::Validate;

use super::error::WorkspaceError;
use super::queries::{self, InsertEntryOutcome};
use super::types::{
    AddEntryRequest, CreateWorkspaceRequest, ReorderEntriesRequest, ReorderWorkspacesRequest,
    UpdateWorkspaceRequest, WorkspaceDetailResponse, WorkspaceEntryResponse, WorkspaceListItem,
    WorkspaceResponse, MAX_WORKSPACE_ICON_LENGTH,
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
        .map_err(|e| WorkspaceError::Validation(crate::validation::format_validation_errors(&e)))?;

    if let Some(icon) = request.icon.as_ref() {
        if icon.chars().count() > MAX_WORKSPACE_ICON_LENGTH {
            return Err(WorkspaceError::Validation(format!(
                "Icon must be at most {MAX_WORKSPACE_ICON_LENGTH} characters"
            )));
        }
    }

    let mut tx = state.db.begin().await?;

    queries::acquire_user_workspace_lock(&mut tx, auth_user.id).await?;

    let row = queries::insert_workspace_if_under_limit(
        &mut tx,
        auth_user.id,
        &request.name,
        request.icon.as_deref(),
        state.config.max_workspaces_per_user,
    )
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
    let rows = queries::list_user_workspaces(&state.db, auth_user.id).await?;
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
    let workspace = queries::find_workspace_for_owner(&state.db, workspace_id, auth_user.id)
        .await?
        .ok_or(WorkspaceError::NotFound)?;

    let entries = queries::list_workspace_entries(&state.db, workspace_id).await?;

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
        .map_err(|e| WorkspaceError::Validation(crate::validation::format_validation_errors(&e)))?;

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

    let row = queries::update_workspace_for_owner(
        &state.db,
        workspace_id,
        auth_user.id,
        request.name.as_deref(),
        should_update_icon,
        new_icon_value,
    )
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
    if !queries::delete_workspace_for_owner(&state.db, workspace_id, auth_user.id).await? {
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
    if !queries::workspace_owned_by(&state.db, workspace_id, auth_user.id).await? {
        return Err(WorkspaceError::NotFound);
    }

    // 2. Verify channel belongs to the claimed guild
    match queries::find_channel_guild_id(&state.db, request.channel_id).await? {
        Some(Some(gid)) if gid == request.guild_id => {}
        _ => return Err(WorkspaceError::ChannelNotFound),
    }

    // 3. Verify guild membership
    if !queries::is_guild_member(&state.db, request.guild_id, auth_user.id).await? {
        return Err(WorkspaceError::ChannelNotFound);
    }

    // 4. Verify channel access (VIEW_CHANNEL permission)
    crate::permissions::require_channel_access(&state.db, auth_user.id, request.channel_id)
        .await
        .map_err(|_| WorkspaceError::ChannelNotFound)?;

    let mut tx = state.db.begin().await?;

    queries::acquire_workspace_entry_lock(&mut tx, workspace_id).await?;

    // 5. Atomic insert with entry limit check inside lock.
    let outcome = queries::insert_workspace_entry_if_under_limit(
        &mut tx,
        workspace_id,
        request.guild_id,
        request.channel_id,
        state.config.max_entries_per_workspace,
    )
    .await?;

    let entry_id = match outcome {
        InsertEntryOutcome::Inserted(id) => {
            tx.commit().await?;
            id
        }
        InsertEntryOutcome::LimitExceeded => return Err(WorkspaceError::EntryLimitExceeded),
        InsertEntryOutcome::DuplicateEntry => return Err(WorkspaceError::DuplicateEntry),
    };

    // 6. Fetch guild/channel names for the response
    let entry_row = queries::find_workspace_entry_with_metadata(&state.db, entry_id).await?;
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
    if !queries::delete_workspace_entry_for_owner(
        &state.db,
        workspace_id,
        entry_id,
        auth_user.id,
    )
    .await?
    {
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
    if !queries::workspace_owned_by(&state.db, workspace_id, auth_user.id).await? {
        return Err(WorkspaceError::NotFound);
    }

    let mut tx = state.db.begin().await?;

    // Verify all entry IDs belong to this workspace AND cover the full set
    let (matched_count, total_count) = queries::count_workspace_entries_for_reorder(
        &mut tx,
        workspace_id,
        &request.entry_ids,
    )
    .await?;

    if matched_count != request.entry_ids.len() as i64 || total_count != matched_count {
        return Err(WorkspaceError::InvalidEntries);
    }

    // Update positions (trigger handles updated_at)
    for (position, entry_id) in request.entry_ids.iter().enumerate() {
        queries::update_workspace_entry_position(
            &mut tx,
            workspace_id,
            *entry_id,
            position as i32,
        )
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
    let (matched_count, total_count) = queries::count_user_workspaces_for_reorder(
        &mut tx,
        auth_user.id,
        &request.workspace_ids,
    )
    .await?;

    if matched_count != request.workspace_ids.len() as i64 || total_count != matched_count {
        return Err(WorkspaceError::InvalidWorkspaces);
    }

    // Update sort_order (trigger handles updated_at)
    for (sort_order, workspace_id) in request.workspace_ids.iter().enumerate() {
        queries::update_workspace_sort_order(
            &mut tx,
            *workspace_id,
            auth_user.id,
            sort_order as i32,
        )
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
