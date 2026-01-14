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
    voice::call::CallState,
    voice::call_service::{CallError, CallService},
};

/// Response for call state
#[derive(Debug, Serialize)]
pub struct CallStateResponse {
    pub channel_id: Uuid,
    #[serde(flatten)]
    pub state: CallState,
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
            CallError::CallNotFound => (StatusCode::NOT_FOUND, "call_not_found"),
            CallError::CallAlreadyExists => (StatusCode::CONFLICT, "call_already_exists"),
            CallError::Redis(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
            CallError::InvalidEvent(_) => (StatusCode::BAD_REQUEST, "invalid_event"),
            CallError::StateTransition(_) => (StatusCode::CONFLICT, "invalid_transition"),
            CallError::Serialization(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };

        let body = Json(CallApiError {
            error: self.to_string(),
            code: code.to_string(),
        });

        (status, body).into_response()
    }
}

/// Custom error type for call handlers that combines CallError and database errors
pub enum CallHandlerError {
    Call(CallError),
    NotFound,
    Forbidden,
    Database(String),
}

impl IntoResponse for CallHandlerError {
    fn into_response(self) -> axum::response::Response {
        match self {
            CallHandlerError::Call(e) => e.into_response(),
            CallHandlerError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(CallApiError {
                    error: "DM channel not found".to_string(),
                    code: "not_found".to_string(),
                }),
            )
                .into_response(),
            CallHandlerError::Forbidden => (
                StatusCode::FORBIDDEN,
                Json(CallApiError {
                    error: "Not a participant of this DM".to_string(),
                    code: "forbidden".to_string(),
                }),
            )
                .into_response(),
            CallHandlerError::Database(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CallApiError {
                    error: format!("Database error: {}", e),
                    code: "internal_error".to_string(),
                }),
            )
                .into_response(),
        }
    }
}

impl From<CallError> for CallHandlerError {
    fn from(e: CallError) -> Self {
        CallHandlerError::Call(e)
    }
}

impl From<sqlx::Error> for CallHandlerError {
    fn from(e: sqlx::Error) -> Self {
        CallHandlerError::Database(e.to_string())
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

/// GET /api/dm/:id/call - Get current call state
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
    })))
}

/// POST /api/dm/:id/call/start - Start a new call
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

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service
        .start_call(channel_id, auth.id, target_users)
        .await?;

    // TODO: Broadcast CallStarted WebSocket event to participants

    Ok((
        StatusCode::CREATED,
        Json(CallStateResponse {
            channel_id,
            state: call_state,
        }),
    ))
}

/// POST /api/dm/:id/call/join - Join an active call
pub async fn join_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallHandlerError> {
    // Verify membership
    verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.join_call(channel_id, auth.id).await?;

    // TODO: Broadcast ParticipantJoined WebSocket event

    Ok(Json(CallStateResponse {
        channel_id,
        state: call_state,
    }))
}

/// POST /api/dm/:id/call/decline - Decline a call
pub async fn decline_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallHandlerError> {
    // Verify membership
    verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.decline_call(channel_id, auth.id).await?;

    // TODO: Broadcast CallDeclined WebSocket event

    Ok(Json(CallStateResponse {
        channel_id,
        state: call_state,
    }))
}

/// POST /api/dm/:id/call/leave - Leave an active call
pub async fn leave_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallHandlerError> {
    // Verify membership
    verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.leave_call(channel_id, auth.id).await?;

    // TODO: Broadcast ParticipantLeft or CallEnded WebSocket event

    Ok(Json(CallStateResponse {
        channel_id,
        state: call_state,
    }))
}

/// Build the call router (to be nested under /api/dm)
pub fn call_router() -> axum::Router<AppState> {
    use axum::routing::{get, post};

    axum::Router::new()
        .route("/:id/call", get(get_call))
        .route("/:id/call/start", post(start_call))
        .route("/:id/call/join", post(join_call))
        .route("/:id/call/decline", post(decline_call))
        .route("/:id/call/leave", post(leave_call))
}
