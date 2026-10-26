//! HTTP handlers for DM voice call API endpoints.

use std::collections::HashSet;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use tracing::warn;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db::{self, ChannelType};
use crate::social::block_cache;
use crate::voice::call::CallState;
use crate::voice::call_service::{
    broadcast_call_ended, broadcast_to_members, spawn_ring_timeout, CallError, CallService,
};
use crate::voice::queries;
use crate::ws::ServerEvent;

/// Response for call state
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CallStateResponse {
    pub channel_id: Uuid,
    #[serde(flatten)]
    #[schema(inline)]
    pub state: CallState,
    pub capabilities: Vec<String>,
}

/// Call API error response
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CallApiError {
    pub error: String,
    pub code: String,
}

impl IntoResponse for CallError {
    fn into_response(self) -> axum::response::Response {
        let (status, code) = match &self {
            Self::CallNotFound => (StatusCode::NOT_FOUND, "call_not_found"),
            Self::CallAlreadyExists => (StatusCode::CONFLICT, "call_already_exists"),
            Self::Contention => (StatusCode::CONFLICT, "call_conflict"),
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
    let participants = queries::list_dm_participant_ids(&state.db, channel_id).await?;

    // Verify user is a participant
    if !participants.contains(&user_id) {
        return Err(CallHandlerError::Forbidden);
    }

    Ok(participants)
}

/// Refuse if `user_id` and any of `others` blocked each other.
///
/// Same rule as sending a DM: the block cache answers first, and because a
/// cache miss allows, Postgres confirms. Lookup failures follow
/// `block_check_fail_open`.
async fn ensure_not_blocked(
    state: &AppState,
    user_id: Uuid,
    others: &[Uuid],
) -> Result<(), CallHandlerError> {
    for &target_id in others {
        match block_cache::is_blocked_either_direction(&state.redis, user_id, target_id).await {
            Ok(true) => return Err(CallHandlerError::Blocked),
            Ok(false) => {}
            Err(e) => {
                warn!(
                    error = %e,
                    %user_id,
                    %target_id,
                    fail_open = state.config.block_check_fail_open,
                    "Redis block check failed, using failsafe policy"
                );
                if !state.config.block_check_fail_open {
                    return Err(CallHandlerError::Blocked);
                }
            }
        }
    }

    match crate::chat::queries::blocked_among(&state.db, user_id, others).await {
        Ok(blocked) if !blocked.is_empty() => Err(CallHandlerError::Blocked),
        Ok(_) => Ok(()),
        Err(e) => {
            warn!(
                error = %e,
                %user_id,
                fail_open = state.config.block_check_fail_open,
                "DB block check failed, using failsafe policy"
            );
            if state.config.block_check_fail_open {
                Ok(())
            } else {
                Err(CallHandlerError::Blocked)
            }
        }
    }
}

/// GET /api/dm/{id}/call - Get current call state
#[utoipa::path(
    get,
    path = "/api/dm/{id}/call",
    tag = "voice",
    params(("id" = Uuid, Path, description = "DM conversation ID")),
    responses(
        (status = 200, description = "Current call state (null if no active call)", body = Option<CallStateResponse>),
        (status = 403, description = "Not a participant of this DM"),
        (status = 404, description = "DM channel not found"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth.id, channel_id = %channel_id))]
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
#[utoipa::path(
    post,
    path = "/api/dm/{id}/call/start",
    tag = "voice",
    params(("id" = Uuid, Path, description = "DM conversation ID")),
    responses(
        (status = 201, description = "Call initiated", body = CallStateResponse),
        (status = 400, description = "Invalid call request"),
        (status = 403, description = "Not a participant or blocked"),
        (status = 404, description = "DM channel not found"),
        (status = 409, description = "Call already exists"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth.id, channel_id = %channel_id))]
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

    let others: Vec<Uuid> = target_users.iter().copied().collect();
    ensure_not_blocked(&state, auth.id, &others).await?;

    let initiator_name = get_username(&state, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let started = call_service
        .start_call(
            channel_id,
            auth.id,
            target_users,
            state.config.call_ring_timeout_secs,
        )
        .await?;
    spawn_ring_timeout(
        state.redis.clone(),
        channel_id,
        started.meta.call_id.clone(),
        std::time::Duration::from_secs(state.config.call_ring_timeout_secs),
    );

    // Ring every other participant on their user channel, so the call rings
    // even if they do not have the DM open. The caller is not rung.
    broadcast_to_members(
        &state.redis,
        &others,
        &ServerEvent::IncomingCall {
            channel_id,
            initiator: auth.id,
            initiator_name,
            // Default capabilities: audio only for now
            capabilities: vec!["audio".to_string()],
        },
    )
    .await;
    let call_state = started.state;

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
#[utoipa::path(
    post,
    path = "/api/dm/{id}/call/join",
    tag = "voice",
    params(("id" = Uuid, Path, description = "DM conversation ID")),
    responses(
        (status = 200, description = "Joined call", body = CallStateResponse),
        (status = 403, description = "Not a participant or blocked"),
        (status = 404, description = "DM channel or call not found"),
        (status = 409, description = "Invalid state transition"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth.id, channel_id = %channel_id))]
