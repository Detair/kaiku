//! Screen Share Handlers

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use sqlx::{PgPool, Row};
use tracing::{error, warn};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db::user_features::UserFeatures;
use crate::permissions::{require_guild_permission, GuildPermissions};
use crate::voice::screen_share::validate_source_label;
use crate::voice::{
    Quality, ScreenShareCheckResponse, ScreenShareError, ScreenShareInfo, ScreenShareStartRequest,
    ScreenShareStopRequest,
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
            Self::AlreadySharing => (
                StatusCode::CONFLICT,
                "Maximum 3 concurrent screen shares per user",
            ),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

/// Fetch channel voice settings for screen share checks.
/// Returns `(guild_id, max_screen_shares)`.
async fn fetch_channel_settings(
    pool: &PgPool,
    channel_id: Uuid,
) -> Result<(Option<Uuid>, u32), ScreenShareError> {
    let row = sqlx::query("SELECT guild_id, max_screen_shares FROM channels WHERE id = $1")
        .bind(channel_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            error!(channel_id = %channel_id, error = %e, "Database error fetching channel");
            ScreenShareError::InternalError
        })?
        .ok_or(ScreenShareError::InternalError)?;

    let guild_id: Option<Uuid> = row.try_get("guild_id").unwrap_or_else(|e| {
        warn!(channel_id = %channel_id, error = %e, "Failed to read guild_id, defaulting to None");
        None
    });

    let raw: i32 = row.try_get("max_screen_shares").unwrap_or_else(|e| {
        warn!(channel_id = %channel_id, error = %e, "Failed to read max_screen_shares, defaulting to 6");
        6
    });
    let max_screen_shares: u32 = raw.try_into().unwrap_or(6);

    Ok((guild_id, max_screen_shares))
}

