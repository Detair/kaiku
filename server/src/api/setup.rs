//! Server Setup API Handlers
//!
//! Endpoints for the first-time setup wizard that configures the server.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use validator::Validate;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Error)]
pub enum SetupError {
    #[error("Server setup has already been completed")]
    SetupAlreadyComplete,

    #[error("Only system administrators can complete setup")]
    Unauthorized,

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for SetupError {
    fn into_response(self) -> Response {
        // Log database errors before converting to response
        if let Self::Database(ref err) = self {
            tracing::error!(
                error = %err,
                error_debug = ?err,
                "Setup endpoint returned database error"
            );
        }

        let (status, code) = match &self {
            Self::SetupAlreadyComplete => (StatusCode::FORBIDDEN, "SETUP_ALREADY_COMPLETE"),
            Self::Unauthorized => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            Self::Validation(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };

        let message = self.to_string();

        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}

// ============================================================================
// Request/Response Types
// ============================================================================

/// Response for GET /api/setup/status
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SetupStatusResponse {
    pub setup_complete: bool,
}

/// Response for GET /api/setup/config
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SetupConfigResponse {
    pub server_name: String,
    pub registration_policy: String,
    pub terms_url: Option<String>,
    pub privacy_url: Option<String>,
}

/// Request body for POST /api/setup/complete
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CompleteSetupRequest {
    #[validate(length(min = 1, max = 64, message = "Server name must be 1-64 characters"))]
    pub server_name: String,
    #[validate(custom(function = "validate_registration_policy"))]
    pub registration_policy: String,
    #[validate(url(message = "Terms URL must be a valid URL"))]
    pub terms_url: Option<String>,
    #[validate(url(message = "Privacy URL must be a valid URL"))]
    pub privacy_url: Option<String>,
}

fn validate_registration_policy(policy: &str) -> Result<(), validator::ValidationError> {
    if matches!(policy, "open" | "invite_only" | "closed") {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_policy"))
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// Check if server setup is complete.
/// GET /api/setup/status
#[utoipa::path(
    get,
    path = "/api/setup/status",
    tag = "setup",
    responses(
        (status = 200, description = "Setup status"),
    ),
)]
#[tracing::instrument(skip(state))]
pub async fn status(
    State(state): State<AppState>,
) -> Result<Json<SetupStatusResponse>, SetupError> {
    let setup_complete = db::is_setup_complete(&state.db).await?;

    Ok(Json(SetupStatusResponse { setup_complete }))
}

/// Get current server configuration (only if setup is incomplete).
/// GET /api/setup/config
#[utoipa::path(
    get,
    path = "/api/setup/config",
    tag = "setup",
    responses(
        (status = 200, description = "Setup configuration"),
    ),
)]
#[tracing::instrument(skip(state))]
pub async fn get_config(
    State(state): State<AppState>,
) -> Result<Json<SetupConfigResponse>, SetupError> {
    // Only allow fetching config if setup is incomplete
    if db::is_setup_complete(&state.db).await.map_err(|e| {
        tracing::error!(
            error = %e,
            operation = "is_setup_complete",
            "Database query failed in get_config handler"
        );
        SetupError::Database(e)
    })? {
        return Err(SetupError::SetupAlreadyComplete);
    }

    // Get server_name (must be string)
    let server_name_value = db::get_config_value(&state.db, "server_name")
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                operation = "get_config_value",
                key = "server_name",
                "Database query failed in get_config handler"
            );
            SetupError::Database(e)
        })?;
    let server_name = server_name_value
        .as_str()
        .ok_or_else(|| {
            tracing::error!(
                key = "server_name",
                actual_value = ?server_name_value,
                "Config value has wrong type (expected string)"
            );
            SetupError::Validation("Invalid server_name type in database".to_string())
        })?
        .to_string();

    // Get registration_policy (must be string)
    let policy_value = db::get_config_value(&state.db, "registration_policy")
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                operation = "get_config_value",
                key = "registration_policy",
                "Database query failed in get_config handler"
            );
            SetupError::Database(e)
        })?;
    let registration_policy = policy_value
        .as_str()
        .ok_or_else(|| {
            tracing::error!(
                key = "registration_policy",
                actual_value = ?policy_value,
                "Config value has wrong type (expected string)"
            );
            SetupError::Validation("Invalid registration_policy type in database".to_string())
        })?
        .to_string();

    // Get terms_url (optional string or null)
    let terms_value = db::get_config_value(&state.db, "terms_url")
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                operation = "get_config_value",
                key = "terms_url",
                "Database query failed in get_config handler"
            );
            SetupError::Database(e)
        })?;
    let terms_url = if terms_value.is_null() {
        None
    } else {
        Some(
            terms_value
                .as_str()
                .ok_or_else(|| {
                    tracing::error!(
                        key = "terms_url",
                        actual_value = ?terms_value,
                        "Config value has wrong type (expected string or null)"
                    );
                    SetupError::Validation("Invalid terms_url type in database".to_string())
                })?
                .to_string(),
        )
    };

    // Get privacy_url (optional string or null)
    let privacy_value = db::get_config_value(&state.db, "privacy_url")
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                operation = "get_config_value",
                key = "privacy_url",
                "Database query failed in get_config handler"
            );
            SetupError::Database(e)
        })?;
    let privacy_url = if privacy_value.is_null() {
        None
    } else {
        Some(
            privacy_value
                .as_str()
                .ok_or_else(|| {
                    tracing::error!(
                        key = "privacy_url",
                        actual_value = ?privacy_value,
                        "Config value has wrong type (expected string or null)"
                    );
                    SetupError::Validation("Invalid privacy_url type in database".to_string())
                })?
                .to_string(),
        )
    };

    Ok(Json(SetupConfigResponse {
        server_name,
        registration_policy,
        terms_url,
        privacy_url,
    }))
}