pub async fn join_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallHandlerError> {
    // Verify membership
    let participants = verify_dm_participant(&state, channel_id, auth.id).await?;

    let others: Vec<Uuid> = participants
        .iter()
        .copied()
        .filter(|&id| id != auth.id)
        .collect();
    ensure_not_blocked(&state, auth.id, &others).await?;

    let call_service = CallService::new(state.redis.clone());
    let call_state = call_service.join_call(channel_id, auth.id).await?.state;

    let username = get_username(&state, auth.id).await?;
    broadcast_to_members(
        &state.redis,
        &participants,
        &ServerEvent::CallParticipantJoined {
            channel_id,
            user_id: auth.id,
            username,
        },
    )
    .await;

    Ok(Json(CallStateResponse {
        channel_id,
        state: call_state,
        capabilities: vec!["audio".to_string()],
    }))
}

/// POST /api/dm/{id}/call/decline - Decline a call
#[utoipa::path(
    post,
    path = "/api/dm/{id}/call/decline",
    tag = "voice",
    params(("id" = Uuid, Path, description = "DM conversation ID")),
    responses(
        (status = 200, description = "Call declined", body = CallStateResponse),
        (status = 403, description = "Not a participant of this DM"),
        (status = 404, description = "DM channel or call not found"),
        (status = 409, description = "Invalid state transition"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth.id, channel_id = %channel_id))]
pub async fn decline_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallHandlerError> {
    let participants = verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let declined = call_service.decline_call(channel_id, auth.id).await?;

    broadcast_to_members(
        &state.redis,
        &participants,
        &ServerEvent::CallDeclined {
            channel_id,
            user_id: auth.id,
        },
    )
    .await;
    // The last decline ended (and deleted) the call.
    broadcast_call_ended(&state.redis, channel_id, &declined.meta, &declined.state).await;
    let call_state = declined.state;

    Ok(Json(CallStateResponse {
        channel_id,
        state: call_state,
        capabilities: vec!["audio".to_string()],
    }))
}

/// POST /api/dm/{id}/call/leave - Leave an active call
#[utoipa::path(
    post,
    path = "/api/dm/{id}/call/leave",
    tag = "voice",
    params(("id" = Uuid, Path, description = "DM conversation ID")),
    responses(
        (status = 200, description = "Left call (may end call if last participant)", body = CallStateResponse),
        (status = 403, description = "Not a participant of this DM"),
        (status = 404, description = "DM channel or call not found"),
        (status = 409, description = "Invalid state transition"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth.id, channel_id = %channel_id))]
pub async fn leave_call(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<CallStateResponse>, CallHandlerError> {
    let participants = verify_dm_participant(&state, channel_id, auth.id).await?;

    let call_service = CallService::new(state.redis.clone());
    let left = call_service.leave_call(channel_id, auth.id).await?;

    broadcast_to_members(
        &state.redis,
        &participants,
        &ServerEvent::CallParticipantLeft {
            channel_id,
            user_id: auth.id,
        },
    )
    .await;
    // Ended (and deleted) if the initiator cancelled or the last one left.
    broadcast_call_ended(&state.redis, channel_id, &left.meta, &left.state).await;
    let call_state = left.state;

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
