//! HTTP handlers for DM voice call API endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

use crate::{
    api::AppState,
    auth::AuthUser,
    db::{self, ChannelType},
    social::block_cache,
    voice::call::CallState,
    voice::call_service::{CallError, CallService},
    ws::{broadcast_to_channel, ServerEvent},
};

/// Response for call state
#[derive(Debug, Serialize)]
pub struct CallStateResponse {
    pub channel_id: Uuid,
    #[serde(flatten)]
    pub state: CallState,
    pub capabilities: Vec<String>,
}

/// Call API error response
#[derive(Debug, Serialize)]
pub struct CallApiError {
    pub error: String,
    pub code: String,
}

impl IntoResponse for CallError {
    fn into_response(self) -> axum::response::Response {
        let (status, code) = match &self {
            Self::CallNotFound => (StatusCode::NOT_FOUND, "call_not_found"),
            Self::CallAlreadyExists => (StatusCode::CONFLICT, "call_already_exists"),
            Self::Redis(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
            Self::InvalidEvent(_) => (StatusCode::BAD_REQUEST, "invalid_event"),
            Self::StateTransition(_) => (StatusCode::CONFLICT, "invalid_transition"),
            Self::Serialization(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };

        let body = Json(CallApiError {
            error: self.to_string(),
            code: code.to_string(),
        });

        (status, body).into_response()
    }
}

/// Custom error type for call handlers that combines `CallError` and database errors
pub enum CallHandlerError {
    Call(CallError),
    NotFound,
    Forbidden,
    Blocked,
    Database(String),
}

impl IntoResponse for CallHandlerError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Call(e) => e.into_response(),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                Json(CallApiError {
                    error: "DM channel not found".to_string(),
                    code: "not_found".to_string(),
                }),
            )
                .into_response(),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                Json(CallApiError {
                    error: "Not a participant of this DM".to_string(),
                    code: "forbidden".to_string(),
                }),
            )
                .into_response(),
            Self::Blocked => (
                StatusCode::FORBIDDEN,
                Json(CallApiError {
                    error: "Cannot call this user".to_string(),
                    code: "blocked".to_string(),
                }),
            )
                .into_response(),
            Self::Database(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CallApiError {
                    error: format!("Database error: {e}"),
                    code: "internal_error".to_string(),
                }),
            )
                .into_response(),
        }
    }
}

impl From<CallError> for CallHandlerError {
    fn from(e: CallError) -> Self {
        Self::Call(e)
    }
}

impl From<sqlx::Error> for CallHandlerError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e.to_string())
    }
}