/// Complete server setup (only if admin and setup is incomplete).
/// POST /api/setup/complete
#[utoipa::path(
    post,
    path = "/api/setup/complete",
    tag = "setup",
    request_body = CompleteSetupRequest,
    responses(
        (status = 204, description = "Setup completed"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn complete(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CompleteSetupRequest>,
) -> Result<StatusCode, SetupError> {
    // Validate input
    body.validate()
        .map_err(|e| SetupError::Validation(e.to_string()))?;

    // Use transaction for atomic setup completion
    let mut tx = state.db.begin().await.map_err(|e| {
        tracing::error!(error = %e, "Failed to start setup completion transaction");
        SetupError::Database(e)
    })?;

    // Verify user is a system admin (inside transaction to prevent TOCTOU race
    // where admin status could be revoked between check and setup completion)
    let is_admin = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM system_admins WHERE user_id = $1) as "exists!""#,
        auth.id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(
            error = %e,
            user_id = %auth.id,
            "Failed to verify admin status during setup completion"
        );
        SetupError::Database(e)
    })?;

    if !is_admin {
        return Err(SetupError::Unauthorized);
    }

    // Atomically check and mark setup as complete using compare-and-swap pattern.
    // This prevents TOCTOU (Time-Of-Check-Time-Of-Use) race where two concurrent
    // admins both see setup_complete=false and both attempt to complete setup.
    // Only ONE transaction will update the row (WHERE value = 'false') - the other
    // will see updated=None and return SetupAlreadyComplete error.
    // This is a critical security mechanism preventing duplicate setup completion.
    let updated = sqlx::query!(
        r#"UPDATE server_config
           SET value = 'true'::jsonb, updated_by = $1, updated_at = NOW()
           WHERE key = 'setup_complete' AND value = 'false'::jsonb
           RETURNING key"#,
        auth.id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to atomically check and update setup_complete");
        SetupError::Database(e)
    })?;

    // If no row was updated, setup was already complete
    if updated.is_none() {
        tracing::warn!(
            admin_id = %auth.id,
            "Attempted to complete setup but it was already marked complete"
        );
        return Err(SetupError::SetupAlreadyComplete);
    }

    // Update server configuration within transaction
    sqlx::query(
        r"UPDATE server_config
           SET value = $2, updated_by = $3, updated_at = NOW()
           WHERE key = $1",
    )
    .bind("server_name")
    .bind(serde_json::json!(body.server_name))
    .bind(auth.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to update server_name during setup");
        SetupError::Database(e)
    })?;

    sqlx::query(
        r"UPDATE server_config
           SET value = $2, updated_by = $3, updated_at = NOW()
           WHERE key = $1",
    )
    .bind("registration_policy")
    .bind(serde_json::json!(body.registration_policy))
    .bind(auth.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to update registration_policy during setup");
        SetupError::Database(e)
    })?;

    sqlx::query(
        r"UPDATE server_config
           SET value = $2, updated_by = $3, updated_at = NOW()
           WHERE key = $1",
    )
    .bind("terms_url")
    .bind(
        body.terms_url
            .as_ref()
            .map(|s| serde_json::json!(s))
            .unwrap_or(serde_json::Value::Null),
    )
    .bind(auth.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to update terms_url during setup");
        SetupError::Database(e)
    })?;

    sqlx::query(
        r"UPDATE server_config
           SET value = $2, updated_by = $3, updated_at = NOW()
           WHERE key = $1",
    )
    .bind("privacy_url")
    .bind(
        body.privacy_url
            .as_ref()
            .map(|s| serde_json::json!(s))
            .unwrap_or(serde_json::Value::Null),
    )
    .bind(auth.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to update privacy_url during setup");
        SetupError::Database(e)
    })?;

    // Commit transaction
    tx.commit().await.map_err(|e| {
        tracing::error!(
            error = %e,
            admin_id = %auth.id,
            "Failed to commit setup completion transaction"
        );
        SetupError::Database(e)
    })?;

    tracing::info!(
        admin_id = %auth.id,
        server_name = %body.server_name,
        registration_policy = %body.registration_policy,
        "Server setup completed successfully"
    );

    Ok(StatusCode::NO_CONTENT)
}
