//! Platform-wide page handlers (system-admin only CRUD + acceptance).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use tracing::error;
use uuid::Uuid;

use super::error::PagesError;
use super::shared::{validate_create_request, validate_slug, validate_update_request};
use super::{queries, CreatePageRequest, Page, PageListItem, ReorderRequest, UpdatePageRequest};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::is_system_admin;

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
