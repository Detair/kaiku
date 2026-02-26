//! User Preferences API
//!
//! Endpoints for managing user preferences that sync across devices.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::ws::{broadcast_to_user, ServerEvent};

// ============================================================================
// Error Types
// ============================================================================

/// Error types for preferences operations.
#[derive(Debug, thiserror::Error)]
pub enum PreferencesError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Validation error: {0}")]
    Validation(String),
}

impl IntoResponse for PreferencesError {
    fn into_response(self) -> Response {
        use serde_json::json;

        let (status, code, message) = match &self {
            Self::Database(err) => {
                tracing::error!("Database error: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "Database error".to_string(),
                )
            }
            Self::Validation(msg) => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                msg.clone(),
            ),
        };

        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}

// ============================================================================
// Request/Response Types
// ============================================================================

/// Response for preferences endpoints
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PreferencesResponse {
    #[schema(value_type = Object)]
    pub preferences: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}

/// Request body for updating preferences
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdatePreferencesRequest {
    #[schema(value_type = Object)]
    pub preferences: serde_json::Value,
}

/// Database row for `user_preferences`
#[derive(Debug, sqlx::FromRow)]
pub struct UserPreferencesRow {
    pub user_id: Uuid,
    pub preferences: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}

// ============================================================================
// Router
// ============================================================================

/// Create the preferences router.
///
/// Routes:
/// - GET / - Get current user's preferences
/// - PUT / - Update current user's preferences (full replacement)
pub fn router() -> Router<AppState> {
    Router::new().route("/", get(get_preferences).put(update_preferences))
}

// ============================================================================
// Validation
// ============================================================================

/// Maximum total size of the preferences JSON payload (64 KB).
const MAX_PREFERENCES_SIZE: usize = 65_536;

/// Limits for the focus section of preferences.
const MAX_FOCUS_MODES: usize = 10;
const MAX_VIP_ENTRIES: usize = 50;
const MAX_KEYWORDS: usize = 5;
const MAX_STRING_LEN: usize = 100;
const MAX_MODE_NAME_LEN: usize = 30;

const VALID_SUPPRESSION_LEVELS: &[&str] = &["all", "except_mentions", "except_dms"];
const VALID_TRIGGER_CATEGORIES: &[&str] = &["game", "coding", "listening", "watching"];

/// Validate the preferences payload: total size limit and focus section structure.
fn validate_preferences(prefs: &serde_json::Value) -> Result<(), PreferencesError> {
    // Total size limit
    let serialized_len = serde_json::to_string(prefs).unwrap_or_default().len();
    if serialized_len > MAX_PREFERENCES_SIZE {
        return Err(PreferencesError::Validation(format!(
            "Preferences payload too large ({serialized_len} bytes, max {MAX_PREFERENCES_SIZE})"
        )));
    }

    // Validate focus section if present
    if let Some(focus) = prefs.get("focus") {
        validate_focus_preferences(focus)?;
    }

    Ok(())
}

fn validate_focus_preferences(focus: &serde_json::Value) -> Result<(), PreferencesError> {
    // modes array
    if let Some(modes) = focus.get("modes") {
        let modes = modes
            .as_array()
            .ok_or_else(|| PreferencesError::Validation("focus.modes must be an array".into()))?;

        if modes.len() > MAX_FOCUS_MODES {
            return Err(PreferencesError::Validation(format!(
                "Too many focus modes ({}, max {MAX_FOCUS_MODES})",
                modes.len()
            )));
        }

        for (i, mode) in modes.iter().enumerate() {
            validate_focus_mode(mode, i)?;
        }
    }

    Ok(())
}

