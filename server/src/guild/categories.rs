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

use super::queries::categories as queries;
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{require_guild_permission, GuildPermissions, PermissionError};

// ============================================================================
// Types
// ============================================================================

/// Category type restriction.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema,
)]
#[sqlx(type_name = "category_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum CategoryType {
    Mixed,
    Text,
    Voice,
}

/// Category response model.
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
pub struct Category {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub name: String,
    pub position: i32,
    pub parent_id: Option<Uuid>,
    pub category_type: CategoryType,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Request to create a new category.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateCategoryRequest {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[serde(default = "default_category_type")]
    pub category_type: CategoryType,
}

const fn default_category_type() -> CategoryType {
    CategoryType::Mixed
}

/// Request to update a category.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateCategoryRequest {
    pub name: Option<String>,
    pub position: Option<i32>,
    /// None = don't change, Some(None) = clear parent, Some(Some(id)) = set parent
    pub parent_id: Option<Option<Uuid>>,
    pub category_type: Option<CategoryType>,
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

    let categories = queries::list_categories(&state.db, guild_id).await?;

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
        match queries::fetch_category_parent(&state.db, parent_id, guild_id).await? {
            None => {
                return Err(CategoryError::Validation(
                    "Parent category not found".to_string(),
                ))
            }
            Some(Some(_)) => {
                return Err(CategoryError::Validation(
                    "Cannot nest more than 2 levels".to_string(),
                ))
            }
            Some(None) => {} // OK - parent is top-level
        }
    }

    // Insert with auto-position
    let category_id = Uuid::now_v7();
    let category = queries::insert_category(
        &state.db,
        category_id,
        guild_id,
        &body.name,
        body.parent_id,
        body.category_type,
    )
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
        match queries::fetch_category_parent(&state.db, *parent_id, guild_id).await? {
            None => {
                return Err(CategoryError::Validation(
                    "Parent category not found".to_string(),
                ))
            }
            Some(Some(_)) => {
                return Err(CategoryError::Validation(
                    "Cannot nest more than 2 levels".to_string(),
                ))
            }
            Some(None) => {} // OK
        }

        // Check that the category being updated doesn't have children
        // (can't make a parent category into a subcategory)
        if queries::has_subcategories(&state.db, category_id).await? {
            return Err(CategoryError::Validation(
                "Cannot make a category with subcategories into a subcategory".to_string(),
            ));
        }
    }

    // If changing category_type, validate no conflicting channels exist
    if let Some(new_type) = &body.category_type {
        if *new_type != CategoryType::Mixed {
            let conflicting_type = match new_type {
                CategoryType::Text => "voice",
                CategoryType::Voice => "text",
                CategoryType::Mixed => unreachable!(),
            };
            if queries::category_has_channel_type(&state.db, category_id, conflicting_type).await? {
                return Err(CategoryError::Validation(format!(
                    "Cannot change to {new_type:?} — category contains {conflicting_type} channels"
                )));
            }
        }
    }

    // Build and execute update query
    let category = queries::update_category(
        &state.db,
        category_id,
        guild_id,
        body.name.as_deref(),
        body.position,
        body.parent_id.is_some(),
        body.parent_id.flatten(),
        body.category_type,
    )
    .await?;

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
    let rows_affected = queries::delete_category(&state.db, category_id, guild_id).await?;

    if rows_affected == 0 {
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
            match queries::fetch_category_parent_tx(&mut tx, parent_id, guild_id).await? {
                None => {
                    return Err(CategoryError::Validation(format!(
                        "Parent category {parent_id} not found"
                    )))
                }
                Some(Some(_)) => {
                    return Err(CategoryError::Validation(
                        "Cannot nest more than 2 levels".to_string(),
                    ))
                }
                Some(None) => {} // OK
            }
        }

        queries::update_category_position(&mut tx, cat.id, guild_id, cat.position, cat.parent_id)
            .await?;
    }

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
