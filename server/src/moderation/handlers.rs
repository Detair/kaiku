//! User-facing report handlers.

use axum::{extract::State, Json};
use fred::prelude::*;
use validator::Validate;

use super::types::{CreateReportRequest, Report, ReportError, ReportResponse};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::ws::{broadcast_admin_event, ServerEvent};

/// POST /api/reports
/// Create a new user report.
pub async fn create_report(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateReportRequest>,
) -> Result<Json<ReportResponse>, ReportError> {
    body.validate()
        .map_err(|e| ReportError::Validation(e.to_string()))?;

    // Cannot report yourself
    if body.target_user_id == auth.id {
        return Err(ReportError::Validation(
            "Cannot report yourself".to_string(),
        ));
    }

    // Check target user exists
    let target_exists: bool =
        sqlx::query_scalar!("SELECT id FROM users WHERE id = $1", body.target_user_id)
            .fetch_optional(&state.db)
            .await?
            .is_some();

    if !target_exists {
        return Err(ReportError::Validation(
            "Target user not found".to_string(),
        ));
    }

    // If reporting a message, verify it exists and belongs to the target user
    if let Some(message_id) = body.target_message_id {
        let msg = sqlx::query!(
            "SELECT user_id FROM messages WHERE id = $1",
            message_id
        )
        .fetch_optional(&state.db)
        .await?;

        match msg {
            Some(m) if m.user_id != body.target_user_id => {
                return Err(ReportError::Validation(
                    "Message does not belong to the target user".to_string(),
                ));
            }
            None => {
                return Err(ReportError::Validation(
                    "Target message not found".to_string(),
                ));
            }
            _ => {}
        }
    }

    // Rate limit: 5 reports per hour per user (Redis counter)
    // Placed after validation to avoid consuming rate limit on invalid input
    let rate_key = format!("report_rate:{}", auth.id);
    let count: i64 = state.redis.incr(&rate_key).await.unwrap_or(1);
    if count == 1 {
        // Set expiry on first increment
        let _: Result<(), _> = state
            .redis
            .expire(&rate_key, 3600, None)
            .await;
    }
    if count > 5 {
        return Err(ReportError::RateLimited);
    }

    // Insert report (unique index will catch duplicates)
    let report = sqlx::query_as::<_, Report>(
        r#"INSERT INTO user_reports (reporter_id, target_type, target_user_id, target_message_id, category, description)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING *"#,
    )
    .bind(auth.id)
    .bind(body.target_type)
    .bind(body.target_user_id)
    .bind(body.target_message_id)
    .bind(body.category)
    .bind(body.description)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref db_err) = e {
            if db_err.constraint() == Some("idx_reports_no_duplicate_active") {
                return ReportError::Duplicate;
            }
        }
        ReportError::Database(e)
    })?;

    // Broadcast to admin events channel
    let event = ServerEvent::AdminReportCreated {
        report_id: report.id,
        category: format!("{:?}", report.category).to_lowercase(),
        target_type: format!("{:?}", report.target_type).to_lowercase(),
    };
    if let Err(e) = broadcast_admin_event(&state.redis, &event).await {
        tracing::warn!("Failed to broadcast admin report event: {}", e);
    }

    Ok(Json(report.into()))
}
