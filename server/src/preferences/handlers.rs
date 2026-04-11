//! HTTP handlers for preferences endpoints.

use axum::extract::State;
use axum::Json;
use chrono::Utc;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::ws::{broadcast_to_user, ServerEvent};

use super::error::PreferencesError;
use super::types::{PreferencesResponse, UpdatePreferencesRequest, UserPreferencesRow};
use super::validation::validate_preferences;

/// GET /api/me/preferences
/// Returns the current user's preferences.
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
/// Updates the current user's preferences (full replacement).
#[utoipa::path(
    put,
    path = "/api/me/preferences",
    tag = "preferences",
    request_body = UpdatePreferencesRequest,
    responses(
        (status = 200, description = "Preferences updated", body = PreferencesResponse),
        (status = 400, description = "Validation error"),
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
