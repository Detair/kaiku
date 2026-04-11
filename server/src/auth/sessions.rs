//! Session management handlers (list, revoke).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use super::error::{AuthError, AuthResult};
use super::helpers::extract_current_token_hash;
use super::middleware::AuthUser;
use super::queries;
use super::types::{RevokeAllResponse, SessionInfo, SessionListResponse};
use super::ua_parser;
use crate::api::AppState;

/// List all active sessions for the authenticated user.
///
/// GET /auth/sessions
#[utoipa::path(
    get,
    path = "/auth/sessions",
    tag = "auth",
    responses(
        (status = 200, description = "Active auth sessions", body = SessionListResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, jar, headers))]
pub async fn list_sessions(
    State(state): State<AppState>,
    auth_user: AuthUser,
    headers: HeaderMap,
    jar: CookieJar,
) -> AuthResult<Json<SessionListResponse>> {
    // Try to identify the current session from cookie (browser) or header (Tauri).
    // Best-effort: if neither is available, all sessions will have is_current = false.
    let current_hash = extract_current_token_hash(&headers, &jar);

    let sessions = queries::list_user_sessions(&state.db, auth_user.id).await?;

    let session_infos: Vec<SessionInfo> = sessions
        .iter()
        .map(|s| {
            let device = s
                .user_agent
                .as_deref()
                .map(ua_parser::parse_device_name)
                .unwrap_or_else(|| "Unknown device".to_string());

            SessionInfo {
                id: s.id,
                device,
                ip_address: s.ip_address.clone(),
                city: s.city.clone(),
                country: s.country.clone(),
                created_at: s.created_at,
                expires_at: s.expires_at,
                is_current: current_hash.as_deref() == Some(&s.token_hash),
            }
        })
        .collect();

    Ok(Json(SessionListResponse {
        sessions: session_infos,
    }))
}

/// Revoke a specific session by ID.
///
/// `DELETE /auth/sessions/{session_id}`
#[utoipa::path(
    delete,
    path = "/auth/sessions/{session_id}",
    tag = "auth",
    params(("session_id" = Uuid, Path, description = "Session ID to revoke")),
    responses(
        (status = 204, description = "Session revoked"),
        (status = 403, description = "Cannot revoke current session or another user's session"),
        (status = 404, description = "Session not found"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, jar, headers))]
pub async fn revoke_session(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(session_id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> AuthResult<StatusCode> {
    // Find the session
    let session = queries::find_session_by_id(&state.db, session_id)
        .await?
        .ok_or_else(|| AuthError::NotFound("Session not found".to_string()))?;

    // Must belong to the authenticated user
    if session.user_id != auth_user.id {
        return Err(AuthError::Forbidden);
    }

    // Cannot revoke current session (use logout instead)
    let current_hash = extract_current_token_hash(&headers, &jar);
    if current_hash.as_deref() == Some(&session.token_hash) {
        return Err(AuthError::Forbidden);
    }

    queries::delete_session_by_id(&state.db, session_id).await?;

    tracing::info!(user_id = %auth_user.id, session_id = %session_id, "Session revoked");

    Ok(StatusCode::NO_CONTENT)
}

/// Revoke all sessions except the current one.
///
/// DELETE /auth/sessions
#[utoipa::path(
    delete,
    path = "/auth/sessions",
    tag = "auth",
    responses(
        (status = 200, description = "All other sessions revoked", body = RevokeAllResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, jar, headers))]
pub async fn revoke_all_other_sessions(
    State(state): State<AppState>,
    auth_user: AuthUser,
    headers: HeaderMap,
    jar: CookieJar,
) -> AuthResult<Json<RevokeAllResponse>> {
    let current_hash = extract_current_token_hash(&headers, &jar);

    // If we cannot identify the current session, refuse to proceed — otherwise
    // we would delete ALL sessions (including the caller's own).
    let current_hash = current_hash.ok_or_else(|| {
        tracing::warn!(user_id = %auth_user.id, "Cannot identify current session for revoke-all-others");
        AuthError::Validation(
            "Cannot identify current session. Please provide a refresh token via cookie or X-Refresh-Token header.".to_string(),
        )
    })?;

    let revoked_count =
        queries::delete_other_user_sessions(&state.db, auth_user.id, &current_hash).await? as i64;
    tracing::info!(user_id = %auth_user.id, revoked_count, "All other sessions revoked");

    Ok(Json(RevokeAllResponse { revoked_count }))
}