fn validate_focus_mode(mode: &serde_json::Value, index: usize) -> Result<(), PreferencesError> {
    let ctx = |field: &str| format!("focus.modes[{index}].{field}");

    // Name length
    if let Some(name) = mode.get("name").and_then(|v| v.as_str()) {
        if name.len() > MAX_MODE_NAME_LEN {
            return Err(PreferencesError::Validation(format!(
                "{} too long ({}, max {MAX_MODE_NAME_LEN})",
                ctx("name"),
                name.len()
            )));
        }
    }

    // Suppression level must be a known value
    if let Some(level) = mode.get("suppressionLevel").and_then(|v| v.as_str()) {
        if !VALID_SUPPRESSION_LEVELS.contains(&level) {
            return Err(PreferencesError::Validation(format!(
                "{} invalid value: {level}",
                ctx("suppressionLevel")
            )));
        }
    }

    // Trigger categories
    if let Some(cats) = mode.get("triggerCategories") {
        if !cats.is_null() {
            let cats = cats.as_array().ok_or_else(|| {
                PreferencesError::Validation(format!(
                    "{} must be an array or null",
                    ctx("triggerCategories")
                ))
            })?;
            for cat in cats {
                let s = cat.as_str().ok_or_else(|| {
                    PreferencesError::Validation(format!(
                        "{} entries must be strings",
                        ctx("triggerCategories")
                    ))
                })?;
                if !VALID_TRIGGER_CATEGORIES.contains(&s) {
                    return Err(PreferencesError::Validation(format!(
                        "{} invalid category: {s}",
                        ctx("triggerCategories")
                    )));
                }
            }
        }
    }

    // VIP user IDs
    validate_string_array(mode, "vipUserIds", MAX_VIP_ENTRIES, MAX_STRING_LEN, &ctx("vipUserIds"))?;

    // VIP channel IDs
    validate_string_array(
        mode,
        "vipChannelIds",
        MAX_VIP_ENTRIES,
        MAX_STRING_LEN,
        &ctx("vipChannelIds"),
    )?;

    // Emergency keywords
    validate_string_array(
        mode,
        "emergencyKeywords",
        MAX_KEYWORDS,
        MAX_STRING_LEN,
        &ctx("emergencyKeywords"),
    )?;

    Ok(())
}

fn validate_string_array(
    obj: &serde_json::Value,
    field: &str,
    max_len: usize,
    max_str_len: usize,
    ctx: &str,
) -> Result<(), PreferencesError> {
    if let Some(arr) = obj.get(field) {
        let arr = arr
            .as_array()
            .ok_or_else(|| PreferencesError::Validation(format!("{ctx} must be an array")))?;

        if arr.len() > max_len {
            return Err(PreferencesError::Validation(format!(
                "{ctx} too many entries ({}, max {max_len})",
                arr.len()
            )));
        }

        for entry in arr {
            let s = entry
                .as_str()
                .ok_or_else(|| PreferencesError::Validation(format!("{ctx} entries must be strings")))?;
            if s.len() > max_str_len {
                return Err(PreferencesError::Validation(format!(
                    "{ctx} entry too long ({}, max {max_str_len})",
                    s.len()
                )));
            }
        }
    }
    Ok(())
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/me/preferences
/// Returns the current user's preferences
#[utoipa::path(
    get,
    path = "/api/me/preferences",
    tag = "preferences",
    responses(
        (status = 200, description = "User preferences"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id))]
pub async fn get_preferences(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<PreferencesResponse>, PreferencesError> {
    let row = sqlx::query_as::<_, UserPreferencesRow>(
        r"
        SELECT user_id, preferences, updated_at
        FROM user_preferences
        WHERE user_id = $1
        ",
    )
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?;

    match row {
        Some(row) => Ok(Json(PreferencesResponse {
            preferences: row.preferences,
            updated_at: row.updated_at,
        })),
        None => {
            // Return empty preferences with current timestamp for new users
            Ok(Json(PreferencesResponse {
                preferences: serde_json::json!({}),
                updated_at: Utc::now(),
            }))
        }
    }
}

/// PUT /api/me/preferences
/// Updates the current user's preferences (full replacement)
#[utoipa::path(
    put,
    path = "/api/me/preferences",
    tag = "preferences",
    request_body = UpdatePreferencesRequest,
    responses(
        (status = 200, description = "Preferences updated", body = PreferencesResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, request), fields(user_id = %auth_user.id))]
pub async fn update_preferences(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<UpdatePreferencesRequest>,
) -> Result<Json<PreferencesResponse>, PreferencesError> {
    validate_preferences(&request.preferences)?;

    let row = sqlx::query_as::<_, UserPreferencesRow>(
        r"
        INSERT INTO user_preferences (user_id, preferences, updated_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (user_id) DO UPDATE
        SET preferences = EXCLUDED.preferences,
            updated_at = NOW()
        RETURNING user_id, preferences, updated_at
        ",
    )
    .bind(auth_user.id)
    .bind(&request.preferences)
    .fetch_one(&state.db)
    .await?;

    // Broadcast to all user's devices via WebSocket
    let event = ServerEvent::PreferencesUpdated {
        preferences: row.preferences.clone(),
        updated_at: row.updated_at,
    };
    if let Err(e) = broadcast_to_user(&state.redis, auth_user.id, &event).await {
        tracing::warn!("Failed to broadcast preferences update: {}", e);
        // Don't fail the request if broadcast fails - the update was successful
    }

    Ok(Json(PreferencesResponse {
        preferences: row.preferences,
        updated_at: row.updated_at,
    }))
}
