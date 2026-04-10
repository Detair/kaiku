//! API handlers for information pages.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use tracing::error;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::pages::{
    queries, CreateCategoryRequest, CreatePageRequest, Page, PageCategory, PageListItem,
    PageRevision, ReorderCategoriesRequest, ReorderRequest, RevisionListItem,
    UpdateCategoryRequest, UpdatePageRequest, MAX_CATEGORIES_PER_GUILD, MAX_CATEGORY_NAME_LENGTH,
    MAX_CONTENT_SIZE, MAX_SLUG_LENGTH, MAX_TITLE_LENGTH,
};
use crate::permissions::{
    is_system_admin, require_guild_permission, GuildPermissions, PermissionError,
};

use super::error::PagesError;

// ============================================================================
// Platform Pages (system admin only)
// ============================================================================

/// List all platform pages.
///
/// Note: Does not extract `AuthUser` — the `require_auth` middleware layer
/// ensures the request is authenticated, but this handler does not need the
/// caller's identity.
#[utoipa::path(
    get,
    path = "/api/pages",
    tag = "pages",
    responses(
        (status = 200, description = "List of platform pages", body = Vec<PageListItem>),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn list_platform_pages(
    State(state): State<AppState>,
) -> Result<Json<Vec<PageListItem>>, PagesError> {
    let pages = queries::list_pages(&state.db, None).await.map_err(|e| {
        error!("Failed to list platform pages: {}", e);
        PagesError::Database(e)
    })?;
    Ok(Json(pages))
}

/// Get a platform page by slug.
///
/// Note: Does not extract `AuthUser` — the `require_auth` middleware layer
/// ensures the request is authenticated, but this handler does not need the
/// caller's identity.
#[utoipa::path(
    get,
    path = "/api/pages/by-slug/{slug}",
    tag = "pages",
    params(("slug" = String, Path, description = "Page slug")),
    responses(
        (status = 200, description = "Platform page", body = Page),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn get_platform_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Page>, PagesError> {
    queries::get_page_by_slug(&state.db, None, &slug)
        .await
        .map_err(|e| {
            error!("Failed to get platform page '{}': {}", slug, e);
            PagesError::Database(e)
        })?
        .map(Json)
        .ok_or(PagesError::NotFound)
}

/// Create a new platform page (system admin only).
#[utoipa::path(
    post,
    path = "/api/pages",
    tag = "pages",
    request_body = CreatePageRequest,
    responses(
        (status = 200, description = "Platform page created", body = Page),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn create_platform_page(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreatePageRequest>,
) -> Result<Json<Page>, PagesError> {
    // Verify system admin (fail-fast on DB error for security)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        PagesError::Internal("Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err(PagesError::AdminRequired);
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
        return Err(PagesError::SlugConflict("Slug already exists".to_string()));
    }

    let recently_deleted = queries::slug_recently_deleted(&state.db, None, &slug)
        .await
        .unwrap_or_else(|e| {
            error!("Recently deleted check failed, assuming deleted: {}", e);
            true
        });
    if recently_deleted {
        return Err(PagesError::SlugConflict(
            "Slug was recently deleted. Try a different slug.".to_string(),
        ));
    }

    // Reject category_id for platform pages
    if req.category_id.is_some() {
        return Err(PagesError::Validation(
            "Categories are not supported for platform pages".to_string(),
        ));
    }

    let mut tx = state.db.begin().await.map_err(|e| {
        error!("Failed to begin transaction: {}", e);
        PagesError::Database(e)
    })?;

    // Advisory lock: serialize platform page creation to enforce strict limits under concurrency.
    // Seed 61 (see registry in server/src/db/mod.rs).
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 61))")
        .bind("platform_pages")
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!("Failed to acquire advisory lock: {}", e);
            PagesError::Database(e)
        })?;

    // Check page limit inside lock for atomicity
    let max_limit = state.config.max_pages_per_guild;
    let at_limit: i64 = sqlx::query_scalar(
        r"SELECT COUNT(*) FROM pages WHERE guild_id IS NULL AND deleted_at IS NULL",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        error!("Page limit check failed: {}", e);
        PagesError::Database(e)
    })?;

    if at_limit >= max_limit {
        return Err(PagesError::LimitExceeded(format!(
            "Maximum {max_limit} pages reached"
        )));
    }

    // Create page inside transaction
    let page = queries::create_page_with_initial_revision_in_tx(
        &mut tx,
        queries::CreatePageParams {
            guild_id: None,
            title: &req.title,
            slug: &slug,
            content: &req.content,
            requires_acceptance: req.requires_acceptance.unwrap_or(false),
            category_id: None,
            created_by: user.id,
        },
    )
    .await
    .map_err(|e| {
        error!("Failed to create platform page: {}", e);
        PagesError::Database(e)
    })?;

    tx.commit().await.map_err(|e| {
        error!("Failed to commit transaction: {}", e);
        PagesError::Database(e)
    })?;

    // Log audit (non-blocking, log errors instead of failing)
    if let Err(e) =
        queries::log_audit(&state.db, page.id, "create", user.id, None, None, None).await
    {
        error!("Failed to log audit for page {}: {}", page.id, e);
    }

    Ok(Json(page))
}

