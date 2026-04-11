//! Page category handlers (CRUD + reorder) for guild-scoped page categories.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use tracing::error;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;

use super::error::PagesError;
use super::shared::check_manage_pages_permission;
use super::{
    queries, CreateCategoryRequest, PageCategory, ReorderCategoriesRequest, UpdateCategoryRequest,
    MAX_CATEGORIES_PER_GUILD, MAX_CATEGORY_NAME_LENGTH,
};

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
