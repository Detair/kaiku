//! Membership screening (rules gate).
//!
//! A screening-enabled guild places new joiners in a `pending` state (see the
//! permission-resolver short-circuit in `permissions::helpers`) until they accept
//! the rules. Admins configure the toggle + rules; a pending member reads the
//! rules and accepts to become `active`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;
use validator::Validate;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{require_guild_permission, GuildPermissions, PermissionError};

#[derive(Debug, Error)]
pub enum ScreeningError {
    #[error("Not a member of this guild")]
    NotMember,
    #[error("Screening is not enabled for this guild")]
    NotEnabled,
    #[error("You are not a pending member")]
    NotPending,
    #[error("{0}")]
    Permission(#[from] PermissionError),
    #[error("Validation failed: {0}")]
    Validation(String),
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for ScreeningError {
    fn into_response(self) -> Response {
        if let Self::Database(e) = &self {
            tracing::error!(error = %e, "Screening database operation failed");
        }
        let (status, code, msg) = match &self {
            Self::NotMember => (StatusCode::FORBIDDEN, "NOT_MEMBER", self.to_string()),
            Self::NotEnabled => (
                StatusCode::BAD_REQUEST,
                "SCREENING_DISABLED",
                self.to_string(),
            ),
            Self::NotPending => (StatusCode::BAD_REQUEST, "NOT_PENDING", self.to_string()),
            Self::Permission(_) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED", self.to_string()),
            Self::Validation(_) => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                self.to_string(),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Database error".to_string(),
            ),
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": msg })),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ScreeningConfig {
    pub screening_enabled: bool,
    pub rules_md: Option<String>,
    /// The caller's own membership state ('active' | 'pending').
    pub my_state: String,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct UpdateScreeningRequest {
    pub enabled: bool,
    #[validate(length(max = 8000))]
    pub rules_md: Option<String>,
}

/// `GET /api/guilds/{id}/screening` — config + rules (readable by a pending member).
#[tracing::instrument(skip(state))]
pub async fn get_screening(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<ScreeningConfig>, ScreeningError> {
    // A pending member has no permissions, so gate on raw membership, not perms.
    let row: Option<(bool, String)> = sqlx::query_as(
        "SELECT g.screening_enabled, gm.membership_state
         FROM guilds g JOIN guild_members gm ON gm.guild_id = g.id
         WHERE g.id = $1 AND gm.user_id = $2",
    )
    .bind(guild_id)
    .bind(auth.id)
    .fetch_optional(&state.db)
    .await?;
    let (screening_enabled, my_state) = row.ok_or(ScreeningError::NotMember)?;

    let rules_md: Option<String> =
        sqlx::query_scalar("SELECT rules_md FROM guild_screening_rules WHERE guild_id = $1")
            .bind(guild_id)
            .fetch_optional(&state.db)
            .await?;

    Ok(Json(ScreeningConfig {
        screening_enabled,
        rules_md,
        my_state,
    }))
}

/// `PUT /api/guilds/{id}/screening` — toggle + rules text (`MANAGE_GUILD`).
#[tracing::instrument(skip(state, body))]
pub async fn update_screening(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<UpdateScreeningRequest>,
) -> Result<Json<serde_json::Value>, ScreeningError> {
    body.validate()
        .map_err(|e| ScreeningError::Validation(crate::validation::format_validation_errors(&e)))?;
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_GUILD)
        .await
        .map_err(|e| match e {
            PermissionError::NotGuildMember => ScreeningError::NotMember,
            other => ScreeningError::Permission(other),
        })?;

    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE guilds SET screening_enabled = $2 WHERE id = $1")
        .bind(guild_id)
        .bind(body.enabled)
        .execute(&mut *tx)
        .await?;

    if let Some(rules) = &body.rules_md {
        sqlx::query(
            "INSERT INTO guild_screening_rules (guild_id, rules_md, updated_by)
             VALUES ($1, $2, $3)
             ON CONFLICT (guild_id) DO UPDATE SET rules_md = EXCLUDED.rules_md,
                 updated_by = EXCLUDED.updated_by, updated_at = NOW()",
        )
        .bind(guild_id)
        .bind(rules)
        .bind(auth.id)
        .execute(&mut *tx)
        .await?;
    }

    // Disabling screening promotes any pending members so nobody is stuck.
    if !body.enabled {
        sqlx::query(
            "UPDATE guild_members SET membership_state = 'active'
             WHERE guild_id = $1 AND membership_state = 'pending'",
        )
        .bind(guild_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(Json(
        serde_json::json!({ "screening_enabled": body.enabled }),
    ))
}

/// `POST /api/guilds/{id}/screening/accept` — a pending member accepts the rules.
#[tracing::instrument(skip(state))]
pub async fn accept_screening(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ScreeningError> {
    // Flip pending → active only if the guild is screening-enabled and the
    // caller is currently pending.
    let affected = sqlx::query(
        "UPDATE guild_members SET membership_state = 'active', accepted_rules_at = NOW()
         WHERE guild_id = $1 AND user_id = $2 AND membership_state = 'pending'
           AND EXISTS (SELECT 1 FROM guilds WHERE id = $1 AND screening_enabled = true)",
    )
    .bind(guild_id)
    .bind(auth.id)
    .execute(&state.db)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(ScreeningError::NotPending);
    }

    if let Err(e) = crate::ws::broadcast_to_guild(
        &state.redis,
        guild_id,
        &crate::ws::ServerEvent::MemberUpdated {
            guild_id,
            user_id: auth.id,
            membership_state: "active".to_string(),
        },
    )
    .await
    {
        tracing::warn!("Failed to broadcast member update on screening accept: {e}");
    }
    Ok(Json(serde_json::json!({ "membership_state": "active" })))
}