/// Update a platform page (system admin only).
#[utoipa::path(
    patch,
    path = "/api/pages/{id}",
    tag = "pages",
    params(("id" = Uuid, Path, description = "Page ID")),
    request_body = UpdatePageRequest,
    responses(
        (status = 200, description = "Platform page updated", body = Page),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn update_platform_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdatePageRequest>,
) -> Result<Json<Page>, PagesError> {
    // Verify system admin (fail-fast on DB error for security)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        PagesError::Internal("Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err(PagesError::AdminRequired);
    }

    // Get existing page
    let old_page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    // Verify it's a platform page
    if old_page.guild_id.is_some() {
        return Err(PagesError::NotPlatformPage);
    }

    // Validate request
    validate_update_request(&req)?;

    // Reject category_id for platform pages (consistent with create_platform_page)
    if req.category_id.is_some() {
        return Err(PagesError::Validation(
            "Categories are not supported for platform pages".to_string(),
        ));
    }

    // Check slug if changed
    if let Some(ref slug) = req.slug {
        validate_slug(slug)?;
        if queries::slug_exists(&state.db, None, slug, Some(id))
            .await
            .unwrap_or_else(|e| {
                error!("Slug check failed, assuming exists: {}", e);
                true
            })
        {
            return Err(PagesError::SlugConflict("Slug already exists".to_string()));
        }
    }

    // Update page (platform pages don't support categories)
    let page = queries::update_page(
        &state.db,
        queries::UpdatePageParams {
            id,
            title: req.title.as_deref(),
            slug: req.slug.as_deref(),
            content: req.content.as_deref(),
            requires_acceptance: req.requires_acceptance,
            category_id: None,
            updated_by: user.id,
        },
    )
    .await
    .map_err(|e| {
        error!("Failed to update platform page {}: {}", id, e);
        PagesError::Database(e)
    })?;

    // Log audit
    if let Err(e) = queries::log_audit(
        &state.db,
        id,
        "update",
        user.id,
        Some(&old_page.content_hash),
        None,
        None,
    )
    .await
    {
        error!("Failed to log audit for page {}: {}", id, e);
    }

    // Create revision on content change (best-effort — concurrent edits may collide
    // on the unique constraint; the page update itself already succeeded)
    if req.content.is_some() {
        if let Err(e) = queries::create_revision(
            &state.db,
            page.id,
            &page.content,
            &page.content_hash,
            &page.title,
            user.id,
        )
        .await
        {
            error!(
                "Revision snapshot failed for page {} (concurrent edit?): {}",
                page.id, e
            );
        }
        // Prune old revisions (best-effort — pruning failure doesn't affect correctness)
        if let Err(e) =
            queries::prune_revisions(&state.db, page.id, state.config.max_revisions_per_page).await
        {
            error!("Failed to prune revisions for page {}: {}", page.id, e);
        }
    }

    Ok(Json(page))
}

