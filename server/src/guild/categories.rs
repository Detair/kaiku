//! Channel Category API Handlers
//!
//! CRUD operations for guild channel categories with support for 2-level nesting.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use thiserror::Error;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{require_guild_permission, GuildPermissions, PermissionError};

// ============================================================================
// Types
// ============================================================================

/// Category response model.
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct Category {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub position: i32,
    pub parent_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Request to create a new category.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateCategoryRequest {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
}

/// Request to update a category.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateCategoryRequest {
    pub name: Option<String>,
    pub position: Option<i32>,
    /// None = don't change, Some(None) = clear parent, Some(Some(id)) = set parent
    pub parent_id: Option<Option<Uuid>>,
}

/// Request to reorder multiple categories.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReorderRequest {
    pub categories: Vec<CategoryPosition>,
}

/// Position specification for a category.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CategoryPosition {
    pub id: Uuid,
    pub position: i32,
    pub parent_id: Option<Uuid>,
}

// ============================================================================
// Error Type
// ============================================================================

#[derive(Debug, Error)]
pub enum CategoryError {
    #[error("Category not found")]
    NotFound,

    #[error("Not a member of this guild")]
    NotMember,

    #[error("{0}")]
    Permission(#[from] PermissionError),

    #[error("Validation failed: {0}")]
    Validation(String),

    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for CategoryError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "not_found", "message": "Category not found"}),
            ),
            Self::NotMember => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "not_member", "message": "Not a member of this guild"}),
            ),
            Self::Permission(e) => {
                let body = match e {
                    PermissionError::MissingPermission(p) => serde_json::json!({
                        "error": "missing_permission",
                        "required": format!("{:?}", p),
                        "message": e.to_string()
                    }),
                    PermissionError::NotGuildMember => serde_json::json!({
                        "error": "not_member",
                        "message": e.to_string()
                    }),
                    _ => serde_json::json!({
                        "error": "permission",
                        "message": e.to_string()
                    }),
                };
                (StatusCode::FORBIDDEN, body)
            }
            Self::Validation(msg) => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "validation", "message": msg}),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "database", "message": "Database error"}),
            ),
        };
        (status, Json(body)).into_response()
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// List all categories in a guild.
///
/// `GET /api/guilds/:guild_id/categories`
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/categories",
    tag = "categories",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, description = "List of categories")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_categories(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<Category>>, CategoryError> {
    // Verify user is member of guild (no specific permission required to view)
    let _ctx = require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::empty())
        .await
        .map_err(|e| match e {
            PermissionError::NotGuildMember => CategoryError::NotMember,
            other => CategoryError::Permission(other),
        })?;

    let categories = sqlx::query_as::<_, Category>(
        r"
        SELECT id, guild_id, name, position, parent_id, created_at
        FROM channel_categories
        WHERE guild_id = $1
        ORDER BY position
        ",
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(categories))
}

/// Create a new category.
///
/// `POST /api/guilds/:guild_id/categories`
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/categories",
    tag = "categories",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = CreateCategoryRequest,
    responses((status = 201, description = "Category created", body = Category)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state, body))]
pub async fn create_category(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<CreateCategoryRequest>,
) -> Result<(StatusCode, Json<Category>), CategoryError> {
    // Validate name length
    if body.name.is_empty() || body.name.len() > 64 {
        return Err(CategoryError::Validation(
            "Name must be 1-64 characters".to_string(),
        ));
    }

    // Check MANAGE_CHANNELS permission
    let _ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_CHANNELS,
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => CategoryError::NotMember,
        other => CategoryError::Permission(other),
    })?;

    // If parent_id specified, verify it's a top-level category (not a subcategory)
    if let Some(parent_id) = body.parent_id {
        let parent_check: Option<(Option<Uuid>,)> = sqlx::query_as(
            "SELECT parent_id FROM channel_categories WHERE id = $1 AND guild_id = $2",
        )
        .bind(parent_id)
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?;

        match parent_check {
            None => {
                return Err(CategoryError::Validation(
                    "Parent category not found".to_string(),
                ))
            }
            Some((Some(_),)) => {
                return Err(CategoryError::Validation(
                    "Cannot nest more than 2 levels".to_string(),
                ))
            }
            Some((None,)) => {} // OK - parent is top-level
        }
    }

    // Insert with auto-position
    let category_id = Uuid::now_v7();
    let category = sqlx::query_as::<_, Category>(
        r"
        INSERT INTO channel_categories (id, guild_id, name, parent_id, position)
        VALUES ($1, $2, $3, $4, (
            SELECT COALESCE(MAX(position) + 1, 0)
            FROM channel_categories
            WHERE guild_id = $2 AND parent_id IS NOT DISTINCT FROM $4
        ))
        RETURNING id, guild_id, name, position, parent_id, created_at
        ",
    )
    .bind(category_id)
    .bind(guild_id)
    .bind(&body.name)
    .bind(body.parent_id)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(category)))
}

/// Update a category.
///
/// `PATCH /api/guilds/:guild_id/categories/:category_id`
#[utoipa::path(
    patch,
    path = "/api/guilds/{id}/categories/{category_id}",
    tag = "categories",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("category_id" = Uuid, Path, description = "Category ID")
    ),
    request_body = UpdateCategoryRequest,
    responses((status = 200, description = "Category updated", body = Category)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state, body))]