/// Verify user is a DM participant and get all participants
async fn verify_dm_participant(
    state: &AppState,
    channel_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<Uuid>, CallHandlerError> {
    // Get channel and verify it's a DM
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(CallHandlerError::NotFound)?;

    if channel.channel_type != ChannelType::Dm {
        return Err(CallHandlerError::NotFound);
    }

    // Get all participants
    let participants: Vec<Uuid> = sqlx::query_scalar!(
        "SELECT user_id FROM dm_participants WHERE channel_id = $1",
        channel_id
    )
    .fetch_all(&state.db)
    .await?;

    // Verify user is a participant
    if !participants.contains(&user_id) {
        return Err(CallHandlerError::Forbidden);
    }

    Ok(participants)
}

/// GET /api/dm/{id}/call - Get current call state
#[tracing::instrument(skip(state, auth))]
pub async fn get_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<Option<CallStateResponse>>, CallHandlerError> {
    // Verify membership
    verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.get_call_state(channel_id).await?;

    Ok(Json(call_state.map(|state| CallStateResponse {
        channel_id,
        state,
        capabilities: vec!["audio".to_string()],
    })))
}

/// Get username for a user ID
async fn get_username(state: &AppState, user_id: Uuid) -> Result<String, CallHandlerError> {
    let user = db::find_user_by_id(&state.db, user_id)
        .await?
        .ok_or(CallHandlerError::NotFound)?;
    Ok(user.username)
}

/// POST /api/dm/{id}/call/start - Start a new call
#[tracing::instrument(skip(state, auth))]
pub async fn start_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<(StatusCode, Json<CallStateResponse>), CallHandlerError> {
    // Verify membership and get other participants
    let participants = verify_dm_participant(&state, channel_id, auth.id).await?;
    let target_users: HashSet<Uuid> = participants
        .into_iter()
        .filter(|&id| id != auth.id)
        .collect();

    if target_users.is_empty() {
        return Err(CallError::InvalidEvent("No other participants in DM".into()).into());
    }

    // Check if any participant has blocked the other
    for &target_id in &target_users {
        if block_cache::is_blocked_either_direction(&state.redis, auth.id, target_id)
            .await
            .unwrap_or(false)
        {
            return Err(CallHandlerError::Blocked);
        }
    }

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service
        .start_call(channel_id, auth.id, target_users)
        .await?;

    // Broadcast IncomingCall to all participants (they're subscribed to the DM channel)
    let initiator_name = get_username(&state, auth.id).await?;
    // Default capabilities: audio only for now
    let capabilities = vec!["audio".to_string()];
    if let Err(e) = broadcast_to_channel(
        &state.redis,
        channel_id,
        &ServerEvent::IncomingCall {
            channel_id,
            initiator: auth.id,
            initiator_name,
            capabilities,
        },
    )
    .await
    {
        tracing::warn!(error = %e, %channel_id, "Failed to broadcast IncomingCall event");
    }

    Ok((
        StatusCode::CREATED,
        Json(CallStateResponse {
            channel_id,
            state: call_state,
            capabilities: vec!["audio".to_string()],
        }),
    ))
}

/// POST /api/dm/{id}/call/join - Join an active call
#[tracing::instrument(skip(state, auth))]
pub async fn join_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallHandlerError> {
    // Verify membership
    let participants = verify_dm_participant(&state, channel_id, auth.id).await?;

    // Check block status with other participants
    for &participant_id in &participants {
        if participant_id != auth.id {
            if block_cache::is_blocked_either_direction(&state.redis, auth.id, participant_id)
                .await
                .unwrap_or(false)
            {
                return Err(CallHandlerError::Blocked);
            }
        }
    }

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.join_call(channel_id, auth.id).await?;

    // Broadcast ParticipantJoined to all participants
    let username = get_username(&state, auth.id).await?;
    if let Err(e) = broadcast_to_channel(
        &state.redis,
        channel_id,
        &ServerEvent::CallParticipantJoined {
            channel_id,
            user_id: auth.id,
            username,
        },
    )
    .await
    {
        tracing::warn!(error = %e, %channel_id, "Failed to broadcast CallParticipantJoined event");
    }

    Ok(Json(CallStateResponse {
        channel_id,
        state: call_state,
        capabilities: vec!["audio".to_string()],
    }))
}

/// POST /api/dm/{id}/call/decline - Decline a call
#[tracing::instrument(skip(state, auth))]
pub async fn decline_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallHandlerError> {
    // Verify membership
    verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.decline_call(channel_id, auth.id).await?;

    // Broadcast CallDeclined to all participants
    if let Err(e) = broadcast_to_channel(
        &state.redis,
        channel_id,
        &ServerEvent::CallDeclined {
            channel_id,
            user_id: auth.id,
        },
    )
    .await
    {
        tracing::warn!(error = %e, %channel_id, "Failed to broadcast CallDeclined event");
    }

    // If call ended due to all declining, broadcast CallEnded
    if let CallState::Ended { reason, .. } = &call_state {
        let reason_str = serde_json::to_string(&reason)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        if let Err(e) = broadcast_to_channel(
            &state.redis,
            channel_id,
            &ServerEvent::CallEnded {
                channel_id,
                reason: reason_str,
                duration_secs: None,
            },
        )
        .await
        {
            tracing::warn!(error = %e, %channel_id, "Failed to broadcast CallEnded event");
        }
    }

    Ok(Json(CallStateResponse {
        channel_id,
        state: call_state,
        capabilities: vec!["audio".to_string()],
    }))
}

/// POST /api/dm/{id}/call/leave - Leave an active call
#[tracing::instrument(skip(state, auth))]
pub async fn leave_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallHandlerError> {
    // Verify membership
    verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.leave_call(channel_id, auth.id).await?;

    // Broadcast ParticipantLeft
    if let Err(e) = broadcast_to_channel(
        &state.redis,
        channel_id,
        &ServerEvent::CallParticipantLeft {
            channel_id,
            user_id: auth.id,
        },
    )
    .await
    {
        tracing::warn!(error = %e, %channel_id, "Failed to broadcast CallParticipantLeft event");
    }

    // If call ended due to last person leaving, broadcast CallEnded
    if let CallState::Ended {
        reason,
        duration_secs,
        ..
    } = &call_state
    {
        let reason_str = serde_json::to_string(&reason)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        if let Err(e) = broadcast_to_channel(
            &state.redis,
            channel_id,
            &ServerEvent::CallEnded {
                channel_id,
                reason: reason_str,
                duration_secs: *duration_secs,
            },
        )
        .await
        {
            tracing::warn!(error = %e, %channel_id, "Failed to broadcast CallEnded event");
        }
    }

    Ok(Json(CallStateResponse {
        channel_id,
        state: call_state,
        capabilities: vec!["audio".to_string()],
    }))
}

/// Build the call router (to be nested under /api/dm)
pub fn call_router() -> axum::Router<AppState> {
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/{id}/call", get(get_call))
        .route("/{id}/call/start", post(start_call))
        .route("/{id}/call/join", post(join_call))
        .route("/{id}/call/decline", post(decline_call))
        .route("/{id}/call/leave", post(leave_call))
}