/// Delete a platform page (system admin only).
#[utoipa::path(
    delete,
    path = "/api/pages/{id}",
    tag = "pages",
    params(("id" = Uuid, Path, description = "Page ID")),
    responses(
        (status = 204, description = "Platform page deleted"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn delete_platform_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, PagesError> {
    // Verify system admin (fail-fast on DB error for security)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        PagesError::Internal("Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err(PagesError::AdminRequired);
    }

    // Get existing page
    let page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    // Verify it's a platform page
    if page.guild_id.is_some() {
        return Err(PagesError::NotPlatformPage);
    }

    // Soft delete
    queries::soft_delete_page(&state.db, id)
        .await
        .map_err(|e| {
            error!("Failed to delete platform page {}: {}", id, e);
            PagesError::Database(e)
        })?;

    // Log audit
    if let Err(e) = queries::log_audit(
        &state.db,
        id,
        "delete",
        user.id,
        Some(&page.content_hash),
        None,
        None,
    )
    .await
    {
        error!("Failed to log audit for page {}: {}", id, e);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Reorder platform pages (system admin only).
#[utoipa::path(
    post,
    path = "/api/pages/reorder",
    tag = "pages",
    request_body = ReorderRequest,
    responses(
        (status = 204, description = "Pages reordered"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn reorder_platform_pages(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<ReorderRequest>,
) -> Result<StatusCode, PagesError> {
    if req.page_ids.len() > 1000 {
        return Err(PagesError::Validation("Too many page IDs".to_string()));
    }

    // Verify system admin (fail-fast on DB error for security)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        PagesError::Internal("Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err(PagesError::AdminRequired);
    }

    queries::reorder_pages(&state.db, None, &req.page_ids)
        .await
        .map_err(|e| {
            // Map Protocol errors (invalid input) to 400, others to 500
            if let sqlx::Error::Protocol(msg) = &e {
                error!("Invalid reorder request: {}", msg);
                PagesError::InvalidReorder(msg.clone())
            } else {
                error!("Failed to reorder platform pages: {}", e);
                PagesError::Database(e)
            }
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Guild Pages
// ============================================================================

/// Convert `PermissionError` to `PagesError`.
fn map_permission_error(err: PermissionError) -> PagesError {
    match err {
        PermissionError::NotGuildMember => PagesError::Forbidden,
        PermissionError::DatabaseError(msg) => {
            error!("Permission database error: {}", msg);
            PagesError::Internal("Internal server error".to_string())
        }
        other => PagesError::Permission(other),
    }
}

/// Check `MANAGE_PAGES` permission for guild.
async fn check_manage_pages_permission(
    state: &AppState,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<(), PagesError> {
    require_guild_permission(&state.db, guild_id, user_id, GuildPermissions::MANAGE_PAGES)
        .await
        .map(|_| ())
        .map_err(map_permission_error)
}

/// List all pages for a guild.
///
/// Note: Does not check guild membership — guild information pages (rules, welcome)
/// are intentionally readable by any authenticated user who has the guild ID.
/// Write operations are protected by `MANAGE_PAGES` permission.
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/pages",
    tag = "pages",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, description = "List of guild pages")),
    security(("bearer_auth" = []))
)]
pub async fn list_guild_pages(
    State(state): State<AppState>,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<PageListItem>>, PagesError> {
    let pages = queries::list_pages(&state.db, Some(guild_id))
        .await
        .map_err(|e| {
            error!("Failed to list guild pages for {}: {}", guild_id, e);
            PagesError::Database(e)
        })?;
    Ok(Json(pages))
}

/// Get a guild page by slug.
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/pages/by-slug/{slug}",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("slug" = String, Path, description = "Page slug")
    ),
    responses((status = 200, description = "Guild page")),
    security(("bearer_auth" = []))
)]
pub async fn get_guild_page(
    State(state): State<AppState>,
    Path((guild_id, slug)): Path<(Uuid, String)>,
) -> Result<Json<Page>, PagesError> {
    queries::get_page_by_slug(&state.db, Some(guild_id), &slug)
        .await
        .map_err(|e| {
            error!("Failed to get guild page '{}' in {}: {}", slug, guild_id, e);
            PagesError::Database(e)
        })?
        .map(Json)
        .ok_or(PagesError::NotFound)
}

/// Create a new guild page.
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/pages",
    tag = "pages",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = CreatePageRequest,
    responses((status = 200, description = "Guild page created", body = Page)),
    security(("bearer_auth" = []))
)]
pub async fn create_guild_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(req): Json<CreatePageRequest>,
) -> Result<Json<Page>, PagesError> {
    // Check permission
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    // Validate request
    validate_create_request(&req)?;

    let slug = req
        .slug
        .clone()
        .unwrap_or_else(|| queries::slugify(&req.title));

    validate_slug(&slug)?;

    // Check slug availability (conservative: assume exists on error)
    if queries::slug_exists(&state.db, Some(guild_id), &slug, None)
        .await
        .unwrap_or_else(|e| {
            error!("Slug check failed, assuming exists: {}", e);
            true
        })
    {
        return Err(PagesError::SlugConflict("Slug already exists".to_string()));
    }

    if queries::slug_recently_deleted(&state.db, Some(guild_id), &slug)
        .await
        .unwrap_or_else(|e| {
            error!("Recently deleted check failed, assuming deleted: {}", e);
            true
        })
    {
        return Err(PagesError::SlugConflict(
            "Slug was recently deleted. Try a different slug.".to_string(),
        ));
    }

    let mut tx = state.db.begin().await.map_err(|e| {
        error!("Failed to begin transaction: {}", e);
        PagesError::Database(e)
    })?;

    // Advisory lock: serialize guild page creation to enforce strict limits under concurrency.
    // Seed 61 (see registry in server/src/db/mod.rs).
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 61))")
        .bind(guild_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            error!("Failed to acquire advisory lock: {}", e);
            PagesError::Database(e)
        })?;

    // Check page limit using per-guild override or instance default, inside lock for atomicity
    let max_limit =
        queries::get_effective_page_limit(&state.db, guild_id, state.config.max_pages_per_guild)
            .await
            .unwrap_or_else(|e| {
                error!(
                    "Failed to get effective page limit for guild {}, using instance default: {}",
                    guild_id, e
                );
                state.config.max_pages_per_guild
            });

    let at_limit: i64 = sqlx::query_scalar(
        r"SELECT COUNT(*) FROM pages WHERE guild_id = $1 AND deleted_at IS NULL",
    )
    .bind(guild_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        error!("Page limit check failed: {}", e);
        PagesError::Database(e)
    })?;

    if at_limit >= max_limit {
        return Err(PagesError::LimitExceeded(format!(
            "Maximum {max_limit} pages reached"
        )));
    }

    // Validate category belongs to this guild
    if let Some(cat_id) = req.category_id {
        let cat = queries::get_category(&state.db, cat_id)
            .await
            .map_err(|e| {
                error!("Failed to validate category: {}", e);
                PagesError::Database(e)
            })?
            .ok_or(PagesError::CategoryNotFound)?;
        if cat.guild_id != guild_id {
            return Err(PagesError::Validation(
                "Category does not belong to this guild".to_string(),
            ));
        }
    }

    // Create page inside transaction
    let page = queries::create_page_with_initial_revision_in_tx(
        &mut tx,
        queries::CreatePageParams {
            guild_id: Some(guild_id),
            title: &req.title,
            slug: &slug,
            content: &req.content,
            requires_acceptance: req.requires_acceptance.unwrap_or(false),
            category_id: req.category_id,
            created_by: user.id,
        },
    )
    .await
    .map_err(|e| {
        error!("Failed to create guild page in {}: {}", guild_id, e);
        PagesError::Database(e)
    })?;

    tx.commit().await.map_err(|e| {
        error!("Failed to commit transaction: {}", e);
        PagesError::Database(e)
    })?;

    // Log audit (non-blocking, log errors instead of failing)
    if let Err(e) =
        queries::log_audit(&state.db, page.id, "create", user.id, None, None, None).await
    {
        error!("Failed to log audit for page {}: {}", page.id, e);
    }

    Ok(Json(page))
}

