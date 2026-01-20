//! Screen Share Handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    api::AppState,
    auth::AuthUser,
    db::user_features::UserFeatures,
    permissions::{require_guild_permission, GuildPermissions},
    voice::{
        screen_share::{check_limit, stop_screen_share, try_start_screen_share},
        Quality, ScreenShareCheckResponse, ScreenShareError, ScreenShareInfo,
        ScreenShareStartRequest,
    },
    ws::{broadcast_to_channel, ServerEvent},
};

impl IntoResponse for ScreenShareError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            Self::NoPermission => (StatusCode::FORBIDDEN, "No permission"),
            Self::LimitReached => (StatusCode::TOO_MANY_REQUESTS, "Limit reached"),
            Self::NotInChannel => (StatusCode::BAD_REQUEST, "Not in channel"),
            Self::QualityNotAllowed => (StatusCode::FORBIDDEN, "Premium quality required"),
            Self::RenegotiationFailed => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Renegotiation failed")
            }
            Self::InternalError => (StatusCode::INTERNAL_SERVER_ERROR, "Internal error"),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

/// Check if screen sharing is allowed.
pub async fn check(
    State(state): State<AppState>,
    user: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(req): Json<ScreenShareStartRequest>,
) -> Result<Json<ScreenShareCheckResponse>, ScreenShareError> {
    // 1. Get channel
    let channel_row =
        sqlx::query("SELECT id, guild_id, max_screen_shares FROM channels WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|_| ScreenShareError::InternalError)?
            .ok_or(ScreenShareError::InternalError)?;

    let guild_id: Option<Uuid> = channel_row.try_get("guild_id").unwrap_or(None);
    let max_screen_shares: i32 = channel_row.try_get("max_screen_shares").unwrap_or(1);

    // 2. Check Permissions if guild
    if let Some(gid) = guild_id {
        if require_guild_permission(
            &state.db,
            gid,
            user.id,
            GuildPermissions::SCREEN_SHARE,
        )
        .await
        .is_err()
        {
            return Ok(Json(ScreenShareCheckResponse::denied(
                ScreenShareError::NoPermission,
            )));
        }
    }

    // 3. Check limits
    if let Err(e) = check_limit(&state.redis, channel_id, max_screen_shares as u32).await {
        return Ok(Json(ScreenShareCheckResponse::denied(e)));
    }

    // 4. Check Premium
    let mut granted_quality = req.quality;
    if req.quality == Quality::Premium {
        let user_row = sqlx::query("SELECT feature_flags FROM users WHERE id = $1")
            .bind(user.id)
            .fetch_optional(&state.db)
            .await
            .map_err(|_| ScreenShareError::InternalError)?
            .ok_or(ScreenShareError::InternalError)?;

        let flags: i64 = user_row.try_get("feature_flags").unwrap_or(0);
        let features = UserFeatures::from_bits_truncate(flags);
        
        if !features.contains(UserFeatures::PREMIUM_VIDEO) {
            granted_quality = Quality::High; // Downgrade
        }
    }

    Ok(Json(ScreenShareCheckResponse::allowed(granted_quality)))
}

/// Start screen sharing.
pub async fn start(
    State(state): State<AppState>,
    user: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(req): Json<ScreenShareStartRequest>,
) -> Result<Json<ScreenShareCheckResponse>, ScreenShareError> {
    // 1. Get channel
    let channel_row =
        sqlx::query("SELECT id, guild_id, max_screen_shares FROM channels WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|_| ScreenShareError::InternalError)?;

    let channel_row = channel_row.ok_or(ScreenShareError::InternalError)?;
    let guild_id: Option<Uuid> = channel_row.try_get("guild_id").unwrap_or(None);
    let max_screen_shares: i32 = channel_row.try_get("max_screen_shares").unwrap_or(1);

    // 2. Check Permissions
    if let Some(gid) = guild_id {
        if require_guild_permission(
            &state.db,
            gid,
            user.id,
            GuildPermissions::SCREEN_SHARE,
        )
        .await
        .is_err()
        {
            return Err(ScreenShareError::NoPermission);
        }
    }

    // 3. Check Premium
    let mut granted_quality = req.quality;
    if req.quality == Quality::Premium {
        let user_row = sqlx::query("SELECT feature_flags FROM users WHERE id = $1")
            .bind(user.id)
            .fetch_optional(&state.db)
            .await
            .map_err(|_| ScreenShareError::InternalError)?
            .ok_or(ScreenShareError::InternalError)?;

        let flags: i64 = user_row.try_get("feature_flags").unwrap_or(0);
        let features = UserFeatures::from_bits_truncate(flags);

        if !features.contains(UserFeatures::PREMIUM_VIDEO) {
            granted_quality = Quality::High; // Downgrade
        }
    }

    // 4. Try start (Redis INCR)
    try_start_screen_share(&state.redis, channel_id, max_screen_shares as u32).await?;

    // 5. Update Room & Broadcast
    if let Some(room) = state.sfu.get_room(channel_id).await {
        let info = ScreenShareInfo::new(
            user.id,
            user.username.clone(),
            req.source_label.clone(),
            req.has_audio,
            granted_quality,
        );
        room.add_screen_share(info).await;

        let event = ServerEvent::ScreenShareStarted {
            channel_id,
            user_id: user.id,
            username: user.username,
            source_label: req.source_label,
            has_audio: req.has_audio,
            quality: granted_quality,
        };
        
        if broadcast_to_channel(&state.redis, channel_id, &event).await.is_err() {
            // Log error but continue
        }
    } else {
        // User not in voice room. Rollback limit.
        stop_screen_share(&state.redis, channel_id).await;
        return Err(ScreenShareError::NotInChannel);
    }

    Ok(Json(ScreenShareCheckResponse::allowed(granted_quality)))
}

/// Stop screen sharing.
pub async fn stop(
    State(state): State<AppState>,
    user: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<(), ScreenShareError> {
    stop_screen_share(&state.redis, channel_id).await;

    if let Some(room) = state.sfu.get_room(channel_id).await {
        room.remove_screen_share(user.id).await;

        let event = ServerEvent::ScreenShareStopped {
            channel_id,
            user_id: user.id,
            reason: "user_stopped".to_string(),
        };
        let _ = broadcast_to_channel(&state.redis, channel_id, &event).await;
    }

    Ok(())
}