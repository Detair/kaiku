//! Data Governance HTTP Handlers

use axum::extract::State;
use axum::http::{HeaderName, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use chrono::{Duration, Utc};
use uuid::Uuid;

use super::error::GovError;
use super::types::{
    CancelDeletionResponse, DeleteAccountRequest, DeleteAccountResponse, ExportJobResponse,
};
use crate::api::AppState;
use crate::auth::{verify_password, AuthUser};
use crate::db;

// ============================================================================
// Constants
// ============================================================================

/// Threshold for considering an export job as stale (abandoned by crash/restart).
const STALE_EXPORT_JOB_THRESHOLD: Duration = Duration::hours(1);

// ============================================================================
// Data Export Handlers
// ============================================================================

/// Request a data export.
///
/// Creates a background job to gather all user data into a downloadable archive.
/// Only one pending/processing export per user is allowed.
#[utoipa::path(
    post,
    path = "/api/me/data-export",
    responses(
        (status = 201, description = "Export job created", body = ExportJobResponse),
        (status = 409, description = "Export already in progress"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn request_export(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, GovError> {
    // Recover stale jobs that may have been left behind by crash/restart so
    // users are not blocked forever by the active-job uniqueness constraint.
    // NOTE: Uses `created_at` for stale detection because `data_export_jobs` has no `updated_at` field.
    // The 1-hour threshold assumes exports complete well within this window.
    // If exports grow to exceed 1 hour, add an `updated_at`/heartbeat column.
    // users are not blocked forever by the active-job uniqueness constraint.
    sqlx::query(
        "UPDATE data_export_jobs
         SET status = 'failed',
             error_message = COALESCE(error_message, 'Job stale after restart; please retry'),
             completed_at = NOW()
         WHERE user_id = $1
           AND status IN ('pending', 'processing')
           AND created_at < NOW() - CAST($2 AS INTERVAL)",
    )
    .bind(auth.id)
    .bind(format!("{}h", STALE_EXPORT_JOB_THRESHOLD.num_hours()))
    .execute(&state.db)
    .await?;

    // Check for existing pending/processing export
    let existing = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM data_export_jobs
         WHERE user_id = $1 AND status IN ('pending', 'processing')
         LIMIT 1",
    )
    .bind(auth.id)
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        return Err(GovError::ExportAlreadyPending);
    }

    // Require S3 for export storage
    let s3 = state.s3.as_ref().ok_or(GovError::StorageNotConfigured)?;

    // Create export job (unique partial index prevents duplicates at DB level)
    let job = sqlx::query_as::<_, db::DataExportJob>(
        "INSERT INTO data_export_jobs (user_id) VALUES ($1) RETURNING *",
    )
    .bind(auth.id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.is_unique_violation() {
                return GovError::ExportAlreadyPending;
            }
        }
        GovError::Database(e)
    })?;

    // Spawn background export worker
    let pool = state.db.clone();
    let s3 = s3.clone();
    let email_service = state.email.clone();
    let job_id = job.id;
    let user_id = auth.id;

    tokio::spawn(async move {
        if let Err(e) =
            super::export::process_export_job(&pool, &s3, &email_service, job_id, user_id).await
        {
            tracing::error!(
                job_id = %job_id,
                user_id = %user_id,
                error = %e,
                "Export job failed"
            );
        }
    });

    let response = ExportJobResponse {
        id: job.id,
        status: job.status,
        file_size_bytes: job.file_size_bytes,
        expires_at: job.expires_at,
        error_message: job.error_message,
        created_at: job.created_at,
        completed_at: job.completed_at,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

/// Get the status of the most recent export job.
#[utoipa::path(
    get,
    path = "/api/me/data-export",
    responses(
        (status = 200, description = "Export job status", body = ExportJobResponse),
        (status = 404, description = "No export job found"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_export_status(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<ExportJobResponse>, GovError> {
    let job = sqlx::query_as::<_, db::DataExportJob>(
        "SELECT * FROM data_export_jobs
         WHERE user_id = $1
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(auth.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(GovError::ExportNotFound)?;

    Ok(Json(ExportJobResponse {
        id: job.id,
        status: job.status,
        file_size_bytes: job.file_size_bytes,
        expires_at: job.expires_at,
        error_message: job.error_message,
        created_at: job.created_at,
        completed_at: job.completed_at,
    }))
}

/// Download the completed export archive.
#[utoipa::path(
    get,
    path = "/api/me/data-export/download",
    responses(
        (status = 200, description = "Export archive (application/zip)"),
        (status = 404, description = "No completed export found"),
        (status = 410, description = "Export has expired"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn download_export(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, GovError> {
    let s3 = state.s3.as_ref().ok_or(GovError::StorageNotConfigured)?;

    let job = sqlx::query_as::<_, db::DataExportJob>(
        "SELECT * FROM data_export_jobs
         WHERE user_id = $1 AND status = 'completed'
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(auth.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(GovError::ExportNotFound)?;

    // Check expiry — if expired, return 410 GONE without changing DB state.
    // cleanup_expired_exports will handle S3 deletion and status update.
    if let Some(expires_at) = job.expires_at {
        if expires_at < Utc::now() {
            return Err(GovError::ExportExpired);
        }
    }

    let s3_key = job.s3_key.ok_or(GovError::ExportNotFound)?;

    // Stream the file from S3
    let stream = s3.get_object_stream(&s3_key).await.map_err(|e| {
        tracing::error!(error = %e, s3_key = %s3_key, "Failed to download export from S3");
        GovError::ExportNotFound
    })?;

    let body = axum::body::Body::new(stream.into_inner());
    let headers = [
        (
            axum::http::header::CONTENT_TYPE,
            "application/zip".to_string(),
        ),
        (
            axum::http::header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"data-export-{}.zip\"", auth.id),
        ),
        (
            HeaderName::from_static("x-content-type-options"),
            "nosniff".to_string(),
        ),
    ];

    Ok((headers, body))
}

// ============================================================================
// Account Deletion Handlers
// ============================================================================

/// Request account deletion with a 30-day grace period.
#[utoipa::path(
    post,
    path = "/api/me/delete-account",
    request_body = DeleteAccountRequest,
    responses(
        (status = 200, description = "Deletion scheduled", body = DeleteAccountResponse),
        (status = 400, description = "Invalid confirmation"),
        (status = 401, description = "Password verification failed"),
        (status = 409, description = "Deletion already scheduled or user owns guilds"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn request_deletion(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<DeleteAccountRequest>,
) -> Result<Json<DeleteAccountResponse>, GovError> {
    // Verify confirmation
    if body.confirm != "DELETE" {
        return Err(GovError::Validation(
            "Confirmation must be the string \"DELETE\"".to_string(),
        ));
    }

    // Fetch user for password verification
    let user = db::find_user_by_id(&state.db, auth.id)
        .await?
        .ok_or(GovError::Validation("User not found".to_string()))?;

    // Check if already scheduled
    if user.deletion_scheduled_at.is_some() {
        return Err(GovError::DeletionAlreadyScheduled);
    }

    // Verify password (local auth) or confirmation (OIDC)
    match user.auth_method {
        db::AuthMethod::Local => {
            let password = body
                .password
                .as_deref()
                .ok_or(GovError::Validation("Password is required".to_string()))?;
            let password_hash = user
                .password_hash
                .as_deref()
                .ok_or(GovError::PasswordInvalid)?;
            let valid =
                verify_password(password, password_hash).map_err(|_| GovError::PasswordInvalid)?;
            if !valid {
                return Err(GovError::PasswordInvalid);
            }
        }
        db::AuthMethod::Oidc => {
            // OIDC users don't have passwords — confirmation string is sufficient
        }
    }

    // Check guild ownership — user must transfer all guilds first
    let owned_guilds: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM guilds WHERE owner_id = $1")
            .bind(auth.id)
            .fetch_all(&state.db)
            .await?;

    if !owned_guilds.is_empty() {
        let guild_names: Vec<String> = owned_guilds.into_iter().map(|(_, name)| name).collect();
        return Err(GovError::OwnsGuilds(guild_names.join(", ")));
    }

    // Schedule deletion (30 days from now)
    let now = Utc::now();
    let scheduled_at = now + Duration::days(30);

    sqlx::query(
        "UPDATE users SET deletion_requested_at = $1, deletion_scheduled_at = $2 WHERE id = $3",
    )
    .bind(now)
    .bind(scheduled_at)
    .bind(auth.id)
    .execute(&state.db)
    .await?;

    // Invalidate all sessions (force logout everywhere)
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(auth.id)
        .execute(&state.db)
        .await?;

    tracing::info!(
        user_id = %auth.id,
        scheduled_at = %scheduled_at,
        "Account deletion scheduled"
    );

    Ok(Json(DeleteAccountResponse {
        deletion_scheduled_at: scheduled_at,
        message: format!(
            "Account scheduled for deletion on {}. You can cancel within 30 days.",
            scheduled_at.format("%Y-%m-%d")
        ),
    }))
}

/// Cancel a pending account deletion.
#[utoipa::path(
    post,
    path = "/api/me/delete-account/cancel",
    responses(
        (status = 200, description = "Deletion cancelled", body = CancelDeletionResponse),
        (status = 404, description = "No pending deletion"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn cancel_deletion(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<CancelDeletionResponse>, GovError> {
    let result = sqlx::query(
        "UPDATE users
         SET deletion_requested_at = NULL, deletion_scheduled_at = NULL
         WHERE id = $1 AND deletion_scheduled_at IS NOT NULL",
    )
    .bind(auth.id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(GovError::NoDeletionPending);
    }

    tracing::info!(user_id = %auth.id, "Account deletion cancelled");

    Ok(Json(CancelDeletionResponse {
        message: "Account deletion has been cancelled.".to_string(),
    }))
}
