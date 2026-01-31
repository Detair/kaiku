//! API handlers for information pages.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use tracing::error;
use uuid::Uuid;

use crate::{
    api::AppState,
    auth::AuthUser,
    pages::{
        queries, CreatePageRequest, Page, PageListItem, ReorderRequest, UpdatePageRequest,
        MAX_CONTENT_SIZE, MAX_PAGES_PER_SCOPE, MAX_SLUG_LENGTH, MAX_TITLE_LENGTH,
    },
    permissions::{is_system_admin, require_guild_permission, GuildPermissions, PermissionError},
};

/// Error response type for page handlers.
type PageResult<T> = Result<T, (StatusCode, String)>;

// ============================================================================
// Platform Pages (system admin only)
// ============================================================================

/// List all platform pages.
pub async fn list_platform_pages(State(state): State<AppState>) -> PageResult<Json<Vec<PageListItem>>> {
    let pages = queries::list_pages(&state.db, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(pages))
}

/// Get a platform page by slug.
pub async fn get_platform_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> PageResult<Json<Page>> {
    queries::get_page_by_slug(&state.db, None, &slug)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Page not found".to_string()))
}

/// Create a new platform page (system admin only).
pub async fn create_platform_page(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreatePageRequest>,
) -> PageResult<Json<Page>> {
    // Verify system admin (fail-fast on DB error for security)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err((StatusCode::FORBIDDEN, "System admin required".to_string()));
    }

    // Validate request
    validate_create_request(&req)?;

    let slug = req
        .slug
        .clone()
        .unwrap_or_else(|| queries::slugify(&req.title));

    validate_slug(&slug)?;

    // Check slug availability (conservative: assume exists on error)
    let slug_exists = queries::slug_exists(&state.db, None, &slug, None)
        .await
        .unwrap_or_else(|e| {
            error!("Slug check failed, assuming exists: {}", e);
            true
        });
    if slug_exists {
        return Err((StatusCode::CONFLICT, "Slug already exists".to_string()));
    }

    let recently_deleted = queries::slug_recently_deleted(&state.db, None, &slug)
        .await
        .unwrap_or_else(|e| {
            error!("Recently deleted check failed: {}", e);
            false
        });
    if recently_deleted {
        return Err((
            StatusCode::CONFLICT,
            "Slug was recently deleted. Try a different slug.".to_string(),
        ));
    }

    // Check page limit (conservative: assume at limit on error)
    let at_limit = queries::is_at_page_limit(&state.db, None)
        .await
        .unwrap_or_else(|e| {
            error!("Page limit check failed, assuming at limit: {}", e);
            true
        });
    if at_limit {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Maximum {MAX_PAGES_PER_SCOPE} pages reached"),
        ));
    }

    // Create page
    let page = queries::create_page(
        &state.db,
        None,
        &req.title,
        &slug,
        &req.content,
        req.requires_acceptance.unwrap_or(false),
        user.id,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Log audit (non-blocking, log errors instead of failing)
    if let Err(e) = queries::log_audit(&state.db, page.id, "create", user.id, None, None, None).await {
        error!("Failed to log audit for page {}: {}", page.id, e);
    }

    Ok(Json(page))
}

/// Update a platform page (system admin only).
pub async fn update_platform_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdatePageRequest>,
) -> PageResult<Json<Page>> {
    // Verify system admin (fail-fast on DB error for security)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err((StatusCode::FORBIDDEN, "System admin required".to_string()));
    }

    // Get existing page
    let old_page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Page not found".to_string()))?;

    // Verify it's a platform page
    if old_page.guild_id.is_some() {
        return Err((StatusCode::BAD_REQUEST, "Not a platform page".to_string()));
    }

    // Validate request
    validate_update_request(&req)?;

    // Check slug if changed
    if let Some(ref slug) = req.slug {
        validate_slug(slug)?;
        if queries::slug_exists(&state.db, None, slug, Some(id))
            .await
            .unwrap_or(true)
        {
            return Err((StatusCode::CONFLICT, "Slug already exists".to_string()));
        }
    }

    // Update page
    let page = queries::update_page(
        &state.db,
        id,
        req.title.as_deref(),
        req.slug.as_deref(),
        req.content.as_deref(),
        req.requires_acceptance,
        user.id,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Log audit
    queries::log_audit(
        &state.db,
        id,
        "update",
        user.id,
        Some(&old_page.content_hash),
        None,
        None,
    )
    .await
    .ok();

    Ok(Json(page))
}