/// Resolve the granted quality tier based on user feature flags.
/// Downgrades Premium to High if user lacks `PREMIUM_VIDEO`.
async fn resolve_quality(
    pool: &PgPool,
    user_id: Uuid,
    requested: Quality,
) -> Result<Quality, ScreenShareError> {
    if requested != Quality::Premium {
        return Ok(requested);
    }

    let user_row = sqlx::query("SELECT feature_flags FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            error!(user_id = %user_id, error = %e, "Database error fetching user features");
            ScreenShareError::InternalError
        })?
        .ok_or(ScreenShareError::InternalError)?;

    let flags: i64 = user_row.try_get("feature_flags").unwrap_or_else(|e| {
        warn!(user_id = %user_id, error = %e, "Failed to read feature_flags, defaulting to 0");
        0
    });
    let features = UserFeatures::from_bits_truncate(flags);

    if features.contains(UserFeatures::PREMIUM_VIDEO) {
        Ok(Quality::Premium)
    } else {
        Ok(Quality::High)
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
    validate_source_label(&req.source_label)?;

    let (guild_id, max_screen_shares) = fetch_channel_settings(&state.db, channel_id).await?;

    // Check guild permissions
    if let Some(gid) = guild_id {
        let required = GuildPermissions::SCREEN_SHARE | GuildPermissions::VOICE_CONNECT;
        if require_guild_permission(&state.db, gid, user.id, required)
            .await
            .is_err()
        {
            return Ok(Json(ScreenShareCheckResponse::denied(
                ScreenShareError::NoPermission,
            )));
        }
    }

    // Check limits via limiter
    let limiter = state
        .screen_share_limiter
        .as_ref()
        .ok_or(ScreenShareError::InternalError)?;
    if let Err(e) = limiter.check(channel_id, max_screen_shares).await {
        return Ok(Json(ScreenShareCheckResponse::denied(e)));
    }

    let granted_quality = resolve_quality(&state.db, user.id, req.quality).await?;
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
    validate_source_label(&req.source_label)?;

    let (guild_id, max_screen_shares) = fetch_channel_settings(&state.db, channel_id).await?;

    // Check guild permissions
    if let Some(gid) = guild_id {
        let required = GuildPermissions::SCREEN_SHARE | GuildPermissions::VOICE_CONNECT;
        if require_guild_permission(&state.db, gid, user.id, required)
            .await
            .is_err()
        {
            return Err(ScreenShareError::NoPermission);
        }
    }

    let granted_quality = resolve_quality(&state.db, user.id, req.quality).await?;

    // Check room membership BEFORE reserving slot
    let room = state
        .sfu
        .get_room(channel_id)
        .await
        .ok_or(ScreenShareError::NotInChannel)?;
    if room.get_peer(user.id).await.is_none() {
        return Err(ScreenShareError::NotInChannel);
    }

    // Check per-user stream limit (max 3 concurrent streams)
    {
        const MAX_STREAMS_PER_USER: usize = 3;
        let count = room.get_user_stream_count(user.id).await;
        if count >= MAX_STREAMS_PER_USER {
            return Err(ScreenShareError::AlreadySharing);
        }
    }

    // Reserve slot via limiter
    let limiter = state
        .screen_share_limiter
        .as_ref()
        .ok_or(ScreenShareError::InternalError)?;
    limiter.start(channel_id, max_screen_shares).await?;

    // Update room & broadcast
    let stream_id = req.stream_id;
    let info = ScreenShareInfo::new(
        stream_id,
        user.id,
        user.username.clone(),
        req.source_label.clone(),
        req.has_audio,
        granted_quality,
    );
    let started_at = info.started_at.to_rfc3339();
    room.add_screen_share(info).await;

    let event = ServerEvent::ScreenShareStarted {
        channel_id,
        user_id: user.id,
        stream_id,
        username: user.username,
        source_label: req.source_label,
        has_audio: req.has_audio,
        quality: granted_quality,
        started_at,
    };
    if let Err(e) = broadcast_to_channel(&state.redis, channel_id, &event).await {
        error!(
            channel_id = %channel_id,
            user_id = %user.id,
            error = %e,
            "Failed to broadcast screen share started event"
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
    request_body = ScreenShareStopRequest,
    responses(
        (status = 200, description = "Screen share stopped"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn stop(
    State(state): State<AppState>,
    user: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(req): Json<ScreenShareStopRequest>,
) -> Result<(), ScreenShareError> {
    // If there is no active SFU room for this channel, the stream cannot exist —
    // treat stop as a no-op (idempotent) and return immediately.
    let Some(room) = state.sfu.get_room(channel_id).await else {
        return Ok(());
    };

    // Find the specific stream and verify ownership
    let share_info = {
        let shares = room.screen_shares.read().await;
        shares.get(&req.stream_id).cloned()
    };

    // If the stream doesn't exist, stopping is a no-op (idempotent).
    let Some(info) = share_info else {
        warn!(
            channel_id = %channel_id,
            stream_id = %req.stream_id,
            user_id = %user.id,
            "Screen share stream not found — treating stop as no-op"
        );
        return Ok(());
    };

    // Verify the requesting user owns the stream
    if info.user_id != user.id {
        warn!(
            channel_id = %channel_id,
            stream_id = %req.stream_id,
            owner_id = %info.user_id,
            requester_id = %user.id,
            "User attempted to stop another user's screen share"
        );
        return Err(ScreenShareError::NoPermission);
    }

    // Decrement Redis counter
    if let Some(ref limiter) = state.screen_share_limiter {
        limiter.stop(channel_id).await;
    } else {
        tracing::warn!("Screen share limiter unavailable during stop — counter not decremented");
    }

    room.remove_screen_share(req.stream_id).await;

    let event = ServerEvent::ScreenShareStopped {
        channel_id,
        user_id: user.id,
        stream_id: req.stream_id,
        reason: "user_stopped".to_string(),
    };
    if let Err(e) = broadcast_to_channel(&state.redis, channel_id, &event).await {
        error!(
            channel_id = %channel_id,
            user_id = %user.id,
            error = %e,
            "Failed to broadcast screen share stopped event"
        );
    }

    Ok(())
}
