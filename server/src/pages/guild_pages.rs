//! Guild-scoped page handlers (CRUD + acceptance).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use tracing::error;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;

use super::error::PagesError;
use super::shared::{
    check_manage_pages_permission, validate_create_request, validate_slug, validate_update_request,
};
use super::{queries, CreatePageRequest, Page, PageListItem, ReorderRequest, UpdatePageRequest};

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