/// Delete a platform page (system admin only).
pub async fn delete_platform_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> PageResult<StatusCode> {
    // Verify system admin (fail-fast on DB error for security)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err((StatusCode::FORBIDDEN, "System admin required".to_string()));
    }

    // Get existing page
    let page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Page not found".to_string()))?;

    // Verify it's a platform page
    if page.guild_id.is_some() {
        return Err((StatusCode::BAD_REQUEST, "Not a platform page".to_string()));
    }

    // Soft delete
    queries::soft_delete_page(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Log audit
    queries::log_audit(
        &state.db,
        id,
        "delete",
        user.id,
        Some(&page.content_hash),
        None,
        None,
    )
    .await
    .ok();

    Ok(StatusCode::NO_CONTENT)
}

/// Reorder platform pages (system admin only).
pub async fn reorder_platform_pages(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<ReorderRequest>,
) -> PageResult<StatusCode> {
    // Verify system admin (fail-fast on DB error for security)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err((StatusCode::FORBIDDEN, "System admin required".to_string()));
    }

    queries::reorder_pages(&state.db, None, &req.page_ids)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Guild Pages
// ============================================================================

/// Convert `PermissionError` to HTTP response.
fn permission_error_to_response(err: PermissionError) -> (StatusCode, String) {
    match err {
        PermissionError::NotGuildMember => (StatusCode::FORBIDDEN, "Not a member of this guild".to_string()),
        PermissionError::MissingPermission(p) => (StatusCode::FORBIDDEN, format!("Missing permission: {p:?}")),
        PermissionError::DatabaseError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        _ => (StatusCode::FORBIDDEN, err.to_string()),
    }
}

/// Check `MANAGE_PAGES` permission for guild.
async fn check_manage_pages_permission(
    state: &AppState,
    guild_id: Uuid,
    user_id: Uuid,
) -> PageResult<()> {
    require_guild_permission(&state.db, guild_id, user_id, GuildPermissions::MANAGE_PAGES)
        .await
        .map(|_| ())
        .map_err(permission_error_to_response)
}

/// List all pages for a guild.
pub async fn list_guild_pages(
    State(state): State<AppState>,
    Path(guild_id): Path<Uuid>,
) -> PageResult<Json<Vec<PageListItem>>> {
    let pages = queries::list_pages(&state.db, Some(guild_id))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(pages))
}

/// Get a guild page by slug.
pub async fn get_guild_page(
    State(state): State<AppState>,
    Path((guild_id, slug)): Path<(Uuid, String)>,
) -> PageResult<Json<Page>> {
    queries::get_page_by_slug(&state.db, Some(guild_id), &slug)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Page not found".to_string()))
}

/// Create a new guild page.
pub async fn create_guild_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(req): Json<CreatePageRequest>,
) -> PageResult<Json<Page>> {
    // Check permission
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    // Validate request
    validate_create_request(&req)?;

    let slug = req
        .slug
        .clone()
        .unwrap_or_else(|| queries::slugify(&req.title));

    validate_slug(&slug)?;

    // Check slug availability
    if queries::slug_exists(&state.db, Some(guild_id), &slug, None)
        .await
        .unwrap_or(true)
    {
        return Err((StatusCode::CONFLICT, "Slug already exists".to_string()));
    }

    if queries::slug_recently_deleted(&state.db, Some(guild_id), &slug)
        .await
        .unwrap_or(false)
    {
        return Err((
            StatusCode::CONFLICT,
            "Slug was recently deleted. Try a different slug.".to_string(),
        ));
    }

    // Check page limit
    if queries::is_at_page_limit(&state.db, Some(guild_id))
        .await
        .unwrap_or(true)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Maximum {MAX_PAGES_PER_SCOPE} pages reached"),
        ));
    }

    // Create page
    let page = queries::create_page(
        &state.db,
        Some(guild_id),
        &req.title,
        &slug,
        &req.content,
        req.requires_acceptance.unwrap_or(false),
        user.id,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Log audit (non-blocking, log errors instead of failing)
    if let Err(e) = queries::log_audit(&state.db, page.id, "create", user.id, None, None, None).await {
        error!("Failed to log audit for page {}: {}", page.id, e);
    }

    Ok(Json(page))
}