pub async fn update_category(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, category_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateCategoryRequest>,
) -> Result<Json<Category>, CategoryError> {
    // Check MANAGE_CHANNELS permission
    let _ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_CHANNELS,
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => CategoryError::NotMember,
        other => CategoryError::Permission(other),
    })?;

    // Validate name if provided
    if let Some(ref name) = body.name {
        if name.is_empty() || name.len() > 64 {
            return Err(CategoryError::Validation(
                "Name must be 1-64 characters".to_string(),
            ));
        }
    }

    // If changing parent_id, validate nesting constraint
    if let Some(Some(parent_id)) = &body.parent_id {
        // Check that the new parent exists and is top-level
        let parent_check: Option<(Option<Uuid>,)> = sqlx::query_as(
            "SELECT parent_id FROM channel_categories WHERE id = $1 AND guild_id = $2",
        )
        .bind(parent_id)
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?;

        match parent_check {
            None => {
                return Err(CategoryError::Validation(
                    "Parent category not found".to_string(),
                ))
            }
            Some((Some(_),)) => {
                return Err(CategoryError::Validation(
                    "Cannot nest more than 2 levels".to_string(),
                ))
            }
            Some((None,)) => {} // OK
        }

        // Check that the category being updated doesn't have children
        // (can't make a parent category into a subcategory)
        let has_children = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM channel_categories WHERE parent_id = $1)",
        )
        .bind(category_id)
        .fetch_one(&state.db)
        .await?;

        if has_children {
            return Err(CategoryError::Validation(
                "Cannot make a category with subcategories into a subcategory".to_string(),
            ));
        }
    }

    // Build and execute update query
    let category = sqlx::query_as::<_, Category>(
        r"
        UPDATE channel_categories
        SET
            name = COALESCE($3, name),
            position = COALESCE($4, position),
            parent_id = CASE WHEN $5 THEN $6 ELSE parent_id END
        WHERE id = $1 AND guild_id = $2
        RETURNING id, guild_id, name, position, parent_id, created_at
        ",
    )
    .bind(category_id)
    .bind(guild_id)
    .bind(&body.name)
    .bind(body.position)
    .bind(body.parent_id.is_some()) // whether to update parent_id
    .bind(body.parent_id.flatten()) // the new parent_id value
    .fetch_optional(&state.db)
    .await?
    .ok_or(CategoryError::NotFound)?;

    Ok(Json(category))
}

/// Delete a category.
///
/// `DELETE /api/guilds/:guild_id/categories/:category_id`
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}/categories/{category_id}",
    tag = "categories",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("category_id" = Uuid, Path, description = "Category ID")
    ),
    responses((status = 204, description = "Category deleted")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn delete_category(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, category_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, CategoryError> {
    // Check MANAGE_CHANNELS permission
    let _ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_CHANNELS,
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => CategoryError::NotMember,
        other => CategoryError::Permission(other),
    })?;

    // Delete category (channels become uncategorized due to ON DELETE SET NULL)
    // Subcategories are deleted due to ON DELETE CASCADE on parent_id
    let result = sqlx::query("DELETE FROM channel_categories WHERE id = $1 AND guild_id = $2")
        .bind(category_id)
        .bind(guild_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(CategoryError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Reorder multiple categories.
///
/// `POST /api/guilds/:guild_id/categories/reorder`
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/categories/reorder",
    tag = "categories",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = ReorderRequest,
    responses((status = 204, description = "Categories reordered")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state, body))]
pub async fn reorder_categories(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<ReorderRequest>,
) -> Result<StatusCode, CategoryError> {
    // Check MANAGE_CHANNELS permission
    let _ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_CHANNELS,
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => CategoryError::NotMember,
        other => CategoryError::Permission(other),
    })?;

    if body.categories.is_empty() {
        return Ok(StatusCode::NO_CONTENT);
    }

    // Update positions in transaction
    let mut tx = state.db.begin().await?;

    for cat in &body.categories {
        // Validate nesting constraint if parent_id is set
        if let Some(parent_id) = cat.parent_id {
            let parent_check: Option<(Option<Uuid>,)> = sqlx::query_as(
                "SELECT parent_id FROM channel_categories WHERE id = $1 AND guild_id = $2",
            )
            .bind(parent_id)
            .bind(guild_id)
            .fetch_optional(&mut *tx)
            .await?;

            match parent_check {
                None => {
                    return Err(CategoryError::Validation(format!(
                        "Parent category {parent_id} not found"
                    )))
                }
                Some((Some(_),)) => {
                    return Err(CategoryError::Validation(
                        "Cannot nest more than 2 levels".to_string(),
                    ))
                }
                Some((None,)) => {} // OK
            }
        }

        sqlx::query(
            r"
            UPDATE channel_categories
            SET position = $3, parent_id = $4
            WHERE id = $1 AND guild_id = $2
            ",
        )
        .bind(cat.id)
        .bind(guild_id)
        .bind(cat.position)
        .bind(cat.parent_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
