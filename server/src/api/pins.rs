//! User Pins API
//!
//! CRUD operations for user's global pins (notes, links, pinned messages).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PinType {
    Note,
    Link,
    Message,
}

impl PinType {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Link => "link",
            Self::Message => "message",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "note" => Some(Self::Note),
            "link" => Some(Self::Link),
            "message" => Some(Self::Message),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize, FromRow)]
pub struct PinRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub pin_type: String,
    pub content: String,
    pub title: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub position: i32,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Pin {
    pub id: Uuid,
    pub pin_type: PinType,
    pub content: String,
    pub title: Option<String>,
    #[schema(value_type = Object)]
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub position: i32,
}

impl From<PinRow> for Pin {
    fn from(row: PinRow) -> Self {
        Self {
            id: row.id,
            pin_type: PinType::from_str(&row.pin_type).unwrap_or(PinType::Note),
            content: row.content,
            title: row.title,
            metadata: row.metadata,
            created_at: row.created_at,
            position: row.position,
        }
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreatePinRequest {
    pub pin_type: PinType,
    pub content: String,
    pub title: Option<String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdatePinRequest {
    pub content: Option<String>,
    pub title: Option<String>,
    #[schema(value_type = Option<Object>)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReorderPinsRequest {
    pub pin_ids: Vec<Uuid>,
}

// ============================================================================
// Constants
// ============================================================================

const MAX_PINS_PER_USER: i64 = 50;
const MAX_CONTENT_LENGTH: usize = 2000;
const MAX_TITLE_LENGTH: usize = 255;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum PinsError {
    #[error("Pin not found")]
    NotFound,
    #[error("Maximum pins limit reached (50)")]
    LimitExceeded,
    #[error("Content exceeds maximum length")]
    ContentTooLong,
    #[error("Title exceeds maximum length")]
    TitleTooLong,
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for PinsError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match &self {
            Self::NotFound => (StatusCode::NOT_FOUND, "PIN_NOT_FOUND", "Pin not found"),
            Self::LimitExceeded => (
                StatusCode::BAD_REQUEST,
                "LIMIT_EXCEEDED",
                "Maximum pins limit reached (50)",
            ),
            Self::ContentTooLong => (
                StatusCode::BAD_REQUEST,
                "CONTENT_TOO_LONG",
                "Content exceeds maximum length",
            ),
            Self::TitleTooLong => (
                StatusCode::BAD_REQUEST,
                "TITLE_TOO_LONG",
                "Title exceeds maximum length",
            ),
            Self::Database(err) => {
                tracing::error!("Database error: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error",
                )
            }
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/me/pins - List user's pins
#[utoipa::path(
    get,
    path = "/api/me/pins",
    tag = "pins",
    responses(
        (status = 200, description = "List of pins"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn list_pins(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<Vec<Pin>>, PinsError> {
    let rows = sqlx::query_as::<_, PinRow>(
        r"
        SELECT id, user_id, pin_type, content, title, metadata, created_at, position
        FROM user_pins
        WHERE user_id = $1
        ORDER BY position ASC, created_at DESC
        ",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;

    let pins: Vec<Pin> = rows.into_iter().map(Pin::from).collect();
    Ok(Json(pins))
}

/// POST /api/me/pins - Create a new pin
#[utoipa::path(
    post,
    path = "/api/me/pins",
    tag = "pins",
    request_body = CreatePinRequest,
    responses(
        (status = 200, description = "Pin created", body = Pin),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn create_pin(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<CreatePinRequest>,
) -> Result<Json<Pin>, PinsError> {
    // Validate content length
    if request.content.len() > MAX_CONTENT_LENGTH {
        return Err(PinsError::ContentTooLong);
    }

    // Validate title length
    if let Some(ref title) = request.title {
        if title.len() > MAX_TITLE_LENGTH {
            return Err(PinsError::TitleTooLong);
        }
    }

    // Check pin count limit
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM user_pins WHERE user_id = $1")
        .bind(auth_user.id)
        .fetch_one(&state.db)
        .await?;

    if count.0 >= MAX_PINS_PER_USER {
        return Err(PinsError::LimitExceeded);
    }

    // Get next position
    let max_pos: (Option<i32>,) =
        sqlx::query_as("SELECT MAX(position) FROM user_pins WHERE user_id = $1")
            .bind(auth_user.id)
            .fetch_one(&state.db)
            .await?;

    let next_position = max_pos.0.map(|v| v + 1).unwrap_or(0);

    // Insert pin
    let row = sqlx::query_as::<_, PinRow>(
        r"
        INSERT INTO user_pins (user_id, pin_type, content, title, metadata, position)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, user_id, pin_type, content, title, metadata, created_at, position
        ",
    )
    .bind(auth_user.id)
    .bind(request.pin_type.as_str())
    .bind(&request.content)
    .bind(&request.title)
    .bind(&request.metadata)
    .bind(next_position)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(Pin::from(row)))
}

/// PUT /api/me/pins/:id - Update a pin
#[utoipa::path(
    put,
    path = "/api/me/pins/{id}",
    tag = "pins",
    params(
        ("id" = Uuid, Path, description = "Pin ID"),
    ),
    request_body = UpdatePinRequest,
    responses(
        (status = 200, description = "Pin updated"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn update_pin(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(pin_id): Path<Uuid>,
    Json(request): Json<UpdatePinRequest>,
) -> Result<Json<Pin>, PinsError> {
    // Validate content length if provided
    if let Some(ref content) = request.content {
        if content.len() > MAX_CONTENT_LENGTH {
            return Err(PinsError::ContentTooLong);
        }
    }

    // Validate title length if provided
    if let Some(ref title) = request.title {
        if title.len() > MAX_TITLE_LENGTH {
            return Err(PinsError::TitleTooLong);
        }
    }

    // Check pin exists and belongs to user
    let existing =
        sqlx::query_as::<_, PinRow>("SELECT * FROM user_pins WHERE id = $1 AND user_id = $2")
            .bind(pin_id)
            .bind(auth_user.id)
            .fetch_optional(&state.db)
            .await?;

    if existing.is_none() {
        return Err(PinsError::NotFound);
    }

    // Update pin
    let row = sqlx::query_as::<_, PinRow>(
        r"
        UPDATE user_pins
        SET content = COALESCE($3, content),
            title = COALESCE($4, title),
            metadata = COALESCE($5, metadata)
        WHERE id = $1 AND user_id = $2
        RETURNING id, user_id, pin_type, content, title, metadata, created_at, position
        ",
    )
    .bind(pin_id)
    .bind(auth_user.id)
    .bind(&request.content)
    .bind(&request.title)
    .bind(&request.metadata)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(Pin::from(row)))
}

/// DELETE /api/me/pins/:id - Delete a pin
#[utoipa::path(
    delete,
    path = "/api/me/pins/{id}",
    tag = "pins",
    params(
        ("id" = Uuid, Path, description = "Pin ID"),
    ),
    responses(
        (status = 204, description = "Pin deleted"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn delete_pin(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(pin_id): Path<Uuid>,
) -> Result<StatusCode, PinsError> {
    let result = sqlx::query("DELETE FROM user_pins WHERE id = $1 AND user_id = $2")
        .bind(pin_id)
        .bind(auth_user.id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(PinsError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// PUT /api/me/pins/reorder - Reorder pins
#[utoipa::path(
    put,
    path = "/api/me/pins/reorder",
    tag = "pins",
    request_body = ReorderPinsRequest,
    responses(
        (status = 204, description = "Pins reordered"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn reorder_pins(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<ReorderPinsRequest>,
) -> Result<StatusCode, PinsError> {
    // Start transaction for atomic reorder
    let mut tx = state.db.begin().await?;

    // Update positions based on order in request
    for (position, pin_id) in request.pin_ids.iter().enumerate() {
        // Verify pin belongs to user and update position
        let _result =
            sqlx::query("UPDATE user_pins SET position = $3 WHERE id = $1 AND user_id = $2")
                .bind(pin_id)
                .bind(auth_user.id)
                .bind(position as i32)
                .execute(&mut *tx)
                .await?;

        // Optional: fail if any pin is not found/owned?
        // Current behavior allows partial updates if we don't check, but transaction ensures all or
        // nothing for the DB. If a pin ID is invalid/not owned, rows_affected will be 0.
        // We might want to ensure we are reordering what we expect, but loose reorder is often
        // acceptable.
    }

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}