/// Update a guild page.
pub async fn update_guild_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path((guild_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdatePageRequest>,
) -> PageResult<Json<Page>> {
    // Check permission
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    // Get existing page
    let old_page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Page not found".to_string()))?;

    // Verify page belongs to this guild
    if old_page.guild_id != Some(guild_id) {
        return Err((StatusCode::NOT_FOUND, "Page not found".to_string()));
    }

    // Validate request
    validate_update_request(&req)?;

    // Check slug if changed
    if let Some(ref slug) = req.slug {
        validate_slug(slug)?;
        if queries::slug_exists(&state.db, Some(guild_id), slug, Some(id))
            .await
            .unwrap_or(true)
        {
            return Err((StatusCode::CONFLICT, "Slug already exists".to_string()));
        }
    }

    // Update page
    let page = queries::update_page(
        &state.db,
        id,
        req.title.as_deref(),
        req.slug.as_deref(),
        req.content.as_deref(),
        req.requires_acceptance,
        user.id,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Log audit
    queries::log_audit(
        &state.db,
        id,
        "update",
        user.id,
        Some(&old_page.content_hash),
        None,
        None,
    )
    .await
    .ok();

    Ok(Json(page))
}

/// Delete a guild page.
pub async fn delete_guild_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path((guild_id, id)): Path<(Uuid, Uuid)>,
) -> PageResult<StatusCode> {
    // Check permission
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    // Get existing page
    let page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Page not found".to_string()))?;

    // Verify page belongs to this guild
    if page.guild_id != Some(guild_id) {
        return Err((StatusCode::NOT_FOUND, "Page not found".to_string()));
    }

    // Soft delete
    queries::soft_delete_page(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Log audit
    queries::log_audit(
        &state.db,
        id,
        "delete",
        user.id,
        Some(&page.content_hash),
        None,
        None,
    )
    .await
    .ok();

    Ok(StatusCode::NO_CONTENT)
}

/// Reorder guild pages.
pub async fn reorder_guild_pages(
    State(state): State<AppState>,
    user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(req): Json<ReorderRequest>,
) -> PageResult<StatusCode> {
    // Check permission
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    queries::reorder_pages(&state.db, Some(guild_id), &req.page_ids)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Acceptance
// ============================================================================

/// Accept a page (record user acceptance).
pub async fn accept_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> PageResult<StatusCode> {
    let page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Page not found".to_string()))?;

    queries::accept_page(&state.db, user.id, id, &page.content_hash)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Get pages requiring acceptance that user hasn't accepted.
pub async fn get_pending_acceptance(
    State(state): State<AppState>,
    user: AuthUser,
) -> PageResult<Json<Vec<PageListItem>>> {
    let pages = queries::get_pending_acceptance(&state.db, user.id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(pages))
}

// ============================================================================
// Validation Helpers
// ============================================================================

fn validate_create_request(req: &CreatePageRequest) -> PageResult<()> {
    if req.title.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Title is required".to_string()));
    }
    if req.title.len() > MAX_TITLE_LENGTH {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Title exceeds {MAX_TITLE_LENGTH} characters"),
        ));
    }
    if req.content.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Content is required".to_string()));
    }
    if req.content.len() > MAX_CONTENT_SIZE {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Content exceeds {MAX_CONTENT_SIZE} bytes"),
        ));
    }
    Ok(())
}

fn validate_update_request(req: &UpdatePageRequest) -> PageResult<()> {
    if let Some(ref title) = req.title {
        if title.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "Title cannot be empty".to_string()));
        }
        if title.len() > MAX_TITLE_LENGTH {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Title exceeds {MAX_TITLE_LENGTH} characters"),
            ));
        }
    }
    if let Some(ref content) = req.content {
        if content.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                "Content cannot be empty".to_string(),
            ));
        }
        if content.len() > MAX_CONTENT_SIZE {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Content exceeds {MAX_CONTENT_SIZE} bytes"),
            ));
        }
    }
    Ok(())
}

fn validate_slug(slug: &str) -> PageResult<()> {
    if slug.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Slug cannot be empty".to_string()));
    }
    if slug.len() > MAX_SLUG_LENGTH {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Slug exceeds {MAX_SLUG_LENGTH} characters"),
        ));
    }
    if queries::is_reserved_slug(slug) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("'{slug}' is a reserved slug"),
        ));
    }
    // Validate slug format (lowercase alphanumeric with dashes)
    let valid = slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--");

    if !valid {
        return Err((
            StatusCode::BAD_REQUEST,
            "Invalid slug format. Use lowercase letters, numbers, and single dashes (e.g., 'terms-of-service', 'faq-page')".to_string(),
        ));
    }
    Ok(())
}
