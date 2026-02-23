//! Screen Share Handlers

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use sqlx::Row;
use tracing::{error, warn};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db::user_features::UserFeatures;
use crate::permissions::{require_guild_permission, GuildPermissions};
use crate::voice::screen_share::{
    check_limit, stop_screen_share, try_start_screen_share, validate_source_label,
};
use crate::voice::{
    Quality, ScreenShareCheckResponse, ScreenShareError, ScreenShareInfo, ScreenShareStartRequest,
};
use crate::ws::{broadcast_to_channel, ServerEvent};

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
            Self::InvalidSourceLabel => (StatusCode::BAD_REQUEST, "Invalid source label"),
            Self::AlreadySharing => (StatusCode::CONFLICT, "Already sharing screen"),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

/// Check if screen sharing is allowed.
#[utoipa::path(
    post,
    path = "/api/channels/{id}/screenshare/check",
    tag = "screenshare",
    params(("id" = Uuid, Path, description = "Channel ID")),
    request_body = ScreenShareStartRequest,
    responses(
        (status = 200, description = "Screen share availability check"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn check(
    State(state): State<AppState>,
    user: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(req): Json<ScreenShareStartRequest>,
) -> Result<Json<ScreenShareCheckResponse>, ScreenShareError> {
    // 0. Validate source_label
    validate_source_label(&req.source_label)?;

    // 1. Get channel
    let channel_row =
        sqlx::query("SELECT id, guild_id, max_screen_shares FROM channels WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| {
                error!(channel_id = %channel_id, error = %e, "Database error fetching channel");
                ScreenShareError::InternalError
            })?
            .ok_or(ScreenShareError::InternalError)?;

    let guild_id: Option<Uuid> = channel_row.try_get("guild_id").unwrap_or_else(|e| {
        warn!(channel_id = %channel_id, error = %e, "Failed to read guild_id, defaulting to None");
        None
    });
    let max_screen_shares: i32 = channel_row.try_get("max_screen_shares").unwrap_or_else(|e| {
        warn!(channel_id = %channel_id, error = %e, "Failed to read max_screen_shares, defaulting to 1");
        1
    });

    // 2. Check Permissions if guild (SCREEN_SHARE + VOICE_CONNECT)
    if let Some(gid) = guild_id {
        let required_perms = GuildPermissions::SCREEN_SHARE | GuildPermissions::VOICE_CONNECT;
        if require_guild_permission(&state.db, gid, user.id, required_perms)
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
            .map_err(|e| {
                error!(user_id = %user.id, error = %e, "Database error fetching user features");
                ScreenShareError::InternalError
            })?
            .ok_or(ScreenShareError::InternalError)?;

        let flags: i64 = user_row.try_get("feature_flags").unwrap_or_else(|e| {
            warn!(user_id = %user.id, error = %e, "Failed to read feature_flags, defaulting to 0");
            0
        });
        let features = UserFeatures::from_bits_truncate(flags);

        if !features.contains(UserFeatures::PREMIUM_VIDEO) {
            granted_quality = Quality::High; // Downgrade
        }
    }

    Ok(Json(ScreenShareCheckResponse::allowed(granted_quality)))
}

/// Start screen sharing.
#[utoipa::path(
    post,
    path = "/api/channels/{id}/screenshare/start",
    tag = "screenshare",
    params(("id" = Uuid, Path, description = "Channel ID")),
    request_body = ScreenShareStartRequest,
    responses(
        (status = 200, description = "Screen share started"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn start(
    State(state): State<AppState>,
    user: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(req): Json<ScreenShareStartRequest>,
) -> Result<Json<ScreenShareCheckResponse>, ScreenShareError> {
    // 0. Validate source_label
    validate_source_label(&req.source_label)?;

    // 1. Get channel
    let channel_row =
        sqlx::query("SELECT id, guild_id, max_screen_shares FROM channels WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| {
                error!(channel_id = %channel_id, error = %e, "Database error fetching channel");
                ScreenShareError::InternalError
            })?
            .ok_or(ScreenShareError::InternalError)?;

    let guild_id: Option<Uuid> = channel_row.try_get("guild_id").unwrap_or_else(|e| {
        warn!(channel_id = %channel_id, error = %e, "Failed to read guild_id, defaulting to None");
        None
    });
    let max_screen_shares: i32 = channel_row.try_get("max_screen_shares").unwrap_or_else(|e| {
        warn!(channel_id = %channel_id, error = %e, "Failed to read max_screen_shares, defaulting to 1");
        1
    });

    // 2. Check Permissions (SCREEN_SHARE + VOICE_CONNECT)
    if let Some(gid) = guild_id {
        let required_perms = GuildPermissions::SCREEN_SHARE | GuildPermissions::VOICE_CONNECT;
        if require_guild_permission(&state.db, gid, user.id, required_perms)
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
            .map_err(|e| {
                error!(user_id = %user.id, error = %e, "Database error fetching user features");
                ScreenShareError::InternalError
            })?
            .ok_or(ScreenShareError::InternalError)?;

        let flags: i64 = user_row.try_get("feature_flags").unwrap_or_else(|e| {
            warn!(user_id = %user.id, error = %e, "Failed to read feature_flags, defaulting to 0");
            0
        });
        let features = UserFeatures::from_bits_truncate(flags);

        if !features.contains(UserFeatures::PREMIUM_VIDEO) {
            granted_quality = Quality::High; // Downgrade
        }
    }

    // 4. Check room membership BEFORE incrementing counter (security fix)
    let room = state
        .sfu
        .get_room(channel_id)
        .await
        .ok_or(ScreenShareError::NotInChannel)?;

    // 4a. Check if user is actually in the room
    if room.get_peer(user.id).await.is_none() {
        return Err(ScreenShareError::NotInChannel);
    }

    // 4b. Check if user already has an active screen share
    {
        let screen_shares = room.screen_shares.read().await;
        if screen_shares.contains_key(&user.id) {
            return Err(ScreenShareError::AlreadySharing);
        }
    }

    // 5. Try start (Redis INCR) - only after all checks pass
    try_start_screen_share(&state.redis, channel_id, max_screen_shares as u32).await?;

    // 6. Update Room & Broadcast
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

    if let Err(e) = broadcast_to_channel(&state.redis, channel_id, &event).await {
        error!(
            channel_id = %channel_id,
            user_id = %user.id,
            error = %e,
            "Failed to broadcast screen share started event - participants may have stale state"
        );
    }

    Ok(Json(ScreenShareCheckResponse::allowed(granted_quality)))
}

/// Stop screen sharing.
#[utoipa::path(
    post,
    path = "/api/channels/{id}/screenshare/stop",
    tag = "screenshare",
    params(("id" = Uuid, Path, description = "Channel ID")),
    responses(
        (status = 200, description = "Screen share stopped"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn stop(
    State(state): State<AppState>,
    user: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<(), ScreenShareError> {
    // Only decrement if user actually had a screen share
    let had_screen_share = if let Some(room) = state.sfu.get_room(channel_id).await {
        let screen_shares = room.screen_shares.read().await;
        screen_shares.contains_key(&user.id)
    } else {
        false
    };

    if had_screen_share {
        stop_screen_share(&state.redis, channel_id).await;
    }

    if let Some(room) = state.sfu.get_room(channel_id).await {
        room.remove_screen_share(user.id).await;

        let event = ServerEvent::ScreenShareStopped {
            channel_id,
            user_id: user.id,
            reason: "user_stopped".to_string(),
        };

        if let Err(e) = broadcast_to_channel(&state.redis, channel_id, &event).await {
            error!(
                channel_id = %channel_id,
                user_id = %user.id,
                error = %e,
                "Failed to broadcast screen share stopped event - participants may have stale state"
            );
        }
    }

    Ok(())
}