/// Update a guild page.
#[utoipa::path(
    patch,
    path = "/api/guilds/{id}/pages/{page_id}",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("page_id" = Uuid, Path, description = "Page ID")
    ),
    request_body = UpdatePageRequest,
    responses((status = 200, description = "Guild page updated", body = Page)),
    security(("bearer_auth" = []))
)]
pub async fn update_guild_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path((guild_id, id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdatePageRequest>,
) -> Result<Json<Page>, PagesError> {
    // Check permission
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    // Get existing page
    let old_page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    // Verify page belongs to this guild
    if old_page.guild_id != Some(guild_id) {
        return Err(PagesError::NotFound);
    }

    // Validate request
    validate_update_request(&req)?;

    // Check slug if changed
    if let Some(ref slug) = req.slug {
        validate_slug(slug)?;
        if queries::slug_exists(&state.db, Some(guild_id), slug, Some(id))
            .await
            .unwrap_or_else(|e| {
                error!("Slug check failed, assuming exists: {}", e);
                true
            })
        {
            return Err(PagesError::SlugConflict("Slug already exists".to_string()));
        }
    }

    // Validate category belongs to this guild
    if let Some(Some(cat_id)) = req.category_id {
        let cat = queries::get_category(&state.db, cat_id)
            .await
            .map_err(|e| {
                error!("Failed to validate category: {}", e);
                PagesError::Database(e)
            })?
            .ok_or(PagesError::CategoryNotFound)?;
        if cat.guild_id != guild_id {
            return Err(PagesError::Validation(
                "Category does not belong to this guild".to_string(),
            ));
        }
    }

    // Update page
    let page = queries::update_page(
        &state.db,
        queries::UpdatePageParams {
            id,
            title: req.title.as_deref(),
            slug: req.slug.as_deref(),
            content: req.content.as_deref(),
            requires_acceptance: req.requires_acceptance,
            category_id: req.category_id,
            updated_by: user.id,
        },
    )
    .await
    .map_err(|e| {
        error!("Failed to update guild page {}: {}", id, e);
        PagesError::Database(e)
    })?;

    // Log audit
    if let Err(e) = queries::log_audit(
        &state.db,
        id,
        "update",
        user.id,
        Some(&old_page.content_hash),
        None,
        None,
    )
    .await
    {
        error!("Failed to log audit for page {}: {}", id, e);
    }

    // Create revision on content change (best-effort — concurrent edits may collide
    // on the unique constraint; the page update itself already succeeded)
    if req.content.is_some() {
        if let Err(e) = queries::create_revision(
            &state.db,
            page.id,
            &page.content,
            &page.content_hash,
            &page.title,
            user.id,
        )
        .await
        {
            error!(
                "Revision snapshot failed for page {} (concurrent edit?): {}",
                page.id, e
            );
        }
        // Prune old revisions (best-effort — pruning failure doesn't affect correctness)
        let max_revisions = queries::get_effective_revision_limit(
            &state.db,
            guild_id,
            state.config.max_revisions_per_page,
        )
        .await
        .unwrap_or_else(|e| {
            error!(
                "Failed to get effective revision limit, using instance default: {}",
                e
            );
            state.config.max_revisions_per_page
        });

        if let Err(e) = queries::prune_revisions(&state.db, page.id, max_revisions).await {
            error!("Failed to prune revisions for page {}: {}", page.id, e);
        }
    }

    Ok(Json(page))
}

/// Delete a guild page.
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}/pages/{page_id}",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("page_id" = Uuid, Path, description = "Page ID")
    ),
    responses((status = 204, description = "Guild page deleted")),
    security(("bearer_auth" = []))
)]
pub async fn delete_guild_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path((guild_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, PagesError> {
    // Check permission
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    // Get existing page
    let page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    // Verify page belongs to this guild
    if page.guild_id != Some(guild_id) {
        return Err(PagesError::NotFound);
    }

    // Soft delete
    queries::soft_delete_page(&state.db, id)
        .await
        .map_err(|e| {
            error!("Failed to delete guild page {}: {}", id, e);
            PagesError::Database(e)
        })?;

    // Log audit
    if let Err(e) = queries::log_audit(
        &state.db,
        id,
        "delete",
        user.id,
        Some(&page.content_hash),
        None,
        None,
    )
    .await
    {
        error!("Failed to log audit for page {}: {}", id, e);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Reorder guild pages.
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/pages/reorder",
    tag = "pages",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = ReorderRequest,
    responses((status = 204, description = "Guild pages reordered")),
    security(("bearer_auth" = []))
)]
pub async fn reorder_guild_pages(
    State(state): State<AppState>,
    user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(req): Json<ReorderRequest>,
) -> Result<StatusCode, PagesError> {
    if req.page_ids.len() > 1000 {
        return Err(PagesError::Validation("Too many page IDs".to_string()));
    }

    // Check permission
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    queries::reorder_pages(&state.db, Some(guild_id), &req.page_ids)
        .await
        .map_err(|e| {
            // Map Protocol errors (invalid input) to 400, others to 500
            if let sqlx::Error::Protocol(msg) = &e {
                error!("Invalid reorder request for guild {}: {}", guild_id, msg);
                PagesError::InvalidReorder(msg.clone())
            } else {
                error!("Failed to reorder guild pages in {}: {}", guild_id, e);
                PagesError::Database(e)
            }
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Acceptance
// ============================================================================

/// Accept a page (record user acceptance).
#[utoipa::path(
    post,
    path = "/api/pages/{id}/accept",
    tag = "pages",
    params(("id" = Uuid, Path, description = "Page ID")),
    responses(
        (status = 204, description = "Page accepted"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn accept_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, PagesError> {
    let page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| {
            error!("Failed to get page {} for acceptance: {}", id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    // Verify it's a platform page (guild pages use the guild-scoped endpoint)
    if page.guild_id.is_some() {
        return Err(PagesError::NotPlatformPage);
    }

    if !page.requires_acceptance {
        return Err(PagesError::Validation(
            "This page does not require acceptance".to_string(),
        ));
    }

    queries::accept_page(&state.db, user.id, id, &page.content_hash)
        .await
        .map_err(|e| {
            error!("Failed to record page acceptance for page {}: {}", id, e);
            PagesError::Database(e)
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Accept a guild page (record user acceptance with guild scope check).
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/pages/{page_id}/accept",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("page_id" = Uuid, Path, description = "Page ID")
    ),
    responses((status = 204, description = "Guild page accepted")),
    security(("bearer_auth" = []))
)]
pub async fn accept_guild_page(
    State(state): State<AppState>,
    user: AuthUser,
    Path((guild_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, PagesError> {
    let page = queries::get_page_by_id(&state.db, id)
        .await
        .map_err(|e| {
            error!("Failed to get page {} for acceptance: {}", id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    // Verify page belongs to this guild
    if page.guild_id != Some(guild_id) {
        return Err(PagesError::NotFound);
    }

    if !page.requires_acceptance {
        return Err(PagesError::Validation(
            "This page does not require acceptance".to_string(),
        ));
    }

    queries::accept_page(&state.db, user.id, id, &page.content_hash)
        .await
        .map_err(|e| {
            error!("Failed to record page acceptance for page {}: {}", id, e);
            PagesError::Database(e)
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Get pages requiring acceptance that user hasn't accepted.
#[utoipa::path(
    get,
    path = "/api/pages/pending-acceptance",
    tag = "pages",
    responses(
        (status = 200, description = "Pages pending acceptance", body = Vec<PageListItem>),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn get_pending_acceptance(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<PageListItem>>, PagesError> {
    let pages = queries::get_pending_acceptance(&state.db, user.id)
        .await
        .map_err(|e| {
            error!("Failed to get pending acceptance: {}", e);
            PagesError::Database(e)
        })?;
    Ok(Json(pages))
}

// ============================================================================
// Revisions
// ============================================================================

/// List revisions for a guild page.
///
/// Note: Does not check guild membership — consistent with `list_guild_pages`.
/// Guild information pages are intentionally readable by any authenticated user
/// who has the guild ID. The `require_auth` middleware ensures authentication.
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/pages/{page_id}/revisions",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("page_id" = Uuid, Path, description = "Page ID")
    ),
    responses((status = 200, description = "List of revisions", body = Vec<RevisionListItem>)),
    security(("bearer_auth" = []))
)]
pub async fn list_guild_page_revisions(
    State(state): State<AppState>,
    Path((guild_id, page_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<RevisionListItem>>, PagesError> {
    let page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if page.guild_id != Some(guild_id) {
        return Err(PagesError::NotFound);
    }

    let revisions = queries::list_revisions(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to list revisions for page {}: {}", page_id, e);
            PagesError::Database(e)
        })?;

    Ok(Json(revisions))
}

/// Get a specific revision of a guild page.
///
/// Note: Does not check guild membership — consistent with `list_guild_pages`.
/// Guild information pages are intentionally readable by any authenticated user
/// who has the guild ID. The `require_auth` middleware ensures authentication.
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/pages/{page_id}/revisions/{n}",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("page_id" = Uuid, Path, description = "Page ID"),
        ("n" = i32, Path, description = "Revision number")
    ),
    responses((status = 200, description = "Revision content", body = PageRevision)),
    security(("bearer_auth" = []))
)]
pub async fn get_guild_page_revision(
    State(state): State<AppState>,
    Path((guild_id, page_id, n)): Path<(Uuid, Uuid, i32)>,
) -> Result<Json<PageRevision>, PagesError> {
    let page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if page.guild_id != Some(guild_id) {
        return Err(PagesError::NotFound);
    }

    queries::get_revision(&state.db, page_id, n)
        .await
        .map_err(|e| {
            error!("Failed to get revision {} for page {}: {}", n, page_id, e);
            PagesError::Database(e)
        })?
        .map(Json)
        .ok_or(PagesError::RevisionNotFound)
}

/// Restore a guild page to a previous revision.
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/pages/{page_id}/revisions/{n}/restore",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("page_id" = Uuid, Path, description = "Page ID"),
        ("n" = i32, Path, description = "Revision number")
    ),
    responses((status = 200, description = "Page restored", body = Page)),
    security(("bearer_auth" = []))
)]
pub async fn restore_guild_page_revision(
    State(state): State<AppState>,
    user: AuthUser,
    Path((guild_id, page_id, n)): Path<(Uuid, Uuid, i32)>,
) -> Result<Json<Page>, PagesError> {
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    let old_page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if old_page.guild_id != Some(guild_id) {
        return Err(PagesError::NotFound);
    }

    let revision = queries::get_revision(&state.db, page_id, n)
        .await
        .map_err(|e| {
            error!("Failed to get revision {} for page {}: {}", n, page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::RevisionNotFound)?;

    let content = revision.content.as_deref().ok_or_else(|| {
        error!(
            "Revision {} for page {} has NULL content",
            n, revision.page_id
        );
        PagesError::CorruptRevision("Revision data is corrupted (missing content)".to_string())
    })?;
    let title = revision.title.as_deref().unwrap_or(&old_page.title);

    // Update page with revision content
    let page = queries::update_page(
        &state.db,
        queries::UpdatePageParams {
            id: page_id,
            title: Some(title),
            slug: None,
            content: Some(content),
            requires_acceptance: None,
            category_id: None,
            updated_by: user.id,
        },
    )
    .await
    .map_err(|e| {
        error!(
            "Failed to restore page {} to revision {}: {}",
            page_id, n, e
        );
        PagesError::Database(e)
    })?;

    // Create a new revision for the restore (best-effort — concurrent operations
    // may collide on the unique constraint; the restore itself already succeeded)
    if let Err(e) = queries::create_revision(
        &state.db,
        page.id,
        &page.content,
        &page.content_hash,
        &page.title,
        user.id,
    )
    .await
    {
        error!(
            "Revision snapshot failed after restore for page {} (concurrent edit?): {}",
            page.id, e
        );
    }

    // Prune old revisions
    let max_revisions = queries::get_effective_revision_limit(
        &state.db,
        guild_id,
        state.config.max_revisions_per_page,
    )
    .await
    .unwrap_or_else(|e| {
        error!(
            "Failed to get effective revision limit, using instance default: {}",
            e
        );
        state.config.max_revisions_per_page
    });

    if let Err(e) = queries::prune_revisions(&state.db, page.id, max_revisions).await {
        error!("Failed to prune revisions for page {}: {}", page.id, e);
    }

    // Audit log
    if let Err(e) = queries::log_audit(
        &state.db,
        page_id,
        "restore",
        user.id,
        Some(&old_page.content_hash),
        None,
        None,
    )
    .await
    {
        error!("Failed to log audit for page {}: {}", page_id, e);
    }

    Ok(Json(page))
}

/// List revisions for a platform page.
#[utoipa::path(
    get,
    path = "/api/pages/{page_id}/revisions",
    tag = "pages",
    params(("page_id" = Uuid, Path, description = "Page ID")),
    responses((status = 200, description = "List of revisions", body = Vec<RevisionListItem>)),
    security(("bearer_auth" = []))
)]
pub async fn list_platform_page_revisions(
    State(state): State<AppState>,
    user: AuthUser,
    Path(page_id): Path<Uuid>,
) -> Result<Json<Vec<RevisionListItem>>, PagesError> {
    // Revision history is admin-only (may contain redacted content)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        PagesError::Internal("Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err(PagesError::AdminRequired);
    }

    let page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if page.guild_id.is_some() {
        return Err(PagesError::NotPlatformPage);
    }

    let revisions = queries::list_revisions(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to list revisions for page {}: {}", page_id, e);
            PagesError::Database(e)
        })?;

    Ok(Json(revisions))
}

/// Get a specific revision of a platform page.
#[utoipa::path(
    get,
    path = "/api/pages/{page_id}/revisions/{n}",
    tag = "pages",
    params(
        ("page_id" = Uuid, Path, description = "Page ID"),
        ("n" = i32, Path, description = "Revision number")
    ),
    responses((status = 200, description = "Revision content", body = PageRevision)),
    security(("bearer_auth" = []))
)]
pub async fn get_platform_page_revision(
    State(state): State<AppState>,
    user: AuthUser,
    Path((page_id, n)): Path<(Uuid, i32)>,
) -> Result<Json<PageRevision>, PagesError> {
    // Revision history is admin-only (may contain redacted content)
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        PagesError::Internal("Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err(PagesError::AdminRequired);
    }

    let page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if page.guild_id.is_some() {
        return Err(PagesError::NotPlatformPage);
    }

    queries::get_revision(&state.db, page_id, n)
        .await
        .map_err(|e| {
            error!("Failed to get revision {} for page {}: {}", n, page_id, e);
            PagesError::Database(e)
        })?
        .map(Json)
        .ok_or(PagesError::RevisionNotFound)
}

/// Restore a platform page to a previous revision (system admin only).
#[utoipa::path(
    post,
    path = "/api/pages/{page_id}/revisions/{n}/restore",
    tag = "pages",
    params(
        ("page_id" = Uuid, Path, description = "Page ID"),
        ("n" = i32, Path, description = "Revision number")
    ),
    responses((status = 200, description = "Page restored", body = Page)),
    security(("bearer_auth" = []))
)]
pub async fn restore_platform_page_revision(
    State(state): State<AppState>,
    user: AuthUser,
    Path((page_id, n)): Path<(Uuid, i32)>,
) -> Result<Json<Page>, PagesError> {
    let is_admin = is_system_admin(&state.db, user.id).await.map_err(|e| {
        error!("Permission check failed: {}", e);
        PagesError::Internal("Permission check failed".to_string())
    })?;
    if !is_admin {
        return Err(PagesError::AdminRequired);
    }

    let old_page = queries::get_page_by_id(&state.db, page_id)
        .await
        .map_err(|e| {
            error!("Failed to get page {}: {}", page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::NotFound)?;

    if old_page.guild_id.is_some() {
        return Err(PagesError::NotPlatformPage);
    }

    let revision = queries::get_revision(&state.db, page_id, n)
        .await
        .map_err(|e| {
            error!("Failed to get revision {} for page {}: {}", n, page_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::RevisionNotFound)?;

    let content = revision.content.as_deref().ok_or_else(|| {
        error!(
            "Revision {} for page {} has NULL content",
            n, revision.page_id
        );
        PagesError::CorruptRevision("Revision data is corrupted (missing content)".to_string())
    })?;
    let title = revision.title.as_deref().unwrap_or(&old_page.title);

    let page = queries::update_page(
        &state.db,
        queries::UpdatePageParams {
            id: page_id,
            title: Some(title),
            slug: None,
            content: Some(content),
            requires_acceptance: None,
            category_id: None,
            updated_by: user.id,
        },
    )
    .await
    .map_err(|e| {
        error!(
            "Failed to restore page {} to revision {}: {}",
            page_id, n, e
        );
        PagesError::Database(e)
    })?;

    // Best-effort — concurrent operations may collide on the unique constraint;
    // the restore itself already succeeded
    if let Err(e) = queries::create_revision(
        &state.db,
        page.id,
        &page.content,
        &page.content_hash,
        &page.title,
        user.id,
    )
    .await
    {
        error!(
            "Revision snapshot failed after restore for page {} (concurrent edit?): {}",
            page.id, e
        );
    }

    if let Err(e) =
        queries::prune_revisions(&state.db, page.id, state.config.max_revisions_per_page).await
    {
        error!("Failed to prune revisions for page {}: {}", page.id, e);
    }

    if let Err(e) = queries::log_audit(
        &state.db,
        page_id,
        "restore",
        user.id,
        Some(&old_page.content_hash),
        None,
        None,
    )
    .await
    {
        error!("Failed to log audit for page {}: {}", page_id, e);
    }

    Ok(Json(page))
}

// ============================================================================
// Categories
// ============================================================================

/// List page categories for a guild.
///
/// Note: Does not check guild membership — consistent with `list_guild_pages`.
/// Categories are metadata for browsing and are intentionally readable by any
/// authenticated user who has the guild ID. The `require_auth` middleware
/// ensures authentication.
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/page-categories",
    tag = "pages",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, description = "List of categories", body = Vec<PageCategory>)),
    security(("bearer_auth" = []))
)]
pub async fn list_guild_categories(
    State(state): State<AppState>,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<PageCategory>>, PagesError> {
    let categories = queries::list_categories(&state.db, guild_id)
        .await
        .map_err(|e| {
            error!("Failed to list categories for guild {}: {}", guild_id, e);
            PagesError::Database(e)
        })?;
    Ok(Json(categories))
}

/// Create a page category for a guild.
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/page-categories",
    tag = "pages",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = CreateCategoryRequest,
    responses((status = 200, description = "Category created", body = PageCategory)),
    security(("bearer_auth" = []))
)]
pub async fn create_guild_category(
    State(state): State<AppState>,
    user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(req): Json<CreateCategoryRequest>,
) -> Result<Json<PageCategory>, PagesError> {
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    let name = req.name.trim();
    if name.is_empty() {
        return Err(PagesError::Validation(
            "Category name is required".to_string(),
        ));
    }
    if name.chars().count() > MAX_CATEGORY_NAME_LENGTH {
        return Err(PagesError::Validation(format!(
            "Category name exceeds {MAX_CATEGORY_NAME_LENGTH} characters"
        )));
    }

    let count = queries::count_categories(&state.db, guild_id)
        .await
        .unwrap_or_else(|e| {
            error!("Category count check failed, assuming at limit: {}", e);
            MAX_CATEGORIES_PER_GUILD
        });
    if count >= MAX_CATEGORIES_PER_GUILD {
        return Err(PagesError::LimitExceeded(format!(
            "Maximum {MAX_CATEGORIES_PER_GUILD} categories reached"
        )));
    }

    let category = queries::create_category(&state.db, guild_id, name)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("idx_page_categories_guild_name") {
                PagesError::CategoryConflict("Category name already exists".to_string())
            } else {
                error!("Failed to create category in guild {}: {}", guild_id, e);
                PagesError::Database(e)
            }
        })?;

    Ok(Json(category))
}

/// Update a page category name.
#[utoipa::path(
    patch,
    path = "/api/guilds/{id}/page-categories/{cat_id}",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("cat_id" = Uuid, Path, description = "Category ID")
    ),
    request_body = UpdateCategoryRequest,
    responses((status = 200, description = "Category updated", body = PageCategory)),
    security(("bearer_auth" = []))
)]
pub async fn update_guild_category(
    State(state): State<AppState>,
    user: AuthUser,
    Path((guild_id, cat_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateCategoryRequest>,
) -> Result<Json<PageCategory>, PagesError> {
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    // Verify category belongs to guild
    let existing = queries::get_category(&state.db, cat_id)
        .await
        .map_err(|e| {
            error!("Failed to get category {}: {}", cat_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::CategoryNotFound)?;

    if existing.guild_id != guild_id {
        return Err(PagesError::CategoryNotFound);
    }

    let name = req.name.trim();
    if name.is_empty() {
        return Err(PagesError::Validation(
            "Category name is required".to_string(),
        ));
    }
    if name.chars().count() > MAX_CATEGORY_NAME_LENGTH {
        return Err(PagesError::Validation(format!(
            "Category name exceeds {MAX_CATEGORY_NAME_LENGTH} characters"
        )));
    }

    let category = queries::update_category(&state.db, cat_id, name)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("idx_page_categories_guild_name") {
                PagesError::CategoryConflict("Category name already exists".to_string())
            } else {
                error!("Failed to update category {}: {}", cat_id, e);
                PagesError::Database(e)
            }
        })?;

    Ok(Json(category))
}

/// Delete a page category.
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}/page-categories/{cat_id}",
    tag = "pages",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("cat_id" = Uuid, Path, description = "Category ID")
    ),
    responses((status = 204, description = "Category deleted")),
    security(("bearer_auth" = []))
)]
pub async fn delete_guild_category(
    State(state): State<AppState>,
    user: AuthUser,
    Path((guild_id, cat_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, PagesError> {
    check_manage_pages_permission(&state, guild_id, user.id).await?;

    let existing = queries::get_category(&state.db, cat_id)
        .await
        .map_err(|e| {
            error!("Failed to get category {}: {}", cat_id, e);
            PagesError::Database(e)
        })?
        .ok_or(PagesError::CategoryNotFound)?;

    if existing.guild_id != guild_id {
        return Err(PagesError::CategoryNotFound);
    }

    queries::delete_category(&state.db, cat_id)
        .await
        .map_err(|e| {
            error!("Failed to delete category {}: {}", cat_id, e);
            PagesError::Database(e)
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// Reorder page categories for a guild.
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/page-categories/reorder",
    tag = "pages",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = ReorderCategoriesRequest,
    responses((status = 204, description = "Categories reordered")),
    security(("bearer_auth" = []))
)]
pub async fn reorder_guild_categories(
    State(state): State<AppState>,
    user: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(req): Json<ReorderCategoriesRequest>,
) -> Result<StatusCode, PagesError> {
    if req.category_ids.len() > MAX_CATEGORIES_PER_GUILD as usize {
        return Err(PagesError::Validation("Too many category IDs".to_string()));
    }

    check_manage_pages_permission(&state, guild_id, user.id).await?;

    queries::reorder_categories(&state.db, guild_id, &req.category_ids)
        .await
        .map_err(|e| {
            // Map Protocol errors (invalid input) to 400, others to 500
            if let sqlx::Error::Protocol(msg) = &e {
                error!("Invalid reorder request for guild {}: {}", guild_id, msg);
                PagesError::InvalidReorder(msg.clone())
            } else {
                error!("Failed to reorder categories in guild {}: {}", guild_id, e);
                PagesError::Database(e)
            }
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Validation Helpers
// ============================================================================

fn validate_create_request(req: &CreatePageRequest) -> Result<(), PagesError> {
    if req.title.is_empty() {
        return Err(PagesError::Validation("Title is required".to_string()));
    }
    if req.title.chars().count() > MAX_TITLE_LENGTH {
        return Err(PagesError::Validation(format!(
            "Title exceeds {MAX_TITLE_LENGTH} characters"
        )));
    }
    if req.content.is_empty() {
        return Err(PagesError::Validation("Content is required".to_string()));
    }
    if req.content.len() > MAX_CONTENT_SIZE {
        return Err(PagesError::Validation(format!(
            "Content exceeds {MAX_CONTENT_SIZE} bytes"
        )));
    }
    Ok(())
}

fn validate_update_request(req: &UpdatePageRequest) -> Result<(), PagesError> {
    if let Some(ref title) = req.title {
        if title.is_empty() {
            return Err(PagesError::Validation(
                "Title cannot be empty".to_string(),
            ));
        }
        if title.chars().count() > MAX_TITLE_LENGTH {
            return Err(PagesError::Validation(format!(
                "Title exceeds {MAX_TITLE_LENGTH} characters"
            )));
        }
    }
    if let Some(ref content) = req.content {
        if content.is_empty() {
            return Err(PagesError::Validation(
                "Content cannot be empty".to_string(),
            ));
        }
        if content.len() > MAX_CONTENT_SIZE {
            return Err(PagesError::Validation(format!(
                "Content exceeds {MAX_CONTENT_SIZE} bytes"
            )));
        }
    }
    Ok(())
}

fn validate_slug(slug: &str) -> Result<(), PagesError> {
    if slug.is_empty() {
        return Err(PagesError::Validation("Slug cannot be empty".to_string()));
    }
    if slug.len() > MAX_SLUG_LENGTH {
        return Err(PagesError::Validation(format!(
            "Slug exceeds {MAX_SLUG_LENGTH} characters"
        )));
    }
    if queries::is_reserved_slug(slug) {
        return Err(PagesError::Validation(format!(
            "'{slug}' is a reserved slug"
        )));
    }
    // Validate slug format (lowercase alphanumeric with dashes)
    let valid = slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--");

    if !valid {
        return Err(PagesError::Validation(
            "Invalid slug format. Use lowercase letters, numbers, and single dashes (e.g., 'terms-of-service', 'faq-page')".to_string(),
        ));
    }
    Ok(())
}
